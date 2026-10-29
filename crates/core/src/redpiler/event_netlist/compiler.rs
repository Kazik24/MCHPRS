//! First compiler from prepared discovery facts into the independent event runtime.
use super::program::{
    Component, ComponentKind, ComponentState as EventComponentState, ElectricalLink, EventCondition, EventContactVariant, EventPiston, EventProfile,
    EventProgram, EventResource, Operation, ResourceEventPhase, ResourceInstruction, ResourceOperation,
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
    let graph = &lowered.program;
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
        let mut state = match lowered.state.components.get(index) {
            Some(ComponentState::Electrical(state)) => EventComponentState {
                output: state.output_strength,
                powered: state.powered,
            },
            _ => EventComponentState::default(),
        };
        let kind = match &node.kind {
            CircuitNodeKind::Observer { .. } => ComponentKind::Observer,
            CircuitNodeKind::Piston(_) => ComponentKind::Piston,
            CircuitNodeKind::Electrical(NodeType::Lamp) => ComponentKind::Lamp,
            CircuitNodeKind::Electrical(NodeType::Repeater { delay, .. }) => ComponentKind::Repeater { delay: *delay },
            CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Constant | NodeType::Wire) | CircuitNodeKind::Wire(_) => ComponentKind::Input,
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
    for (target_index, _node) in graph.nodes().iter().enumerate() {
        if !matches!(
            components[target_index].kind,
            ComponentKind::Lamp | ComponentKind::Piston | ComponentKind::Repeater { .. }
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
            expand_power(
                graph,
                PowerTarget::Input { node: target, port },
                0,
                None,
                &mut Vec::new(),
                &mut leaves,
            )
            .map_err(|reason| CompileError {
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
                    ComponentKind::Input | ComponentKind::Observer | ComponentKind::Repeater { .. }
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
    let program =
        EventProgram::new_with_movement(components, links, operations, resources, pistons, resource_operations).map_err(|reason| CompileError {
            position: None,
            reason: reason.into(),
        })?;
    Ok(CompiledEvent {
        program: Arc::new(program),
        positions: graph.position_index(),
    })
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
