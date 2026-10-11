use super::*;
mod redstone_fuzz;
mod research;

use crate::plot::worldedit::{load_schematic, paste_clipboard};
use crate::plot::{PLOT_WIDTH, PlotWorld};
use crate::redpiler::analysis::AnalysisLimits;
use crate::redpiler::{CompileError, Compiler, CompilerOptions};
use crate::world::storage::Chunk;
use mchprs_blocks::blocks::{Block, RedstonePiston};
use mchprs_blocks::{BlockFace, BlockFacing, BlockPos};
use mchprs_world::{AdvancePhase, PistonAction, TickPriority};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use super::geometry::{GeometryError, GeometryIssue, GeometryView, prepare_geometry};

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
    prepare_geometry(
        world,
        world.get_corners(),
        &world.scheduler().iter_entries().collect::<Vec<_>>(),
        &TaskMonitor::default(),
        AnalysisLimits::default(),
    )
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
