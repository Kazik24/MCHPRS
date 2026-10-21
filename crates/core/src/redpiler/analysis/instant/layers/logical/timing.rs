//! Source-derived support for a retained physical clock-control island.
use super::*;
use crate::redpiler::program_graph::SettledTimingIsland;

const MAX_TIMING_ACTORS: usize = 128;
const MAX_TIMING_NODES: usize = 512;
const MAX_TIMING_INPUTS: usize = 1024;

/// Find the unique empty- or single-resource-lane piston whose base observer feeds
/// its Quasi input, then recursively trace only raw power and state dependencies.
///
/// Callback closure is deliberately excluded: the runtime keeps callback delivery
/// within this returned physical island and ignores internal datapath reset callbacks.
pub(crate) fn infer(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<SettledTimingIsland, String> {
    let observers_by_base = observers_by_base(graph, monitor)?;
    let power = index_power(graph);
    let clock = find_clock(graph, &power, &observers_by_base, monitor)?;
    let bud_resources: FxHashSet<_> = graph.plans.bud_memory_reads.iter().map(|read| read.resource).collect();
    let bud_actors: FxHashSet<_> = graph.plans.bud_memory_reads.iter().map(|read| read.storage).collect();

    let mut actors = FxHashSet::default();
    let mut nodes = FxHashSet::default();
    let mut wires = FxHashSet::default();
    let mut triggers = FxHashSet::default();
    let mut pending_actors = vec![clock];
    let mut pending_nodes = Vec::<NodeId>::new();
    let mut pending_inputs = Vec::<PowerTarget>::new();
    let mut seen_inputs = FxHashSet::default();

    let internal_wires: FxHashSet<_> = graph
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| matches!(&node.kind, CircuitNodeKind::Wire(_)).then_some(NodeId::new(index as u32)))
        .collect();
    let state = FxHashMap::default();
    let mut tracer = signals::SignalTracer::new(graph, &power, &internal_wires, &state, LayerInputRole::RequestSample);
    let mut raw = LayerBuilder::new(monitor).map_err(|error| format!("construct timing expression arena: {error:?}"))?;

    while !pending_actors.is_empty() || !pending_nodes.is_empty() || !pending_inputs.is_empty() {
        if monitor.cancelled() {
            return Err("cancelled while deriving timing-island support".into());
        }
        while let Some(actor) = pending_actors.pop() {
            if !actors.insert(actor) {
                continue;
            }
            if actors.len() > MAX_TIMING_ACTORS {
                return Err(format!("timing island exceeds {MAX_TIMING_ACTORS} piston actors"));
            }
            if bud_actors.contains(&actor) {
                return Err(format!("clock power depends on BUD storage actor {actor:?}"));
            }
            let spec = graph
                .pistons
                .get(actor.index())
                .ok_or_else(|| format!("timing island references missing piston {actor:?}"))?;
            nodes.insert(spec.node);
            pending_inputs.push(PowerTarget::Input {
                node: spec.node,
                port: InputPort::Main,
            });
            pending_inputs.push(PowerTarget::Input {
                node: spec.node,
                port: InputPort::Quasi,
            });
            if let Some(observers) = observers_by_base.get(&spec.base) {
                pending_nodes.extend(observers.iter().copied());
            }
        }

        while let Some(node) = pending_nodes.pop() {
            if !nodes.insert(node) {
                continue;
            }
            if nodes.len() > MAX_TIMING_NODES {
                return Err(format!("timing island exceeds {MAX_TIMING_NODES} graph nodes"));
            }
            match &graph
                .nodes
                .get(node.index())
                .ok_or_else(|| format!("timing island references missing node {node:?}"))?
                .kind
            {
                CircuitNodeKind::Piston(actor) => pending_actors.push(*actor),
                CircuitNodeKind::Wire(_) => {
                    wires.insert(node);
                    pending_inputs.push(PowerTarget::Input { node, port: InputPort::Main });
                }
                // Contact-state predicates are surfaced as symbolic StateTest leaves. A bare
                // contact node has no independent power output or timing state to retain.
                CircuitNodeKind::Contact(_) => {}
                CircuitNodeKind::Observer { watched, .. } | CircuitNodeKind::BuiltinReset { watched, .. } => {
                    add_site_owners(graph, *watched, &mut pending_actors, &mut pending_nodes)?;
                }
                CircuitNodeKind::Electrical(kind) => match kind {
                    NodeType::Lever => {
                        triggers.insert(node);
                    }
                    NodeType::Torch | NodeType::Repeater { .. } => {
                        for port in [InputPort::Main, InputPort::Side, InputPort::Quasi] {
                            pending_inputs.push(PowerTarget::Input { node, port });
                        }
                    }
                    other => return Err(format!("timing power cone reaches unsupported component {other:?} at {node:?}")),
                },
            }
        }

        while let Some(input) = pending_inputs.pop() {
            if !seen_inputs.insert(input) {
                continue;
            }
            if seen_inputs.len() > MAX_TIMING_INPUTS {
                return Err(format!("timing island exceeds {MAX_TIMING_INPUTS} power targets"));
            }
            let root = tracer
                .query(input, 0, &mut raw)
                .map_err(|error| format!("trace timing input {input:?}: {error:?}"))?;
            // SignalTracer memoizes wire Main targets and conductor sites while expanding
            // dust and wire paths. Keep the exact wire actors in the retained physical island.
            let queried_wires = tracer.queried_wire_nodes();
            wires.extend(queried_wires.iter().copied());
            pending_nodes.extend(queried_wires);
            for source in expression_inputs(&raw, root, monitor)? {
                match source {
                    LayerInputSource::SourceAbove { site, .. } => match graph
                        .sites
                        .get(site.index())
                        .ok_or_else(|| format!("timing source references missing site {site:?}"))?
                        .source
                    {
                        SiteSource::Component(node) => pending_nodes.push(node),
                        // Any moving-resource dependency appears as an explicit ResourceAt or
                        // ResourceEnabled guard in the same expression and is handled above.
                        SiteSource::Contact(_) => {}
                        SiteSource::Constant(_) => {}
                    },
                    LayerInputSource::State(test) | LayerInputSource::BoundaryState { test, .. } => {
                        process_state(graph, test, &bud_resources, &bud_actors, &mut pending_actors)?;
                    }
                    other => return Err(format!("timing power query produced unsupported source {other:?}")),
                }
            }
        }
    }

    if triggers.len() != 1 {
        return Err(format!(
            "timing power cone has {} external levers; expected exactly one trigger: {triggers:?}",
            triggers.len()
        ));
    }
    let trigger = *triggers.iter().next().unwrap();
    let mut actors: Vec<_> = actors.into_iter().collect();
    let mut nodes: Vec<_> = nodes.into_iter().chain(wires.iter().copied()).collect();
    actors.sort_unstable();
    nodes.sort_unstable();
    nodes.dedup();
    let actor_specs = actors
        .iter()
        .map(|&piston| {
            let spec = graph.pistons[piston.index()];
            ClockActorSpec {
                piston,
                node: spec.node,
                base: spec.base,
                near: spec.near,
                far: spec.far,
                lane: spec.lane,
                resource: match spec.lane {
                    PistonLane::SingleResource { far } => graph
                        .resources
                        .iter()
                        .enumerate()
                        .find_map(|(index, resource)| resource.contacts.contains(&far).then_some(ResourceId::new(index as u32))),
                    PistonLane::Empty => None,
                },
                sticky: spec.sticky,
            }
        })
        .collect::<Vec<_>>();
    let node_specs = nodes
        .iter()
        .filter_map(|&node| {
            let spec = &graph.nodes[node.index()];
            let (kind, watched) = match spec.kind {
                CircuitNodeKind::Electrical(NodeType::Lever) => (ClockNodeKind::Lever, None),
                CircuitNodeKind::Electrical(NodeType::Torch) => (ClockNodeKind::Torch, None),
                CircuitNodeKind::Electrical(NodeType::Repeater { .. }) => (ClockNodeKind::Repeater, None),
                CircuitNodeKind::Observer { watched, .. } => (ClockNodeKind::Observer, Some(watched)),
                CircuitNodeKind::BuiltinReset { watched, .. } => (ClockNodeKind::BuiltinReset, Some(watched)),
                _ => return None,
            };
            let watched_contact = watched.and_then(|site| match graph.sites[site.index()].source {
                SiteSource::Contact(contact) => Some(contact),
                _ => None,
            });
            Some(ClockNodeSpec { node, kind, watched_contact })
        })
        .collect::<Vec<_>>();
    Ok(SettledTimingIsland {
        clock,
        actors: actors.into_boxed_slice(),
        nodes: nodes.into_boxed_slice(),
        trigger,
        actor_specs: actor_specs.into_boxed_slice(),
        node_specs: node_specs.into_boxed_slice(),
    })
}

