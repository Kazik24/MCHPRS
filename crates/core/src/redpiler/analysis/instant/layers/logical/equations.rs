//! Bounded, iterative source-equation extraction and Boolean lowering.
use super::*;
use std::collections::VecDeque;

#[derive(Clone, Copy, PartialEq, Eq)]
enum AbstractValue {
    Constant(bool),
    Unknown,
}

#[derive(Debug)]
pub(super) enum SourceLoweringError {
    Unsupported(String),
    BudgetExceeded(String),
    Fatal(String),
}

impl From<String> for SourceLoweringError {
    fn from(message: String) -> Self {
        Self::Fatal(message)
    }
}

impl From<&str> for SourceLoweringError {
    fn from(message: &str) -> Self {
        Self::Fatal(message.into())
    }
}

pub(super) fn lower_deferred_sources(
    equations: &mut Equations<'_>,
    roots: &mut [LayerExprId],
    monitor: &TaskMonitor,
) -> Result<(), SourceLoweringError> {
    const MAX_SOURCE_NODES: usize = 200_000;
    const MAX_SOURCE_EDGES: usize = 2_000_000;
    if !equations.defer_sources {
        return Err("iterative source lowering requires deferred source inputs".into());
    }

    let mut source_roots = FxHashMap::<LayerInputSource, LayerExprId>::default();
    let mut dependencies = FxHashMap::<LayerInputSource, FxHashSet<LayerInputSource>>::default();
    let mut edge_count = 0usize;
    let reachable_roots = roots.iter().try_fold(FxHashSet::default(), |mut sources, &root| {
        sources.extend(reachable_sources(equations, root)?);
        Ok::<_, String>(sources)
    })?;
    equations.deferred_roots.retain(|source| reachable_roots.contains(source));
    let mut pending: VecDeque<_> = sorted_sources(&equations.deferred_roots).into();
    let mut discovered = FxHashSet::default();
    while let Some(source) = pending.pop_front() {
        check_cancelled(monitor).map_err(|error| SourceLoweringError::Fatal(format!("{error:?}")))?;
        if !discovered.insert(source) {
            continue;
        }
        if discovered.len() > MAX_SOURCE_NODES {
            return Err(SourceLoweringError::BudgetExceeded(format!(
                "logical source equation graph exceeds {MAX_SOURCE_NODES} nodes"
            )));
        }
        equations.deferred_parent = Some(source);
        equations.expanded.clear();
        let root = equations.source_value(source).map_err(|error| map_source_error(source, error))?;
        let reachable = reachable_sources(equations, root)?;
        let children = equations
            .deferred_children
            .remove(&source)
            .unwrap_or_default()
            .into_iter()
            .filter(|child| reachable.contains(child))
            .collect::<FxHashSet<_>>();
        edge_count = edge_count.saturating_add(children.len());
        if edge_count > MAX_SOURCE_EDGES {
            return Err(SourceLoweringError::BudgetExceeded(format!(
                "logical source equation graph exceeds {MAX_SOURCE_EDGES} edges"
            )));
        }
        pending.extend(sorted_sources(&children));
        dependencies.insert(source, children);
        source_roots.insert(source, root);
    }
    equations.deferred_parent = None;

    lower_source_equations(equations, &source_roots, &dependencies, roots, monitor)
}

