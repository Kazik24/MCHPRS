//! Test-only worklist diagnostic for logical source dependencies.
//! Edges precede constant propagation between source equations; a structural cycle
//! does not establish persistent state or authorize a register cut.
use super::*;
use std::collections::VecDeque;

pub(crate) fn diagnostic_source_dependency_graph(graph: &CompileGraph, targets: &[PowerTarget], monitor: &TaskMonitor) -> Result<String, String> {
    const MAX_NODES: usize = 100_000;
    const MAX_EDGES: usize = 1_000_000;
    if targets.is_empty() {
        return Err("source dependency diagnostic requires a target".into());
    }

    let mut equations = Equations::new(graph, monitor).map_err(|error| format!("{error:?}"))?;
    equations.retain_classic = true;
    equations.defer_sources = true;
    let mut target_errors = Vec::new();
    let mut target_roots = FxHashSet::default();
    for &target in targets {
        check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
        equations.deferred_roots.clear();
        equations.expanded.clear();
        match equations.query(target, 0) {
            Ok(root) => {
                let reachable = reachable_sources(&equations, root)?;
                equations.deferred_roots.retain(|source| reachable.contains(source));
                target_roots.extend(equations.deferred_roots.drain());
            }
            Err(error) => target_errors.push(format!("{target:?}: {error:?}")),
        }
    }

    let mut queue: VecDeque<_> = target_roots.iter().copied().collect();
    let mut visited = FxHashSet::default();
    let mut source_errors = Vec::new();
    while let Some(source) = queue.pop_front() {
        check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
        if !visited.insert(source) {
            continue;
        }
        if visited.len() > MAX_NODES {
            return Err(format!("source graph diagnostic exceeded {MAX_NODES} nodes"));
        }
        equations.deferred_parent = Some(source);
        // Raw tracer expressions are memoized, but each source equation must expand
        // their input leaves again so its direct dependency edges are recorded.
        equations.expanded.clear();
        match equations.source_value(source) {
            Ok(root) => {
                let reachable = reachable_sources(&equations, root)?;
                let children = equations
                    .deferred_children
                    .remove(&source)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|child| reachable.contains(child))
                    .collect::<FxHashSet<_>>();
                for child in children {
                    equations.deferred_edges.insert((source, child));
                    queue.push_back(child);
                }
            }
            Err(error) => {
                source_errors.push(format!("{source:?}: {error:?}"));
                equations.deferred_children.remove(&source);
            }
        }
        if equations.deferred_edges.len() > MAX_EDGES {
            return Err(format!("source graph diagnostic exceeded {MAX_EDGES} edges"));
        }
    }
    equations.deferred_parent = None;

    target_roots.retain(|root| visited.contains(root));
    let mut nodes: Vec<_> = visited.into_iter().collect();
    nodes.sort_unstable_by_key(|source| format!("{source:?}"));
    let mut adjacency = FxHashMap::<LayerInputSource, Vec<LayerInputSource>>::default();
    let mut indegree = FxHashMap::<LayerInputSource, usize>::default();
    for &node in &nodes {
        adjacency.entry(node).or_default();
        indegree.entry(node).or_default();
    }
    for &(parent, child) in &equations.deferred_edges {
        adjacency.entry(parent).or_default().push(child);
        *indegree.entry(child).or_default() += 1;
    }
    for children in adjacency.values_mut() {
        children.sort_unstable_by_key(|source| format!("{source:?}"));
        children.dedup();
    }

    let mut ready: VecDeque<_> = nodes.iter().copied().filter(|source| indegree[source] == 0).collect();
    let mut longest = FxHashMap::<LayerInputSource, usize>::default();
    let mut previous = FxHashMap::<LayerInputSource, LayerInputSource>::default();
    for &root in &target_roots {
        longest.insert(root, 1);
    }
    let mut longest_end = None;
    let mut processed = 0;
    while let Some(node) = ready.pop_front() {
        check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
        processed += 1;
        let depth = longest.get(&node).copied().unwrap_or(1);
        if longest_end.map_or(true, |end| depth > longest[&end]) {
            longest_end = Some(node);
        }
        for &child in &adjacency[&node] {
            let child_depth = depth + 1;
            longest
                .entry(child)
                .and_modify(|previous_depth| {
                    if child_depth > *previous_depth {
                        *previous_depth = child_depth;
                        previous.insert(child, node);
                    }
                })
                .or_insert_with(|| {
                    previous.insert(child, node);
                    child_depth
                });
            let child_indegree = indegree.get_mut(&child).expect("all dependency nodes have indegrees");
            *child_indegree -= 1;
            if *child_indegree == 0 {
                ready.push_back(child);
            }
        }
    }
    let residual_nodes: FxHashSet<_> = nodes.iter().copied().filter(|node| indegree[node] != 0).collect();
    let cycle = find_cycle(&nodes, &adjacency, &residual_nodes);
    let longest_path = longest_end.map_or_else(Vec::new, |end| recover_path(end, &previous));
    let terminals = nodes.iter().filter(|node| adjacency[*node].is_empty()).count();
    let resource_inputs = nodes
        .iter()
        .filter(|source| {
            matches!(
                source,
                LayerInputSource::State(StateTest::ResourceAt { .. } | StateTest::ResourceEnabled(_))
            )
        })
        .count();
    let component_inputs = nodes
        .iter()
        .filter(|source| matches!(source, LayerInputSource::SourceAbove { .. }))
        .count();
    let cycle_details = describe_path(graph, &equations, cycle.as_deref().unwrap_or_default());
    let longest_details = describe_path(graph, &equations, &longest_path);

    Ok(format!(
        "targets={targets:?} nodes={} edges={} terminals={terminals} resource_inputs={resource_inputs} source_above_inputs={component_inputs} longest_acyclic_path={} processed_acyclic_nodes={processed} residual_nodes={} cycle_witness={cycle_details} longest_path={longest_details} partial={} target_errors={target_errors:?} source_errors={source_errors:?}",
        nodes.len(),
        equations.deferred_edges.len(),
        longest_path.len(),
        residual_nodes.len(),
        !target_errors.is_empty() || !source_errors.is_empty(),
    ))
}

