use super::*;
mod admission;
mod compatibility;
mod legalization;
mod outputs;
mod redstone_fuzz;
mod research;
use crate::plot::worldedit::{load_schematic, paste_clipboard};
use crate::plot::{PlotWorld, PLOT_WIDTH};
use crate::redpiler::{CompileError, Compiler, CompilerOptions};
use crate::world::storage::Chunk;
use mchprs_blocks::blocks::{RedstoneObserver, RedstonePistonHead};
use mchprs_world::{PistonAction, PistonEvent, TickPriority};
use serde_json::{json, Value};
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

fn observer_seed(world: &mut PlotWorld) {
    let facing = BlockFacing::South;
    world.set_block(
        BASE,
        Block::Piston {
            piston: RedstonePiston {
                facing,
                sticky: true,
                extended: true,
            },
        },
    );
    world.set_block(
        BASE.offset(BlockFace::South),
        Block::PistonHead {
            head: RedstonePistonHead {
                facing,
                sticky: true,
                short: false,
            },
        },
    );
    world.set_block(BASE + BlockPos::new(0, 0, 2), Block::RedstoneBlock);
    world.set_block(
        BASE + BlockPos::new(0, 1, 0),
        Block::Observer {
            observer: RedstoneObserver {
                facing: BlockFacing::Down,
                powered: false,
            },
        },
    );
    world.set_block(BASE + BlockPos::new(0, 2, 0), Block::Stone {});
}

#[test]
fn observer_orientation_cap_and_head_are_context_guards() {
    let mut world = empty();
    observer_seed(&mut world);
    let report = analyze_world(&world);
    assert_eq!(report.pistons[0].reset_seeds, [ResetSeed::ObserverAbove]);
    assert!(
        !report.can_compile(),
        "a reset seed is not an executable certificate"
    );
    let head_pos = BASE + BlockPos::new(0, 0, 1);
    let Block::PistonHead { mut head } = world.get_block(head_pos) else {
        unreachable!()
    };
    head.short = true;
    world.set_block(head_pos, Block::PistonHead { head });
    assert!(analyze_world(&world).pistons[0]
        .diagnostics
        .contains(&PistonDiagnostic::MissingOrMismatchedHead));
    head.short = false;
    world.set_block(head_pos, Block::PistonHead { head });
    world.set_block(BASE + BlockPos::new(0, 2, 0), Block::Glass {});
    let report = analyze_world(&world);
    assert!(report.pistons[0].reset_seeds.is_empty());
    assert!(report.pistons[0]
        .diagnostics
        .contains(&PistonDiagnostic::NonconductingObserverCap));
    world.set_block(BASE + BlockPos::new(0, 2, 0), Block::RedstoneBlock);
    assert!(analyze_world(&world).pistons[0]
        .diagnostics
        .contains(&PistonDiagnostic::PoweredObserverCap));
    world.set_block(
        BASE + BlockPos::new(0, 1, 0),
        Block::Observer {
            observer: RedstoneObserver {
                facing: BlockFacing::North,
                powered: false,
            },
        },
    );
    assert!(analyze_world(&world).pistons[0].reset_seeds.is_empty());
    world.set_block(BASE + BlockPos::new(0, 0, 1), Block::Air);
    assert!(analyze_world(&world).pistons[0]
        .diagnostics
        .contains(&PistonDiagnostic::MissingOrMismatchedHead));
}

#[test]
fn queued_work_phase_and_orphan_heads_are_admission_blockers() {
    let mut world = empty();
    world.set_block(
        BASE,
        Block::PistonHead {
            head: Default::default(),
        },
    );
    world.piston_state_mut().phase = AdvancePhase::PistonEvents;
    world.piston_state_mut().events.push_back(PistonEvent {
        pos: BASE,
        sticky: true,
        facing: BlockFace::South,
        action: PistonAction::Retract,
    });
    let report = analyze_world(&world);
    assert!(report
        .issues
        .iter()
        .any(|i| matches!(i, AdmissionIssue::UnownedPistonHead { .. })));
    assert!(report
        .issues
        .iter()
        .any(|i| matches!(i, AdmissionIssue::EntryPhase { .. })));
    assert!(report
        .issues
        .iter()
        .any(|i| matches!(i, AdmissionIssue::PendingPistonEvents { count: 1 })));
}