fn find_clock(
    graph: &CompileGraph,
    power: &PowerIndex,
    observers_by_base: &FxHashMap<ContactId, Vec<NodeId>>,
    monitor: &TaskMonitor,
) -> Result<PistonId, String> {
    let mut candidates = Vec::new();
    let internal_wires: FxHashSet<_> = graph
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| matches!(&node.kind, CircuitNodeKind::Wire(_)).then_some(NodeId::new(index as u32)))
        .collect();
    let state = FxHashMap::default();
    for (index, piston) in graph.pistons.iter().enumerate() {
        if monitor.cancelled() {
            return Err("cancelled while identifying the timing piston".into());
        }
        // Preserve the existing BUD counter's empty-lane selector. The direct
        // single-resource clock family is only considered for no-BUD frames.
        if graph.plans.bud_memory_reads.is_empty() != matches!(piston.lane, PistonLane::SingleResource { .. }) {
            continue;
        }
        let actor = PistonId::new(index as u32);
        let base_observers: Vec<_> = observers_by_base
            .get(&piston.base)
            .into_iter()
            .flatten()
            .filter(|observer| {
                matches!(
                    &graph.nodes[observer.index()].kind,
                    CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. }
                )
            })
            .copied()
            .collect();
        if base_observers.is_empty() {
            continue;
        }
        if graph.plans.bud_memory_reads.is_empty() && !direct_lever_main(graph, power, actor, monitor)? {
            continue;
        }
        // Conducted power is represented by intermediate conductor targets, so inspecting
        // only direct links misses observers behind a solid block. Resolve the actual Quasi
        // Boolean support and require this base observer to occur among its component leaves.
        let mut tracer = signals::SignalTracer::new(graph, power, &internal_wires, &state, LayerInputRole::RequestSample);
        let mut builder = LayerBuilder::new(monitor).map_err(|error| format!("construct clock selector expression arena: {error:?}"))?;
        let target = PowerTarget::Input {
            node: piston.node,
            port: InputPort::Quasi,
        };
        let root = match tracer.query(target, 0, &mut builder) {
            Ok(root) => root,
            Err(error) if unsupported_candidate_error(&error) => continue,
            Err(error) => return Err(format!("trace candidate clock Quasi {target:?}: {error:?}")),
        };
        let leaves = expression_inputs(&builder, root, monitor).map_err(|error| format!("read candidate clock Quasi support {target:?}: {error}"))?;
        let mut quasi_sources = FxHashSet::default();
        let mut local_quasi_only = true;
        for leaf in leaves {
            match leaf {
                LayerInputSource::SourceAbove { site, .. } => match graph.sites[site.index()].source {
                    SiteSource::Component(node) if base_observers.contains(&node) => {
                        quasi_sources.insert(node);
                    }
                    SiteSource::Contact(_) | SiteSource::Constant(_) => {}
                    SiteSource::Component(_) => local_quasi_only = false,
                },
                LayerInputSource::State(test) | LayerInputSource::BoundaryState { test, .. } if owns_clock_state(graph, actor, test) => {}
                _ => local_quasi_only = false,
            }
        }
        let quasi_is_observed = local_quasi_only && base_observers.iter().any(|observer| quasi_sources.contains(observer));
        if quasi_is_observed {
            candidates.push(PistonId::new(index as u32));
        }
    }
    if candidates.len() != 1 {
        return Err(format!(
            "found {} clock pistons whose base observer feeds Quasi: {candidates:?}",
            candidates.len()
        ));
    }
    Ok(candidates[0])
}