fn lower_source_equations(
    equations: &mut Equations<'_>,
    source_roots: &FxHashMap<LayerInputSource, LayerExprId>,
    dependencies: &FxHashMap<LayerInputSource, FxHashSet<LayerInputSource>>,
    roots: &mut [LayerExprId],
    monitor: &TaskMonitor,
) -> Result<(), SourceLoweringError> {
    let mut parents = FxHashMap::<LayerInputSource, Vec<LayerInputSource>>::default();
    for (&parent, children) in dependencies {
        for &child in children {
            parents.entry(child).or_default().push(parent);
        }
    }

    // Propagate constants from equations whose live leaves are all known. Enqueue
    // only their dependents, so a long acyclic chain remains linear in graph size.
    let mut constants = FxHashMap::<LayerInputSource, bool>::default();
    let mut initial_sources: Vec<_> = source_roots.keys().copied().collect();
    sort_sources(&mut initial_sources);
    let mut constant_work: VecDeque<_> = initial_sources.into();
    while let Some(source) = constant_work.pop_front() {
        check_cancelled(monitor).map_err(|error| SourceLoweringError::Fatal(format!("{error:?}")))?;
        if constants.contains_key(&source) {
            continue;
        }
        let values = abstract_values(equations, source_roots[&source], &dependencies[&source], &constants)?;
        if let AbstractValue::Constant(value) = values[&source_roots[&source]] {
            constants.insert(source, value);
            if let Some(source_parents) = parents.get(&source) {
                constant_work.extend(sorted_sources(&source_parents.iter().copied().collect()));
            }
        }
    }

    let mut live_dependencies = FxHashMap::<LayerInputSource, FxHashSet<LayerInputSource>>::default();
    let mut equation_sources: Vec<_> = source_roots.keys().copied().collect();
    sort_sources(&mut equation_sources);
    for source in equation_sources {
        let root = source_roots[&source];
        check_cancelled(monitor).map_err(|error| SourceLoweringError::Fatal(format!("{error:?}")))?;
        if constants.contains_key(&source) {
            live_dependencies.insert(source, FxHashSet::default());
            continue;
        }
        let values = abstract_values(equations, root, &dependencies[&source], &constants)?;
        let mut live = FxHashSet::default();
        collect_live_dependencies(equations, root, &dependencies[&source], &constants, &values, &mut live)?;
        live_dependencies.insert(source, live);
    }

    // Ignore equation components pruned from every requested output by global
    // constant propagation; an unreachable feedback loop cannot affect the artifact.
    let all_sources: FxHashSet<_> = source_roots.keys().copied().collect();
    let mut active = FxHashSet::default();
    let mut active_work = VecDeque::new();
    for &root in roots.iter() {
        let values = abstract_values(equations, root, &all_sources, &constants)?;
        let mut live = FxHashSet::default();
        collect_live_dependencies(equations, root, &all_sources, &constants, &values, &mut live)?;
        active_work.extend(sorted_sources(&live));
    }
    while let Some(source) = active_work.pop_front() {
        if !active.insert(source) || constants.contains_key(&source) {
            continue;
        }
        let children: FxHashSet<_> = live_dependencies.get(&source).into_iter().flatten().copied().collect();
        active_work.extend(sorted_sources(&children));
    }

    let mut remaining = FxHashMap::<LayerInputSource, usize>::default();
    let mut dependents = FxHashMap::<LayerInputSource, Vec<LayerInputSource>>::default();
    for source in sorted_sources(&active) {
        if constants.contains_key(&source) {
            continue;
        }
        let dependencies = &live_dependencies[&source];
        remaining.insert(source, dependencies.len());
        for dependency in sorted_sources(dependencies) {
            if !source_roots.contains_key(&dependency) || constants.contains_key(&dependency) {
                return Err(SourceLoweringError::Fatal(format!(
                    "logical source dependency {dependency:?} has no equation or constant"
                )));
            }
            dependents.entry(dependency).or_default().push(source);
        }
    }
    let mut ready: Vec<_> = remaining.iter().filter_map(|(&source, &count)| (count == 0).then_some(source)).collect();
    sort_sources(&mut ready);
    let mut topo: VecDeque<_> = ready.into();
    let mut order = Vec::with_capacity(remaining.len());
    while let Some(source) = topo.pop_front() {
        order.push(source);
        if let Some(source_parents) = dependents.get(&source) {
            for parent in source_parents {
                let count = remaining.get_mut(parent).expect("dependency parent has a remaining count");
                *count -= 1;
                if *count == 0 {
                    topo.push_back(*parent);
                }
            }
        }
    }
    if order.len() != remaining.len() {
        let cycle_nodes: FxHashSet<_> = remaining.iter().filter_map(|(&source, &count)| (count > 0).then_some(source)).collect();
        let mut adjacency = FxHashMap::<LayerInputSource, Vec<LayerInputSource>>::default();
        for (&source, dependencies) in &live_dependencies {
            if !cycle_nodes.contains(&source) {
                continue;
            }
            let mut dependencies: Vec<_> = dependencies
                .iter()
                .copied()
                .filter(|dependency| cycle_nodes.contains(dependency))
                .collect();
            sort_sources(&mut dependencies);
            adjacency.insert(source, dependencies);
        }
        let cycle = find_cycle(&adjacency, &cycle_nodes);
        #[cfg(test)]
        if equations.resource_cycle_witness.is_none() {
            equations.resource_cycle_witness = cycle.as_deref().and_then(|cycle| {
                cycle.iter().find_map(|source| match source {
                    LayerInputSource::State(StateTest::ResourceAt { resource, contact }) => Some((*resource, *contact)),
                    _ => None,
                })
            });
        }
        let witness = cycle.unwrap_or_default();
        #[cfg(test)]
        {
            equations.source_cycle_witness = witness.clone();
            if let Some(&anchor) = witness.first() {
                let component = strongly_connected_component(&adjacency, anchor, &cycle_nodes);
                let mut edges = Vec::new();
                for &source in &component {
                    if let Some(dependencies) = adjacency.get(&source) {
                        edges.extend(
                            dependencies
                                .iter()
                                .filter(|dependency| component.contains(dependency))
                                .map(|&dependency| (source, dependency)),
                        );
                    }
                }
                edges.sort_by_cached_key(|(source, dependency)| (format!("{source:?}"), format!("{dependency:?}")));
                equations.source_scc_edges = edges;
            }
        }
        return Err(SourceLoweringError::Unsupported(format!(
            "logical source equations contain unresolved feedback (cycle nodes={}, witness={})",
            witness.len().saturating_sub(1),
            bounded_cycle_witness(&witness)
        )));
    }

    let old = &equations.reduced;
    let mut lowered = LayerBuilder::new(old.monitor).map_err(|error| map_lowering_error("expression arena", error))?;
    let mut source_values = FxHashMap::<LayerInputSource, LayerExprId>::default();
    let mut constant_sources: Vec<_> = constants.keys().copied().collect();
    sort_sources(&mut constant_sources);
    for source in constant_sources {
        let value = constants[&source];
        source_values.insert(source, lowered.constant(value).map_err(|error| map_lowering_error("constant", error))?);
    }
    for source in order {
        check_cancelled(monitor).map_err(|error| SourceLoweringError::Fatal(format!("{error:?}")))?;
        let root = source_roots[&source];
        let values = abstract_values(equations, root, &dependencies[&source], &constants)?;
        let value = lower_template(equations, &mut lowered, root, &dependencies[&source], &values, &source_values)?;
        source_values.insert(source, value);
    }
    for root in roots {
        let values = abstract_values(equations, *root, &all_sources, &constants)?;
        *root = lower_template(equations, &mut lowered, *root, &all_sources, &values, &source_values)?;
    }
    equations.reduced = lowered;
    equations.defer_sources = false;
    equations.deferred_parent = None;
    equations.deferred_roots.clear();
    equations.deferred_children.clear();
    Ok(())
}

