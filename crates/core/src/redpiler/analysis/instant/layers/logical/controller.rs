//! Bounded source-support candidate for a retained physical timing controller.
//!
//! This diagnostic traces power and callback sources without the settled
//! Near-disabled transfer assumption. Its result is not permission to bypass
//! GraphBackend: temporal and full callback closure still need validation.
use super::*;
use crate::redpiler::components::NodeType;
use rustc_hash::FxHashSet;

const MAX_CONTROLLER_ACTORS: usize = 256;
const MAX_CONTROLLER_NODES: usize = 512;
const MAX_CONTROLLER_INPUTS: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ControllerSupport {
    /// Piston actors whose phase or resource state is part of the controller.
    pub(crate) actors: Box<[PistonId]>,
    /// Power-producing, observer, and reset nodes in the closed source cone.
    pub(crate) nodes: Box<[NodeId]>,
    /// Movable resources whose state guards participate in controller power.
    pub(crate) resources: Box<[ResourceId]>,
    /// Wire nodes returned by SignalTracer after its conductor queries.
    pub(crate) wires: Box<[NodeId]>,
    /// Declared external lever inputs; callers must bind these at runtime.
    pub(crate) external_inputs: Box<[NodeId]>,
}

/// Derive a bounded candidate power/notification support for a clock actor.
///
/// The walk recursively expands piston resource and pose guards, power-producing
/// components, and every callback source that can notify an observer or built-in
/// reset in the cone. It declines as soon as BUD storage or an unsupported source
/// enters the support. This is a diagnostic candidate, not a temporal certificate.
/// No data-word enumeration or timing-period assumption is used.
pub(crate) fn controller_support(graph: &CompileGraph, clock: PistonId, monitor: &TaskMonitor) -> Result<ControllerSupport, String> {
    let equations = Equations::new(graph, monitor).map_err(|error| format!("construct controller tracer: {error:?}"))?;
    let bud_resources: FxHashSet<_> = graph.plans.bud_memory_reads.iter().map(|read| read.resource).collect();
    let bud_actors: FxHashSet<_> = graph.plans.bud_memory_reads.iter().map(|read| read.storage).collect();
    let mut actors = FxHashSet::default();
    let mut nodes = FxHashSet::default();
    let mut wires = FxHashSet::default();
    let mut resources = FxHashSet::default();
    let mut external_inputs = FxHashSet::default();
    let mut pending_actors = vec![clock];
    let mut pending_nodes = Vec::<NodeId>::new();
    let mut pending_inputs = Vec::<PowerTarget>::new();
    let mut seen_inputs = FxHashSet::default();
    let state = FxHashMap::default();
    let mut tracer = signals::SignalTracer::new(graph, &equations.power, &equations.wires, &state, LayerInputRole::RequestSample);
    let mut raw = LayerBuilder::new(monitor).map_err(|error| format!("construct controller expression arena: {error:?}"))?;

    while !pending_actors.is_empty() || !pending_nodes.is_empty() || !pending_inputs.is_empty() {
        if monitor.cancelled() {
            return Err("cancelled while proving controller source closure".into());
        }
        while let Some(actor) = pending_actors.pop() {
            if !actors.insert(actor) {
                continue;
            }
            if actors.len() > MAX_CONTROLLER_ACTORS {
                return Err(format!("controller closure exceeds {MAX_CONTROLLER_ACTORS} piston actors"));
            }
            if bud_actors.contains(&actor) {
                let register = graph.plans.bud_memory_reads.iter().find(|read| read.storage == actor).unwrap();
                return Err(format!(
                    "controller reaches BUD storage piston {actor:?} through resource {:?}",
                    register.resource
                ));
            }
            let spec = graph
                .pistons
                .get(actor.index())
                .ok_or_else(|| format!("controller references missing piston {actor:?}"))?;
            nodes.insert(spec.node);
            pending_inputs.push(PowerTarget::Input {
                node: spec.node,
                port: InputPort::Main,
            });
            pending_inputs.push(PowerTarget::Input {
                node: spec.node,
                port: InputPort::Quasi,
            });
            for receiver in [CallbackReceiver::PistonBase(actor), CallbackReceiver::PistonHead(actor)]
                .into_iter()
                .chain([spec.base, spec.near, spec.far].map(CallbackReceiver::Contact))
            {
                add_callback_sources(
                    graph,
                    receiver,
                    &bud_resources,
                    &bud_actors,
                    &mut resources,
                    &mut pending_nodes,
                    &mut pending_actors,
                    monitor,
                )?;
            }
        }

        while let Some(node) = pending_nodes.pop() {
            if !nodes.insert(node) {
                continue;
            }
            if nodes.len() > MAX_CONTROLLER_NODES {
                return Err(format!("controller closure exceeds {MAX_CONTROLLER_NODES} graph nodes"));
            }
            match &graph
                .nodes
                .get(node.index())
                .ok_or_else(|| format!("controller references missing node {node:?}"))?
                .kind
            {
                CircuitNodeKind::Piston(actor) => pending_actors.push(*actor),
                CircuitNodeKind::Wire(_) => {
                    wires.insert(node);
                    add_callback_sources(
                        graph,
                        CallbackReceiver::Component(node),
                        &bud_resources,
                        &bud_actors,
                        &mut resources,
                        &mut pending_nodes,
                        &mut pending_actors,
                        monitor,
                    )?;
                    // Query the retained wire actor through its original Main input;
                    // avoid treating all geometrical neighbors as live dependencies.
                    pending_inputs.push(PowerTarget::Input { node, port: InputPort::Main });
                }
                CircuitNodeKind::Contact(contact) => {
                    add_contact_owners(graph, *contact, &mut pending_actors)?;
                    add_callback_sources(
                        graph,
                        CallbackReceiver::Contact(*contact),
                        &bud_resources,
                        &bud_actors,
                        &mut resources,
                        &mut pending_nodes,
                        &mut pending_actors,
                        monitor,
                    )?;
                }
                CircuitNodeKind::Observer { watched, .. } | CircuitNodeKind::BuiltinReset { watched, .. } => {
                    add_site_owners(graph, *watched, &mut pending_actors, &mut pending_nodes)?;
                    add_callback_sources(
                        graph,
                        CallbackReceiver::Component(node),
                        &bud_resources,
                        &bud_actors,
                        &mut resources,
                        &mut pending_nodes,
                        &mut pending_actors,
                        monitor,
                    )?;
                }
                CircuitNodeKind::Electrical(kind) => match kind {
                    NodeType::Lever => {
                        external_inputs.insert(node);
                    }
                    NodeType::Torch | NodeType::Repeater { .. } => {
                        add_callback_sources(
                            graph,
                            CallbackReceiver::Component(node),
                            &bud_resources,
                            &bud_actors,
                            &mut resources,
                            &mut pending_nodes,
                            &mut pending_actors,
                            monitor,
                        )?;
                        for port in [InputPort::Main, InputPort::Side, InputPort::Quasi] {
                            pending_inputs.push(PowerTarget::Input { node, port });
                        }
                    }
                    other => return Err(format!("controller reaches unsupported electrical source {other:?} at {node:?}")),
                },
            }
        }

        while let Some(input) = pending_inputs.pop() {
            if !seen_inputs.insert(input) {
                continue;
            }
            if seen_inputs.len() > MAX_CONTROLLER_INPUTS {
                return Err(format!("controller closure exceeds {MAX_CONTROLLER_INPUTS} power inputs"));
            }
            let root = tracer
                .query(input, 0, &mut raw)
                .map_err(|error| format!("trace controller power target {input:?}: {error:?}"))?;
            pending_nodes.extend(tracer.queried_wire_nodes().into_iter().filter(|node| !nodes.contains(node)));
            let leaves = expression_inputs(&raw, root, monitor)?;
            for source in leaves {
                match source {
                    LayerInputSource::SourceAbove { site, .. } => {
                        match graph
                            .sites
                            .get(site.index())
                            .ok_or_else(|| format!("controller source references missing site {site:?}"))?
                            .source
                        {
                            SiteSource::Component(node) => pending_nodes.push(node),
                            SiteSource::Contact(contact) => add_contact_owners(graph, contact, &mut pending_actors)?,
                            SiteSource::Constant(_) => {}
                        }
                    }
                    LayerInputSource::WireDot { node } => pending_nodes.push(node),
                    LayerInputSource::State(test) | LayerInputSource::BoundaryState { test, .. } => {
                        process_state(graph, test, &bud_resources, &bud_actors, &mut resources, &mut pending_actors)?;
                    }
                    LayerInputSource::BoundaryEvent { .. }
                    | LayerInputSource::BoundaryRequest { .. }
                    | LayerInputSource::InstantFired { .. }
                    | LayerInputSource::PowerContribution { .. }
                    | LayerInputSource::LocalResetOutput { .. } => {
                        return Err(format!("controller power query produced unsupported boundary source {source:?}"));
                    }
                }
            }
        }
    }

    let mut actors: Vec<_> = actors.into_iter().collect();
    let mut nodes: Vec<_> = nodes.into_iter().collect();
    let mut wires: Vec<_> = wires.into_iter().collect();
    let mut resources: Vec<_> = resources.into_iter().collect();
    let mut external_inputs: Vec<_> = external_inputs.into_iter().collect();
    actors.sort_unstable();
    nodes.sort_unstable();
    wires.sort_unstable();
    resources.sort_unstable();
    external_inputs.sort_unstable();
    Ok(ControllerSupport {
        actors: actors.into_boxed_slice(),
        nodes: nodes.into_boxed_slice(),
        wires: wires.into_boxed_slice(),
        resources: resources.into_boxed_slice(),
        external_inputs: external_inputs.into_boxed_slice(),
    })
}