#[cfg(test)]
pub(super) fn diagnostic_candidate_support(graph: &CompileGraph, clock: PistonId, monitor: &TaskMonitor) -> Result<String, String> {
    let spec = graph.pistons.get(clock.index()).ok_or_else(|| format!("missing candidate {clock:?}"))?;
    let power = index_power(graph);
    let internal_wires: FxHashSet<_> = graph
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| matches!(&node.kind, CircuitNodeKind::Wire(_)).then_some(NodeId::new(index as u32)))
        .collect();
    let state = FxHashMap::default();
    let mut report = format!(
        "candidate={clock:?} spec={spec:?} direct_lever_main={:?}",
        direct_lever_main(graph, &power, clock, monitor)
    );
    for port in [InputPort::Main, InputPort::Quasi] {
        let mut tracer = signals::SignalTracer::new(graph, &power, &internal_wires, &state, LayerInputRole::RequestSample);
        let mut builder = LayerBuilder::new(monitor).map_err(|error| format!("construct diagnostic expression arena: {error:?}"))?;
        let target = PowerTarget::Input { node: spec.node, port };
        match tracer.query(target, 0, &mut builder) {
            Ok(root) => {
                let mut leaves = expression_inputs(&builder, root, monitor)?
                    .into_iter()
                    .map(|leaf| format!("{leaf:?}"))
                    .collect::<Vec<_>>();
                leaves.sort();
                report.push_str(&format!("\n{port:?} root={root:?} leaves={leaves:#?}"));
            }
            Err(error) => report.push_str(&format!("\n{port:?} error={error:?}")),
        }
    }
    Ok(report)
}

