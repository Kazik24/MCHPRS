//! First compiler from prepared discovery facts into the independent event runtime.
use super::program::{
    Component, ComponentKind, ComponentState as EventComponentState, ElectricalLink, EventCondition, EventContactVariant, EventPiston, EventProfile,
    EventProgram, EventResource, Operation, PistonEventPhase, PistonOperation, ResourceEventPhase, ResourceInstruction, ResourceOperation,
};
use crate::redpiler::components::NodeType;
use crate::redpiler::program_graph::{
    CircuitNodeKind, ComponentState, Guard, InputPort, LoweredCircuit, NodeId, PistonAvailability, PowerPort, PowerTarget, ProfileElectrical,
    SiteSource, SiteTest, StateTest,
};
use mchprs_blocks::{BlockPos, blocks::Block};
use std::sync::Arc;

pub(crate) struct CompiledEvent {
    pub(crate) program: Arc<EventProgram>,
    pub(crate) positions: rustc_hash::FxHashMap<BlockPos, NodeId>,
}

#[derive(Debug)]
pub(crate) struct CompileError {
    pub(crate) position: Option<BlockPos>,
    pub(crate) reason: String,
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.position {
            Some(position) => write!(f, "unsupported event dependency at {position:?}: {}", self.reason),
            None => write!(f, "unsupported event dependency: {}", self.reason),
        }
    }
}

pub(crate) fn compile(lowered: &LoweredCircuit) -> Result<CompiledEvent, CompileError> {
    compile_inner(lowered, None, None)
}

// shortcut: one piston and static geometry only; upgrade when movement is modeled.
pub(super) fn compile_with_wire_discovery(
    lowered: &LoweredCircuit,
    discovery: &super::scratch::WireDiscovery,
) -> Result<CompiledEvent, CompileError> {
    compile_inner(lowered, Some(discovery), None)
}

pub(super) struct ResourceDiscovery<'a> {
    pub(super) arrangements: &'a super::scratch::ResourceWireArrangements,
    pub(super) actors: &'a super::scratch::PistonResourceMap,
    pub(super) receiver: BlockPos,
    pub(super) departure_far: &'a [BlockPos],
    pub(super) arrival_far: &'a [BlockPos],
    pub(super) piston_notifications: &'a [crate::world::PistonNotificationObservation],
}

pub(super) fn compile_with_resource_discovery(lowered: &LoweredCircuit, discovery: &ResourceDiscovery<'_>) -> Result<CompiledEvent, CompileError> {
    compile_inner(lowered, Some(&discovery.arrangements.at_far), Some(discovery))
}