#[test]
fn analysis_limits_and_cancellation_are_errors_not_partial_certificates() {
    let world = empty();
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
    let mut world = world;
    observer_seed(&mut world);
    for (limits, expected) in [
        (
            AnalysisLimits {
                max_cells: 1,
                max_pistons: 10,
                ..Default::default()
            },
            AnalysisError::CellLimit,
        ),
        (
            AnalysisLimits {
                max_cells: 16_777_216,
                max_pistons: 0,
                ..Default::default()
            },
            AnalysisError::PistonLimit,
        ),
    ] {
        assert_eq!(
            analyze(
                &world,
                world.get_corners(),
                &[],
                &Default::default(),
                limits
            )
            .unwrap_err(),
            expected
        );
    }
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
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> (PlotWorld, (BlockPos, BlockPos), Value) {
    let path = root()
        .join("test_data/instant-pistons-io/fixtures")
        .join(format!("{name}.json"));
    load_fixture(&path)
}

fn load_fixture(path: &Path) -> (PlotWorld, (BlockPos, BlockPos), Value) {
    let manifest: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let bytes = std::fs::read(root().join(manifest["fixture"].as_str().unwrap())).unwrap();
    assert_eq!(manifest["sha256"], format!("{:x}", Sha256::digest(&bytes)));
    let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    let mut world = empty();
    paste_clipboard(
        &mut world,
        &clipboard,
        BASE + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z),
        false,
    );
    let last = BASE
        + BlockPos::new(
            clipboard.size_x as i32 - 1,
            clipboard.size_y as i32 - 1,
            clipboard.size_z as i32 - 1,
        );
    (world, (BASE, last), manifest)
}

fn snapshot(world: &PlotWorld, bounds: (BlockPos, BlockPos)) -> Value {
    let mut cells = Vec::new();
    for y in bounds.0.y..=bounds.1.y {
        for z in bounds.0.z..=bounds.1.z {
            for x in bounds.0.x..=bounds.1.x {
                let pos = BlockPos::new(x, y, z);
                cells.push(json!([
                    world.get_block_raw(pos),
                    world.get_block_entity(pos)
                ]));
            }
        }
    }
    json!({ "cells": cells, "pistons": world.piston_state(), "ticks": world.scheduler().iter_entries().collect::<Vec<_>>() })
}

#[test]
fn all_io_schematics_have_read_only_deterministic_inventories() {
    check_inventories("instant-pistons-io", 22);
}

#[test]
fn original_examples_and_nanotick_counterexample_are_analysis_only() {
    check_inventories("instant-pistons", 20);
}

fn check_inventories(pack: &str, minimum_count: usize) {
    let mut paths: Vec<_> = std::fs::read_dir(root().join("test_data").join(pack).join("fixtures"))
        .unwrap()
        .map(|p| p.unwrap().path())
        .collect();
    paths.sort();
    let mut checked = 0;
    for path in paths {
        let name = path.file_stem().unwrap().to_str().unwrap();
        // CPU inventories are intentionally outside the current acceptance set.
        if matches!(name, "pm1_sort" | "q2ck_lycore5_for_sorting") {
            continue;
        }
        checked += 1;
        let (world, bounds, manifest) = load_fixture(&path);
        let before = snapshot(&world, bounds);
        let report = analyze_world(&world);
        let inspection: Value = serde_json::from_slice(
            &std::fs::read(root().join(manifest["inspection"].as_str().unwrap())).unwrap(),
        )
        .unwrap();
        let cells = inspection["nonair_cells"].as_array().unwrap();
        let bases = cells
            .iter()
            .filter(|c| {
                c["state"]
                    .as_str()
                    .unwrap()
                    .starts_with("minecraft:sticky_piston[")
                    || c["state"]
                        .as_str()
                        .unwrap()
                        .starts_with("minecraft:piston[")
            })
            .count();
        assert_eq!(report.pistons.len(), bases, "{name}");
        assert_eq!(report.nonair_blocks, cells.len(), "{name}");
        assert!(
            !report.can_compile(),
            "{name}: runtime is not yet available"
        );
        assert_eq!(
            snapshot(&world, bounds),
            before,
            "{name}: analysis changed the world"
        );
        assert_eq!(
            serde_json::to_value(&report).unwrap(),
            serde_json::to_value(analyze_world(&world)).unwrap(),
            "{name}: nondeterministic report"
        );
    }
    assert!(checked >= minimum_count);
}