/// The direct-resource clock family is admitted only when Main has one external
/// lever source plus state owned by that same piston/resource. This inexpensive
/// typed check rejects datapath self-reset pistons before inspecting their Quasi cone.
fn direct_lever_main(graph: &CompileGraph, power: &PowerIndex, clock: PistonId, monitor: &TaskMonitor) -> Result<bool, String> {
    let spec = graph
        .pistons
        .get(clock.index())
        .ok_or_else(|| format!("missing candidate clock {clock:?}"))?;
    if !spec.sticky || !matches!(spec.lane, PistonLane::SingleResource { .. }) {
        return Ok(false);
    }
    let internal_wires: FxHashSet<_> = graph
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| matches!(&node.kind, CircuitNodeKind::Wire(_)).then_some(NodeId::new(index as u32)))
        .collect();
    let state = FxHashMap::default();
    let mut tracer = signals::SignalTracer::new(graph, power, &internal_wires, &state, LayerInputRole::RequestSample);
    let mut builder = LayerBuilder::new(monitor).map_err(|error| format!("construct direct-clock Main selector: {error:?}"))?;
    let target = PowerTarget::Input {
        node: spec.node,
        port: InputPort::Main,
    };
    let root = match tracer.query(target, 0, &mut builder) {
        Ok(root) => root,
        Err(error) if unsupported_candidate_error(&error) => return Ok(false),
        Err(error) => return Err(format!("trace direct-clock Main {target:?}: {error:?}")),
    };
    let leaves = expression_inputs(&builder, root, monitor)?;
    let mut triggers = FxHashSet::default();
    for leaf in leaves {
        match leaf {
            LayerInputSource::SourceAbove {
                site,
                port: PowerPort::Output | PowerPort::Strong(_) | PowerPort::Weak(_),
                threshold: 0,
            } => {
                let Some(SiteSource::Component(node)) = graph.sites.get(site.index()).map(|site| site.source) else {
                    return Ok(false);
                };
                if !matches!(
                    graph.nodes.get(node.index()).map(|node| &node.kind),
                    Some(CircuitNodeKind::Electrical(NodeType::Lever))
                ) {
                    return Ok(false);
                }
                triggers.insert(node);
            }
            LayerInputSource::State(test) | LayerInputSource::BoundaryState { test, .. } if owns_clock_state(graph, clock, test) => {}
            _ => return Ok(false),
        }
    }
    Ok(triggers.len() == 1)
}

fn owns_clock_state(graph: &CompileGraph, clock: PistonId, test: StateTest) -> bool {
    match test {
        StateTest::PistonExtended(actor) | StateTest::PistonHeadPresent(actor) | StateTest::PistonAvailability { piston: actor, .. } => {
            actor == clock
        }
        StateTest::ResourceAt { resource, contact } => {
            clock_resource(graph, clock) == Some(resource) && graph.resources[resource.index()].contacts.contains(&contact)
        }
        StateTest::ResourceEnabled(resource) => clock_resource(graph, clock) == Some(resource),
    }
}

fn clock_resource(graph: &CompileGraph, clock: PistonId) -> Option<ResourceId> {
    let PistonLane::SingleResource { far } = graph.pistons.get(clock.index())?.lane else {
        return None;
    };
    let mut resources = graph
        .resources
        .iter()
        .enumerate()
        .filter_map(|(index, resource)| resource.contacts.contains(&far).then_some(ResourceId::new(index as u32)));
    let resource = resources.next()?;
    resources.next().is_none().then_some(resource)
}

fn unsupported_candidate_error(error: &ConstructionError) -> bool {
    matches!(
        error,
        ConstructionError::Invalid(reason)
            if reason.contains("electrically active contact component")
                || reason.contains("zero-loss conductor cycle")
    )
}