fn compile_inner(
    lowered: &LoweredCircuit,
    discovery: Option<&super::scratch::WireDiscovery>,
    movement: Option<&ResourceDiscovery<'_>>,
) -> Result<CompiledEvent, CompileError> {
    let graph = &lowered.program;
    let mut target_positions = graph.position_index();
    for piston in graph.pistons() {
        if target_positions.insert(piston.position, piston.node).is_some() {
            return Err(CompileError {
                position: Some(piston.position),
                reason: "piston base collides with another executable target".into(),
            });
        }
    }
    if let Some(tick) = lowered.imported_ticks.first() {
        return Err(CompileError {
            position: Some(tick.position),
            reason: "pending entry ticks are not represented".into(),
        });
    }
    let at = |node: NodeId| graph.nodes()[node.index()].position;
    for (index, piston) in graph.pistons().iter().enumerate() {
        let state = lowered.state.pistons[index];
        if state.availability != PistonAvailability::Ready || state.extended != state.head_present {
            return Err(CompileError {
                position: Some(piston.position),
                reason: "initial piston motion/head state is not represented".into(),
            });
        }
    }
    let mut components = Vec::with_capacity(graph.nodes().len());
    for (index, node) in graph.nodes().iter().enumerate() {
        if matches!(lowered.state.components.get(index), Some(ComponentState::Electrical(state)) if state.pending_tick) {
            return Err(CompileError {
                position: node.position,
                reason: "pending component work is not represented".into(),
            });
        }
        let mut state = match lowered.state.components.get(index) {
            Some(ComponentState::Electrical(state)) => EventComponentState {
                output: state.output_strength,
                powered: state.powered,
            },
            Some(ComponentState::Wire(wire)) => EventComponentState {
                output: wire.power,
                powered: wire.power > 0,
            },
            _ => EventComponentState::default(),
        };
        let kind = match &node.kind {
            CircuitNodeKind::Observer { .. } => ComponentKind::Observer,
            CircuitNodeKind::Piston(_) => ComponentKind::Piston,
            CircuitNodeKind::Electrical(NodeType::Lamp) => ComponentKind::Lamp,
            CircuitNodeKind::Electrical(NodeType::Torch) => ComponentKind::Torch,
            CircuitNodeKind::Electrical(NodeType::Repeater { delay, .. }) => ComponentKind::Repeater { delay: *delay },
            CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Constant) => ComponentKind::Input,
            CircuitNodeKind::Electrical(NodeType::Wire) | CircuitNodeKind::Wire(_) => ComponentKind::Wire,
            CircuitNodeKind::Electrical(NodeType::Comparator { .. }) => {
                return Err(CompileError {
                    position: node.position,
                    reason: "comparator input/output behavior is not compiled".into(),
                });
            }
            CircuitNodeKind::BuiltinReset { .. } => {
                return Err(CompileError {
                    position: node.position,
                    reason: "builtin reset timing is not compiled".into(),
                });
            }
            CircuitNodeKind::Contact(_) => ComponentKind::Input,
            CircuitNodeKind::Electrical(kind) => {
                return Err(CompileError {
                    position: node.position,
                    reason: format!("{kind:?} component behavior is not compiled"),
                });
            }
        };
        if kind == ComponentKind::Input {
            state.powered = state.output > 0;
        }
        components.push(Component { kind, initial: state });
    }

    let mut links = Vec::new();
    let mut operations = Vec::new();
    let mut resource_operations = Vec::new();
    let mut piston_operations = Vec::new();
    let explicit_receiver = movement.and_then(|movement| target_positions.get(&movement.receiver).copied());
    for (target_index, _node) in graph.nodes().iter().enumerate() {
        if explicit_receiver.is_some_and(|receiver| receiver.index() == target_index) {
            continue;
        }
        if !matches!(
            components[target_index].kind,
            ComponentKind::Lamp | ComponentKind::Piston | ComponentKind::Repeater { .. } | ComponentKind::Torch
        ) {
            continue;
        }
        let target = NodeId::new(target_index as u32);
        let ports: &[InputPort] = if components[target_index].kind == ComponentKind::Piston {
            &[InputPort::Main, InputPort::Quasi]
        } else {
            &[InputPort::Main]
        };
        for &port in ports {
            let mut leaves = Vec::new();
            expand_power(graph, PowerTarget::Input { node: target, port }, 0, None, &mut Vec::new(), &mut leaves).map_err(|reason| CompileError {
                position: at(target),
                reason,
            })?;
            for leaf in leaves {
                let source = match leaf.source {
                    LeafSource::Node(source) => source,
                    LeafSource::Constant(strength) => {
                        let source = NodeId::new(components.len() as u32);
                        components.push(Component {
                            kind: ComponentKind::Input,
                            initial: EventComponentState {
                                output: strength,
                                powered: strength > 0,
                            },
                        });
                        source
                    }
                };
                if !matches!(
                    components[source.index()].kind,
                    ComponentKind::Input | ComponentKind::Wire | ComponentKind::Observer | ComponentKind::Repeater { .. } | ComponentKind::Torch
                ) {
                    return Err(CompileError {
                        position: at(source),
                        reason: "electrical dependency has an unsupported source".into(),
                    });
                }
                let id = links.len();
                links.push(ElectricalLink {
                    source,
                    target,
                    port,
                    attenuation: leaf.attenuation,
                    condition: leaf.condition,
                });
                operations.push((source, Operation::Contribution(id)));
                if port != InputPort::Quasi {
                    operations.push((source, Operation::Notify(target)));
                }
            }
        }
    }

    let resources: Vec<_> = graph
        .resources()
        .iter()
        .enumerate()
        .map(|(index, resource)| EventResource {
            contacts: resource.contacts.clone(),
            initial_at: lowered.state.resources[index].at,
        })
        .collect();
    let pistons: Vec<_> = graph
        .pistons()
        .iter()
        .enumerate()
        .map(|(index, piston)| EventPiston {
            input: piston.node,
            resource: graph.resource_for_piston(crate::redpiler::program_graph::PistonId::new(index as u32)),
            near: piston.near,
            far: piston.far,
            sticky: piston.sticky,
            initial_extended: lowered.state.pistons[index].extended,
            initial_head_present: lowered.state.pistons[index].head_present,
        })
        .collect();
    let mut wire_watchers = Vec::new();
    if let Some(discovery) = discovery {
        // shortcut: movable resources can change discovered geometry; admit them only after rediscovery is modeled.
        if (movement.is_none() && (pistons.len() != 1 || !graph.resources().is_empty())) || (movement.is_some() && graph.resources().len() != 1) {
            return Err(CompileError {
                position: None,
                reason: "wire discovery requires one piston and static geometry".into(),
            });
        }
        if let Some(movement) = movement {
            let a = movement.arrangements;
            let Some(resource_fact) = movement
                .actors
                .resources
                .iter()
                .find(|resource| resource.contacts.contains(&a.receiver[1].source))
            else {
                return Err(CompileError {
                    position: Some(movement.receiver),
                    reason: "receiver contact has no discovered movable resource".into(),
                });
            };
            let [actor] = resource_fact.acting_pistons.as_slice() else {
                return Err(CompileError {
                    position: Some(movement.receiver),
                    reason: "resource actor group is empty or shared by multiple pistons".into(),
                });
            };
            let piston = &pistons[actor.index()];
            let resource = resource_fact.id;
            let Some(resource_contact) = graph
                .contacts()
                .iter()
                .position(|contact| contact.position == a.receiver[1].source)
                .map(|index| crate::redpiler::program_graph::ContactId::new(index as u32))
            else {
                return Err(CompileError {
                    position: Some(movement.receiver),
                    reason: "discovered resource receiver contact is not compiled".into(),
                });
            };
            let receiver = explicit_receiver.ok_or_else(|| CompileError {
                position: Some(movement.receiver),
                reason: "discovered repeater missing from graph".into(),
            })?;
            let unsupported = [
                (
                    graph.resources()[resource.index()].contacts.len() != 2,
                    "resource contact count is not represented",
                ),
                (!lowered.state.resources[resource.index()].enabled, "initial resource is disabled"),
                (piston.resource != Some(resource), "resource actor does not own the graph resource"),
                (
                    resource_fact.material != (Block::RedstoneBlock {}),
                    "resource material is not a redstone block",
                ),
                (
                    resource_fact.initial_contact != a.receiver[1].source,
                    "resource initial contact differs from discovered receiver source",
                ),
                (resource_fact.contacts.len() != 2, "resource arrangement count is not represented"),
                (!piston.sticky, "resource actor is not sticky"),
                (
                    components[receiver.index()].kind != (ComponentKind::Repeater { delay: 2 }),
                    "receiver is not a delay-2 repeater",
                ),
                (
                    a.at_far != a.at_near || a.at_far != a.in_motion,
                    "wire arrangement changes across resource contacts",
                ),
                (
                    a.receiver.map(|input| input.strength) != [0, 15, 0],
                    "receiver strengths do not match near/far/moving",
                ),
                (
                    a.receiver
                        .iter()
                        .any(|input| input.source != a.receiver[1].source || input.attenuation != 0),
                    "receiver source or attenuation differs across arrangements",
                ),
                (
                    a.receiver.iter().any(|input| input.direction != a.receiver[1].direction),
                    "receiver direction differs across arrangements",
                ),
                (
                    links.iter().any(|link| link.condition.as_ref().is_some_and(uses_resource)),
                    "other graph links depend on the movable resource",
                ),
            ]
            .into_iter()
            .find_map(|(condition, reason)| condition.then_some(reason));
            if let Some(reason) = unsupported {
                return Err(CompileError {
                    position: Some(movement.receiver),
                    reason: reason.into(),
                });
            }
            let source = NodeId::new(components.len() as u32);
            components.push(Component {
                kind: ComponentKind::Input,
                initial: EventComponentState { output: 15, powered: true },
            });
            let id = links.len();
            links.push(ElectricalLink {
                source,
                target: receiver,
                port: InputPort::Main,
                attenuation: a.receiver[1].attenuation,
                condition: Some(EventCondition::All(
                    vec![
                        EventCondition::ResourceAt {
                            resource,
                            contact: resource_contact,
                        },
                        EventCondition::ResourceEnabled(resource),
                    ]
                    .into_boxed_slice(),
                )),
            });
            operations.push((source, Operation::Contribution(id)));
            let positions = graph.position_index();
            for (phase, observed) in [
                (ResourceEventPhase::Departure, movement.departure_far),
                (ResourceEventPhase::Arrival, movement.arrival_far),
            ] {
                for &position in observed {
                    let target = positions.get(&position).copied().ok_or_else(|| CompileError {
                        position: Some(position),
                        reason: "movement watcher is not represented in the graph".into(),
                    })?;
                    let instruction = match components[target.index()].kind {
                        ComponentKind::Observer => ResourceInstruction::NotifyObserver { target, shape: true },
                        ComponentKind::Lamp | ComponentKind::Piston | ComponentKind::Repeater { .. } | ComponentKind::Torch => {
                            ResourceInstruction::Notify(target)
                        }
                        _ => {
                            return Err(CompileError {
                                position: Some(position),
                                reason: "unsupported movement watcher receiver".into(),
                            });
                        }
                    };
                    resource_operations.push(ResourceOperation {
                        resource,
                        contact: resource_contact,
                        phase,
                        instruction,
                    });
                }
            }
            for observation in movement.piston_notifications {
                let (actor_position, action, phase) = match (observation.event, observation.motion) {
                    (Some(event), None) if observation.phase == mchprs_world::AdvancePhase::PistonEvents => {
                        (event.pos, event.action, PistonEventPhase::Mutation)
                    }
                    (None, Some(motion)) if observation.phase == mchprs_world::AdvancePhase::MovingEntities => {
                        if motion.part != observation.source {
                            return Err(CompileError {
                                position: Some(observation.source),
                                reason: "completion callback does not match its recorded motion part".into(),
                            });
                        }
                        (motion.actor, motion.action, PistonEventPhase::Completion)
                    }
                    (None, None) => continue,
                    _ => {
                        return Err(CompileError {
                            position: Some(observation.source),
                            reason: "piston callback phase lacks matching motion provenance".into(),
                        });
                    }
                };
                if let Some(event) = observation.event.filter(|_| phase == PistonEventPhase::Mutation) {
                    if event.pos != actor_position || event.action != action {
                        return Err(CompileError {
                            position: Some(observation.source),
                            reason: "piston mutation callback provenance is inconsistent".into(),
                        });
                    }
                }
                if observation.phase == mchprs_world::AdvancePhase::MovingEntities && observation.motion.is_none() {
                    return Err(CompileError {
                        position: Some(observation.source),
                        reason: "piston completion callback has no motion provenance".into(),
                    });
                }
                let Some(piston) = graph.pistons().iter().position(|piston| piston.position == actor_position) else {
                    return Err(CompileError {
                        position: Some(actor_position),
                        reason: "scratch piston notification actor is not represented".into(),
                    });
                };
                let Some(target) = target_positions.get(&observation.target).copied() else {
                    if matches!(
                        observation.target_block,
                        Block::Piston { .. }
                            | Block::RedstoneWire { .. }
                            | Block::RedstoneRepeater { .. }
                            | Block::RedstoneTorch { .. }
                            | Block::RedstoneWallTorch { .. }
                            | Block::RedstoneLamp { .. }
                            | Block::Observer { .. }
                    ) {
                        return Err(CompileError {
                            position: Some(observation.target),
                            reason: "scratch callback target is an executable block missing from the target index".into(),
                        });
                    }
                    continue;
                };
                let instruction = match components[target.index()].kind {
                    ComponentKind::Observer if observation.shape => ResourceInstruction::NotifyObserver { target, shape: true },
                    ComponentKind::Observer => continue,
                    ComponentKind::Wire => ResourceInstruction::RecomputeWire(target),
                    ComponentKind::Lamp | ComponentKind::Piston | ComponentKind::Repeater { .. } | ComponentKind::Torch => {
                        ResourceInstruction::Notify(target)
                    }
                    ComponentKind::Input => continue,
                };
                piston_operations.push(PistonOperation {
                    piston: crate::redpiler::program_graph::PistonId::new(piston as u32),
                    action,
                    phase,
                    instruction,
                });
            }
        }
        let positions = graph.position_index();
        for watch in &discovery.watchers {
            let wire = positions.get(&watch.wire).copied().ok_or_else(|| CompileError {
                position: Some(watch.wire),
                reason: "discovered wire missing from graph".into(),
            })?;
            let Some(piston_index) = graph.pistons().iter().position(|piston| piston.position == watch.piston) else {
                return Err(CompileError {
                    position: Some(watch.piston),
                    reason: "discovered piston watcher is not represented".into(),
                });
            };
            let piston = pistons[piston_index].input;
            if components[wire.index()].kind != ComponentKind::Wire {
                return Err(CompileError {
                    position: Some(watch.wire),
                    reason: "invalid discovered wire watcher".into(),
                });
            }
            wire_watchers.push((wire, piston));
        }
    } else {
        for piston in &pistons {
            let piston_position = graph.nodes()[piston.input.index()].position;
            let Some(piston_position) = piston_position else { continue };
            for (wire_index, node) in graph.nodes().iter().enumerate() {
                if !matches!(node.kind, CircuitNodeKind::Wire(_)) {
                    continue;
                }
                let Some(wire_position) = node.position else { continue };
                let distance = piston_position.x.abs_diff(wire_position.x)
                    + piston_position.y.abs_diff(wire_position.y)
                    + piston_position.z.abs_diff(wire_position.z);
                if distance <= 2 {
                    wire_watchers.push((NodeId::new(wire_index as u32), piston.input));
                }
            }
        }
    }
    let mut wire_targets: Vec<NodeId> = wire_watchers.iter().map(|(wire, _)| *wire).collect();
    if let Some(movement) = movement {
        let positions = graph.position_index();
        for source in &movement.arrangements.at_far.sources {
            let wire = positions.get(&source.wire).copied().ok_or_else(|| CompileError {
                position: Some(source.wire),
                reason: "discovered wire missing from graph".into(),
            })?;
            wire_targets.push(wire);
        }
    }
    wire_targets.extend(
        links
            .iter()
            .filter_map(|link| (components[link.source.index()].kind == ComponentKind::Wire).then_some(link.source)),
    );
    wire_targets.sort_by_key(|wire| wire.index());
    wire_targets.dedup();
    let mut wire_recomputes = Vec::new();
    for wire in wire_targets {
        let mut leaves = Vec::new();
        if let Some(discovery) = discovery.filter(|d| movement.is_some() || d.watchers.iter().any(|watch| Some(watch.wire) == at(wire))) {
            let positions = graph.position_index();
            for dependency in discovery.sources.iter().filter(|source| Some(source.wire) == at(wire)) {
                let source = positions.get(&dependency.source).copied().ok_or_else(|| CompileError {
                    position: Some(dependency.source),
                    reason: "discovered source missing from graph".into(),
                })?;
                if !matches!(components[source.index()].kind, ComponentKind::Input | ComponentKind::Torch) {
                    return Err(CompileError {
                        position: Some(dependency.source),
                        reason: "discovered source is not a static input".into(),
                    });
                }
                leaves.push(Leaf {
                    source: LeafSource::Node(source),
                    attenuation: dependency.attenuation,
                    condition: None,
                });
            }
        } else {
            expand_power(
                graph,
                PowerTarget::Input {
                    node: wire,
                    port: InputPort::Main,
                },
                0,
                None,
                &mut Vec::new(),
                &mut leaves,
            )
            .map_err(|reason| CompileError { position: at(wire), reason })?;
        }
        for leaf in leaves {
            let source = match leaf.source {
                LeafSource::Node(source) => source,
                LeafSource::Constant(strength) => {
                    let source = NodeId::new(components.len() as u32);
                    components.push(Component {
                        kind: ComponentKind::Input,
                        initial: EventComponentState {
                            output: strength,
                            powered: strength > 0,
                        },
                    });
                    source
                }
            };
            let id = links.len();
            links.push(ElectricalLink {
                source,
                target: wire,
                port: InputPort::Main,
                attenuation: leaf.attenuation,
                condition: leaf.condition,
            });
            operations.push((source, Operation::Contribution(id)));
            wire_recomputes.push((source, wire));
        }
    }
    wire_recomputes.sort_by_key(|(source, wire)| (source.index(), wire.index()));
    wire_recomputes.dedup();
    operations.extend(wire_recomputes.into_iter().map(|(source, wire)| (source, Operation::RecomputeWire(wire))));
    for (resource_index, resource) in graph.resources().iter().enumerate() {
        for &contact in resource.contacts.iter() {
            for (observer_index, node) in graph.nodes().iter().enumerate() {
                let CircuitNodeKind::Observer { watched, .. } = node.kind else {
                    continue;
                };
                if matches!(graph.sites()[watched.index()].source, SiteSource::Contact(watched_contact) if watched_contact == contact) {
                    for phase in [ResourceEventPhase::Departure, ResourceEventPhase::Arrival] {
                        resource_operations.push(ResourceOperation {
                            resource: crate::redpiler::program_graph::ResourceId::new(resource_index as u32),
                            contact,
                            phase,
                            instruction: ResourceInstruction::NotifyObserver {
                                target: NodeId::new(observer_index as u32),
                                shape: true,
                            },
                        });
                    }
                }
            }
        }
    }
    let program = EventProgram::new_with_piston_operations(
        components,
        links,
        operations,
        resources,
        pistons,
        resource_operations,
        wire_watchers,
        explicit_receiver,
        piston_operations,
    )
    .map_err(|reason| CompileError {
        position: None,
        reason: reason.into(),
    })?;
    Ok(CompiledEvent {
        program: Arc::new(program),
        positions: target_positions,
    })
}

