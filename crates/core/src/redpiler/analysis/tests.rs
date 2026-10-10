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
