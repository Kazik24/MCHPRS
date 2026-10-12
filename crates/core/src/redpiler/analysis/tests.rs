use super::*;
mod callback_routes;
mod deep_parser_audit;
mod export_json;
mod redstone_fuzz;
mod research;

use crate::plot::worldedit::{load_schematic, paste_clipboard};
use crate::plot::{PLOT_WIDTH, PlotWorld};
use crate::redpiler::analysis::AnalysisLimits;
use crate::redpiler::compile_graph::{NodeState, NodeType};
use crate::redpiler::program_graph::{
    CircuitNodeKind, ComponentState, Guard, GuardId, InputPort, PowerPort, ResourceState, StateTest,
};
use crate::redpiler::{CompileError, Compiler, CompilerOptions};
use crate::world::storage::Chunk;
use mchprs_blocks::block_entities::{BlockEntity, ContainerType};
use mchprs_blocks::blocks::{
    Block, ComparatorMode, RedstoneComparator, RedstonePiston, RedstoneRepeater,
};
use mchprs_blocks::{BlockDirection, BlockFace, BlockFacing, BlockPos};
use mchprs_world::{AdvancePhase, PistonAction, TickPriority};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use super::electrical::{
    self, ComparatorOverride, ElectricalDraft, ElectricalError, ElectricalLink, ElectricalTarget,
    WireRelationKind,
};
use super::geometry::{
    GeometryError, GeometryIssue, GeometryResult, GeometryView, prepare_geometry,
};

fn empty() -> PlotWorld {
    PlotWorld::from_chunks(
        0,
        0,
        (0..PLOT_WIDTH)
            .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
            .collect(),
        Default::default(),
    )
}

fn analyze_world(world: &PlotWorld) -> AnalysisReport {
    analyze(
        world,
        world.get_corners(),
        &world.scheduler().iter_entries().collect::<Vec<_>>(),
        &Default::default(),
        Default::default(),
    )
    .unwrap()
}

const BASE: BlockPos = BlockPos::new(40, 30, 40);

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn local_pos(pos: &Value) -> BlockPos {
    BASE + BlockPos::new(
        pos[0].as_i64().unwrap() as i32,
        pos[1].as_i64().unwrap() as i32,
        pos[2].as_i64().unwrap() as i32,
    )
}

fn lever_action(world: &mut PlotWorld, pos: BlockPos, powered: bool) {
    let Block::Lever { mut lever } = world.get_block(pos) else {
        panic!("lever at {pos:?}");
    };
    if lever.powered == powered {
        return;
    }
    lever.powered = powered;
    world.set_block(pos, Block::Lever { lever });
    crate::redstone::update_surrounding_blocks(world, pos);
    let face = match lever.face {
        mchprs_blocks::blocks::LeverFace::Floor => BlockFace::Bottom,
        mchprs_blocks::blocks::LeverFace::Ceiling => BlockFace::Top,
        mchprs_blocks::blocks::LeverFace::Wall => lever.facing.opposite().block_face(),
    };
    crate::redstone::update_surrounding_blocks(world, pos.offset(face));
}

#[test]
fn analysis_limits_and_cancellation_are_errors() {
    let mut world = empty();
    world.set_block(BASE, Block::Stone {});
    let monitor = TaskMonitor::default();
    monitor.cancel();
    assert_eq!(
        analyze(
            &world,
            world.get_corners(),
            &[],
            &monitor,
            Default::default()
        )
        .unwrap_err(),
        AnalysisError::Cancelled
    );
    assert_eq!(
        analyze(
            &world,
            (BASE, BlockPos::new(42, -1, 42)),
            &[],
            &Default::default(),
            Default::default()
        )
        .unwrap_err(),
        AnalysisError::InvalidBounds
    );
    assert_eq!(
        analyze(
            &world,
            world.get_corners(),
            &[],
            &Default::default(),
            AnalysisLimits {
                max_cells: 1,
                ..Default::default()
            }
        )
        .unwrap_err(),
        AnalysisError::CellLimit
    );
    assert!(matches!(
        analyze(
            &world,
            (BlockPos::new(-32, 20, 0), BlockPos::new(-16, 30, 15)),
            &[],
            &Default::default(),
            Default::default()
        ),
        Err(AnalysisError::UnloadedChunk { .. })
    ));
    let mut piston_world = empty();
    piston_world.set_block(
        BASE,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::South,
                sticky: true,
                extended: false,
            },
        },
    );
    assert_eq!(
        analyze(
            &piston_world,
            piston_world.get_corners(),
            &[],
            &Default::default(),
            AnalysisLimits {
                max_pistons: 0,
                ..Default::default()
            }
        )
        .unwrap_err(),
        AnalysisError::PistonLimit
    );
}

#[test]
fn piston_head_moving_piston_and_unsafe_entry_state_are_admission_blockers() {
    let mut world = empty();
    world.set_block(
        BASE,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::South,
                sticky: true,
                extended: false,
            },
        },
    );
    world.set_block(
        BASE + BlockPos::new(1, 0, 0),
        Block::MovingPiston {
            moving: Default::default(),
        },
    );
    world.set_block(
        BASE + BlockPos::new(2, 0, 0),
        Block::PistonHead {
            head: Default::default(),
        },
    );
    world.piston_state_mut().phase = AdvancePhase::PistonEvents;

    let report = analyze_world(&world);
    assert_eq!(report.pistons[0], BASE);
    assert!(
        report
            .issues
            .iter()
            .any(|i| matches!(i, AdmissionIssue::PistonRuntimeUnavailable { pos } if *pos == BASE))
    );
    assert!(
        report
            .issues
            .iter()
            .any(|i| matches!(i, AdmissionIssue::MovingPiston { .. }))
    );
    assert!(
        report
            .issues
            .iter()
            .any(|i| matches!(i, AdmissionIssue::UnownedPistonHead { .. }))
    );
    assert!(
        report
            .issues
            .iter()
            .any(|i| matches!(i, AdmissionIssue::EntryPhase { .. }))
    );
}

#[test]
fn queued_events_motions_and_movement_work_block_compilation() {
    let mut world = empty();
    let state = world.piston_state_mut();
    state.events.push_back(mchprs_world::PistonEvent {
        pos: BASE,
        sticky: true,
        facing: BlockFace::South,
        action: PistonAction::Extend,
    });
    state.motions.push_back(mchprs_world::PistonMotion {
        pos: BASE,
        identity: 1,
        progress: 0.0,
        previous_progress: 0.0,
        last_tick: 0,
        carried_entity: None,
    });
    state.movement_work.push((BASE, 1));

    let report = analyze_world(&world);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| matches!(issue, AdmissionIssue::PendingPistonEvents { count: 1 }))
    );
    assert!(
        report
            .issues
            .iter()
            .any(|issue| matches!(issue, AdmissionIssue::PendingPistonMotions { count: 1 }))
    );
    assert!(
        report
            .issues
            .iter()
            .any(|issue| matches!(issue, AdmissionIssue::PendingMovementWork { count: 1 }))
    );
}