fn bounded_cycle_witness(cycle: &[LayerInputSource]) -> String {
    const HEAD: usize = 24;
    const TAIL: usize = 8;
    if cycle.len() <= HEAD + TAIL {
        return format!("{cycle:?}");
    }
    let head = &cycle[..HEAD];
    let tail = &cycle[cycle.len() - TAIL..];
    format!("head={head:?}, tail={tail:?}")
}

pub(super) fn reachable_sources(equations: &Equations<'_>, root: LayerExprId) -> Result<FxHashSet<LayerInputSource>, String> {
    let mut sources = FxHashSet::default();
    let mut visited = FxHashSet::default();
    let mut pending = vec![root];
    while let Some(expression) = pending.pop() {
        check_cancelled(equations.reduced.monitor).map_err(|error| format!("{error:?}"))?;
        if !visited.insert(expression) {
            continue;
        }
        match equations.reduced.expression(expression).map_err(|error| format!("{error:?}"))? {
            LayerExpr::Constant(_) => {}
            LayerExpr::Input(input) => {
                let binding = equations
                    .reduced
                    .inputs
                    .get(input.0 as usize)
                    .ok_or("logical expression references a missing input")?;
                sources.insert(binding.source);
            }
            LayerExpr::Not(child) => pending.push(*child),
            LayerExpr::All(children) | LayerExpr::Any(children) => pending.extend(children.iter().copied()),
        }
    }
    Ok(sources)
}

fn abstract_values(
    equations: &Equations<'_>,
    root: LayerExprId,
    dependencies: &FxHashSet<LayerInputSource>,
    constants: &FxHashMap<LayerInputSource, bool>,
) -> Result<FxHashMap<LayerExprId, AbstractValue>, String> {
    let mut reachable = FxHashSet::default();
    let mut pending = vec![root];
    while let Some(id) = pending.pop() {
        if !reachable.insert(id) {
            continue;
        }
        match equations.reduced.expression(id).map_err(|error| format!("{error:?}"))? {
            LayerExpr::Not(child) => pending.push(*child),
            LayerExpr::All(children) | LayerExpr::Any(children) => pending.extend(children.iter().copied()),
            LayerExpr::Constant(_) | LayerExpr::Input(_) => {}
        }
    }
    let mut ordered: Vec<_> = reachable.into_iter().collect();
    ordered.sort_unstable();
    let mut values = FxHashMap::default();
    for id in ordered {
        let expression = equations.reduced.expression(id).map_err(|error| format!("{error:?}"))?;
        let value = match expression {
            LayerExpr::Constant(value) => AbstractValue::Constant(*value),
            LayerExpr::Input(input) => {
                let source = equations
                    .reduced
                    .inputs
                    .get(input.0 as usize)
                    .ok_or("logical equation input is missing")?
                    .source;
                if dependencies.contains(&source) {
                    constants.get(&source).copied().map_or(AbstractValue::Unknown, AbstractValue::Constant)
                } else {
                    AbstractValue::Unknown
                }
            }
            LayerExpr::Not(child) => match values.get(child).copied().ok_or("logical equation has a forward operand")? {
                AbstractValue::Constant(value) => AbstractValue::Constant(!value),
                AbstractValue::Unknown => AbstractValue::Unknown,
            },
            LayerExpr::All(children) => {
                let mut unknown = false;
                let mut false_value = false;
                for child in children.iter() {
                    match values.get(child).copied().ok_or("logical equation has a forward operand")? {
                        AbstractValue::Constant(false) => false_value = true,
                        AbstractValue::Constant(true) => {}
                        AbstractValue::Unknown => unknown = true,
                    }
                }
                if false_value {
                    AbstractValue::Constant(false)
                } else if unknown {
                    AbstractValue::Unknown
                } else {
                    AbstractValue::Constant(true)
                }
            }
            LayerExpr::Any(children) => {
                let mut unknown = false;
                let mut true_value = false;
                for child in children.iter() {
                    match values.get(child).copied().ok_or("logical equation has a forward operand")? {
                        AbstractValue::Constant(true) => true_value = true,
                        AbstractValue::Constant(false) => {}
                        AbstractValue::Unknown => unknown = true,
                    }
                }
                if true_value {
                    AbstractValue::Constant(true)
                } else if unknown {
                    AbstractValue::Unknown
                } else {
                    AbstractValue::Constant(false)
                }
            }
        };
        values.insert(id, value);
    }
    Ok(values)
}