fn uses_resource(condition: &EventCondition) -> bool {
    match condition {
        EventCondition::ResourceAt { .. } | EventCondition::ResourceEnabled(_) | EventCondition::ContactTest { .. } => true,
        EventCondition::Not(inner) => uses_resource(inner),
        EventCondition::All(items) | EventCondition::Any(items) => items.iter().any(uses_resource),
        _ => false,
    }
}

#[derive(Clone, Copy)]
enum LeafSource {
    Node(NodeId),
    Constant(u8),
}

struct Leaf {
    source: LeafSource,
    attenuation: u8,
    condition: Option<EventCondition>,
}

fn expand_power(
    graph: &crate::redpiler::program_graph::CircuitProgram,
    target: PowerTarget,
    attenuation: u8,
    condition: Option<EventCondition>,
    path: &mut Vec<PowerTarget>,
    leaves: &mut Vec<Leaf>,
) -> Result<(), String> {
    if attenuation >= 15 {
        return Ok(());
    }
    // shortcut: cyclic paths cannot improve max signal strength; upgrade if transfer rules gain negative loss.
    if path.contains(&target) {
        return Ok(());
    }
    if path.len() > 32 {
        return Err(format!("conducted power depth at {target:?}"));
    }
    path.push(target);
    if let Some((constant, inputs)) = graph.fixed_signal_query(target) {
        if constant > 0 {
            leaves.push(Leaf {
                source: LeafSource::Constant(constant.saturating_sub(attenuation)),
                attenuation: 0,
                condition: condition.clone(),
            });
        }
        for input in inputs {
            if input.require_cross {
                return Err(format!("isolated-wire shape condition at source {:?} is not compiled", input.node));
            }
            let loss = attenuation.saturating_add(input.attenuation);
            if matches!(graph.nodes()[input.node.index()].kind, CircuitNodeKind::Wire(_)) {
                expand_power(
                    graph,
                    PowerTarget::Input {
                        node: input.node,
                        port: InputPort::Main,
                    },
                    loss,
                    condition.clone(),
                    path,
                    leaves,
                )?;
            } else {
                leaves.push(Leaf {
                    source: LeafSource::Node(input.node),
                    attenuation: loss,
                    condition: condition.clone(),
                });
            }
        }
        path.pop();
        return Ok(());
    }
    for link in graph.power_links_to(target) {
        let link_condition = compile_guard(graph, link.guard, 0)
            .map_err(|reason| format!("{reason}; original Graph predicate: {:#?}", graph.guards()[link.guard.index()]))?;
        let condition = Some(match condition.clone() {
            Some(previous) => EventCondition::All(vec![previous, link_condition].into_boxed_slice()),
            None => link_condition,
        });
        let loss = attenuation.saturating_add(link.attenuation);
        if loss >= 15 {
            continue;
        }
        let site = &graph.sites()[link.from.index()];
        if let PowerPort::Conducted { include_dust } = link.from_port {
            expand_power(
                graph,
                PowerTarget::Conductor {
                    site: link.from,
                    include_dust,
                },
                loss,
                condition,
                path,
                leaves,
            )?;
            continue;
        }
        match site.source {
            SiteSource::Component(source) if matches!(graph.nodes()[source.index()].kind, CircuitNodeKind::Wire(_)) => {
                expand_power(
                    graph,
                    PowerTarget::Input {
                        node: source,
                        port: InputPort::Main,
                    },
                    loss,
                    condition,
                    path,
                    leaves,
                )?;
            }
            SiteSource::Component(source) => leaves.push(Leaf {
                source: LeafSource::Node(source),
                attenuation: loss,
                condition,
            }),
            SiteSource::Constant(_) if site.source_strength > 0 => leaves.push(Leaf {
                source: LeafSource::Constant(site.source_strength.saturating_sub(loss)),
                attenuation: 0,
                condition,
            }),
            SiteSource::Constant(_) => {}
            SiteSource::Contact(contact) => {
                for &resource in graph.resources_at_contact(contact) {
                    let profile = graph.profiles()[graph.resources()[resource.index()].material.index()];
                    if profile.electrical == ProfileElectrical::RedstoneSource {
                        let resource_condition = EventCondition::All(
                            vec![
                                EventCondition::ResourceAt { resource, contact },
                                EventCondition::ResourceEnabled(resource),
                            ]
                            .into_boxed_slice(),
                        );
                        let condition = Some(match condition.clone() {
                            Some(previous) => EventCondition::All(vec![previous, resource_condition].into_boxed_slice()),
                            None => resource_condition,
                        });
                        leaves.push(Leaf {
                            source: LeafSource::Constant(15u8.saturating_sub(loss)),
                            attenuation: 0,
                            condition,
                        });
                    }
                }
            }
        }
    }
    path.pop();
    Ok(())
}