#[test]
fn piston_compile_rejection_preserves_the_interpreter_world() {
    let mut world = empty();
    world.set_block(
        BASE,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::South,
                sticky: true,
                extended: false,
            },
        },
    );
    let before = world.get_block(BASE);
    let mut compiler = Compiler::default();
    assert!(matches!(
        compiler.compile(
            &world,
            world.get_corners(),
            Default::default(),
            Vec::new(),
            Default::default()
        ),
        Err(CompileError::Unsupported(_))
    ));
    assert!(!compiler.is_active());
    assert_eq!(world.get_block(BASE), before);
}

#[test]
fn ordinary_compile_and_reset_still_work() {
    let mut world = empty();
    let lever = Block::Lever {
        lever: Default::default(),
    };
    world.set_block(BASE, lever);
    let bounds = world.get_corners();
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &world,
            bounds,
            CompilerOptions::default(),
            Vec::new(),
            Default::default(),
        )
        .unwrap();
    assert!(compiler.is_active());
    compiler.on_use_block(BASE);
    compiler.flush(&mut world);
    assert!(matches!(world.get_block(BASE), Block::Lever { lever } if lever.powered));
    compiler.reset(&mut world, bounds);
    assert!(!compiler.is_active());
}

fn piston_block(facing: BlockFace, sticky: bool, extended: bool) -> Block {
    Block::Piston {
        piston: RedstonePiston {
            facing: facing.into(),
            sticky,
            extended,
        },
    }
}

fn prepare_geometry_for(world: &PlotWorld) -> Result<geometry::GeometryResult, GeometryError> {
    prepare_geometry_with_bounds(world, world.get_corners())
}

fn prepare_geometry_with_bounds(
    world: &PlotWorld,
    bounds: (BlockPos, BlockPos),
) -> Result<geometry::GeometryResult, GeometryError> {
    prepare_geometry(
        world,
        bounds,
        &world.scheduler().iter_entries().collect::<Vec<_>>(),
        &TaskMonitor::default(),
        AnalysisLimits::default(),
    )
}

fn prepare_electrical_for(world: &PlotWorld) -> (GeometryResult, ElectricalDraft) {
    let geometry = prepare_geometry_for(world).unwrap();
    let electrical = electrical::prepare_electrical(
        world,
        geometry.report.bounds,
        &geometry.draft,
        &Default::default(),
    )
    .unwrap();
    (geometry, electrical)
}

fn assert_dynamic_override_outside_inventory(
    world: &PlotWorld,
    bounds: (BlockPos, BlockPos),
    comparator: BlockPos,
    source: BlockPos,
) {
    let geometry = prepare_geometry_with_bounds(world, bounds).unwrap();
    let error =
        match electrical::prepare_electrical(world, bounds, &geometry.draft, &Default::default()) {
            Err(error) => error,
            Ok(_) => panic!("external dynamic override must reject electrical preparation"),
        };
    assert_eq!(
        error,
        ElectricalError::DynamicComparatorOverrideNotInventoried { comparator, source }
    );
    let message = error.to_string();
    assert!(message.contains(&format!("{comparator:?}")));
    assert!(message.contains(&format!("{source:?}")));
}

fn lamp_node_at(
    draft: &ElectricalDraft,
    position: BlockPos,
) -> crate::redpiler::program_graph::NodeId {
    draft
        .nodes
        .iter()
        .position(|node| {
            node.position == Some(position)
                && matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Lamp))
        })
        .map(|index| crate::redpiler::program_graph::NodeId::new(index as u32))
        .unwrap_or_else(|| panic!("no lamp node at {position:?}"))
}

fn comparator_node_at(
    draft: &ElectricalDraft,
    position: BlockPos,
) -> crate::redpiler::program_graph::NodeId {
    draft
        .nodes
        .iter()
        .position(|node| {
            node.position == Some(position)
                && matches!(
                    node.kind,
                    CircuitNodeKind::Electrical(NodeType::Comparator { .. })
                )
        })
        .map(|index| crate::redpiler::program_graph::NodeId::new(index as u32))
        .unwrap_or_else(|| panic!("no comparator node at {position:?}"))
}

fn electrical_source_pos(draft: &ElectricalDraft, link: &ElectricalLink) -> BlockPos {
    draft.sites[link.from.index()].position
}

fn comparator_override_value(draft: &ElectricalDraft, value: ComparatorOverride) -> u8 {
    match value {
        ComparatorOverride::Literal(value) => value,
        ComparatorOverride::LiveComponentOutput(node) => match &draft.nodes[node.index()].state {
            ComponentState::Electrical(state) => state.output_strength,
            _ => panic!("live comparator override must reference an electrical component"),
        },
    }
}

fn electrical_guard_holds(
    draft: &ElectricalDraft,
    guard: GuardId,
    pistons: &[crate::redpiler::program_graph::PistonState],
    resources: &[ResourceState],
) -> bool {
    match &draft.guards[guard.index()] {
        Guard::Always => true,
        Guard::State(StateTest::PistonExtended(piston)) => pistons[piston.index()].extended,
        Guard::State(StateTest::PistonAvailability { piston, is }) => {
            pistons[piston.index()].availability == *is
        }
        Guard::State(StateTest::ResourceAt { resource, contact }) => {
            resources[resource.index()].at == *contact
        }
        Guard::State(StateTest::ResourceEnabled(resource)) => resources[resource.index()].enabled,
        Guard::Not(other) => !electrical_guard_holds(draft, *other, pistons, resources),
        Guard::All(children) => children
            .iter()
            .all(|child| electrical_guard_holds(draft, *child, pistons, resources)),
        Guard::Any(children) => children
            .iter()
            .any(|child| electrical_guard_holds(draft, *child, pistons, resources)),
        Guard::Site { .. } => panic!("site guard is outside this resource-state check"),
    }
}