fn expression_inputs(builder: &LayerBuilder<'_>, root: LayerExprId, monitor: &TaskMonitor) -> Result<FxHashSet<LayerInputSource>, String> {
    let mut pending = vec![root];
    let mut seen = FxHashSet::default();
    let mut inputs = FxHashSet::default();
    while let Some(id) = pending.pop() {
        if monitor.cancelled() {
            return Err("cancelled while reading controller expression support".into());
        }
        if !seen.insert(id) {
            continue;
        }
        match builder
            .expressions
            .get(id.0 as usize)
            .ok_or_else(|| format!("controller expression {id:?} is missing"))?
        {
            LayerExpr::Constant(_) => {}
            LayerExpr::Input(input) => {
                let row = builder
                    .inputs
                    .get(input.0 as usize)
                    .ok_or_else(|| format!("controller input {input:?} is missing"))?;
                inputs.insert(row.source);
            }
            LayerExpr::Not(child) => pending.push(*child),
            LayerExpr::All(children) | LayerExpr::Any(children) => pending.extend(children.iter().copied()),
        }
    }
    Ok(inputs)
}

fn process_state(
    graph: &CompileGraph,
    test: StateTest,
    bud_resources: &FxHashSet<ResourceId>,
    bud_actors: &FxHashSet<PistonId>,
    resources: &mut FxHashSet<ResourceId>,
    pending_actors: &mut Vec<PistonId>,
) -> Result<(), String> {
    match test {
        StateTest::ResourceAt { resource, contact } => {
            if bud_resources.contains(&resource) {
                return Err(format!("controller reads BUD register resource {resource:?} at {contact:?}"));
            }
            let spec = graph
                .resources
                .get(resource.index())
                .ok_or_else(|| format!("controller references missing resource {resource:?}"))?;
            if !spec.contacts.contains(&contact) {
                return Err(format!("controller reads resource {resource:?} at non-member contact {contact:?}"));
            }
            resources.insert(resource);
            enqueue_resource_owner(graph, resource, bud_actors, pending_actors)?;
        }
        StateTest::ResourceEnabled(resource) => {
            if bud_resources.contains(&resource) {
                return Err(format!("controller reads BUD register resource-enabled state {resource:?}"));
            }
            graph
                .resources
                .get(resource.index())
                .ok_or_else(|| format!("controller references missing resource {resource:?}"))?;
            resources.insert(resource);
            enqueue_resource_owner(graph, resource, bud_actors, pending_actors)?;
        }
        StateTest::PistonExtended(actor) | StateTest::PistonHeadPresent(actor) | StateTest::PistonAvailability { piston: actor, .. } => {
            if bud_actors.contains(&actor) {
                return Err(format!("controller reads BUD storage piston state {actor:?}"));
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
    if owners.len() != 1 {
        return Err(format!(
            "resource {resource:?} has {} possible timing-controller lane owners: {owners:?}",
            owners.len()
        ));
    }
    let owner = owners[0];
    if bud_actors.contains(&owner) {
        return Err(format!("controller resource {resource:?} is stored by BUD piston {owner:?}"));
    }
    pending_actors.push(owner);
    Ok(())
}

fn add_contact_owners(graph: &CompileGraph, contact: ContactId, pending_actors: &mut Vec<PistonId>) -> Result<(), String> {
    let mut found = false;
    for (index, piston) in graph.pistons.iter().enumerate() {
        if [piston.base, piston.near, piston.far].contains(&contact) {
            pending_actors.push(PistonId::new(index as u32));
            found = true;
        }
    }
    if !found {
        let node = graph
            .contacts
            .get(contact.index())
            .ok_or_else(|| format!("controller references missing contact {contact:?}"))?
            .node;
        if let CircuitNodeKind::Piston(actor) = &graph.nodes[node.index()].kind {
            pending_actors.push(*actor);
        }
    }
    Ok(())
}

fn add_site_owners(graph: &CompileGraph, site: SiteId, pending_actors: &mut Vec<PistonId>, pending_nodes: &mut Vec<NodeId>) -> Result<(), String> {
    match graph
        .sites
        .get(site.index())
        .ok_or_else(|| format!("controller references missing site {site:?}"))?
        .source
    {
        SiteSource::Component(node) => pending_nodes.push(node),
        SiteSource::Contact(contact) => add_contact_owners(graph, contact, pending_actors)?,
        SiteSource::Constant(_) => {}
    }
    Ok(())
}

fn add_callback_sources(
    graph: &CompileGraph,
    receiver: CallbackReceiver,
    bud_resources: &FxHashSet<ResourceId>,
    bud_actors: &FxHashSet<PistonId>,
    resources: &mut FxHashSet<ResourceId>,
    pending_nodes: &mut Vec<NodeId>,
    pending_actors: &mut Vec<PistonId>,
    monitor: &TaskMonitor,
) -> Result<(), String> {
    for route in &graph.routes {
        if monitor.cancelled() {
            return Err("cancelled while proving controller callback closure".into());
        }
        for delivery in route
            .deliveries
            .iter()
            .filter(|delivery| delivery_matches_receiver(graph, delivery.receiver, delivery.target, receiver))
        {
            pending_nodes.push(route.source);
            let mut guards = vec![delivery.guard];
            let mut seen = FxHashSet::default();
            while let Some(guard) = guards.pop() {
                if monitor.cancelled() {
                    return Err("cancelled while proving controller callback guards".into());
                }
                if !seen.insert(guard) {
                    continue;
                }
                match graph
                    .guards
                    .get(guard.index())
                    .ok_or_else(|| format!("controller callback references missing guard {guard:?}"))?
                {
                    Guard::Always => {}
                    Guard::State(test) => process_state(graph, *test, bud_resources, bud_actors, resources, pending_actors)?,
                    Guard::Site { site, .. } => add_site_owners(graph, *site, pending_actors, pending_nodes)?,
                    Guard::Not(child) => guards.push(*child),
                    Guard::All(children) | Guard::Any(children) => guards.extend(children.iter().copied()),
                }
            }
        }
    }
    Ok(())
}

fn delivery_matches_receiver(graph: &CompileGraph, actual: Option<CallbackReceiver>, target: SiteId, expected: CallbackReceiver) -> bool {
    if actual == Some(expected) {
        return true;
    }
    if actual.is_some() {
        return false;
    }
    let Some(site) = graph.sites.get(target.index()) else {
        return false;
    };
    match (expected, site.source) {
        (CallbackReceiver::Component(node), SiteSource::Component(target)) => node == target,
        (CallbackReceiver::Contact(contact), SiteSource::Contact(target)) => contact == target,
        (CallbackReceiver::PistonBase(piston), SiteSource::Contact(target)) => graph.pistons[piston.index()].base == target,
        (CallbackReceiver::PistonHead(piston), SiteSource::Contact(target)) => graph.pistons[piston.index()].near == target,
        _ => false,
    }
}