fn reachable_sources(equations: &Equations<'_>, root: LayerExprId) -> Result<FxHashSet<LayerInputSource>, String> {
    let mut sources = FxHashSet::default();
    let mut visited = FxHashSet::default();
    let mut pending = vec![root];
    while let Some(expression) = pending.pop() {
        if !visited.insert(expression) {
            continue;
        }
        let value = equations
            .reduced
            .expression(expression)
            .map_err(|error| format!("reading reduced source equation: {error:?}"))?;
        match value {
            LayerExpr::Constant(_) => {}
            LayerExpr::Input(input) => {
                let binding = equations
                    .reduced
                    .inputs
                    .get(input.0 as usize)
                    .ok_or("source equation references a missing input")?;
                sources.insert(binding.source);
            }
            LayerExpr::Not(child) => pending.push(*child),
            LayerExpr::All(children) | LayerExpr::Any(children) => pending.extend(children.iter().copied()),
        }
    }
    Ok(sources)
}

fn find_cycle(
    nodes: &[LayerInputSource],
    adjacency: &FxHashMap<LayerInputSource, Vec<LayerInputSource>>,
    allowed: &FxHashSet<LayerInputSource>,
) -> Option<Vec<LayerInputSource>> {
    let mut color = FxHashMap::<LayerInputSource, u8>::default();
    for &root in nodes {
        if !allowed.contains(&root) || color.contains_key(&root) {
            continue;
        }
        let mut stack = vec![(root, 0usize)];
        color.insert(root, 1);
        while let Some((node, child_index)) = stack.last_mut() {
            let children = adjacency.get(node).map(Vec::as_slice).unwrap_or_default();
            if *child_index == children.len() {
                color.insert(*node, 2);
                stack.pop();
                continue;
            }
            let child = children[*child_index];
            *child_index += 1;
            if !allowed.contains(&child) {
                continue;
            }
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

fn recover_path(end: LayerInputSource, previous: &FxHashMap<LayerInputSource, LayerInputSource>) -> Vec<LayerInputSource> {
    let mut reversed = vec![end];
    let mut cursor = end;
    while let Some(parent) = previous.get(&cursor).copied() {
        if reversed.contains(&parent) {
            break;
        }
        reversed.push(parent);
        cursor = parent;
    }
    reversed.reverse();
    reversed
}

fn source_context(graph: &CompileGraph, equations: &Equations<'_>, source: LayerInputSource) -> String {
    match source {
        LayerInputSource::State(StateTest::ResourceAt { resource, contact }) => {
            let cell_owners = equations.cells.get(&resource).map(|(_, owners)| owners.as_slice()).unwrap_or_default();
            let owners = cell_owners
                .iter()
                .map(|owner| {
                    let piston = graph.pistons[owner.index()];
                    let reset = graph
                        .plans
                        .instant
                        .boundaries
                        .as_ref()
                        .into_iter()
                        .flat_map(|catalog| catalog.cells.iter())
                        .filter(|cell| cell.resources.iter().any(|pin| pin.resource == resource))
                        .flat_map(|cell| cell.uses.iter())
                        .filter(|use_| use_.actor == *owner)
                        .map(|use_| match use_.reset.observer() {
                            Some(observer) => format!("observer={observer:?}@{:?}", graph.nodes[observer.index()].position),
                            None => format!("reset={:?}", use_.reset),
                        })
                        .collect::<Vec<_>>();
                    format!("{owner:?}@{:?}:{reset:?}", piston.position)
                })
                .collect::<Vec<_>>();
            format!("ResourceAt({resource:?},{contact:?},owners={owners:?})")
        }
        LayerInputSource::SourceAbove { site, port, threshold } => {
            let spec = &graph.sites[site.index()];
            match spec.source {
                SiteSource::Component(node) => format!(
                    "SourceAbove({site:?},{port:?},>{threshold},node={node:?}@{:?}:{:?})",
                    graph.nodes[node.index()].position,
                    graph.nodes[node.index()].kind
                ),
                source => format!("SourceAbove({site:?},{port:?},>{threshold},{source:?})"),
            }
        }
        _ => format!("{source:?}"),
    }
}

fn describe_path(graph: &CompileGraph, equations: &Equations<'_>, path: &[LayerInputSource]) -> String {
    const SAMPLE: usize = 48;
    let head = path
        .iter()
        .take(SAMPLE)
        .map(|source| source_context(graph, equations, *source))
        .collect::<Vec<_>>();
    let tail = if path.len() > SAMPLE {
        path.iter()
            .skip(path.len() - SAMPLE)
            .map(|source| source_context(graph, equations, *source))
            .collect()
    } else {
        Vec::new()
    };
    format!("len={} head={head:?} tail={tail:?}", path.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_witness_is_closed_and_uses_real_dependency_edges() {
        let nodes = [0, 1, 2].map(|index| LayerInputSource::State(StateTest::ResourceEnabled(ResourceId::new(index))));
        let allowed: FxHashSet<_> = nodes.into_iter().collect();
        let mut adjacency = FxHashMap::default();
        adjacency.insert(nodes[0], vec![nodes[1]]);
        adjacency.insert(nodes[1], vec![nodes[2]]);
        assert!(find_cycle(&nodes, &adjacency, &allowed).is_none());

        adjacency.insert(nodes[2], vec![nodes[0]]);
        let cycle = find_cycle(&nodes, &adjacency, &allowed).expect("backedge closes the cycle");
        assert!(cycle.len() >= 2);
        assert_eq!(cycle.first(), cycle.last());
        assert!(cycle.windows(2).all(|edge| adjacency[&edge[0]].contains(&edge[1])));
    }
}