fn collect_live_dependencies(
    equations: &Equations<'_>,
    root: LayerExprId,
    dependencies: &FxHashSet<LayerInputSource>,
    constants: &FxHashMap<LayerInputSource, bool>,
    values: &FxHashMap<LayerExprId, AbstractValue>,
    result: &mut FxHashSet<LayerInputSource>,
) -> Result<(), String> {
    let mut pending = vec![root];
    let mut visited = FxHashSet::default();
    while let Some(expression) = pending.pop() {
        check_cancelled(equations.reduced.monitor).map_err(|error| format!("{error:?}"))?;
        if !visited.insert(expression) || matches!(values.get(&expression), Some(AbstractValue::Constant(_))) {
            continue;
        }
        match equations.reduced.expression(expression).map_err(|error| format!("{error:?}"))? {
            LayerExpr::Constant(_) => {}
            LayerExpr::Input(input) => {
                let source = equations
                    .reduced
                    .inputs
                    .get(input.0 as usize)
                    .ok_or("logical equation input is missing")?
                    .source;
                if dependencies.contains(&source) && !constants.contains_key(&source) {
                    result.insert(source);
                }
            }
            LayerExpr::Not(child) => pending.push(*child),
            LayerExpr::All(children) | LayerExpr::Any(children) => pending.extend(children.iter().copied()),
        }
    }
    Ok(())
}

fn lower_template(
    equations: &Equations<'_>,
    builder: &mut LayerBuilder<'_>,
    root: LayerExprId,
    dependencies: &FxHashSet<LayerInputSource>,
    abstract_values: &FxHashMap<LayerExprId, AbstractValue>,
    lowered: &FxHashMap<LayerInputSource, LayerExprId>,
) -> Result<LayerExprId, SourceLoweringError> {
    let mut values = FxHashMap::<LayerExprId, LayerExprId>::default();
    let mut pending = vec![(root, false)];
    while let Some((expression, expanded)) = pending.pop() {
        check_cancelled(equations.reduced.monitor).map_err(|error| SourceLoweringError::Fatal(format!("{error:?}")))?;
        if values.contains_key(&expression) {
            continue;
        }
        if let AbstractValue::Constant(value) = abstract_values[&expression] {
            values.insert(
                expression,
                builder.constant(value).map_err(|error| map_lowering_error("constant", error))?,
            );
            continue;
        }
        let old = equations
            .reduced
            .expression(expression)
            .map_err(|error| SourceLoweringError::Fatal(format!("{error:?}")))?;
        if !expanded {
            pending.push((expression, true));
            match old {
                LayerExpr::Not(child) => pending.push((*child, false)),
                LayerExpr::All(children) | LayerExpr::Any(children) => {
                    pending.extend(
                        children
                            .iter()
                            .filter(|child| abstract_values[child] == AbstractValue::Unknown)
                            .map(|child| (*child, false)),
                    );
                }
                _ => {}
            }
            continue;
        }
        let value = match old {
            LayerExpr::Constant(value) => builder.constant(*value).map_err(|error| map_lowering_error("constant", error)),
            LayerExpr::Input(input) => {
                let binding = *equations
                    .reduced
                    .inputs
                    .get(input.0 as usize)
                    .ok_or_else(|| SourceLoweringError::Fatal("logical input is missing".into()))?;
                if dependencies.contains(&binding.source) {
                    lowered.get(&binding.source).copied().ok_or_else(|| {
                        SourceLoweringError::Fatal(format!("logical dependency {:?} was not lowered before its consumer", binding.source))
                    })
                } else {
                    builder
                        .input(binding.source, binding.role)
                        .and_then(|input| builder.input_expr(input))
                        .map_err(|error| map_lowering_error("input", error))
                }
            }
            LayerExpr::Not(child) => builder.not(values[child]).map_err(|error| map_lowering_error("negation", error)),
            LayerExpr::All(children) => builder
                .all(
                    children
                        .iter()
                        .filter(|child| abstract_values[child] == AbstractValue::Unknown)
                        .map(|child| values[child]),
                )
                .map_err(|error| map_lowering_error("conjunction", error)),
            LayerExpr::Any(children) => builder
                .any(
                    children
                        .iter()
                        .filter(|child| abstract_values[child] == AbstractValue::Unknown)
                        .map(|child| values[child]),
                )
                .map_err(|error| map_lowering_error("disjunction", error)),
        }?;
        values.insert(expression, value);
    }
    Ok(values[&root])
}