#[test]
fn electrical_channels_preserve_direction_and_one_cube_conduction() {
    let mut direct = empty();
    direct.set_block(BASE, Block::RedstoneLamp { lit: false });
    let source = BASE.offset(BlockFace::East);
    direct.set_block(source, Block::RedstoneBlock {});
    let (_, draft) = prepare_electrical_for(&direct);
    let lamp = lamp_node_at(&draft, BASE);
    let weak = draft.links.iter().find(|link| {
        electrical_source_pos(&draft, link) == source
            && link.from_port == PowerPort::Weak(BlockFace::East)
            && link.to
                == ElectricalTarget::Input {
                    node: lamp,
                    port: InputPort::Main,
                }
    });
    assert!(
        weak.is_some(),
        "redstone block weak power must reach the east-facing query"
    );
    assert!(!draft.links.iter().any(|link| {
        electrical_source_pos(&draft, link) == source
            && matches!(link.from_port, PowerPort::Strong(_))
    }));
    assert!(draft.nodes.iter().any(|node| {
        node.position == Some(source)
            && matches!(
                &node.state,
                ComponentState::Electrical(state) if state.output_strength == 15
            )
    }));

    let mut directional = empty();
    let repeater = RedstoneRepeater {
        facing: BlockDirection::East,
        powered: true,
        ..Default::default()
    };
    directional.set_block(BASE, Block::RedstoneRepeater { repeater });
    let west_lamp = BASE.offset(BlockFace::West);
    let east_lamp = BASE.offset(BlockFace::East);
    directional.set_block(west_lamp, Block::RedstoneLamp { lit: false });
    directional.set_block(east_lamp, Block::RedstoneLamp { lit: false });
    let (_, draft) = prepare_electrical_for(&directional);
    let west_node = lamp_node_at(&draft, west_lamp);
    let east_node = lamp_node_at(&draft, east_lamp);
    assert!(draft.links.iter().any(|link| {
        electrical_source_pos(&draft, link) == BASE
            && link.from_port == PowerPort::Weak(BlockFace::East)
            && link.to
                == ElectricalTarget::Input {
                    node: west_node,
                    port: InputPort::Main,
                }
    }));
    assert!(!draft.links.iter().any(|link| {
        electrical_source_pos(&draft, link) == BASE
            && link.to
                == ElectricalTarget::Input {
                    node: east_node,
                    port: InputPort::Main,
                }
    }));

    let mut conducted = empty();
    conducted.set_block(BASE, Block::RedstoneLamp { lit: false });
    let cube = BASE.offset(BlockFace::East);
    let next_cube = cube.offset(BlockFace::East);
    let torch = cube.offset(BlockFace::Bottom);
    conducted.set_block(cube, Block::Stone {});
    conducted.set_block(next_cube, Block::Stone {});
    let remote_torch = next_cube.offset(BlockFace::Bottom);
    conducted.set_block(remote_torch, Block::RedstoneTorch { lit: true });
    conducted.set_block(remote_torch.offset(BlockFace::Bottom), Block::Stone {});
    conducted.set_block(torch, Block::RedstoneTorch { lit: true });
    conducted.set_block(torch.offset(BlockFace::Bottom), Block::Stone {});
    let (_, draft) = prepare_electrical_for(&conducted);
    let lamp = lamp_node_at(&draft, BASE);
    let conductor_site = draft
        .sites
        .iter()
        .position(|site| site.position == cube)
        .map(|index| crate::redpiler::program_graph::SiteId::new(index as u32))
        .unwrap();
    assert!(draft.links.iter().any(|link| {
        link.from == conductor_site
            && link.from_port == PowerPort::Conducted { include_dust: true }
            && link.to
                == ElectricalTarget::Input {
                    node: lamp,
                    port: InputPort::Main,
                }
    }));
    assert!(draft.links.iter().any(|link| {
        electrical_source_pos(&draft, link) == torch
            && link.from_port == PowerPort::Strong(BlockFace::Bottom)
            && link.to
                == ElectricalTarget::Conductor {
                    site: conductor_site,
                    include_dust: true,
                }
    }));
    assert!(
        !draft.links.iter().any(|link| {
            electrical_source_pos(&draft, link) == next_cube
                && matches!(
                    link.to,
                    ElectricalTarget::Conductor {
                        site,
                        include_dust: true
                    } if site == conductor_site
                )
        }),
        "a conductor must not relay its conducted charge into another conductor"
    );
}

#[test]
fn electrical_comparator_side_tracks_resource_contact_alternatives() {
    let mut world = empty();
    let comparator = RedstoneComparator::new(BlockDirection::South, ComparatorMode::Compare, false);
    world.set_block(BASE, Block::RedstoneComparator { comparator });
    let side = BASE.offset(BlockFace::East);
    let piston = side.offset(BlockFace::North).offset(BlockFace::North);
    let near = piston.offset(BlockFace::South);
    world.set_block(piston, piston_block(BlockFace::South, true, false));
    world.set_block(near, Block::RedstoneBlock {});

    let (geometry, draft) = prepare_electrical_for(&world);
    let comparator_node = comparator_node_at(&draft, BASE);
    let side_contact = geometry
        .draft
        .contacts
        .iter()
        .position(|contact| contact.position == side)
        .map(|index| crate::redpiler::program_graph::ContactId::new(index as u32))
        .expect("comparator side is a resource contact");
    let side_link = draft
        .links
        .iter()
        .find(|link| {
            electrical_source_pos(&draft, link) == side
                && link.to
                    == ElectricalTarget::Input {
                        node: comparator_node,
                        port: InputPort::Side,
                    }
        })
        .expect("future redstone-block contact must be discovered");
    assert_eq!(
        geometry.draft.initial_resources[0].at,
        geometry.draft.resources[0]
            .spec
            .contacts
            .iter()
            .copied()
            .find(|contact| *contact != side_contact)
            .unwrap()
    );
    assert!(!electrical_guard_holds(
        &draft,
        side_link.guard,
        &geometry.draft.initial_pistons,
        &geometry.draft.initial_resources,
    ));
    let mut at_side = geometry.draft.initial_resources.clone();
    at_side[0].at = side_contact;
    assert!(electrical_guard_holds(
        &draft,
        side_link.guard,
        &geometry.draft.initial_pistons,
        &at_side,
    ));
    at_side[0].at = geometry.draft.initial_resources[0].at;
    assert!(!electrical_guard_holds(
        &draft,
        side_link.guard,
        &geometry.draft.initial_pistons,
        &at_side,
    ));
}