#[test]
fn shared_or_payloads_form_a_group_even_when_ownership_can_transfer() {
    let (world, _, _) = fixture("or_1");
    let report = analyze_world(&world);
    assert_eq!(report.pistons.len(), 3); // Includes the new output stage.
    assert!(report.payload_groups.iter().any(|g| g.members.len() == 2));
    assert!(!report.can_compile());
    let (world, _, _) = fixture("or_interpreter_illigal");
    assert!(
        !analyze_world(&world).can_compile(),
        "the excluded OR must never be admitted by a geometric seed"
    );
}

#[test]
fn supplied_reset_families_have_return_paths_and_shared_payload_closure() {
    use super::families::ResetFamily;
    for (name, family) in [
        ("instant_observer", ResetFamily::ObserverAbove),
        ("instant_down", ResetFamily::ObserverAbove),
        ("instant_torch", ResetFamily::Torch),
        ("instant_down_torch_reset", ResetFamily::Torch),
        ("instant_reset_redstone", ResetFamily::DustBelowHead),
        ("instant_reset_redstone_2", ResetFamily::LateralDust),
    ] {
        let (world, _, _) = fixture(name);
        let report = analyze_world(&world);
        let r = &report.recognition[0];
        assert!(r.is_matched(), "{name}: {r:?}");
        assert!(
            r.resets.iter().any(|reset| reset.family == family),
            "{name}: {r:?}"
        );
        assert!(
            report.group_recognition[0].has_reset_closure(),
            "{name}: {:?}",
            report.group_recognition
        );
        assert!(
            !report.can_compile(),
            "recognition alone must not activate a runtime"
        );
    }
    let (world, _, _) = fixture("or_1");
    let report = analyze_world(&world);
    assert!(
        report.recognition.iter().all(|p| p.is_matched()),
        "{:?}",
        report.recognition
    );
    assert!(
        report
            .group_recognition
            .iter()
            .all(|g| g.has_reset_closure()),
        "{:?}",
        report.group_recognition
    );
    let (world, _, _) = fixture("or_interpreter_illigal");
    let report = analyze_world(&world);
    let shared = report
        .payload_groups
        .iter()
        .position(|g| g.members.len() == 2)
        .unwrap();
    assert!(
        report.group_recognition[shared]
            .failures
            .iter()
            .any(|f| matches!(f, families::GroupFailure::ResetSupplyLost { .. })),
        "{:?}",
        report.group_recognition
    );
}