fn map_lowering_error(context: &str, error: ConstructionError) -> SourceLoweringError {
    match error {
        ConstructionError::BudgetExceeded => SourceLoweringError::BudgetExceeded(format!("lowering {context} exceeded the logical proof budget")),
        ConstructionError::Unsupported(message) => SourceLoweringError::Fatal(format!("lowering {context}: {message}")),
        ConstructionError::FeedbackRequiresRegister(source) => {
            SourceLoweringError::Fatal(format!("lowering {context}: logical feedback requires a register: {source:?}"))
        }
        ConstructionError::Invalid(message) => SourceLoweringError::Fatal(format!("lowering {context}: {message}")),
    }
}

fn map_source_error(source: LayerInputSource, error: ConstructionError) -> SourceLoweringError {
    match error {
        ConstructionError::BudgetExceeded => SourceLoweringError::BudgetExceeded("logical source equation exceeded its construction budget".into()),
        ConstructionError::Unsupported(message) => SourceLoweringError::Unsupported(format!("logical source equation {source:?}: {message}")),
        ConstructionError::FeedbackRequiresRegister(feedback) => SourceLoweringError::Unsupported(format!(
            "logical source equation {source:?}: logical feedback requires a register: {feedback:?}"
        )),
        ConstructionError::Invalid(message) => SourceLoweringError::Fatal(format!("logical source equation {source:?}: {message}")),
    }
}

fn find_cycle(
    dependencies: &FxHashMap<LayerInputSource, Vec<LayerInputSource>>,
    allowed: &FxHashSet<LayerInputSource>,
) -> Option<Vec<LayerInputSource>> {
    let mut color = FxHashMap::<LayerInputSource, u8>::default();
    let roots = sorted_sources(allowed);
    for root in roots {
        if color.contains_key(&root) {
            continue;
        }
        let mut stack = vec![(root, 0usize)];
        color.insert(root, 1);
        while let Some((node, child_index)) = stack.last_mut() {
            let children = dependencies.get(node).map(Vec::as_slice).unwrap_or_default();
            if *child_index >= children.len() {
                color.insert(*node, 2);
                stack.pop();
                continue;
            }
            let child = children[*child_index];
            *child_index += 1;
            match color.get(&child).copied().unwrap_or(0) {
                0 => {
                    color.insert(child, 1);
                    stack.push((child, 0));
                }
                1 => {
                    let start = stack.iter().position(|(candidate, _)| *candidate == child)?;
                    let mut cycle: Vec<_> = stack[start..].iter().map(|(candidate, _)| *candidate).collect();
                    cycle.push(child);
                    return Some(cycle);
                }
                _ => {}
            }
        }
    }
    None
}

#[cfg(test)]
fn strongly_connected_component(
    adjacency: &FxHashMap<LayerInputSource, Vec<LayerInputSource>>,
    start: LayerInputSource,
    allowed: &FxHashSet<LayerInputSource>,
) -> FxHashSet<LayerInputSource> {
    fn reachable(
        start: LayerInputSource,
        adjacency: &FxHashMap<LayerInputSource, Vec<LayerInputSource>>,
        allowed: &FxHashSet<LayerInputSource>,
    ) -> FxHashSet<LayerInputSource> {
        let mut seen = FxHashSet::default();
        let mut pending = vec![start];
        while let Some(source) = pending.pop() {
            if !allowed.contains(&source) || !seen.insert(source) {
                continue;
            }
            pending.extend(adjacency.get(&source).into_iter().flatten().copied());
        }
        seen
    }

    let mut reverse = FxHashMap::<LayerInputSource, Vec<LayerInputSource>>::default();
    for (&source, dependencies) in adjacency {
        for &dependency in dependencies {
            reverse.entry(dependency).or_default().push(source);
        }
    }
    let forward = reachable(start, adjacency, allowed);
    let backward = reachable(start, &reverse, allowed);
    forward.intersection(&backward).copied().collect()
}

fn sorted_sources(sources: &FxHashSet<LayerInputSource>) -> Vec<LayerInputSource> {
    let mut values: Vec<_> = sources.iter().copied().collect();
    sort_sources(&mut values);
    values
}