#[test]
fn electrical_wire_conductor_reads_exclude_dust_self_feedback() {
    let mut world = empty();
    let mut wire: mchprs_blocks::blocks::RedstoneWire = Default::default();
    wire.power = 15;
    wire.east = mchprs_blocks::blocks::RedstoneWireSide::Side;
    world.set_block(BASE, Block::RedstoneWire { wire });
    let cube = BASE.offset(BlockFace::East);
    world.set_block(cube, Block::Stone {});
    let lamp = cube.offset(BlockFace::East);
    world.set_block(lamp, Block::RedstoneLamp { lit: false });

    let (_, draft) = prepare_electrical_for(&world);
    let wire_node = draft
        .nodes
        .iter()
        .position(|node| {
            node.position == Some(BASE) && matches!(node.kind, CircuitNodeKind::Wire(_))
        })
        .map(|index| crate::redpiler::program_graph::NodeId::new(index as u32))
        .unwrap();
    let cube_site = draft
        .sites
        .iter()
        .position(|site| site.position == cube)
        .map(|index| crate::redpiler::program_graph::SiteId::new(index as u32))
        .unwrap();
    assert!(draft.links.iter().any(|link| {
        link.from == cube_site
            && link.from_port
                == PowerPort::Conducted {
                    include_dust: false,
                }
            && link.to
                == ElectricalTarget::Input {
                    node: wire_node,
                    port: InputPort::Main,
                }
    }));
    assert!(draft.links.iter().any(|link| {
        electrical_source_pos(&draft, link) == BASE
            && link.from_port == PowerPort::Strong(BlockFace::West)
            && link.to
                == ElectricalTarget::Conductor {
                    site: cube_site,
                    include_dust: true,
                }
    }));
    assert!(!draft.links.iter().any(|link| {
        electrical_source_pos(&draft, link) == BASE
            && link.from_port == PowerPort::Strong(BlockFace::West)
            && link.to
                == ElectricalTarget::Conductor {
                    site: cube_site,
                    include_dust: false,
                }
    }));
}

#[test]
fn electrical_diode_wire_inputs_use_stored_strength() {
    let mut repeater_world = empty();
    let repeater = RedstoneRepeater {
        facing: BlockDirection::South,
        ..Default::default()
    };
    repeater_world.set_block(BASE, Block::RedstoneRepeater { repeater });
    let rear = BASE.offset(BlockFace::South);
    repeater_world.set_block(
        rear,
        Block::RedstoneWire {
            wire: Default::default(),
        },
    );
    let (_, repeater_draft) = prepare_electrical_for(&repeater_world);
    let repeater_node = repeater_draft
        .nodes
        .iter()
        .position(|node| {
            node.position == Some(BASE)
                && matches!(
                    node.kind,
                    CircuitNodeKind::Electrical(NodeType::Repeater { .. })
                )
        })
        .map(|index| crate::redpiler::program_graph::NodeId::new(index as u32))
        .unwrap();
    assert!(repeater_draft.links.iter().any(|link| {
        electrical_source_pos(&repeater_draft, link) == rear
            && link.from_port == PowerPort::Stored
            && link.to
                == ElectricalTarget::Input {
                    node: repeater_node,
                    port: InputPort::Main,
                }
            && link.attenuation == 0
    }));

    let mut comparator_world = empty();
    let comparator = RedstoneComparator::new(BlockDirection::South, ComparatorMode::Compare, false);
    comparator_world.set_block(BASE, Block::RedstoneComparator { comparator });
    let rear = BASE.offset(BlockFace::South);
    let side = BASE.offset(BlockFace::East);
    for position in [rear, side] {
        comparator_world.set_block(
            position,
            Block::RedstoneWire {
                wire: Default::default(),
            },
        );
    }
    let (_, comparator_draft) = prepare_electrical_for(&comparator_world);
    let comparator_node = comparator_node_at(&comparator_draft, BASE);
    for (position, port) in [(rear, InputPort::Main), (side, InputPort::Side)] {
        assert!(comparator_draft.links.iter().any(|link| {
            electrical_source_pos(&comparator_draft, link) == position
                && link.from_port == PowerPort::Stored
                && link.to
                    == ElectricalTarget::Input {
                        node: comparator_node,
                        port,
                    }
                && link.attenuation == 0
        }));
    }
}

#[test]
fn electrical_piston_qc_is_a_separate_sampled_channel() {
    let mut world = empty();
    world.set_block(BASE, piston_block(BlockFace::South, true, false));
    let front = BASE.offset(BlockFace::South);
    world.set_block(front, Block::RedstoneBlock {});
    let qc_source = BASE.offset(BlockFace::Top).offset(BlockFace::East);
    world.set_block(qc_source, Block::RedstoneBlock {});
    let (geometry, draft) = prepare_electrical_for(&world);
    let piston = geometry.draft.pistons[0].node;

    assert!(draft.links.iter().any(|link| {
        electrical_source_pos(&draft, link) == qc_source
            && link.from_port == PowerPort::Weak(BlockFace::East)
            && link.to
                == ElectricalTarget::Input {
                    node: piston,
                    port: InputPort::Quasi,
                }
    }));
    assert!(!draft.links.iter().any(|link| {
        electrical_source_pos(&draft, link) == front
            && link.to
                == ElectricalTarget::Input {
                    node: piston,
                    port: InputPort::Main,
                }
    }));
    let qc_sample = |draft: &ElectricalDraft| {
        draft
            .links
            .iter()
            .filter(|link| {
                electrical_source_pos(draft, link) == qc_source
                    && matches!(
                        link.to,
                        ElectricalTarget::Input {
                            node,
                            port: InputPort::Quasi
                        } if node == piston
                    )
                    && electrical_guard_holds(
                        draft,
                        link.guard,
                        &geometry.draft.initial_pistons,
                        &geometry.draft.initial_resources,
                    )
            })
            .map(|_| 15)
            .max()
            .unwrap_or(0)
    };
    assert_eq!(qc_sample(&draft), 15);
    world.set_block(qc_source, Block::Air);
    let (_, without_qc) = prepare_electrical_for(&world);
    assert_eq!(qc_sample(&without_qc), 0);
    // ElectricalDraft contains no callback relation, so changing QC changes only the sampled value.
}