fn observers_by_base(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<FxHashMap<ContactId, Vec<NodeId>>, String> {
    let mut observers = FxHashMap::<ContactId, Vec<NodeId>>::default();
    for (index, node) in graph.nodes.iter().enumerate() {
        if monitor.cancelled() {
            return Err("cancelled while indexing timing observers".into());
        }
        let (CircuitNodeKind::Observer { watched, .. } | CircuitNodeKind::BuiltinReset { watched, .. }) = &node.kind else {
            continue;
        };
        if let SiteSource::Contact(contact) = graph.sites[watched.index()].source {
            observers.entry(contact).or_default().push(NodeId::new(index as u32));
        }
    }
    Ok(observers)
}

fn process_state(
    graph: &CompileGraph,
    test: StateTest,
    bud_resources: &FxHashSet<ResourceId>,
    bud_actors: &FxHashSet<PistonId>,
    pending_actors: &mut Vec<PistonId>,
) -> Result<(), String> {
    match test {
        StateTest::ResourceAt { resource, contact } => {
            if bud_resources.contains(&resource) {
                return Err(format!("clock power reads BUD register {resource:?} at {contact:?}"));
            }
            let spec = graph
                .resources
                .get(resource.index())
                .ok_or_else(|| format!("timing power references missing resource {resource:?}"))?;
            if !spec.contacts.contains(&contact) {
                return Err(format!("timing power reads resource {resource:?} at non-member contact {contact:?}"));
            }
            enqueue_resource_owner(graph, resource, bud_actors, pending_actors)?;
        }
        StateTest::ResourceEnabled(resource) => {
            if bud_resources.contains(&resource) {
                return Err(format!("clock power reads BUD register enabled state {resource:?}"));
            }
            graph
                .resources
                .get(resource.index())
                .ok_or_else(|| format!("timing power references missing resource {resource:?}"))?;
            enqueue_resource_owner(graph, resource, bud_actors, pending_actors)?;
        }
        StateTest::PistonExtended(actor) | StateTest::PistonHeadPresent(actor) | StateTest::PistonAvailability { piston: actor, .. } => {
            if bud_actors.contains(&actor) {
                return Err(format!("clock power reads BUD storage piston state {actor:?}"));
            }
            pending_actors.push(actor);
        }
    }
    Ok(())
}

fn enqueue_resource_owner(
    graph: &CompileGraph,
    resource: ResourceId,
    bud_actors: &FxHashSet<PistonId>,
    pending_actors: &mut Vec<PistonId>,
) -> Result<(), String> {
    let contacts = &graph.resources[resource.index()].contacts;
    let owners: Vec<_> = graph
        .pistons
        .iter()
        .enumerate()
        .filter_map(|(index, piston)| match piston.lane {
            PistonLane::SingleResource { far } if contacts.contains(&far) => Some(PistonId::new(index as u32)),
            _ => None,
        })
        .collect();
    let owner = (owners.len() == 1).then(|| owners[0]);
    let Some(owner) = owner else {
        return Err(format!("resource {resource:?} has no unique timing-island piston owner"));
    };
    if bud_actors.contains(&owner) {
        return Err(format!("clock power resource {resource:?} is carried by BUD storage actor {owner:?}"));
    }
    pending_actors.push(owner);
    Ok(())
}

fn add_site_owners(graph: &CompileGraph, site: SiteId, pending_actors: &mut Vec<PistonId>, pending_nodes: &mut Vec<NodeId>) -> Result<(), String> {
    let site = graph
        .sites
        .get(site.index())
        .ok_or_else(|| format!("timing observer references missing site {site:?}"))?;
    match site.source {
        SiteSource::Component(node) => pending_nodes.push(node),
        SiteSource::Contact(contact) => {
            let owners: Vec<_> = graph
                .pistons
                .iter()
                .enumerate()
                .filter_map(|(index, piston)| (piston.base == contact).then_some(PistonId::new(index as u32)))
                .collect();
            if owners.len() != 1 {
                return Err(format!(
                    "timing observer watches contact {contact:?} with {} piston-base owners",
                    owners.len()
                ));
            }
            pending_actors.push(owners[0]);
        }
        SiteSource::Constant(_) => {}
    }
    Ok(())
}

fn expression_inputs(builder: &LayerBuilder<'_>, root: LayerExprId, monitor: &TaskMonitor) -> Result<FxHashSet<LayerInputSource>, String> {
    let mut pending = vec![root];
    let mut seen = FxHashSet::default();
    let mut inputs = FxHashSet::default();
    while let Some(id) = pending.pop() {
        if monitor.cancelled() {
            return Err("cancelled while reading timing expression support".into());
        }
        if !seen.insert(id) {
            continue;
        }
        match builder
            .expressions
            .get(id.0 as usize)
            .ok_or_else(|| format!("timing expression {id:?} is missing"))?
        {
            LayerExpr::Constant(_) => {}
            LayerExpr::Input(input) => {
                let row = builder
                    .inputs
                    .get(input.0 as usize)
                    .ok_or_else(|| format!("timing input {input:?} is missing"))?;
                inputs.insert(row.source);
            }
            LayerExpr::Not(child) => pending.push(*child),
            LayerExpr::All(children) | LayerExpr::Any(children) => pending.extend(children.iter().copied()),
        }
    }
    Ok(inputs)
}
