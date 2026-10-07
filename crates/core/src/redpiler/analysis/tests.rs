use super::*;
mod ideal;
mod legalization;
mod observer_logical;
mod outputs;
mod regions;
mod research;
use crate::plot::worldedit::{load_schematic, paste_clipboard};
use crate::plot::{PlotWorld, PLOT_WIDTH};
use crate::redpiler::instant::contract::{ElectricalState, Strength, TriggerState};
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

#[test]
fn extracted_conditional_adder_logic_matches_arithmetic() {
    use crate::redstone;
    for name in ["adder_1bit", "adder_11bits"] {
        let (mut world, _, manifest) = fixture(name);
        let report = analyze_world(&world);
        let logic = crate::redpiler::instant::logic::extract(&world, &report, &Default::default())
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        eprintln!(
            "{name}: {} decisions, {} source ports",
            logic.arena.nodes.len(),
            logic.sources.len()
        );
        eprintln!(
            "ports {name}: {:?}",
            report
                .ports
                .outputs
                .iter()
                .map(|o| (o.consumer - BASE, o.group))
                .collect::<Vec<_>>()
        );
        for case in manifest["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["expectation"].is_object())
        {
            for action in case["actions"].as_array().into_iter().flatten() {
                if action["op"] == "lever" {
                    let local = &action["pos"];
                    let pos = BASE
                        + BlockPos::new(
                            local[0].as_i64().unwrap() as i32,
                            local[1].as_i64().unwrap() as i32,
                            local[2].as_i64().unwrap() as i32,
                        );
                    let Block::Lever { mut lever } = world.get_block(pos) else {
                        panic!("{action}")
                    };
                    lever.powered = action["powered"].as_bool().unwrap();
                    world.set_block(pos, Block::Lever { lever });
                }
            }
            let fired =
                logic.evaluate(|pos| redstone::source_strength(world.get_block(pos), &world, pos));
            let outputs = &manifest["ports"]["observations"]["sum_payload"];
            let outputs: Vec<_> = if outputs[0].is_number() {
                vec![outputs]
            } else {
                outputs.as_array().unwrap().iter().collect()
            };
            let mut result = 0;
            for (bit, local) in outputs.iter().enumerate() {
                let pos = BASE
                    + BlockPos::new(
                        local[0].as_i64().unwrap() as i32,
                        local[1].as_i64().unwrap() as i32,
                        local[2].as_i64().unwrap() as i32,
                    );
                let group = report
                    .payload_groups
                    .iter()
                    .find(|g| g.positions.contains(&pos))
                    .unwrap();
                if group.members.iter().any(|&id| fired[id]) {
                    result |= 1u16 << bit;
                }
            }
            assert_eq!(
                result as u64,
                case["expectation"]["sum"].as_u64().unwrap(),
                "{name} {}",
                case["id"]
            );
        }
    }
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

fn apply_adder_actions(compiled: &mut PlotWorld, compiler: &mut Compiler, case: &Value) {
    for action in case["actions"].as_array().unwrap() {
        if action["op"] == "lever" {
            let pos = local_pos(&action["pos"]);
            let powered = action["powered"].as_bool().unwrap();
            if matches!(compiled.get_block(pos), Block::Lever { lever } if lever.powered != powered)
            {
                compiler.on_use_block(pos);
                compiler.flush(compiled);
            }
        } else if action["op"] == "wait_ready" {
            for _ in 0..16 {
                compiler.tick();
                compiler.flush(compiled);
            }
        }
    }
}

#[test]
fn compiled_adder_matches_arithmetic_and_holds_each_logical_transaction() {
    for assume_instant in [false, true] {
        for optimize in [false, true] {
            for io_only in [false, true] {
                let (_, _, manifest) = fixture("adder_11bits");
                for case in manifest["cases"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|c| c["expectation"].is_object())
                {
                    let (mut world, bounds, _) = fixture("adder_11bits");
                    let mut compiler = Compiler::default();
                    compiler
                        .compile(
                            &world,
                            world.get_corners(),
                            CompilerOptions {
                                assume_instant,
                                optimize,
                                io_only,
                                ..Default::default()
                            },
                            vec![],
                            Default::default(),
                        )
                        .unwrap();
                    assert!(!compiler
                        .backend
                        .as_ref()
                        .unwrap()
                        .logical_stats()
                        .is_empty());
                    apply_adder_actions(&mut world, &mut compiler, case);
                    for _ in 0..16 {
                        compiler.tick();
                        compiler.flush(&mut world);
                    }
                    for _ in 0..8 {
                        assert_eq!(
                            regions::repeater_value(
                                &world,
                                &manifest["ports"]["observations"]["sum_repeater"],
                                BlockPos::new(0, 0, 0)
                            ) as u64,
                            case["expectation"]["sum"].as_u64().unwrap(),
                            "{}",
                            case["id"]
                        );
                        compiler.tick();
                        compiler.flush(&mut world);
                        assert!(world.piston_state().events.is_empty());
                        assert!(world.piston_state().motions.is_empty());
                    }
                    compiler.reset(&mut world, bounds);
                    assert!(world.piston_state().events.is_empty());
                    assert!(world.piston_state().motions.is_empty());
                }
            }
        }
    }
}

#[test]
fn extracted_counter_transition_matches_all_sixteen_bit_states() {
    use crate::redpiler::instant::{boolean::Variable, clocked, logic};
    let (world, _, manifest) = fixture("counter_basic");
    let report = analyze_world(&world);
    let program = clocked::recognize(&world, &report, &Default::default(), false)
        .unwrap()
        .unwrap();
    let logic = logic::extract_with_state(
        &world,
        &report,
        &Default::default(),
        program.memory.iter().map(|m| m.actor).collect(),
        Some(program.clock),
    )
    .unwrap();
    let ids: Vec<_> = manifest["ports"]["observations"]["memory"]
        .as_array()
        .unwrap()
        .iter()
        .map(|local| {
            program
                .memory
                .iter()
                .find(|m| m.base == local_pos(local))
                .unwrap()
                .actor
        })
        .collect();
    assert_eq!(program.memory.len(), ids.len());
    let mut bits = vec![false; report.pistons.len()];
    for count in 0..=u16::MAX {
        for (bit, &actor) in ids.iter().enumerate() {
            bits[actor] = count & (1 << bit) != 0;
        }
        let mut next = 0u16;
        for (bit, &actor) in ids.iter().enumerate() {
            if logic.arena.evaluate(logic.responses[actor], |v| match v {
                Variable::Signal { .. } => false,
                Variable::Memory(actor) => bits[actor],
                Variable::Actuator(_)
                | Variable::Geometry { .. }
                | Variable::Observer(_)
                | Variable::WireDot(_) => unreachable!(),
            }) {
                next |= 1 << bit;
            }
        }
        assert_eq!(
            next,
            count.wrapping_add(1),
            "counter transition from {count}"
        );
    }
}

#[test]
fn clocked_counter_rejects_missing_sampling_extra_writers_and_exposed_clock() {
    for mutation in ["sampling", "cap", "writer", "consumer"] {
        let (mut world, bounds, _) = fixture("counter_basic");
        let expected = match mutation {
            "sampling" => {
                world.set_block(BASE + BlockPos::new(3, 11, 19), Block::Air);
                "independent sampling output"
            }
            "cap" => {
                world.set_block(BASE + BlockPos::new(2, 13, 19), Block::Glass {});
                "observer-clock generator"
            }
            "writer" => {
                world.set_block(BASE + BlockPos::new(3, 10, 4), Block::Stone {});
                world.set_block(
                    BASE + BlockPos::new(3, 11, 4),
                    Block::Lever {
                        lever: mchprs_blocks::blocks::Lever::new(
                            mchprs_blocks::blocks::LeverFace::Floor,
                            mchprs_blocks::BlockDirection::East,
                            false,
                        ),
                    },
                );
                "writer"
            }
            "consumer" => {
                world.set_block(
                    BASE + BlockPos::new(4, 12, 19),
                    Block::RedstoneLamp { lit: false },
                );
                "ordinary consumer"
            }
            _ => unreachable!(),
        };
        let before = snapshot(&world, bounds);
        let mut compiler = Compiler::default();
        let error = compiler
            .compile(
                &world,
                world.get_corners(),
                Default::default(),
                Vec::new(),
                Default::default(),
            )
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{mutation}: {error}");
        assert!(!compiler.is_active());
        assert_eq!(snapshot(&world, bounds), before, "{mutation}");
    }
}

#[test]
fn compiled_counter_clock_and_storage_follow_rotated_translated_geometry() {
    use mchprs_blocks::blocks::RotateAmt;
    let (original, bounds, manifest) = fixture("counter_basic");
    let width = bounds.1.x - bounds.0.x + 1;
    let length = bounds.1.z - bounds.0.z + 1;
    for rotation in [
        RotateAmt::Rotate90,
        RotateAmt::Rotate180,
        RotateAmt::Rotate270,
    ] {
        let transform = |p: BlockPos| {
            let (x, z) = match rotation {
                RotateAmt::Rotate90 => (length - 1 - p.z, p.x),
                RotateAmt::Rotate180 => (width - 1 - p.x, length - 1 - p.z),
                RotateAmt::Rotate270 => (p.z, width - 1 - p.x),
            };
            BASE + BlockPos::new(x + 71, p.y + 20, z + 89)
        };
        let mut world = empty();
        crate::world::for_each_block_optimized(&original, bounds.0, bounds.1, |pos| {
            let mut block = original.get_block(pos);
            if block == Block::Air {
                return;
            }
            block.rotate(rotation);
            let target = transform(pos - BASE);
            world.set_block(target, block);
            if let Some(entity) = original.get_block_entity(pos) {
                world.set_block_entity(target, entity.clone());
            }
        });
        let mut compiler = Compiler::default();
        compiler
            .compile(
                &world,
                world.get_corners(),
                CompilerOptions {
                    optimize: true,
                    io_only: true,
                    ..Default::default()
                },
                Vec::new(),
                Default::default(),
            )
            .unwrap();
        let trigger = transform(local_pos(&manifest["ports"]["inputs"]["trigger"]) - BASE);
        compiler.on_use_block(trigger);
        for tick in 1..=102 {
            compiler.tick();
            compiler.flush(&mut world);
            if tick % 6 != 5 {
                continue;
            }
            let mut value = 0u16;
            for (bit, local) in manifest["ports"]["observations"]["repeater"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
            {
                let pos = transform(local_pos(local) - BASE);
                if matches!(world.get_block(pos),Block::RedstoneRepeater {repeater} if !repeater.powered)
                {
                    value |= 1 << bit;
                }
            }
            assert_eq!(value, (tick - 5) / 6, "counter {rotation:?} tick {tick}");
        }
    }
}

#[test]
fn compiled_adder_is_derived_from_rotated_and_translated_geometry() {
    use mchprs_blocks::blocks::RotateAmt;
    let (original, bounds, manifest) = fixture("adder_11bits");
    let width = bounds.1.x - bounds.0.x + 1;
    let length = bounds.1.z - bounds.0.z + 1;
    for rotation in [
        RotateAmt::Rotate90,
        RotateAmt::Rotate180,
        RotateAmt::Rotate270,
    ] {
        let shift = BlockPos::new(71, 20, 89);
        let transform = |p: BlockPos| {
            let (x, z) = match rotation {
                RotateAmt::Rotate90 => (length - 1 - p.z, p.x),
                RotateAmt::Rotate180 => (width - 1 - p.x, length - 1 - p.z),
                RotateAmt::Rotate270 => (p.z, width - 1 - p.x),
            };
            BlockPos::new(x, p.y, z) + shift
        };
        let mut cells = Vec::new();
        crate::world::for_each_block_optimized(&original, bounds.0, bounds.1, |pos| {
            let mut block = original.get_block(pos);
            block.rotate(rotation);
            if block != Block::Air {
                cells.push((
                    BASE + transform(pos - BASE),
                    block,
                    original.get_block_entity(pos).cloned(),
                ));
            }
        });
        for case in manifest["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["expectation"].is_object())
        {
            let mut compiled = empty();
            for &(pos, block, ref entity) in &cells {
                compiled.set_block(pos, block);
                if let Some(entity) = entity {
                    compiled.set_block_entity(pos, entity.clone());
                }
            }
            let mut transformed = case.clone();
            for action in transformed["actions"].as_array_mut().unwrap() {
                if action["op"] == "lever" {
                    let p = transform(local_pos(&action["pos"]) - BASE);
                    action["pos"] = json!([p.x, p.y, p.z]);
                }
            }
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &compiled,
                    compiled.get_corners(),
                    CompilerOptions {
                        optimize: true,
                        io_only: true,
                        ..Default::default()
                    },
                    Vec::new(),
                    Default::default(),
                )
                .unwrap_or_else(|e| panic!("{rotation:?} {}: {e}", case["id"]));
            apply_adder_actions(&mut compiled, &mut compiler, &transformed);
            for tick in 1..=16 {
                compiler.tick();
                compiler.flush(&mut compiled);
                if tick >= 12 {
                    let mut value = 0u16;
                    for (bit, local) in manifest["ports"]["observations"]["sum_repeater"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .enumerate()
                    {
                        let pos = BASE + transform(local_pos(local) - BASE);
                        if matches!(compiled.get_block(pos),Block::RedstoneRepeater { repeater } if !repeater.powered)
                        {
                            value |= 1 << bit;
                        }
                    }
                    assert_eq!(
                        value as u64,
                        case["expectation"]["sum"].as_u64().unwrap(),
                        "{rotation:?} {}, tick {tick}",
                        case["id"]
                    );
                }
            }
        }
    }
}

#[test]
fn unrelated_ordinary_nodes_keep_working_in_a_compiled_piston_plot() {
    let (mut world, _, _) = fixture("adder_11bits");
    let lever_pos = BlockPos::new(0, 40, 0);
    let lamp_pos = BlockPos::new(1, 40, 0);
    world.set_block(
        lever_pos,
        Block::Lever {
            lever: mchprs_blocks::blocks::Lever::new(
                mchprs_blocks::blocks::LeverFace::Floor,
                mchprs_blocks::BlockDirection::East,
                true,
            ),
        },
    );
    world.set_block(lamp_pos, Block::RedstoneLamp { lit: true });
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
    compiler.on_use_block(lever_pos);
    for _ in 0..4 {
        compiler.tick();
        compiler.flush(&mut world);
    }
    assert_eq!(
        world.get_block(lamp_pos),
        Block::RedstoneLamp { lit: false }
    );
    compiler.reset(&mut world, empty().get_corners());
    assert!(matches!(world.get_block(lever_pos),Block::Lever { lever } if !lever.powered));
}

#[test]
fn uncertified_instant_boundaries_fail_transactionally() {
    for name in ["and_3", "or_interpreter_illigal", "bud_noninstantinputs"] {
        let (world, bounds, _) = fixture(name);
        let before = snapshot(&world, bounds);
        let mut compiler = Compiler::default();
        let result = compiler.compile(
            &world,
            world.get_corners(),
            Default::default(),
            Vec::new(),
            Default::default(),
        );
        let error = result.unwrap_err().to_string();
        if name != "bud_noninstantinputs" {
            assert!(error.contains("certified instant reset owner")
                || error.contains("neither a certified observer reset nor a proven payload-following response"), "{name}: {error}");
        }
        assert!(!compiler.is_active());
        assert!(compiler.current_flags().is_none());
        assert_eq!(snapshot(&world, bounds), before, "{name}");
    }
}

#[test]
fn adders_prepare_conditional_payload_graphs_without_mutating_imports() {
    for pack in ["instant-pistons", "instant-pistons-io"] {
        let (world, bounds, _) = load_fixture(
            &root()
                .join("test_data")
                .join(pack)
                .join("fixtures/adder_11bits.json"),
        );
        let before = snapshot(&world, bounds);
        let report = analyze_world(&world);
        assert_eq!(report.pistons.len(), 142, "{pack}");
        assert_eq!(
            report
                .pistons
                .iter()
                .filter(|p| matches!(world.get_block(p.payload), Block::Wool { .. }))
                .count(),
            44,
            "{pack}"
        );
        assert!(report.recognition.iter().all(|r| !r
            .failures
            .iter()
            .any(|f| matches!(f, families::RecognitionFailure::UnsupportedPayload { .. }))));
        let candidate = graph::prepare_candidate_graph(
            &world,
            world.get_corners(),
            &[],
            &Default::default(),
            Default::default(),
        )
        .unwrap_or_else(|e| panic!("{pack}: {e}"));
        assert_eq!(candidate.summary().instant_inputs, 0);
        assert!(candidate.summary().mobile_sources > 142);
        assert_eq!(snapshot(&world, bounds), before, "{pack}");
    }
}

#[test]
fn unsupported_adder_payload_names_the_block_and_owner_without_mutation() {
    let (mut world, bounds, _) = fixture("adder_11bits");
    let payload = BASE + BlockPos::new(13, 1, 2);
    world.set_block(payload, Block::Glass {});
    let before = snapshot(&world, bounds);
    let mut compiler = Compiler::default();
    let message = compiler
        .compile(
            &world,
            world.get_corners(),
            Default::default(),
            Vec::new(),
            Default::default(),
        )
        .unwrap_err()
        .to_string();
    assert!(
        message.contains("minecraft:glass") && message.contains(&format!("{payload:?}")),
        "{message}"
    );
    assert!(!message.contains("Some("));
    assert!(!compiler.is_active());
    assert_eq!(snapshot(&world, bounds), before);
}

#[test]
fn dust_on_a_mobile_payload_is_rejected_without_activating_a_partial_program() {
    let (mut world, bounds, _) = fixture("adder_11bits");
    let dust = BASE + BlockPos::new(10, 4, 1);
    world.set_block(
        dust,
        Block::RedstoneWire {
            wire: Default::default(),
        },
    );
    let before = snapshot(&world, bounds);
    let mut compiler = Compiler::default();
    let error = compiler
        .compile(
            &world,
            world.get_corners(),
            Default::default(),
            Vec::new(),
            Default::default(),
        )
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("moving payload support") && error.contains(&format!("{dust:?}")),
        "{error}"
    );
    assert!(!compiler.is_active());
    assert_eq!(snapshot(&world, bounds), before);
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
fn candidate_graphs_preserve_mobile_aliases_ports_and_world_state_under_optimization() {
    use crate::redpiler::compile_graph::NodeType;
    use petgraph::Direction;
    for name in [
        "instant_observer",
        "instant_torch",
        "instant_down",
        "instant_down_torch_reset",
        "instant_reset_redstone",
        "instant_reset_redstone_2",
        "instant_chain",
        "or_1",
    ] {
        let (world, bounds, _) = fixture(name);
        let before = snapshot(&world, bounds);
        for optimize in [false, true] {
            for io_only in [false, true] {
                let candidate = graph::prepare_candidate_graph(
                    &world,
                    world.get_corners(),
                    &[],
                    &CompilerOptions {
                        optimize,
                        io_only,
                        ..Default::default()
                    },
                    Default::default(),
                )
                .unwrap_or_else(|e| panic!("{name} optimize={optimize} io={io_only}: {e}"));
                let summary = candidate.summary();
                assert_eq!(summary.instant_inputs, 0, "{name}");
                assert!(summary.compiled_outputs > 0, "{name}");
                assert_eq!(
                    summary.mobile_sources,
                    candidate
                        .report
                        .payload_groups
                        .iter()
                        .map(|g| g.positions.len())
                        .sum::<usize>(),
                    "{name}"
                );
                for node in candidate.graph.node_weights() {
                    if let NodeType::MobileSource { alias, .. } = node.ty {
                        assert!(node.block.is_none());
                        assert!(!node.is_removable());
                        assert_eq!(
                            node.state.output_strength,
                            if world.get_block(alias) == Block::RedstoneBlock {
                                15
                            } else {
                                0
                            }
                        );
                    }
                    if let Some((pos, _)) = node.block {
                        assert!(
                            !candidate
                                .report
                                .payload_groups
                                .iter()
                                .any(|g| g.positions.contains(&pos)),
                            "{name}: mobile block entered ordinary constant folding"
                        );
                    }
                }
                for output in &candidate.report.ports.outputs {
                    let node = candidate
                        .graph
                        .node_indices()
                        .find(|&id| {
                            candidate.graph[id]
                                .block
                                .is_some_and(|(pos, _)| pos == output.consumer)
                        })
                        .unwrap();
                    assert!(candidate.graph[node].is_output);
                    assert!(
                        candidate
                            .graph
                            .neighbors_directed(node, Direction::Incoming)
                            .any(|id| matches!(
                                candidate.graph[id].ty,
                                NodeType::InstantOutput { .. }
                            )),
                        "{name}: no logical projection reaches consumer"
                    );
                }
                assert_eq!(
                    snapshot(&world, bounds),
                    before,
                    "{name}: candidate preparation mutated the world"
                );
            }
        }
    }
}

#[test]
fn candidate_graphs_reject_uncertified_reset_groups_and_unowned_observers() {
    let (world, bounds, _) = fixture("or_interpreter_illigal");
    let before = snapshot(&world, bounds);
    let error = graph::prepare_candidate_graph(
        &world,
        world.get_corners(),
        &[],
        &Default::default(),
        Default::default(),
    )
    .unwrap_err();
    assert!(
        matches!(error, graph::GraphPreparationError::Execution(_)),
        "{error}"
    );
    let error = error.to_string();
    assert!(
        error.contains("reset") || error.contains("cycle") || error.contains("sampling"),
        "{error}"
    );
    assert_eq!(snapshot(&world, bounds), before);
    let (mut world, bounds, _) = fixture("instant_observer");
    world.set_block(
        BASE,
        Block::Observer {
            observer: RedstoneObserver {
                facing: BlockFacing::North,
                powered: false,
            },
        },
    );
    let before = snapshot(&world, bounds);
    let error = graph::prepare_candidate_graph(
        &world,
        world.get_corners(),
        &[],
        &Default::default(),
        Default::default(),
    )
    .unwrap_err();
    assert!(
        matches!(error, graph::GraphPreparationError::Execution(_)),
        "{error}"
    );
    assert!(
        error.to_string().contains("no supported region owner"),
        "{error}"
    );
    assert_eq!(snapshot(&world, bounds), before);
}

#[test]
fn recognized_families_and_graph_ports_survive_horizontal_rotation() {
    use mchprs_blocks::blocks::RotateAmt;
    for name in [
        "instant_observer",
        "instant_torch",
        "instant_down",
        "instant_down_torch_reset",
        "instant_reset_redstone",
        "instant_reset_redstone_2",
        "or_1",
    ] {
        let (original, bounds, _) = fixture(name);
        let width = bounds.1.x - bounds.0.x + 1;
        let length = bounds.1.z - bounds.0.z + 1;
        for rotation in [
            RotateAmt::Rotate90,
            RotateAmt::Rotate180,
            RotateAmt::Rotate270,
        ] {
            let mut world = empty();
            for y in bounds.0.y..=bounds.1.y {
                for z in bounds.0.z..=bounds.1.z {
                    for x in bounds.0.x..=bounds.1.x {
                        let pos = BlockPos::new(x, y, z);
                        let local = pos - BASE;
                        let (x, z) = match rotation {
                            RotateAmt::Rotate90 => (length - 1 - local.z, local.x),
                            RotateAmt::Rotate180 => (width - 1 - local.x, length - 1 - local.z),
                            RotateAmt::Rotate270 => (local.z, width - 1 - local.x),
                        };
                        let target = BASE + BlockPos::new(x, local.y, z);
                        let mut block = original.get_block(pos);
                        block.rotate(rotation);
                        world.set_block(target, block);
                        if let Some(entity) = original.get_block_entity(pos) {
                            world.set_block_entity(target, entity.clone());
                        }
                    }
                }
            }
            let candidate = graph::prepare_candidate_graph(
                &world,
                world.get_corners(),
                &[],
                &CompilerOptions {
                    optimize: true,
                    io_only: true,
                    ..Default::default()
                },
                Default::default(),
            )
            .unwrap_or_else(|e| panic!("{name} {rotation:?}: {e}"));
            assert!(candidate.report.recognition.iter().all(|p| p.is_matched()));
            assert!(!candidate.report.ports.outputs.is_empty());
        }
    }
}

#[test]
fn exposed_reset_signals_and_missing_graph_sources_are_explicit_errors() {
    let (mut world, _, _) = fixture("instant_observer");
    let cap = BASE + BlockPos::new(0, 3, 5);
    world.set_block(
        cap.offset(BlockFace::East),
        Block::RedstoneLamp { lit: false },
    );
    let error = graph::prepare_candidate_graph(
        &world,
        world.get_corners(),
        &[],
        &Default::default(),
        Default::default(),
    )
    .unwrap_err();
    assert!(
        matches!(error, graph::GraphPreparationError::Execution(_)),
        "{error}"
    );
    assert!(error.to_string().contains("ordinary consumer"), "{error}");

    let mut world = empty();
    world.set_block(BASE, Block::RedstoneLamp { lit: false });
    world.set_block(
        BASE.offset(BlockFace::North),
        Block::Lever {
            lever: mchprs_blocks::blocks::Lever::new(
                mchprs_blocks::blocks::LeverFace::Floor,
                mchprs_blocks::BlockDirection::North,
                true,
            ),
        },
    );
    let before = snapshot(&world, (BASE, BASE));
    let mut compiler = Compiler::default();
    assert!(matches!(
        compiler.compile(
            &world,
            (BASE, BASE),
            Default::default(),
            vec![],
            Default::default()
        ),
        Err(CompileError::Graph(
            crate::redpiler::compile_graph::GraphError::MissingSource { .. }
        ))
    ));
    assert!(!compiler.is_active());
    assert_eq!(snapshot(&world, (BASE, BASE)), before);
}

#[test]
fn comparator_side_ports_keep_mobile_supplies_on_the_electrical_side_channel() {
    use crate::redpiler::compile_graph::{LinkType, NodeType};
    use mchprs_blocks::blocks::{ComparatorMode, RedstoneComparator};
    use petgraph::visit::EdgeRef;
    use petgraph::Direction;
    let (mut world, _, _) = fixture("instant_observer");
    let head = BASE + BlockPos::new(0, 1, 6);
    let pos = head.offset(BlockFace::East);
    world.set_block(
        pos,
        Block::RedstoneComparator {
            comparator: RedstoneComparator::new(
                mchprs_blocks::BlockDirection::North,
                ComparatorMode::Compare,
                false,
            ),
        },
    );
    world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
    let candidate = graph::prepare_candidate_graph(
        &world,
        world.get_corners(),
        &[],
        &Default::default(),
        Default::default(),
    )
    .unwrap();
    assert!(candidate
        .report
        .ports
        .outputs
        .iter()
        .any(|o| o.consumer == pos && o.input == ports::ConsumerInput::ComparatorSide));
    let id = candidate
        .graph
        .node_indices()
        .find(|&id| candidate.graph[id].block.is_some_and(|(p, _)| p == pos))
        .unwrap();
    assert!(candidate
        .graph
        .edges_directed(id, Direction::Incoming)
        .any(|edge| edge.weight().ty == LinkType::Side
            && matches!(
                candidate.graph[edge.source()].ty,
                NodeType::InstantOutput { .. }
            )));
}

#[test]
fn shared_reset_exposure_inventory_does_not_mutate_logical_preparation() {
    use mchprs_blocks::blocks::{Lever, LeverFace};
    use mchprs_blocks::BlockDirection;
    let mut world = empty();
    observer_seed(&mut world);
    let second = BASE + BlockPos::new(1, 1, 0);
    for offset in [
        BlockPos::new(0, 0, 0),
        BlockPos::new(0, 0, 1),
        BlockPos::new(0, 0, 2),
        BlockPos::new(0, 1, 0),
        BlockPos::new(0, 2, 0),
    ] {
        world.set_block(second + offset, world.get_block(BASE + offset));
    }
    for pos in [BASE.offset(BlockFace::West), second.offset(BlockFace::East)] {
        world.set_block(
            pos,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, true),
            },
        );
        world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
    }
    let report = analyze_world(&world);
    assert!(
        report.recognition.iter().all(|r| r.is_matched()),
        "{report:?}"
    );
    assert!(report
        .ports
        .reset_exposures
        .contains(&ports::ResetExposure {
            source: BASE.offset(BlockFace::Top),
            consumer: second,
        }));
    let bounds = world.get_corners();
    let before = snapshot(&world, bounds);
    let candidate = graph::prepare_candidate_graph(
        &world,
        bounds,
        &[],
        &Default::default(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(candidate.summary().instant_inputs, 0);
    assert_eq!(snapshot(&world, bounds), before);

    // One torch can physically reach both bases. It cannot be removed as
    // independently owned reset circuitry for two separate payload protocols.
    let piston = world.get_block(BASE);
    let head = world.get_block(BASE.offset(BlockFace::South));
    let mut world = empty();
    world.set_block(BASE, Block::Stone {});
    world.set_block(
        BASE.offset(BlockFace::Top),
        Block::RedstoneTorch { lit: false },
    );
    world.set_block(
        BASE.offset(BlockFace::North),
        Block::Lever {
            lever: Lever::new(LeverFace::Wall, BlockDirection::North, true),
        },
    );
    for side in [BlockFace::West, BlockFace::East] {
        let pos = BASE.offset(side);
        world.set_block(pos, piston);
        world.set_block(pos.offset(BlockFace::South), head);
        world.set_block(pos + BlockPos::new(0, 0, 2), Block::RedstoneBlock);
    }
    let report = analyze_world(&world);
    assert!(
        report.recognition.iter().all(|r| r.is_matched()),
        "{report:?}"
    );
    assert_eq!(report.ports.reset_exposures.len(), 2);
    let bounds = world.get_corners();
    let before = snapshot(&world, bounds);
    let candidate = graph::prepare_candidate_graph(
        &world,
        bounds,
        &[],
        &Default::default(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(candidate.summary().instant_inputs, 0);
    assert_eq!(snapshot(&world, bounds), before);
}

#[test]
fn candidate_graph_requires_context_for_updates_that_do_not_provide_power() {
    let mut world = empty();
    observer_seed(&mut world);
    let lever = BASE.offset(BlockFace::West);
    world.set_block(
        lever,
        Block::Lever {
            lever: mchprs_blocks::blocks::Lever::new(
                mchprs_blocks::blocks::LeverFace::Floor,
                mchprs_blocks::BlockDirection::North,
                true,
            ),
        },
    );
    world.set_block(lever.offset(BlockFace::Bottom), Block::Stone {});
    let bounds = (
        BASE + BlockPos::new(-1, -3, -3),
        BASE + BlockPos::new(1, 4, 3),
    );
    let report = analyze(&world, bounds, &[], &Default::default(), Default::default()).unwrap();
    assert!(report.recognition[0].is_matched(), "{report:?}");
    assert!(report.recognition[0].inputs.outside_bounds.is_empty());
    assert!(!report.ports.pistons[0].outside_bounds.is_empty());
    let error = graph::prepare_candidate_graph(
        &world,
        bounds,
        &[],
        &Default::default(),
        Default::default(),
    )
    .unwrap_err();
    assert!(
        matches!(error, graph::GraphPreparationError::Execution(_)),
        "{error}"
    );
    assert!(
        error
            .to_string()
            .contains("update context outside the selection"),
        "{error}"
    );
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
        Err(CompileError::Instant(_))
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

#[test]
fn contract_edges_follow_lever_torch_delay_and_actual_repeater_consumers() {
    fn local(value: &Value) -> BlockPos {
        BASE + BlockPos::new(
            value[0].as_i64().unwrap() as i32,
            value[1].as_i64().unwrap() as i32,
            value[2].as_i64().unwrap() as i32,
        )
    }
    fn torch_strength(world: &PlotWorld, pos: BlockPos) -> Strength {
        match world.get_block(pos) {
            Block::RedstoneWallTorch { lit: true, .. } | Block::RedstoneTorch { lit: true } => {
                Strength::FULL
            }
            Block::RedstoneWallTorch { lit: false, .. } | Block::RedstoneTorch { lit: false } => {
                Strength::ZERO
            }
            block => panic!("input torch was replaced: {block:?}"),
        }
    }
    for name in ["instant_observer", "instant_torch"] {
        let (mut world, _, manifest) = fixture(name);
        let inputs = &manifest["ports"]["inputs"];
        let observations = &manifest["ports"]["observations"];
        let source = local(&inputs["trigger"]);
        let torch = local(&observations["input_torch"]);
        let output = local(&observations["raw_output"]);
        let consumer = local(&observations["repeater"]);
        let mut electrical = ElectricalState::new(torch_strength(&world, torch));
        let mut trigger = TriggerState::default();
        let Block::Lever { mut lever } = world.get_block(source) else {
            panic!("missing input lever")
        };
        lever.powered = true;
        world.set_block(source, Block::Lever { lever });
        // Match both notification paths in interaction::on_use. The lever is a
        // prepared source; the falling edge appears after its torch's delay.
        crate::redstone::update_surrounding_blocks(&mut world, source);
        crate::redstone::update_surrounding_blocks(
            &mut world,
            source.offset(lever.facing.opposite().block_face()),
        );
        assert_eq!(torch_strength(&world, torch), Strength::FULL);
        let mut accepted_at = Vec::new();
        for tick in 1..=24 {
            world.tick_interpreted();
            let change = electrical.observe(torch_strength(&world, torch));
            trigger.observe(change);
            if change.falling() {
                trigger.recheck();
            }
            if trigger.accept(true) {
                accepted_at.push(tick);
            }
            if tick == 2 {
                assert!(
                    matches!(world.get_block(output), Block::RedstoneWire { wire } if wire.power == 0)
                );
            }
            if tick == 5 {
                assert!(
                    matches!(world.get_block(consumer), Block::RedstoneRepeater { repeater } if repeater.powered)
                );
            }
            if tick == 6 {
                assert!(
                    matches!(world.get_block(consumer), Block::RedstoneRepeater { repeater } if !repeater.powered)
                );
            }
        }
        assert_eq!(
            accepted_at,
            [2],
            "{name}: held zero must not create another external wave"
        );
    }
}