#[test]
fn electrical_resource_guards_gate_emission_and_conduction() {
    let mut source_world = empty();
    source_world.set_block(BASE, piston_block(BlockFace::South, true, false));
    let near = BASE.offset(BlockFace::South);
    source_world.set_block(near, Block::RedstoneBlock {});
    let lamp_pos = near.offset(BlockFace::East);
    source_world.set_block(lamp_pos, Block::RedstoneLamp { lit: false });
    let (geometry, draft) = prepare_electrical_for(&source_world);
    let lamp = lamp_node_at(&draft, lamp_pos);
    let source_link = draft
        .links
        .iter()
        .find(|link| {
            electrical_source_pos(&draft, link) == near
                && link.from_port == PowerPort::Weak(BlockFace::West)
                && link.to
                    == ElectricalTarget::Input {
                        node: lamp,
                        port: InputPort::Main,
                    }
        })
        .unwrap();
    assert!(electrical_guard_holds(
        &draft,
        source_link.guard,
        &geometry.draft.initial_pistons,
        &geometry.draft.initial_resources,
    ));
    let channel_value = |resources: &[ResourceState]| {
        draft
            .links
            .iter()
            .filter(|link| {
                electrical_source_pos(&draft, link) == near
                    && link.to
                        == ElectricalTarget::Input {
                            node: lamp,
                            port: InputPort::Main,
                        }
                    && electrical_guard_holds(
                        &draft,
                        link.guard,
                        &geometry.draft.initial_pistons,
                        resources,
                    )
            })
            .map(|_| 15)
            .max()
            .unwrap_or(0)
    };
    assert_eq!(channel_value(&geometry.draft.initial_resources), 15);
    let mut disabled = geometry.draft.initial_resources.clone();
    disabled[0].enabled = false;
    assert_eq!(channel_value(&disabled), 0);
    assert!(!electrical_guard_holds(
        &draft,
        source_link.guard,
        &geometry.draft.initial_pistons,
        &disabled,
    ));

    let mut conductor_world = empty();
    conductor_world.set_block(BASE, piston_block(BlockFace::South, true, false));
    conductor_world.set_block(near, Block::from_name("white_wool").unwrap());
    conductor_world.set_block(
        near.offset(BlockFace::East),
        Block::RedstoneLamp { lit: false },
    );
    let torch = near.offset(BlockFace::Bottom);
    conductor_world.set_block(torch, Block::RedstoneTorch { lit: true });
    conductor_world.set_block(torch.offset(BlockFace::Bottom), Block::Stone {});
    let (geometry, draft) = prepare_electrical_for(&conductor_world);
    let lamp = lamp_node_at(&draft, near.offset(BlockFace::East));
    let contact = geometry
        .draft
        .contacts
        .iter()
        .position(|contact| contact.position == near)
        .unwrap();
    let conductor_site = draft
        .sites
        .iter()
        .position(|site| site.position == near)
        .map(|index| crate::redpiler::program_graph::SiteId::new(index as u32))
        .unwrap();
    let conducted_link = draft
        .links
        .iter()
        .find(|link| {
            link.from == conductor_site
                && link.from_port == PowerPort::Conducted { include_dust: true }
                && link.to
                    == ElectricalTarget::Input {
                        node: lamp,
                        port: InputPort::Main,
                    }
        })
        .unwrap();
    let incident_link = draft
        .links
        .iter()
        .find(|link| {
            electrical_source_pos(&draft, link) == torch
                && link.from_port == PowerPort::Strong(BlockFace::Bottom)
                && link.to
                    == ElectricalTarget::Conductor {
                        site: conductor_site,
                        include_dust: true,
                    }
        })
        .unwrap();
    assert!(electrical_guard_holds(
        &draft,
        conducted_link.guard,
        &geometry.draft.initial_pistons,
        &geometry.draft.initial_resources,
    ));
    assert!(electrical_guard_holds(
        &draft,
        incident_link.guard,
        &geometry.draft.initial_pistons,
        &geometry.draft.initial_resources,
    ));
    let mut moved = geometry.draft.initial_resources.clone();
    moved[0].at = geometry.draft.resources[0]
        .spec
        .contacts
        .iter()
        .copied()
        .find(|candidate| candidate.index() != contact)
        .unwrap();
    assert!(!electrical_guard_holds(
        &draft,
        conducted_link.guard,
        &geometry.draft.initial_pistons,
        &moved,
    ));
    assert!(!electrical_guard_holds(
        &draft,
        incident_link.guard,
        &geometry.draft.initial_pistons,
        &moved,
    ));
}

#[test]
fn electrical_dust_keeps_vertices_routes_and_attenuation() {
    let mut world = empty();
    let east = BASE.offset(BlockFace::East);
    let up = east.offset(BlockFace::Top);
    let south = BASE.offset(BlockFace::South);
    let down = south.offset(BlockFace::Bottom);
    for position in [BASE, east, up, south, down] {
        world.set_block(
            position,
            Block::RedstoneWire {
                wire: Default::default(),
            },
        );
    }
    let (_, draft) = prepare_electrical_for(&world);
    assert_eq!(draft.wires.len(), 5);
    for plan in &draft.wires {
        let unique: rustc_hash::FxHashSet<_> = plan.canonical_neighbors.iter().copied().collect();
        assert_eq!(unique.len(), 24);
    }
    let root_wire = draft
        .nodes
        .iter()
        .position(|node| {
            node.position == Some(BASE) && matches!(node.kind, CircuitNodeKind::Wire(_))
        })
        .unwrap();
    let root_wire = crate::redpiler::program_graph::NodeId::new(root_wire as u32);
    let root_plan = draft
        .wires
        .iter()
        .find(|plan| plan.node == root_wire)
        .unwrap();
    for (actual, canonical) in root_plan
        .canonical_neighbors
        .iter()
        .zip(crate::world::wire_cache::positions(BASE))
    {
        assert_eq!(draft.sites[actual.index()].position, canonical);
    }
    let east_site = draft
        .sites
        .iter()
        .position(|site| site.position == east)
        .map(|index| crate::redpiler::program_graph::SiteId::new(index as u32))
        .unwrap();
    assert!(draft.links.iter().any(|link| {
        link.from == east_site
            && link.from_port == PowerPort::Output
            && link.to
                == ElectricalTarget::Input {
                    node: root_wire,
                    port: InputPort::Main,
                }
            && link.attenuation == 1
    }));
    assert!(draft.wire_relations.iter().any(|relation| {
        relation.from == root_wire
            && relation.kind == WireRelationKind::Ascend
            && draft.sites[relation.to.index()].position == up
    }));
    assert!(draft.wire_relations.iter().any(|relation| {
        relation.from == root_wire
            && relation.kind == WireRelationKind::Descend
            && draft.sites[relation.to.index()].position == down
    }));
    assert_eq!(electrical::attenuate_power(15, 14), 1);
    assert_eq!(electrical::attenuate_power(15, 15), 0);
}