fn sort_sources(sources: &mut [LayerInputSource]) {
    // Keep the established textual order while formatting each key once.
    sources.sort_by_cached_key(|source| format!("{source:?}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(index: u32) -> LayerInputSource {
        LayerInputSource::State(StateTest::ResourceEnabled(ResourceId::new(index)))
    }

    #[test]
    fn cached_debug_sort_matches_old_order_for_every_source_variant_and_edge() {
        use crate::redpiler::program_graph::{PowerLink, PowerPort, PowerTarget, RouteOccurrence};

        let link = PowerLink {
            from: SiteId::new(9),
            from_port: PowerPort::Strong(mchprs_blocks::BlockFace::West),
            to: PowerTarget::Input {
                node: NodeId::new(7),
                port: InputPort::Quasi,
            },
            face: mchprs_blocks::BlockFace::East,
            attenuation: 3,
            guard: GuardId::new(5),
        };
        let occurrence = RouteOccurrence {
            route: RouteId::new(11),
            ordinal: 2,
        };
        let other_occurrence = RouteOccurrence {
            route: RouteId::new(3),
            ordinal: 7,
        };
        let other_link = PowerLink {
            from: SiteId::new(1),
            from_port: PowerPort::Conducted { include_dust: true },
            to: PowerTarget::Conductor {
                site: SiteId::new(2),
                include_dust: false,
            },
            face: mchprs_blocks::BlockFace::South,
            attenuation: 0,
            guard: GuardId::new(4),
        };
        let sources = [
            LayerInputSource::SourceAbove {
                site: SiteId::new(8),
                port: PowerPort::Weak(mchprs_blocks::BlockFace::North),
                threshold: 4,
            },
            LayerInputSource::SourceAbove {
                site: SiteId::new(18),
                port: PowerPort::Strong(mchprs_blocks::BlockFace::East),
                threshold: 15,
            },
            LayerInputSource::BoundaryEvent { port: 3, occurrence },
            LayerInputSource::BoundaryEvent {
                port: 13,
                occurrence: other_occurrence,
            },
            LayerInputSource::BoundaryRequest {
                port: 4,
                occurrence,
                actor: PistonId::new(6),
            },
            LayerInputSource::BoundaryRequest {
                port: 14,
                occurrence: other_occurrence,
                actor: PistonId::new(16),
            },
            LayerInputSource::BoundaryState {
                port: 5,
                test: StateTest::PistonAvailability {
                    piston: PistonId::new(6),
                    is: PistonAvailability::Ready,
                },
            },
            LayerInputSource::BoundaryState {
                port: 15,
                test: StateTest::ResourceEnabled(ResourceId::new(17)),
            },
            LayerInputSource::PowerContribution { port: 7, link },
            LayerInputSource::PowerContribution { port: 17, link: other_link },
            LayerInputSource::LocalResetOutput {
                observer: NodeId::new(10),
                link,
            },
            LayerInputSource::LocalResetOutput {
                observer: NodeId::new(20),
                link: other_link,
            },
            LayerInputSource::State(StateTest::ResourceAt {
                resource: ResourceId::new(12),
                contact: ContactId::new(13),
            }),
            LayerInputSource::State(StateTest::ResourceAt {
                resource: ResourceId::new(22),
                contact: ContactId::new(23),
            }),
            LayerInputSource::InstantFired { actor: PistonId::new(14) },
            LayerInputSource::InstantFired { actor: PistonId::new(24) },
        ];
        let debug_keys: FxHashSet<_> = sources.iter().map(|source| format!("{source:?}")).collect();
        assert_eq!(debug_keys.len(), sources.len(), "Debug keys distinguish the source variants and fields");
        let mut expected = sources.to_vec();
        expected.sort_unstable_by_key(|source| format!("{source:?}"));

        for insertion_order in [sources.to_vec(), sources.iter().rev().copied().collect(), {
            let mut rotated = sources.to_vec();
            rotated.rotate_left(3);
            rotated
        }] {
            let set: FxHashSet<_> = insertion_order.into_iter().collect();
            let mut actual: Vec<_> = set.into_iter().collect();
            sort_sources(&mut actual);
            assert_eq!(actual, expected);
        }

        let mut tied = vec![sources[2], sources[2], sources[0], sources[0]];
        let mut tied_expected = tied.clone();
        tied_expected.sort_unstable_by_key(|source| format!("{source:?}"));
        sort_sources(&mut tied);
        assert_eq!(tied, tied_expected);

        let edges = [
            (sources[2], sources[0]),
            (sources[2], sources[4]),
            (sources[0], sources[2]),
            (sources[2], sources[0]),
        ];
        let mut expected_edges = edges.to_vec();
        expected_edges.sort_unstable_by_key(|(source, dependency)| (format!("{source:?}"), format!("{dependency:?}")));
        let mut cached_edges = edges.to_vec();
        cached_edges.sort_by_cached_key(|(source, dependency)| (format!("{source:?}"), format!("{dependency:?}")));
        assert_eq!(cached_edges, expected_edges);
        for insertion_order in [edges.to_vec(), edges.iter().rev().copied().collect()] {
            let set: FxHashSet<_> = insertion_order.into_iter().collect();
            let mut actual_edges: Vec<_> = set.into_iter().collect();
            actual_edges.sort_by_cached_key(|(source, dependency)| (format!("{source:?}"), format!("{dependency:?}")));
            let mut expected_set: Vec<_> = edges.iter().copied().collect::<FxHashSet<_>>().into_iter().collect();
            expected_set.sort_unstable_by_key(|(source, dependency)| (format!("{source:?}"), format!("{dependency:?}")));
            assert_eq!(actual_edges, expected_set);
        }
    }

    fn make_equations<'a>(graph: &'a CompileGraph, monitor: &'a TaskMonitor) -> Equations<'a> {
        Equations {
            graph,
            power: index_power(graph),
            wires: FxHashSet::default(),
            resets: FxHashSet::default(),
            cells: FxHashMap::default(),
            unique_cell_resources: FxHashSet::default(),
            disabled_destinations: FxHashSet::default(),
            actors: FxHashSet::default(),
            registers: FxHashMap::default(),
            expanded_components: FxHashSet::default(),
            inline_repeaters: false,
            stateful_repeaters: FxHashSet::default(),
            raw: LayerBuilder::new(monitor).unwrap(),
            signal_memo: signals::SignalMemo::default(),
            reduced: LayerBuilder::new(monitor).unwrap(),
            expanded: FxHashMap::default(),
            sources: FxHashMap::default(),
            pending: FxHashSet::default(),
            pending_repeaters: FxHashSet::default(),
            retain_classic: true,
            resource_semantics: ResourceSemantics::default(),
            fired_inputs: FxHashMap::default(),
            firing_resources: FxHashMap::default(),
            one_shot_contracts: FxHashMap::default(),
            diagnostic_unknown_resources: false,
            resource_cycle_witness: None,
            source_cycle_witness: Vec::new(),
            source_scc_edges: Vec::new(),
            defer_sources: true,
            stable_wire_modes: false,
            deferred_parent: None,
            deferred_roots: FxHashSet::default(),
            deferred_children: FxHashMap::default(),
            feedback_state_sources: Vec::new(),
        }
    }

    fn input(builder: &mut LayerBuilder<'_>, source: LayerInputSource) -> LayerExprId {
        let input = builder.input(source, LayerInputRole::RequestSample).unwrap();
        builder.input_expr(input).unwrap()
    }

    fn graph() -> CompileGraph {
        super::super::super::tests::empty_graph(PowerLink {
            from: SiteId::new(0),
            from_port: PowerPort::Output,
            to: PowerTarget::Input {
                node: NodeId::new(0),
                port: InputPort::Main,
            },
            face: mchprs_blocks::BlockFace::Top,
            attenuation: 0,
            guard: GuardId::new(0),
        })
    }

    #[test]
    fn iterative_cycle_walk_handles_long_chains_and_returns_a_closed_witness() {
        let nodes: Vec<_> = (0..513).map(source).collect();
        let allowed: FxHashSet<_> = nodes.iter().copied().collect();
        let mut adjacency = FxHashMap::default();
        for pair in nodes.windows(2) {
            adjacency.insert(pair[0], vec![pair[1]]);
        }

        assert_eq!(find_cycle(&adjacency, &allowed), None);
        adjacency.insert(*nodes.last().unwrap(), vec![nodes[200]]);
        let cycle = find_cycle(&adjacency, &allowed).expect("the back edge closes a cycle");
        assert_eq!(cycle.first(), cycle.last());
        assert!(cycle.windows(2).all(|pair| adjacency[&pair[0]].contains(&pair[1])));
    }

    #[test]
    fn source_scc_excludes_one_way_descendants_and_keeps_every_internal_edge() {
        let a = source(30);
        let b = source(31);
        let c = source(32);
        let d = source(33);
        let adjacency: FxHashMap<_, _> = [(a, vec![b, c]), (b, vec![a, c]), (c, vec![d]), (d, vec![])].into_iter().collect();
        let allowed = FxHashSet::from_iter([a, b, c, d]);
        let component = strongly_connected_component(&adjacency, a, &allowed);
        assert_eq!(component, FxHashSet::from_iter([a, b]));
        let mut internal_edges = FxHashSet::default();
        for &node in &component {
            for &next in &adjacency[&node] {
                if component.contains(&next) {
                    internal_edges.insert((node, next));
                }
            }
        }
        assert_eq!(internal_edges, FxHashSet::from_iter([(a, b), (b, a)]));
    }

    #[test]
    fn source_lowering_handles_long_chains_and_preserves_terminal_leaves() {
        let graph = graph();
        let monitor = TaskMonitor::default();
        let mut equations = make_equations(&graph, &monitor);
        let mut source_roots = FxHashMap::default();
        let mut dependencies = FxHashMap::default();
        let nodes: Vec<_> = (1000..1513).map(source).collect();
        let terminal = source(2000);
        for pair in nodes.windows(2) {
            let root = input(&mut equations.reduced, pair[1]);
            source_roots.insert(pair[0], root);
            dependencies.insert(pair[0], FxHashSet::from_iter([pair[1]]));
        }
        let last_root = input(&mut equations.reduced, terminal);
        source_roots.insert(*nodes.last().unwrap(), last_root);
        dependencies.insert(*nodes.last().unwrap(), FxHashSet::default());
        let mut roots = [input(&mut equations.reduced, nodes[0])];

        lower_source_equations(&mut equations, &source_roots, &dependencies, &mut roots, &monitor).unwrap();

        assert!(equations.reduced.inputs.iter().any(|input| input.source == terminal));
        assert!(matches!(equations.reduced.expression(roots[0]), Ok(LayerExpr::Input(_))));
    }

    #[test]
    fn source_lowering_propagates_constants_before_cycle_detection_and_ignores_dead_cycles() {
        let graph = graph();
        let monitor = TaskMonitor::default();
        let a = source(10);
        let b = source(11);
        let c = source(12);
        let outside = source(13);

        let mut equations = make_equations(&graph, &monitor);
        let b_expr = input(&mut equations.reduced, b);
        let c_expr = input(&mut equations.reduced, c);
        let a_root = equations.reduced.all([b_expr, c_expr]).unwrap();
        let a_expr = input(&mut equations.reduced, a);
        let c_root = equations.reduced.constant(false).unwrap();
        let mut source_roots: FxHashMap<_, _> = [(a, a_root), (b, a_expr), (c, c_root)].into_iter().collect();
        let dependencies: FxHashMap<_, _> = [
            (a, FxHashSet::from_iter([b, c])),
            (b, FxHashSet::from_iter([a])),
            (c, FxHashSet::default()),
        ]
        .into_iter()
        .collect();
        let mut roots = [input(&mut equations.reduced, a)];

        lower_source_equations(&mut equations, &source_roots, &dependencies, &mut roots, &monitor).unwrap();
        assert_eq!(*equations.reduced.expression(roots[0]).unwrap(), LayerExpr::Constant(false));

        let mut equations = make_equations(&graph, &monitor);
        let a_root = input(&mut equations.reduced, b);
        let b_root = input(&mut equations.reduced, a);
        let dead_root = input(&mut equations.reduced, outside);
        source_roots = [(a, a_root), (b, b_root), (outside, dead_root)].into_iter().collect();
        let dependencies: FxHashMap<_, _> = [
            (a, FxHashSet::from_iter([b])),
            (b, FxHashSet::from_iter([a])),
            (outside, FxHashSet::default()),
        ]
        .into_iter()
        .collect();
        let mut roots = [input(&mut equations.reduced, outside)];

        lower_source_equations(&mut equations, &source_roots, &dependencies, &mut roots, &monitor).unwrap();
        assert!(matches!(equations.reduced.expression(roots[0]), Ok(LayerExpr::Input(_))));
    }

    #[test]
    fn source_lowering_declines_a_live_nonconstant_cycle() {
        let graph = graph();
        let monitor = TaskMonitor::default();
        let mut equations = make_equations(&graph, &monitor);
        let a = source(21);
        let b = source(22);
        let a_root = input(&mut equations.reduced, b);
        let b_root = input(&mut equations.reduced, a);
        let source_roots: FxHashMap<_, _> = [(a, a_root), (b, b_root)].into_iter().collect();
        let dependencies: FxHashMap<_, _> = [(a, FxHashSet::from_iter([b])), (b, FxHashSet::from_iter([a]))].into_iter().collect();
        let mut roots = [input(&mut equations.reduced, a)];

        assert!(matches!(
            lower_source_equations(&mut equations, &source_roots, &dependencies, &mut roots, &monitor),
            Err(SourceLoweringError::Unsupported(_))
        ));
    }

    #[test]
    fn semantic_unsupported_category_does_not_depend_on_diagnostic_wording() {
        assert!(matches!(
            map_source_error(source(21), ConstructionError::Unsupported("any diagnostic wording".into())),
            SourceLoweringError::Unsupported(_)
        ));
        for message in [
            "missing input",
            "input is not a component",
            "resource ID is invalid",
            "logical cell destination requires an explicit transfer equation: diagnostic text",
            "unsupported logical source NodeId(7): SomeElectricalKind",
        ] {
            assert!(matches!(
                map_source_error(source(21), ConstructionError::Invalid(message.into())),
                SourceLoweringError::Fatal(_)
            ));
        }
    }

    #[test]
    fn unresolved_feedback_declines_with_a_stable_readable_reason() {
        let error = map_source_error(source(21), ConstructionError::FeedbackRequiresRegister(source(22)));
        let SourceLoweringError::Unsupported(message) = error else {
            panic!("unresolved feedback must decline logical lowering");
        };
        assert_eq!(
            message,
            "logical source equation State(ResourceEnabled(ResourceId(21))): logical feedback requires a register: State(ResourceEnabled(ResourceId(22)))"
        );
    }

    #[test]
    fn lowering_budget_is_distinct_from_malformed_expression_errors() {
        assert!(matches!(
            map_lowering_error("disjunction", ConstructionError::BudgetExceeded),
            SourceLoweringError::BudgetExceeded(_)
        ));
        assert!(matches!(
            map_lowering_error("disjunction", ConstructionError::Invalid("missing operand".into())),
            SourceLoweringError::Fatal(_)
        ));
    }
}