fn compile_guard(
    graph: &crate::redpiler::program_graph::CircuitProgram,
    id: crate::redpiler::program_graph::GuardId,
    depth: usize,
) -> Result<EventCondition, String> {
    if depth > 32 {
        return Err(format!("Graph guard depth exceeded at {id:?}"));
    }
    Ok(match &graph.guards()[id.index()] {
        Guard::Always => EventCondition::All(Box::new([])),
        Guard::State(StateTest::ResourceAt { resource, contact }) => EventCondition::ResourceAt {
            resource: *resource,
            contact: *contact,
        },
        Guard::State(StateTest::ResourceEnabled(resource)) => EventCondition::ResourceEnabled(*resource),
        Guard::State(StateTest::PistonExtended(piston)) => EventCondition::PistonExtended {
            piston: *piston,
            extended: true,
        },
        Guard::State(StateTest::PistonHeadPresent(piston)) => EventCondition::PistonHeadPresent {
            piston: *piston,
            present: true,
        },
        Guard::State(StateTest::PistonAvailability {
            piston,
            is: PistonAvailability::Ready,
        }) => EventCondition::PistonReady(*piston),
        Guard::State(_) => return Err(format!("unsupported Graph predicate: {:#?}", graph.guards()[id.index()])),
        Guard::Site { site: _, view, test: _ } if *view == crate::redpiler::program_graph::OperandView::Captured => EventCondition::Any(Box::new([])),
        Guard::Site { site, test, .. } => match graph.sites()[site.index()].source {
            SiteSource::Constant(profile) => static_profile_condition(graph, profile, *test),
            SiteSource::Component(node) => {
                let block = graph.nodes()[node.index()].block;
                let matches = match test {
                    SiteTest::Solid => block.is_some_and(Block::is_solid),
                    SiteTest::Transparent => block.is_some_and(Block::is_transparent),
                    SiteTest::IsWire => matches!(graph.nodes()[node.index()].kind, CircuitNodeKind::Wire(_)),
                    SiteTest::Profile(_) => false,
                };
                if matches {
                    EventCondition::All(Box::new([]))
                } else {
                    EventCondition::Any(Box::new([]))
                }
            }
            SiteSource::Contact(contact) => {
                let contact_spec = graph
                    .contacts()
                    .get(contact.index())
                    .ok_or_else(|| format!("missing contact table entry {contact:?}"))?;
                let variants = contact_spec
                    .variants
                    .iter()
                    .map(|variant| {
                        Ok(EventContactVariant {
                            profile: event_profile(graph, variant.profile),
                            condition: compile_guard(graph, variant.when, depth + 1)?,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                EventCondition::ContactTest {
                    contact,
                    test: *test,
                    variants: variants.into_boxed_slice(),
                }
            }
        },
        Guard::Not(child) => EventCondition::Not(Box::new(compile_guard(graph, *child, depth + 1)?)),
        Guard::All(children) => EventCondition::All(
            children
                .iter()
                .map(|child| compile_guard(graph, *child, depth + 1))
                .collect::<Result<Vec<_>, _>>()?
                .into_boxed_slice(),
        ),
        Guard::Any(children) => EventCondition::Any(
            children
                .iter()
                .map(|child| compile_guard(graph, *child, depth + 1))
                .collect::<Result<Vec<_>, _>>()?
                .into_boxed_slice(),
        ),
    })
}

fn event_profile(graph: &crate::redpiler::program_graph::CircuitProgram, id: crate::redpiler::program_graph::ProfileId) -> EventProfile {
    let profile = graph.profiles()[id.index()];
    EventProfile {
        id,
        solid: profile.solid,
        transparent: profile.transparent,
        wire: graph.profile_block(id).is_some_and(|block| matches!(block, Block::RedstoneWire { .. })),
    }
}

fn static_profile_condition(
    graph: &crate::redpiler::program_graph::CircuitProgram,
    id: crate::redpiler::program_graph::ProfileId,
    test: SiteTest,
) -> EventCondition {
    let profile = event_profile(graph, id);
    let matches = match test {
        SiteTest::Solid => profile.solid,
        SiteTest::Transparent => profile.transparent,
        SiteTest::IsWire => profile.wire,
        SiteTest::Profile(expected) => profile.id == expected,
    };
    if matches {
        EventCondition::All(Box::new([]))
    } else {
        EventCondition::Any(Box::new([]))
    }
}