#[test]
fn electrical_comparator_overrides_replace_rear_power_in_order() {
    let mut direct_world = empty();
    let comparator = RedstoneComparator::new(BlockDirection::South, ComparatorMode::Compare, false);
    direct_world.set_block(BASE, Block::RedstoneComparator { comparator });
    let rear = BASE.offset(BlockFace::South);
    direct_world.set_block(rear, Block::Cauldron { level: 3 });
    direct_world.set_block(rear.offset(BlockFace::South), Block::RedstoneBlock {});
    let (_, direct_draft) = prepare_electrical_for(&direct_world);
    let direct = direct_draft
        .comparators
        .iter()
        .find(|plan| direct_draft.nodes[plan.node.index()].position == Some(BASE))
        .unwrap();
    assert_eq!(direct_draft.sites[direct.rear.index()].position, rear);
    assert_eq!(direct_world.get_block(rear), Block::Cauldron { level: 3 });
    assert_eq!(direct.direct_override, Some(ComparatorOverride::Literal(3)));
    assert!(direct.far_override.is_none());
    assert!(!direct_draft.links.iter().any(|link| {
        link.to
            == ElectricalTarget::Input {
                node: direct.node,
                port: InputPort::Main,
            }
    }));

    let mut container_world = empty();
    container_world.set_block(BASE, Block::RedstoneComparator { comparator });
    container_world.set_block(rear, Block::from_name("barrel").unwrap());
    container_world.set_block_entity(
        rear,
        BlockEntity::Container {
            comparator_override: 12,
            inventory: Default::default(),
            ty: ContainerType::Barrel,
        },
    );
    let (_, container_draft) = prepare_electrical_for(&container_world);
    let container = container_draft
        .comparators
        .iter()
        .find(|plan| container_draft.nodes[plan.node.index()].position == Some(BASE))
        .unwrap();
    assert_eq!(
        container.direct_override,
        Some(ComparatorOverride::Literal(12))
    );

    let mut far_world = empty();
    far_world.set_block(BASE, Block::RedstoneComparator { comparator });
    far_world.set_block(rear, Block::Stone {});
    far_world.set_block(rear.offset(BlockFace::South), Block::Cauldron { level: 2 });
    let (_, far_draft) = prepare_electrical_for(&far_world);
    let far = far_draft
        .comparators
        .iter()
        .find(|plan| far_draft.nodes[plan.node.index()].position == Some(BASE))
        .unwrap();
    assert_eq!(far.direct_override, None);
    assert_eq!(
        far.far_override.map(|(_, value, _)| value),
        Some(ComparatorOverride::Literal(2))
    );
    assert_eq!(far.far_threshold, 15);
    assert!(far_draft.links.iter().any(|link| {
        link.from_port == PowerPort::Conducted { include_dust: true }
            && electrical_source_pos(&far_draft, link) == rear
            && link.to
                == ElectricalTarget::Input {
                    node: far.node,
                    port: InputPort::Main,
                }
    }));
}

#[test]
fn electrical_dynamic_comparator_overrides_read_live_component_output() {
    let comparator = RedstoneComparator::new(BlockDirection::South, ComparatorMode::Compare, false);
    for block in [
        Block::from_name("copper_bulb").unwrap(),
        Block::from_name("command_block").unwrap(),
    ] {
        let mut world = empty();
        world.set_block(BASE, Block::RedstoneComparator { comparator });
        let source = BASE.offset(BlockFace::South);
        world.set_block(source, block);
        let (_, mut draft) = prepare_electrical_for(&world);
        let comparator_node = comparator_node_at(&draft, BASE);
        let plan = draft
            .comparators
            .iter()
            .find(|plan| plan.node == comparator_node)
            .unwrap();
        let source_node = draft
            .nodes
            .iter()
            .position(|node| node.position == Some(source))
            .map(|index| crate::redpiler::program_graph::NodeId::new(index as u32))
            .unwrap();
        assert_eq!(
            plan.direct_override,
            Some(ComparatorOverride::LiveComponentOutput(source_node))
        );
        assert_eq!(
            comparator_override_value(&draft, plan.direct_override.unwrap()),
            match &draft.nodes[source_node.index()].state {
                ComponentState::Electrical(state) => state.output_strength,
                _ => unreachable!(),
            }
        );

        draft.nodes[source_node.index()].state =
            ComponentState::Electrical(NodeState::with_strength(11));
        assert_eq!(
            comparator_override_value(&draft, plan.direct_override.unwrap()),
            11,
            "the override reads the component output after preparation"
        );
    }
}

#[test]
fn electrical_boundary_direct_dynamic_override_returns_rejection() {
    let mut world = empty();
    let comparator = RedstoneComparator::new(BlockDirection::South, ComparatorMode::Compare, false);
    world.set_block(BASE, Block::RedstoneComparator { comparator });
    let source = BASE.offset(BlockFace::South);
    world.set_block(source, Block::from_name("copper_bulb").unwrap());

    assert_dynamic_override_outside_inventory(&world, (BASE, BASE), BASE, source);
}

#[test]
fn electrical_boundary_far_dynamic_override_returns_rejection() {
    let mut world = empty();
    let comparator = RedstoneComparator::new(BlockDirection::South, ComparatorMode::Compare, false);
    world.set_block(BASE, Block::RedstoneComparator { comparator });
    let rear = BASE.offset(BlockFace::South);
    let source = rear.offset(BlockFace::South);
    world.set_block(rear, Block::Stone {});
    world.set_block(source, Block::from_name("command_block").unwrap());

    assert_dynamic_override_outside_inventory(&world, (BASE, rear), BASE, source);
}

fn geometry_io_fixture(name: &str) -> PlotWorld {
    let directory = root().join("test_data/instant-pistons-io");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(directory.join("download-manifest.json")).unwrap())
            .unwrap();
    let entry = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == name)
        .unwrap();
    let bytes = std::fs::read(directory.join(name)).unwrap();
    assert_eq!(entry["bytes"].as_u64().unwrap() as usize, bytes.len());
    assert_eq!(entry["sha256"], format!("{:x}", Sha256::digest(&bytes)));
    let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    let mut world = empty();
    paste_clipboard(
        &mut world,
        &clipboard,
        BASE + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z),
        false,
    );
    world
}

#[test]
fn geometry_accepts_matching_heads_for_every_facing_and_rejects_missing_heads() {
    for facing in BlockFace::values() {
        let mut world = empty();
        let piston = RedstonePiston {
            facing: facing.into(),
            sticky: true,
            extended: true,
        };
        world.set_block(BASE, Block::Piston { piston });
        world.set_block(
            BASE.offset(facing),
            Block::PistonHead {
                head: piston.into(),
            },
        );
        let parsed = prepare_geometry_for(&world).unwrap();
        assert_eq!(parsed.draft.pistons[0].facing, facing);
        assert!(parsed.draft.initial_pistons[0].extended);
        let base = parsed
            .draft
            .contacts
            .iter()
            .find(|contact| contact.position == BASE)
            .unwrap();
        assert!(base.variants.iter().any(|variant| {
            variant.view
                == GeometryView::PistonBase {
                    piston: parsed.draft.pistons[0].id,
                    extended: true,
                }
        }));
        assert!(base.variants.iter().any(|variant| {
            variant.view == GeometryView::MovingPistonBase(parsed.draft.pistons[0].id)
        }));

        let mut malformed = empty();
        malformed.set_block(BASE, Block::Piston { piston });
        assert!(matches!(
            prepare_geometry_for(&malformed),
            Err(GeometryError::Rejected(GeometryIssue::MalformedHead {
                actor,
                position,
                ..
            })) if actor == BASE && position == BASE.offset(facing)
        ));
    }
}