fn lever_action(world: &mut PlotWorld, pos: BlockPos, powered: bool) {
    let Block::Lever { mut lever } = world.get_block(pos) else {
        panic!("lever at {pos:?}")
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

fn local_pos(local: &Value) -> BlockPos {
    BASE + BlockPos::new(
        local[0].as_i64().unwrap() as i32,
        local[1].as_i64().unwrap() as i32,
        local[2].as_i64().unwrap() as i32,
    )
}

#[test]
fn reset_guards_reject_mutations_and_forced_power() {
    use super::families::RecognitionFailure;
    let (mut world, _, _) = fixture("instant_observer");
    let base = BASE + BlockPos::new(0, 1, 5);
    let cap = base + BlockPos::new(0, 2, 0);
    world.set_block(cap, Block::Glass {});
    assert!(analyze_world(&world).recognition[0]
        .failures
        .contains(&RecognitionFailure::NonconductingSupport { pos: cap }));
    world.set_block(cap, Block::Stone {});
    let writer = cap.offset(BlockFace::East);
    world.set_block(
        writer,
        Block::Lever {
            lever: mchprs_blocks::blocks::Lever::new(
                mchprs_blocks::blocks::LeverFace::Wall,
                mchprs_blocks::BlockDirection::East,
                false,
            ),
        },
    );
    assert!(analyze_world(&world).recognition[0]
        .failures
        .contains(&RecognitionFailure::AdditionalResetWriter { pos: writer }));
    world.set_block(writer, Block::Air);
    world.schedule_tick(base.offset(BlockFace::Top), 2, TickPriority::Normal);
    assert!(analyze_world(&world).recognition[0]
        .failures
        .iter()
        .any(|f| matches!(f, RecognitionFailure::PendingReset { .. })));

    let (world, _, _) = fixture("instant_blocked");
    assert!(analyze_world(&world).recognition[0]
        .failures
        .iter()
        .any(|f| matches!(f, RecognitionFailure::ForcedPower { .. })));
    let (mut world, _, _) = fixture("instant_reset_redstone");
    let under_head = BASE + BlockPos::new(0, 1, 6);
    world.set_block(under_head.offset(BlockFace::Bottom), Block::Air);
    assert!(analyze_world(&world).recognition[0]
        .failures
        .iter()
        .any(|f| matches!(f, RecognitionFailure::NonconductingSupport { .. })));
}

#[test]
fn ports_find_real_repeater_faces_and_separate_bud_sampling() {
    use super::ports::{ConsumerKind, UpdateKind};
    for name in [
        "instant_observer",
        "instant_torch",
        "instant_down",
        "instant_down_torch_reset",
    ] {
        let (world, _, manifest) = fixture(name);
        let repeater = &manifest["ports"]["observations"]["repeater"];
        let pos = BASE
            + BlockPos::new(
                repeater[0].as_i64().unwrap() as i32,
                repeater[1].as_i64().unwrap() as i32,
                repeater[2].as_i64().unwrap() as i32,
            );
        let report = analyze_world(&world);
        let output = report
            .ports
            .outputs
            .iter()
            .find(|o| o.consumer == pos)
            .unwrap();
        assert_eq!(
            output.kind,
            ConsumerKind::Repeater {
                delay: 2,
                locked: false
            }
        );
        assert!(output
            .dependencies
            .sources
            .iter()
            .any(|d| matches!(d.kind, topology::SourceKind::MobilePayload { .. })));
    }
    let (world, _, _) = fixture("bud_noninstantinputs");
    let report = analyze_world(&world);
    let memory = report
        .pistons
        .iter()
        .position(|p| p.pos == BASE + BlockPos::new(0, 3, 3))
        .unwrap();
    let updates = &report.ports.pistons[memory].updates;
    assert!(updates
        .iter()
        .any(|u| u.source == BASE + BlockPos::new(0, 3, 2)
            && u.kind == UpdateKind::WireNotification
            && u.independent_of_power));
    assert!(report.recognition[memory]
        .inputs
        .wires
        .contains(&(BASE + BlockPos::new(0, 6, 3))));
    assert!(!updates
        .iter()
        .any(|u| u.source == BASE + BlockPos::new(0, 6, 3)));
}

#[test]
fn dependency_budget_and_partial_context_do_not_produce_certificates() {
    let (world, _, _) = fixture("instant_observer");
    assert_eq!(
        analyze(
            &world,
            world.get_corners(),
            &[],
            &Default::default(),
            AnalysisLimits {
                max_dependency_steps: 1,
                ..Default::default()
            }
        )
        .unwrap_err(),
        AnalysisError::DependencyLimit
    );
    let base = BASE + BlockPos::new(0, 1, 5);
    let report = analyze(
        &world,
        (base, base + BlockPos::new(0, 2, 2)),
        &[],
        &Default::default(),
        Default::default(),
    )
    .unwrap();
    assert!(!report.recognition[0].is_matched());
    assert!(report.recognition[0]
        .failures
        .iter()
        .any(|f| matches!(f, families::RecognitionFailure::OutsideBounds { .. })));
}

#[test]
fn retracted_bud_storage_is_unsupported_entry_without_a_false_head_mismatch() {
    let (mut world, _, _) = fixture("bud_noninstantinputs");
    let pos = BASE + BlockPos::new(0, 3, 3);
    let Block::Piston { mut piston } = world.get_block(pos) else {
        panic!("expected the fixture's downward BUD piston");
    };
    piston.extended = false;
    world.set_block(pos, Block::Piston { piston });
    world.set_block(pos.offset(BlockFace::Bottom), Block::RedstoneBlock);
    world.set_block(pos + BlockPos::new(0, -2, 0), Block::Air);
    let report = analyze_world(&world);
    let index = report.pistons.iter().position(|p| p.pos == pos).unwrap();
    let failures = &report.recognition[index].failures;
    assert!(failures.contains(&families::RecognitionFailure::RetractedEntry));
    assert!(failures.contains(&families::RecognitionFailure::UnsampledEntry));
    assert!(!failures.contains(&families::RecognitionFailure::MismatchedHead));
}

#[test]
fn removed_palette_entries_do_not_block_compilation() {
    let mut world = empty();
    observer_seed(&mut world);
    world.flush_block_changes();
    for pos in [
        BASE,
        BASE + BlockPos::new(0, 0, 1),
        BASE + BlockPos::new(0, 0, 2),
        BASE + BlockPos::new(0, 1, 0),
    ] {
        world.set_block(pos, Block::Air);
    }
    // Retain the stone cap so the occupied-section palette fast path is tested.
    assert!(!world.get_chunk(2, 2).unwrap().requires_interpreter());
    world.flush_block_changes();
    assert!(!world.get_chunk(2, 2).unwrap().requires_interpreter());
    assert!(analyze_world(&world).can_compile());
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &world,
            world.get_corners(),
            Default::default(),
            Vec::new(),
            Default::default(),
        )
        .unwrap();
    assert!(compiler.is_active());
}

#[test]
fn failed_and_cancelled_compile_preserve_world_and_scheduler() {
    let (mut world, bounds, _) = fixture("instant_observer");
    world.schedule_tick(BASE + BlockPos::new(0, 2, 5), 3, TickPriority::High);
    let before = snapshot(&world, bounds);
    let mut compiler = Compiler::default();
    let ticks = world.scheduler().iter_entries().collect();
    assert!(matches!(
        compiler.compile(
            &world,
            world.get_corners(),
            Default::default(),
            ticks,
            Default::default()
        ),
        Err(CompileError::Unsupported(_))
    ));
    assert!(!compiler.is_active());
    assert_eq!(snapshot(&world, bounds), before);
    let monitor = std::sync::Arc::new(TaskMonitor::default());
    monitor.cancel();
    assert!(matches!(
        compiler.compile(
            &world,
            world.get_corners(),
            Default::default(),
            Vec::new(),
            monitor
        ),
        Err(CompileError::Analysis(AnalysisError::Cancelled))
    ));
    assert_eq!(snapshot(&world, bounds), before);
}

#[test]
fn recompilation_requires_reset_and_never_reuses_stale_aliases() {
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
    assert!(matches!(
        compiler.compile(
            &world,
            bounds,
            Default::default(),
            Vec::new(),
            Default::default()
        ),
        Err(CompileError::AlreadyActive)
    ));
    compiler.on_use_block(BASE);
    compiler.flush(&mut world);
    assert!(matches!(world.get_block(BASE), Block::Lever { lever } if lever.powered));
    compiler.reset(&mut world, bounds);
    world.set_block(BASE, Block::Air);
    let next = BASE + BlockPos::new(3, 0, 0);
    world.set_block(next, lever);
    compiler
        .compile(
            &world,
            bounds,
            Default::default(),
            Vec::new(),
            Default::default(),
        )
        .unwrap();
    compiler.on_use_block(next);
    compiler.flush(&mut world);
    assert_eq!(world.get_block(BASE), Block::Air);
    assert!(matches!(world.get_block(next), Block::Lever { lever } if lever.powered));
}

#[test]
fn io_only_reset_preserves_containers_closed_at_activation() {
    let mut world = empty();
    let closed = Block::Barrel {
        facing: BlockFacing::Up,
        open: false,
    };
    world.set_block(
        BASE,
        Block::Barrel {
            facing: BlockFacing::Up,
            open: true,
        },
    );
    let bounds = world.get_corners();
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &world,
            bounds,
            CompilerOptions::parse("--io-only").unwrap(),
            Vec::new(),
            Default::default(),
        )
        .unwrap();
    // The plot closes viewers only when the staged backend has succeeded.
    world.set_block(BASE, closed);
    compiler.reset(&mut world, bounds);
    assert_eq!(world.get_block(BASE), closed);
}