#[test]
fn geometry_includes_both_stationary_base_profiles_for_either_entry_pose() {
    for initially_extended in [false, true] {
        let mut world = empty();
        let piston = RedstonePiston {
            facing: BlockFace::South.into(),
            sticky: true,
            extended: initially_extended,
        };
        world.set_block(BASE, Block::Piston { piston });
        if initially_extended {
            world.set_block(
                BASE.offset(BlockFace::South),
                Block::PistonHead {
                    head: piston.into(),
                },
            );
        }

        let parsed = prepare_geometry_for(&world).unwrap();
        assert_eq!(parsed.draft.initial_pistons[0].extended, initially_extended);
        let base = parsed
            .draft
            .contacts
            .iter()
            .find(|contact| contact.position == BASE)
            .unwrap();
        for extended in [false, true] {
            let variant = base
                .variants
                .iter()
                .find(|variant| {
                    variant.view
                        == GeometryView::PistonBase {
                            piston: parsed.draft.pistons[0].id,
                            extended,
                        }
                })
                .unwrap();
            assert!(matches!(
                Block::from_id(parsed.draft.profiles[variant.profile.index()].raw_state),
                Block::Piston { piston } if piston.extended == extended
            ));
        }
    }
}

#[test]
fn geometry_keeps_empty_ordinary_far_as_fixed_context() {
    let mut world = empty();
    world.set_block(BASE, piston_block(BlockFace::South, false, false));
    let far = BASE + BlockPos::new(0, 0, 2);
    let observer = Block::Observer {
        observer: Default::default(),
    };
    world.set_block(far, observer);

    let parsed = prepare_geometry_for(&world).unwrap();
    assert_eq!(
        parsed.draft.pistons[0].lane,
        crate::redpiler::program_graph::PistonLane::Empty
    );
    let far_contact = parsed
        .draft
        .contacts
        .iter()
        .find(|contact| contact.position == far)
        .unwrap();
    assert!(far_contact.variants.iter().any(|variant| {
        variant.view == GeometryView::Fixed
            && Block::from_id(parsed.draft.profiles[variant.profile.index()].raw_state) == observer
    }));
    assert!(parsed.report.issues.iter().any(|issue| matches!(
        issue,
        AdmissionIssue::PistonRuntimeUnavailable { pos } if *pos == BASE
    )));
}

#[test]
fn geometry_groups_one_shared_payload_and_records_both_source_actors() {
    let mut world = empty();
    let first = BASE;
    let second = BASE + BlockPos::new(0, 0, 4);
    world.set_block(first, piston_block(BlockFace::South, true, false));
    world.set_block(second, piston_block(BlockFace::North, true, false));
    let payload = BASE + BlockPos::new(0, 0, 2);
    world.set_block(payload, Block::RedstoneBlock {});

    let parsed = prepare_geometry_for(&world).unwrap();
    assert_eq!(parsed.draft.resources.len(), 1);
    let resource = &parsed.draft.resources[0];
    assert_eq!(resource.source, payload);
    assert_eq!(resource.spec.contacts.len(), 3);
    assert_eq!(resource.witnesses.len(), 2);
    assert!(
        resource
            .witnesses
            .iter()
            .all(|witness| witness.operation == geometry::ResourceOperation::Pull)
    );
    assert_eq!(parsed.draft.initial_resources[0].at, resource.initial.at);
}

#[test]
fn geometry_rejects_an_ordinary_push_after_perpendicular_shared_transfer() {
    let mut blocked = empty();
    let ordinary = BASE;
    let ordinary_near = BASE + BlockPos::new(0, 0, 1);
    let sticky = BASE + BlockPos::new(-2, 0, 1);
    let source = BASE + BlockPos::new(-1, 0, 1);
    blocked.set_block(ordinary, piston_block(BlockFace::South, false, false));
    blocked.set_block(sticky, piston_block(BlockFace::East, true, false));
    blocked.set_block(source, Block::RedstoneBlock {});
    blocked.set_block(
        ordinary + BlockPos::new(0, 0, 2),
        Block::from_name("chest").unwrap(),
    );
    assert!(prepare_geometry_for(&blocked).is_ok());

    let mut world = empty();
    world.set_block(ordinary, piston_block(BlockFace::South, false, false));
    world.set_block(sticky, piston_block(BlockFace::East, true, false));
    world.set_block(source, Block::RedstoneBlock {});

    assert!(matches!(
        prepare_geometry_for(&world),
        Err(GeometryError::Rejected(GeometryIssue::OrdinaryPush {
            actor,
            position,
            block,
        })) if actor == ordinary
            && position == ordinary_near
            && block == Block::RedstoneBlock {}.get_id()
    ));

    let mut changing_head = empty();
    changing_head.set_block(ordinary, piston_block(BlockFace::South, false, false));
    changing_head.set_block(sticky, piston_block(BlockFace::East, true, false));
    changing_head.set_block(source, Block::RedstoneBlock {});
    let head_base = BASE + BlockPos::new(-1, 0, 2);
    let head_piston = RedstonePiston {
        facing: BlockFace::East.into(),
        sticky: false,
        extended: true,
    };
    changing_head.set_block(
        head_base,
        Block::Piston {
            piston: head_piston,
        },
    );
    changing_head.set_block(
        head_base.offset(BlockFace::East),
        Block::PistonHead {
            head: head_piston.into(),
        },
    );
    assert!(matches!(
        prepare_geometry_for(&changing_head),
        Err(GeometryError::Rejected(GeometryIssue::ChangingBlocker {
            actor,
            position,
        })) if actor == ordinary && position == ordinary + BlockPos::new(0, 0, 2)
    ));
}

#[test]
fn geometry_inventories_shared_or_and_adder_payloads_from_frozen_fixtures() {
    for name in ["OR_1.schem", "ADDER_1BIT.schem"] {
        let world = geometry_io_fixture(name);
        let parsed = prepare_geometry_for(&world).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert!(
            parsed
                .draft
                .resources
                .iter()
                .any(|resource| resource.spec.contacts.len() > 1),
            "{name} did not expose a shared payload group"
        );
    }
}

#[test]
fn geometry_admits_supported_inert_cubes_and_rejects_observer_transport() {
    let mut supported = empty();
    supported.set_block(BASE, piston_block(BlockFace::South, true, false));
    let near = BASE + BlockPos::new(0, 0, 1);
    supported.set_block(near, Block::from_name("white_wool").unwrap());
    let parsed = prepare_geometry_for(&supported).unwrap();
    assert_eq!(parsed.draft.resources.len(), 1);

    let mut unsupported = empty();
    unsupported.set_block(BASE, piston_block(BlockFace::South, true, false));
    let observer = Block::Observer {
        observer: Default::default(),
    };
    unsupported.set_block(near, observer);
    assert!(matches!(
        prepare_geometry_for(&unsupported),
        Err(GeometryError::Rejected(GeometryIssue::UnsupportedMaterial {
            actor,
            position,
            block,
        })) if actor == BASE && position == near && block == observer.get_id()
    ));
}

#[test]
fn geometry_distinguishes_blocked_lines_from_unsupported_pushes() {
    let mut blocked = empty();
    blocked.set_block(BASE, piston_block(BlockFace::South, false, false));
    let near = BASE + BlockPos::new(0, 0, 1);
    blocked.set_block(near, Block::from_name("chest").unwrap());
    assert!(prepare_geometry_for(&blocked).is_ok());

    let mut ordinary_push = empty();
    ordinary_push.set_block(BASE, piston_block(BlockFace::South, false, false));
    ordinary_push.set_block(near, Block::RedstoneBlock {});
    assert!(matches!(
        prepare_geometry_for(&ordinary_push),
        Err(GeometryError::Rejected(GeometryIssue::OrdinaryPush {
            actor,
            position,
            block,
        })) if actor == BASE && position == near && block == Block::RedstoneBlock {}.get_id()
    ));

    let mut longer = empty();
    longer.set_block(BASE, piston_block(BlockFace::South, true, false));
    longer.set_block(near, Block::RedstoneBlock {});
    let second = near + BlockPos::new(0, 0, 1);
    longer.set_block(second, Block::from_name("stone").unwrap());
    assert!(matches!(
        prepare_geometry_for(&longer),
        Err(GeometryError::Rejected(GeometryIssue::PossibleLongerPush {
            actor,
            position,
            block,
        })) if actor == BASE && position == second && block == Block::from_name("stone").unwrap().get_id()
    ));

    let mut escaping = empty();
    escaping.set_block(BASE, piston_block(BlockFace::South, true, false));
    escaping.set_block(near, Block::RedstoneBlock {});
    let bounds = (BASE, near);
    assert!(matches!(
        prepare_geometry(
            &escaping,
            bounds,
            &[],
            &TaskMonitor::default(),
            AnalysisLimits::default(),
        ),
        Err(GeometryError::Rejected(GeometryIssue::OutsideFootprint {
            actor,
            position,
        })) if actor == BASE && position == BASE + BlockPos::new(0, 0, 2)
    ));
}

#[test]
fn geometry_reports_unknown_external_occupancy_instead_of_air() {
    let edge = BlockPos::new(PLOT_WIDTH as i32 * 16 - 1, 30, 40);
    let mut world = empty();
    world.set_block(edge, piston_block(BlockFace::East, false, false));

    assert!(matches!(
        prepare_geometry(
            &world,
            (edge, edge),
            &[],
            &TaskMonitor::default(),
            AnalysisLimits::default(),
        ),
        Err(GeometryError::Rejected(
            GeometryIssue::UnknownExternalOccupancy { actor, position }
        )) if actor == edge && position == edge.offset(BlockFace::East)
    ));
}

#[test]
fn geometry_rejects_support_loss_and_leaves_all_entry_state_untouched() {
    let mut world = empty();
    world.set_block(BASE, piston_block(BlockFace::South, true, false));
    let near = BASE + BlockPos::new(0, 0, 1);
    world.set_block(near, Block::RedstoneBlock {});
    world.set_block(
        near + BlockPos::new(0, 1, 0),
        Block::RedstoneWire {
            wire: Default::default(),
        },
    );
    let bounds = (BASE, BASE + BlockPos::new(0, 1, 2));
    let snapshot = || {
        let blocks = [BASE, near, near + BlockPos::new(0, 1, 0)]
            .map(|position| world.get_block_raw(position));
        let entities = [BASE, near, near + BlockPos::new(0, 1, 0)]
            .map(|position| format!("{:?}", world.get_block_entity(position)));
        (
            blocks,
            entities,
            format!("{:?}", world.scheduler().iter_entries().collect::<Vec<_>>()),
            format!("{:?}", world.piston_state()),
        )
    };
    let before = snapshot();
    let result = prepare_geometry(
        &world,
        bounds,
        &[],
        &TaskMonitor::default(),
        AnalysisLimits::default(),
    );
    assert!(
        matches!(
            &result,
            Err(GeometryError::Rejected(GeometryIssue::SupportLoss {
                actor,
                changed,
                dependent,
                ..
            })) if *actor == BASE && *changed == near && *dependent == near + BlockPos::new(0, 1, 0)
        ),
        "{result:?}"
    );
    assert_eq!(snapshot(), before);

    let cancelled = TaskMonitor::default();
    cancelled.cancel();
    assert!(matches!(
        prepare_geometry(&world, bounds, &[], &cancelled, AnalysisLimits::default()),
        Err(GeometryError::Analysis(AnalysisError::Cancelled))
    ));
    assert_eq!(snapshot(), before);

    let budget = AnalysisLimits {
        max_cells: 4,
        ..AnalysisLimits::default()
    };
    assert!(matches!(
        prepare_geometry(
            &world,
            (BASE, BASE + BlockPos::new(0, 0, 2)),
            &[],
            &TaskMonitor::default(),
            budget,
        ),
        Err(GeometryError::Rejected(GeometryIssue::ContactLimit))
    ));
    assert_eq!(snapshot(), before);
}

#[test]
fn geometry_preserves_scheduled_ticks_and_saved_piston_pose() {
    let mut world = empty();
    world.set_block(BASE, piston_block(BlockFace::South, false, false));
    world.set_block(BASE + BlockPos::new(-1, 0, 0), Block::RedstoneBlock {});
    world.schedule_tick(BASE, 2, TickPriority::Normal);
    let Block::Piston { piston } = world.get_block(BASE) else {
        unreachable!();
    };
    assert!(crate::redstone::piston::should_piston_extend(
        &world,
        piston.facing,
        BASE
    ));

    let parsed = prepare_geometry_for(&world).unwrap();
    assert!(!parsed.draft.initial_pistons[0].extended);
    assert_eq!(parsed.report.pending_ticks, 1);
}

#[test]
fn geometry_rejects_moving_cells_and_missing_motion_metadata() {
    let position = BASE + BlockPos::new(0, 0, 1);
    let mut missing = empty();
    missing.set_block(
        position,
        Block::MovingPiston {
            moving: Default::default(),
        },
    );
    assert!(matches!(
        prepare_geometry_for(&missing),
        Err(GeometryError::Rejected(GeometryIssue::MissingMotionMetadata { position: found }))
            if found == position
    ));

    let mut moving = empty();
    moving.set_block(
        position,
        Block::MovingPiston {
            moving: Default::default(),
        },
    );
    moving.set_block_entity(
        position,
        mchprs_blocks::block_entities::BlockEntity::MovingPiston(Default::default()),
    );
    moving
        .piston_state_mut()
        .motions
        .push_back(mchprs_world::PistonMotion {
            pos: position,
            identity: 1,
            progress: 0.0,
            previous_progress: 0.0,
            last_tick: 0,
            carried_entity: None,
        });
    assert!(matches!(
        prepare_geometry_for(&moving),
        Err(GeometryError::Entry(AdmissionIssue::MovingPiston { pos })) if pos == position
    ));
}
