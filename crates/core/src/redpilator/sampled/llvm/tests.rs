use super::encoding::{read_u32, validate_blob};
use super::*;

#[test]
#[cfg(any(windows, target_os = "linux"))]
fn llvm_notification_tags_validate_physical_piston_routes() {
    use super::super::program::MoveStep;
    use crate::world::World;
    use mchprs_blocks::{BlockFace, BlockFacing, blocks::RedstonePiston};
    let mut world = super::super::tests::host();
    let base = BlockPos::new(15, 15, 15);
    let lamp = base + BlockPos::new(3, 0, 0);
    let wire = base + BlockPos::new(-2, 0, 0);
    let spare = base + BlockPos::new(0, 0, 3);
    world.set_block(
        base,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::North,
                sticky: true,
                extended: false,
            },
        },
    );
    world.set_block(spare, world.get_block(base));
    world.set_block(lamp, Block::RedstoneLamp { lit: false });
    world.set_block(wire.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(wire, Block::from_name("redstone_wire").unwrap());
    let mut program = super::super::compile_program(
        &world,
        (base - BlockPos::new(4, 4, 4), base + BlockPos::new(4, 4, 4)),
        super::super::ScratchEntry {
            scheduled: vec![],
            piston: world.piston_state().clone(),
        },
        None,
    )
    .unwrap();
    let piston = program.component_indices[&base];
    let tag = PISTON_HEAD_NOTIFICATION | piston;
    program.source_watchers[piston] = smallvec::smallvec![tag];
    let wire_start = program.movement_wires.len();
    let step_index = program.movement_steps.len();
    program.movement_wires.push(program.component_indices[&wire]);
    program.movement_steps.push(MoveStep {
        node: tag,
        watcher: true,
        wire: false,
        wires: wire_start..wire_start + 1,
    });
    let encoded = LlvmProgram::encode(&program).unwrap();
    let library = Library::load().unwrap();
    let native_size = |blob: &[u8]| unsafe { (library.state_size)(&encoded.descriptor, blob.as_ptr(), blob.len() as u32, 0) };
    assert!(native_size(&encoded.blob) > 0);
    let directory = encoded.descriptor.source_offset as usize + encoded.descriptor.source_count as usize * 4;
    let word_offset = |table: usize| read_u32(&encoded.blob, directory + table * 8).unwrap() as usize;
    let watcher_index = program.source_watchers[..piston].iter().map(|row| row.len()).sum::<usize>();
    for (table, index) in [(4, watcher_index), (23, step_index * 5)] {
        for invalid in [
            PISTON_HEAD_NOTIFICATION as u32 | program.component_indices[&lamp] as u32,
            PISTON_HEAD_NOTIFICATION as u32 | encoded.descriptor.component_count,
            0xc000_0000,
            u32::MAX,
        ] {
            let mut corrupt = encoded.blob.clone();
            let offset = word_offset(table) + index * 4;
            corrupt[offset..offset + 4].copy_from_slice(&invalid.to_le_bytes());
            assert!(validate_blob(&encoded.descriptor, &corrupt).is_err(), "table {table}, tag {invalid:#x}");
            assert_eq!(native_size(&corrupt), 0, "table {table}, tag {invalid:#x}");
        }
    }
    let mut corrupt = encoded.blob.clone();
    let offset = word_offset(24) + wire_start * 4;
    corrupt[offset..offset + 4].copy_from_slice(&(PISTON_HEAD_NOTIFICATION as u32 | piston as u32).to_le_bytes());
    assert!(validate_blob(&encoded.descriptor, &corrupt).is_err(), "wire IDs must remain untagged");
    assert_eq!(native_size(&corrupt), 0);
    let mut corrupt = encoded.blob.clone();
    let record = program.piston_bindings.iter().position(|binding| binding.component == piston).unwrap();
    let offset = word_offset(7) + record * 10 * 4;
    corrupt[offset..offset + 4].copy_from_slice(&(program.component_indices[&spare] as u32).to_le_bytes());
    assert!(
        validate_blob(&encoded.descriptor, &corrupt).is_err(),
        "head route must have an owning piston record"
    );
    assert_eq!(native_size(&corrupt), 0);
    if let Some(high_bit) = 1usize.checked_shl(32) {
        assert!(encode_notification(&program, high_bit | tag).is_err());
    }
}

#[test]
fn llvm_errors_distinguish_invalid_state_capacity_and_work_limits() {
    assert!(native_status_reason(1).contains("invalid runtime state"));
    assert!(native_status_reason(2).contains("unsupported runtime operation"));
    assert!(native_status_reason(3).contains("capacity exceeded"));
    assert!(native_status_reason(4).contains("per-tick operation limit"));
    assert!(native_status_reason(99).contains("unknown error"));
}

#[test]
fn unsupported_component_reports_its_position() {
    let pos = BlockPos::new(-199, 66, -175);
    let block = Block::RedstoneLamp { lit: false };
    let mut program = Program::new(vec![pos], vec![block], vec![], vec![[vec![], vec![]]], vec![], vec![], vec![]).unwrap();
    program.initial[0] = Block::Stone {};
    let error = unsupported_reason(&program).unwrap();
    assert_eq!(error.position, Some(pos));
    assert_eq!(error.reason, "stone");
    assert!(crate::redpilator::CompileError::Unsupported(error).to_string().contains("-199"));
}

#[test]
fn unsupported_components_count_all_and_keep_first_three() {
    let positions: Vec<_> = (0..5).map(|x| BlockPos::new(x, 66, 0)).collect();
    let blocks = vec![Block::RedstoneLamp { lit: false }; positions.len()];
    let mut program = Program::new(positions.clone(), blocks, vec![], vec![[vec![], vec![]]; 5], vec![], vec![], vec![]).unwrap();
    program.initial.fill(Block::Stone {});
    let (first, total) = unsupported_components(&program);
    assert_eq!(total, 5);
    assert_eq!(first.iter().map(|e| e.position.unwrap()).collect::<Vec<_>>(), positions[..3]);
    let message = crate::redpilator::CompileError::UnsupportedComponents { first, total }.to_string();
    assert!(message.contains("and 2 other unsupported LLVM components"));
}

include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/redpilator/tests/sampled/llvm_regressions.rs"));

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn llvm_read_marks_changes_without_losing_unpublished_cells() {
    let mut program = super::super::tests::repeater_program(1, false, false, false);
    program.prepare_projection();
    let control = program.positions[0];
    let mut projection = program.clone().into_runtime_prepared();
    let mut native = LlvmRuntime::new(&program, &[]).unwrap();
    native.sync_projection(&mut projection).unwrap();
    projection.mark_published(false);
    native.sync_projection(&mut projection).unwrap();
    assert!(projection.dirty_cells.is_empty());
    native.use_control(control).unwrap();
    native.sync_projection(&mut projection).unwrap();
    let dirty = projection.dirty_cells.clone();
    assert!(!dirty.is_empty());
    native.sync_projection(&mut projection).unwrap();
    assert_eq!(projection.dirty_cells, dirty);
}

#[cfg(any(windows, target_os = "linux"))]
fn compare_execution(mut program: Program, controls: &[(usize, usize)], ticks: usize) -> (super::super::Runtime, super::super::Runtime, LlvmRuntime) {
    if program.projection_positions.is_empty() || program.effective_links.len() != program.links.len() {
        program.prepare_projection();
    }
    let positions = program.positions.clone();
    let mut projection = program.clone().into_runtime_prepared();
    let mut rust = program.clone().into_runtime_prepared();
    rust.set_pending(Vec::new()).unwrap();
    let mut native = LlvmRuntime::new(&program, &[]).unwrap();
    drop(program);
    for tick in 0..ticks {
        for &(_, source) in controls.iter().filter(|&&(at, _)| at == tick) {
            rust.use_control(positions[source]).unwrap();
            native.use_control(positions[source]).unwrap();
        }
        let rust_result = rust.tick();
        let native_result = native.tick();
        assert!(
            rust_result.is_ok() && native_result.is_ok(),
            "execution tick {tick}: Rust={rust_result:?}, LLVM={native_result:?}"
        );
        let mut states = vec![0u8; positions.len()];
        assert_eq!(
            unsafe { (native.library.read_state)(native.state.as_ptr().cast(), states.as_mut_ptr(), states.len() as u32) },
            0
        );
        let expected: Vec<_> = rust.blocks.iter().map(|block| component_state(*block).1).collect();
        assert_eq!(states, expected, "state tick {tick}");
        assert_eq!(native.pending_entries().unwrap(), rust.pending_entries(), "scheduler tick {tick}");
        native.sync_projection(&mut projection).unwrap();
        assert_eq!(projection.projected_blocks(), rust.projected_blocks(), "projection tick {tick}");
        assert_eq!(projection.token_positions, rust.token_positions, "tokens tick {tick}");
        assert_eq!(projection.token_digits, rust.token_digits, "token digits tick {tick}");
        assert_eq!(projection.piston_extended, rust.piston_extended, "pistons tick {tick}");
        assert_eq!(projection.piston_head_present, rust.piston_head_present, "heads tick {tick}");
        assert_eq!(projection.piston_head_moving, rust.piston_head_moving, "moving heads tick {tick}");
        assert_eq!(projection.piston_base_moving, rust.piston_base_moving, "moving bases tick {tick}");
        assert_eq!(projection.piston_events, rust.piston_events, "piston events tick {tick}");
        assert_eq!(projection.contributions, rust.contributions, "ordinary contributions tick {tick}");
        assert_eq!(
            projection.arrangement_contributions, rust.arrangement_contributions,
            "arrangement contributions tick {tick}"
        );
        assert_eq!(projection.group_arrangements, rust.group_arrangements, "arrangements tick {tick}");
        assert_eq!(projection.routing_attenuations, rust.routing_attenuations, "routing tick {tick}");
        assert_eq!(projection.comparator_outputs, rust.comparator_outputs, "comparators tick {tick}");
        assert_eq!(
            projection.reactive_wire_strengths, rust.reactive_wire_strengths,
            "reactive wires tick {tick}"
        );
        assert_eq!(projection.notes, rust.notes, "note effects tick {tick}");
        assert_eq!(projection.command_blocks, rust.command_blocks, "command entities tick {tick}");
        let emissions = |r: &super::super::Runtime| {
            r.command_emissions
                .iter()
                .map(|event| (event.pos, event.tick, event.message.recipient.clone(), event.message.message.clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(emissions(&projection), emissions(&rust), "command emissions tick {tick}");
        // Unlimited replay still has wall-clock accounting windows, which can reset between backend calls.
        let output = |r: &super::super::Runtime| {
            r.command_output_state
                .as_ref()
                .map(|state| (state.limited.then_some((state.count, state.bytes)), state.queued, state.limited))
        };
        assert_eq!(output(&projection), output(&rust), "command output state tick {tick}");
        assert_eq!(projection.note_queued, rust.note_queued, "queued effects tick {tick}");
        assert_eq!(projection.watched_strengths, rust.watched_strengths, "watched wires tick {tick}");
        for (node, (a, b)) in projection.input_levels.iter().zip(&rust.input_levels).enumerate() {
            assert_eq!(
                (a[0].active, a[1].active),
                (b[0].active, b[1].active),
                "input counts tick {tick} node {node}"
            );
        }
        let motions = |r: &super::super::Runtime| {
            r.active_motions
                .iter()
                .map(|&slot| {
                    let m = r.motions[slot];
                    (
                        m.piston,
                        m.token,
                        m.destination,
                        m.start,
                        m.landing,
                        m.order,
                        m.extending,
                        m.piston_done,
                        m.token_done,
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(motions(&projection), motions(&rust), "motions tick {tick}");
        if positions.len() > 1000 && tick % 256 == 0 {
            eprintln!("LLVM shadow matched tick {tick}");
        }
    }
    (rust, projection, native)
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
#[ignore = "full fixed PM1 execution and Rust/LLVM snapshot comparison"]
fn llvm_fixed_pm1_execution_matches_rust() {
    use super::super::cpu_trace::cpus;
    use crate::{plot::worldedit::load_schematic, redpilator::Compiler, world::World};
    let cpu = cpus::Cpu {
        schematic: "PM1_SORT_FIXED.schem",
        sha256: "ad68a0160990c72612c0105802fade337758de127dcdbd4cc1a195059ad2d893",
        ..cpus::CPUS[0]
    };
    let mut rust_world = cpus::load_cpu(cpu);
    let mut native_world = rust_world.activation_snapshot();
    assert!(rust_world.scheduler().iter_entries().next().is_none());
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test_data")
        .join(cpu.schematic);
    let clip = load_schematic(std::fs::File::open(path).unwrap()).unwrap();
    let bounds = (
        cpu.origin,
        cpu.origin + BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1),
    );
    let started = std::time::Instant::now();
    let program = super::super::compile_program(
        &rust_world,
        bounds,
        super::super::ScratchEntry {
            scheduled: vec![],
            piston: rust_world.piston_state().clone(),
        },
        None,
    )
    .unwrap();
    eprintln!("PM1 sampled compile_ms={}", started.elapsed().as_millis());
    let start = program.component_indices[&(cpu.origin + cpu.start)];
    let stop = program.component_indices[&(cpu.origin + cpu.stop.unwrap())];
    let ticks = std::env::var("MCHPRS_LLVM_VERIFY_TICKS")
        .ok()
        .map_or(cpu.active_ticks as usize + 100, |s| s.parse().unwrap());
    let mut controls = vec![(0, start)];
    if ticks > cpu.active_ticks as usize {
        controls.push((cpu.active_ticks as usize, stop));
    }
    let (rust, projection, native) = compare_execution(program, &controls, ticks);
    let chat: Vec<_> = rust
        .command_emissions
        .iter()
        .map(|event| (event.tick as u32, event.message.message.clone()))
        .collect();
    assert!(!chat.is_empty(), "PM1 start produced no command output");
    eprintln!(
        "PM1 matched_ticks={ticks} messages={} halt_observed={} last_message={:?}",
        chat.len(),
        chat.iter().any(|(_, text)| text.contains("shut")),
        chat.last()
    );
    let mut rust_compiler = Compiler::default();
    rust_compiler.runtime = Some(rust);
    let mut native_compiler = Compiler::default();
    native_compiler.runtime = Some(projection);
    native_compiler.llvm_runtime = Some(native);
    rust_compiler.try_reset(&mut rust_world, bounds).unwrap();
    native_compiler.try_reset(&mut native_world, bounds).unwrap();
    assert_eq!(
        native_world.command_output().collect::<Vec<_>>(),
        rust_world.command_output().collect::<Vec<_>>(),
        "PM1 published messages after reset"
    );
    assert_eq!(
        cpus::checkpoint(&native_world, ticks as u32, &chat),
        cpus::checkpoint(&rust_world, ticks as u32, &chat),
        "PM1 full reset checkpoint"
    );
    eprintln!("PM1 full reset checkpoint matched");
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
#[ignore = "full ANPU native/Rust comparison; requires the LLVM DLL"]
fn llvm_anpu_matches_rust_state_and_effects_each_tick() {
    use super::super::cpu_trace::cpus;
    use crate::{plot::worldedit::load_schematic, world::World};
    let cpu = cpus::CPUS[1];
    let world = cpus::load_cpu(cpu);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test_data")
        .join(cpu.schematic);
    let clip = load_schematic(std::fs::File::open(path).unwrap()).unwrap();
    let bounds = (
        cpu.origin,
        cpu.origin + BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1),
    );
    let started = std::time::Instant::now();
    let program = super::super::compile_program(
        &world,
        bounds,
        super::super::ScratchEntry {
            scheduled: vec![],
            piston: world.piston_state().clone(),
        },
        None,
    )
    .unwrap();
    eprintln!("ANPU sampled compile_ms={}", started.elapsed().as_millis());
    let start = program.positions.iter().position(|&pos| pos == cpu.origin + cpu.start).unwrap();
    let ticks = std::env::var("MCHPRS_LLVM_VERIFY_TICKS").ok().map_or(5000, |s| s.parse().unwrap());
    compare_execution(program, &[(0, start)], ticks);
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
#[ignore = "full ZCore native/Rust snapshot comparison; requires the LLVM DLL"]
fn llvm_zcore_matches_rust_state_and_motion_each_tick() {
    mod cpus {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/test_support/cpus.rs"));
    }
    use crate::{plot::worldedit::load_schematic, world::World};
    let cpu = cpus::CPUS[3];
    let mut world = cpus::load_cpu(cpu);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test_data")
        .join(cpu.schematic);
    let clip = load_schematic(std::fs::File::open(path).unwrap()).unwrap();
    let bounds = (
        cpu.origin,
        cpu.origin + BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1),
    );
    let program = super::super::compile_program(
        &world,
        bounds,
        super::super::ScratchEntry {
            scheduled: vec![],
            piston: world.piston_state().clone(),
        },
        None,
    )
    .unwrap();
    let start = program.positions.iter().position(|&pos| pos == cpu.origin + cpu.start).unwrap();
    let run = program
        .positions
        .iter()
        .position(|&pos| pos == cpu.origin + BlockPos::new(207, 10, 154))
        .unwrap();
    cpus::click_cpu(&mut world, cpu, cpu.start);
    let mut idle = 0;
    let mut preparation = 0;
    for tick in 1..=4096 {
        world.tick_interpreted();
        let quiet =
            world.scheduler().iter_entries().next().is_none() && world.piston_state().events.is_empty() && world.piston_state().motions.is_empty();
        idle = if quiet { idle + 1 } else { 0 };
        if idle == 20 {
            preparation = tick;
            break;
        }
    }
    assert!(preparation > 0);
    let ticks = std::env::var("MCHPRS_LLVM_VERIFY_TICKS").ok().map_or(4096, |s| s.parse().unwrap());
    compare_execution(program, &[(0, start), (preparation, run)], preparation + ticks);
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn llvm_wire_strengths_torches_and_observer_notifications_match_rust() {
    if std::env::var_os("MCHPRS_LLVM_DLL").is_none()
        && !std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/native_engine.dll")
            .exists()
    {
        return;
    }
    use super::super::program::{ElectricalLink, WatchedWire};
    use mchprs_blocks::blocks::RedstoneWire;
    let initial = vec![
        Block::from_name("lever").unwrap(),
        Block::RedstoneWire {
            wire: RedstoneWire::default(),
        },
        Block::from_name("lever").unwrap(),
        Block::RedstoneLamp { lit: false },
        Block::RedstoneTorch { lit: true },
        Block::from_name("observer").unwrap(),
    ];
    let links = vec![
        ElectricalLink {
            source: 0,
            target: 1,
            attenuation: 0,
        },
        ElectricalLink {
            source: 2,
            target: 1,
            attenuation: 2,
        },
        ElectricalLink {
            source: 1,
            target: 3,
            attenuation: 0,
        },
        ElectricalLink {
            source: 1,
            target: 4,
            attenuation: 0,
        },
    ];
    let mut edges = vec![[Vec::new(), Vec::new()]; 6];
    for (source, link) in [(0, 0), (2, 1)] {
        let ops = vec![
            Operation::Contribution(link),
            Operation::RefreshWire(1),
            Operation::CheckWatchedWire(0),
            Operation::UpdateLamp(3),
            Operation::UpdateTorch(4),
        ];
        edges[source] = [ops.clone(), ops];
    }
    let mut program = Program::new(
        (0..6).map(|x| BlockPos::new(x, 15, 15)).collect(),
        initial,
        vec![0, 2],
        edges,
        links,
        vec![WatchedWire { wire: 1, watchers: vec![5] }],
        Vec::new(),
    )
    .unwrap();
    program.source_watchers[0] = smallvec::smallvec![5];
    program.source_watchers[4] = smallvec::smallvec![5];
    compare_execution(program, &[(0, 0), (4, 2), (8, 0), (12, 2), (25, 2), (26, 2), (38, 0), (39, 0)], 96);
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn llvm_repeater_side_locks_and_lock_observers_match_rust() {
    if std::env::var_os("MCHPRS_LLVM_DLL").is_none()
        && !std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/native_engine.dll")
            .exists()
    {
        return;
    }
    use super::super::program::{ElectricalLink, RepeaterRecord};
    use mchprs_blocks::blocks::RedstoneRepeater;
    let initial = vec![
        Block::from_name("lever").unwrap(),
        Block::from_name("lever").unwrap(),
        Block::RedstoneRepeater {
            repeater: RedstoneRepeater::default(),
        },
        Block::from_name("observer").unwrap(),
        Block::RedstoneRepeater {
            repeater: RedstoneRepeater::default(),
        },
    ];
    let a = vec![Operation::Contribution(0), Operation::UpdateRepeater(2)];
    let b = vec![Operation::Contribution(1), Operation::UpdateRepeater(4)];
    let c = vec![Operation::UpdateRepeater(2)];
    let mut program = Program::new(
        (0..5).map(|x| BlockPos::new(x, 15, 15)).collect(),
        initial,
        vec![0, 1],
        vec![[a.clone(), a], [b.clone(), b], [vec![], vec![]], [vec![], vec![]], [c.clone(), c]],
        vec![
            ElectricalLink {
                source: 0,
                target: 2,
                attenuation: 0,
            },
            ElectricalLink {
                source: 1,
                target: 4,
                attenuation: 0,
            },
        ],
        Vec::new(),
        vec![
            RepeaterRecord {
                component: 2,
                output_is_diode: false,
                side_sources: Some(vec![4]),
            },
            RepeaterRecord {
                component: 4,
                output_is_diode: false,
                side_sources: None,
            },
        ],
    )
    .unwrap();
    program.source_watchers[2] = smallvec::smallvec![3];
    compare_execution(program, &[(0, 1), (4, 0), (8, 1), (13, 1), (17, 0), (21, 1), (32, 0), (34, 0)], 96);
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn llvm_compressed_concrete_axes_preserve_motion_and_wire_state() {
    use crate::world::World;
    use mchprs_blocks::{
        BlockDirection, BlockFace, BlockFacing,
        blocks::{Lever, LeverFace, RedstonePiston},
    };
    let mut world = super::super::tests::host();
    let base = BlockPos::new(15, 15, 15);
    let lever = base.offset(BlockFace::East);
    let upper = BlockPos::new(15, 14, 15);
    let lower = BlockPos::new(15, 13, 15);
    world.set_block(
        lever,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    world.set_block(lever.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        base,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::Down,
                sticky: true,
                extended: false,
            },
        },
    );
    world.set_block(upper, Block::from_name("black_concrete").unwrap());
    world.set_block(lower.offset(BlockFace::Bottom), Block::from_name("redstone_wire").unwrap());
    world.set_block(BlockPos::new(16, 12, 15), Block::Stone {});
    world.set_block(BlockPos::new(16, 13, 15), Block::from_name("redstone_wire").unwrap());
    world.set_block(BlockPos::new(14, 12, 15), Block::RedstoneBlock);
    for pos in [lower.offset(BlockFace::Bottom), BlockPos::new(16, 13, 15)] {
        let wire = crate::redstone::wire::get_state_for_placement(&world, pos);
        world.set_block(pos, Block::RedstoneWire { wire });
    }
    let program = super::super::compile_program(
        &world,
        (BlockPos::new(10, 10, 10), BlockPos::new(20, 20, 20)),
        super::super::ScratchEntry {
            scheduled: vec![],
            piston: world.piston_state().clone(),
        },
        None,
    )
    .unwrap();
    assert!(
        program
            .movement_groups
            .iter()
            .flat_map(|group| &group.axes)
            .any(|axis| axis.states.len() == 3 && axis.states.iter().max() == Some(&1))
    );
    let source = program.positions.iter().position(|&pos| pos == lever).unwrap();
    compare_execution(
        program,
        &[(0, source), (6, source), (12, source), (13, source), (20, source), (28, source)],
        40,
    );
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn llvm_empty_head_notifies_quasi_powered_piston_immediately() {
    use crate::world::World;
    use mchprs_blocks::{
        BlockDirection, BlockFace, BlockFacing,
        blocks::{Lever, LeverFace, RedstonePiston},
    };
    let mut world = super::super::tests::host();
    let driver = BlockPos::new(10, 10, 10);
    let bud = BlockPos::new(9, 10, 9);
    let lever = BlockPos::new(10, 10, 11);
    world.set_block(
        lever,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    world.set_block(lever.offset(BlockFace::Bottom), Block::Stone {});
    for (pos, facing) in [(driver, BlockFacing::North), (bud, BlockFacing::Down)] {
        world.set_block(
            pos,
            Block::Piston {
                piston: RedstonePiston {
                    facing,
                    sticky: true,
                    extended: false,
                },
            },
        );
    }
    world.set_block(bud + BlockPos::new(0, 2, 0), Block::RedstoneBlock);
    let program = super::super::compile_program(
        &world,
        (BlockPos::new(5, 5, 5), BlockPos::new(15, 15, 15)),
        super::super::ScratchEntry {
            scheduled: vec![],
            piston: world.piston_state().clone(),
        },
        None,
    )
    .unwrap();
    let source = program.positions.iter().position(|&pos| pos == lever).unwrap();
    compare_execution(program, &[(0, source), (5, source), (10, source), (11, source)], 32);
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn llvm_moving_redstone_block_and_observers_match_rust() {
    if std::env::var_os("MCHPRS_LLVM_DLL").is_none()
        && !std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/native_engine.dll")
            .exists()
    {
        return;
    }
    use crate::world::World;
    use mchprs_blocks::blocks::RedstonePiston;
    use mchprs_blocks::{BlockFace, BlockFacing};
    for early in [false, true] {
        let mut world = super::super::tests::host();
        let lever = BlockPos::new(15, 15, 15);
        let base = lever.offset(BlockFace::East);
        let home = base.offset(BlockFace::East);
        world.set_block(lever, Block::from_name("lever").unwrap());
        world.set_block(lever.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(
            base,
            Block::Piston {
                piston: RedstonePiston {
                    facing: BlockFacing::East,
                    sticky: true,
                    extended: false,
                },
            },
        );
        world.set_block(home, Block::RedstoneBlock);
        let lamp = home.offset(BlockFace::East).offset(BlockFace::East);
        world.set_block(lamp, Block::RedstoneLamp { lit: false });
        let Block::Observer { mut observer } = Block::from_name("observer").unwrap() else {
            unreachable!()
        };
        observer.facing = BlockFacing::North;
        world.set_block(lamp.offset(BlockFace::South), Block::Observer { observer });
        let program = super::super::compile_program(
            &world,
            (BlockPos::new(10, 10, 10), BlockPos::new(23, 20, 20)),
            super::super::ScratchEntry {
                scheduled: vec![],
                piston: world.piston_state().clone(),
            },
            None,
        )
        .unwrap();
        let source = program.positions.iter().position(|&pos| pos == lever).unwrap();
        assert!(!program.arrangement_links.is_empty());
        let controls = if early {
            vec![(0, source), (1, source), (8, source), (9, source), (20, source), (27, source)]
        } else {
            vec![(0, source), (7, source), (15, source), (23, source), (32, source), (41, source)]
        };
        compare_execution(program, &controls, 64);
    }
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn llvm_repeaters_match_rust_delays_pulses_and_pending_work() {
    if std::env::var_os("MCHPRS_LLVM_DLL").is_none()
        && !std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/native_engine.dll")
            .exists()
    {
        return;
    }
    for delay in 1..=4 {
        for locked in [false, true] {
            for diode in [false, true] {
                for asymmetric in [false, true] {
                    let mut program = super::super::tests::repeater_program(delay, false, locked, diode);
                    if asymmetric {
                        program.output_programs[0][0] = 0..0;
                    }
                    program.prepare_projection();
                    let mut rust = program.clone().into_runtime_prepared();
                    rust.set_pending(Vec::new()).unwrap();
                    let control = program.positions[0];
                    let mut native = LlvmRuntime::new(&program, &[]).unwrap();
                    for tick in 0..96 {
                        if [0, 1, 12, 32, 35, 60].contains(&tick) {
                            rust.use_control(control).unwrap();
                            native.use_control(control).unwrap();
                        }
                        rust.tick().unwrap();
                        native.tick().unwrap();
                        let mut states = [0u8; 3];
                        assert_eq!(
                            unsafe { (native.library.read_state)(native.state.as_ptr().cast(), states.as_mut_ptr(), 3) },
                            0
                        );
                        let expected: Vec<_> = rust.blocks.iter().map(|block| component_state(*block).1).collect();
                        assert_eq!(states.as_slice(), expected, "delay={delay} locked={locked} diode={diode} tick={tick}");
                        assert_eq!(native.pending_entries().unwrap(), rust.pending_entries(), "pending at tick {tick}");
                    }
                }
            }
        }
    }
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn llvm_comparator_analog_levels_and_scheduling_match_rust() {
    use super::super::program::{ComparatorRecord, ElectricalLink};
    use mchprs_blocks::blocks::ComparatorMode;
    for mode in [ComparatorMode::Compare, ComparatorMode::Subtract] {
        for stored in [0, 5, 15] {
            let Block::RedstoneComparator { mut comparator } = Block::from_name("comparator").unwrap() else {
                unreachable!()
            };
            comparator.mode = mode;
            comparator.powered = stored != 0;
            let rear = vec![Operation::Contribution(0), Operation::UpdateComparator(2)];
            let side = vec![Operation::UpdateComparator(2)];
            let out = vec![Operation::Contribution(1), Operation::UpdateLamp(3)];
            let mut program = Program::new(
                (0..4).map(|x| BlockPos::new(x, 15, 15)).collect(),
                vec![
                    Block::from_name("lever").unwrap(),
                    Block::from_name("lever").unwrap(),
                    Block::RedstoneComparator { comparator },
                    Block::RedstoneLamp { lit: stored != 0 },
                ],
                vec![0, 1],
                vec![[rear.clone(), rear], [side.clone(), side], [out.clone(), out], [vec![], vec![]]],
                vec![
                    ElectricalLink {
                        source: 0,
                        target: 2,
                        attenuation: 5,
                    },
                    ElectricalLink {
                        source: 2,
                        target: 3,
                        attenuation: 0,
                    },
                ],
                vec![],
                vec![],
            )
            .unwrap();
            program.comparators.push(ComparatorRecord {
                component: 2,
                stored,
                rear_override: None,
                far_override: None,
                output_is_diode: false,
            });
            program.side_links.push(ElectricalLink {
                source: 1,
                target: 2,
                attenuation: 8,
            });
            program.prepare_projection();
            let mut rust = program.clone().into_runtime_prepared();
            let mut native = LlvmRuntime::new(&program, &[]).unwrap();
            let mut handoff: Option<super::super::Runtime> = None;
            for tick in 0..32 {
                for &(_, source) in [(0, 0), (4, 1), (8, 0), (12, 0), (16, 1), (20, 0)].iter().filter(|&&(at, _)| at == tick) {
                    let pos = program.positions[source];
                    rust.use_control(pos).unwrap();
                    native.use_control(pos).unwrap();
                    if let Some(runtime) = &mut handoff {
                        runtime.use_control(pos).unwrap();
                    }
                }
                rust.tick().unwrap();
                native.tick().unwrap();
                if let Some(runtime) = &mut handoff {
                    runtime.tick().unwrap();
                }
                if tick == 1 {
                    let mut runtime = program.clone().into_runtime_prepared();
                    native.sync_publication(&mut runtime).unwrap();
                    native.sync_projection(&mut runtime).unwrap();
                    handoff = Some(runtime);
                }
                let mut projection = program.clone().into_runtime_prepared();
                native.sync_projection(&mut projection).unwrap();
                for runtime in std::iter::once(&projection).chain(handoff.iter()) {
                    assert_eq!(runtime.projected_blocks(), rust.projected_blocks(), "tick {tick}");
                    assert_eq!(runtime.input_strength(2), rust.input_strength(2), "rear tick {tick}");
                    assert_eq!(
                        runtime.input_levels[2][1].maximum(&runtime.input_histograms),
                        rust.input_levels[2][1].maximum(&rust.input_histograms),
                        "side tick {tick}"
                    );
                    assert_eq!(runtime.comparator_outputs, rust.comparator_outputs, "output tick {tick}");
                    assert_eq!(runtime.pending_entries(), rust.pending_entries(), "pending tick {tick}");
                }
            }
        }
    }
}

#[test]
fn descriptor_layout_and_bounds_are_stable() {
    assert_eq!(std::mem::size_of::<NativeApiDescriptor>(), 16);
    assert_eq!(std::mem::align_of::<NativeApiDescriptor>(), 8);
    assert_eq!(std::mem::offset_of!(NativeApiDescriptor, semantic_version), 8);
    assert_eq!(std::mem::size_of::<NativePendingTick>(), 12);
    assert_eq!(std::mem::size_of::<NativeProgramDescriptor>(), 52);
    assert_eq!(std::mem::align_of::<NativeProgramDescriptor>(), 4);
    assert_eq!(std::mem::offset_of!(NativeProgramDescriptor, operation_offset), 12);
    assert_eq!(std::mem::offset_of!(NativeProgramDescriptor, component_offset), 28);
    assert_eq!(std::mem::offset_of!(NativeProgramDescriptor, source_count), 40);
    assert_eq!(std::mem::offset_of!(NativeProgramDescriptor, watched_wire_count), 48);
    let descriptor = NativeProgramDescriptor {
        abi_version: ABI_VERSION,
        header_bytes: HEADER_BYTES as u32,
        total_bytes: (HEADER_BYTES + AUX_TABLES * 8) as u32,
        operation_offset: HEADER_BYTES as u32,
        operation_count: 0,
        link_offset: HEADER_BYTES as u32,
        link_count: 0,
        component_offset: HEADER_BYTES as u32,
        component_count: 0,
        source_offset: HEADER_BYTES as u32,
        source_count: 0,
        arrangement_link_count: 0,
        watched_wire_count: 0,
    };
    let mut blob = Vec::from(*b"MCHN");
    blob.extend_from_slice(&ABI_VERSION.to_le_bytes());
    blob.extend_from_slice(&(HEADER_BYTES as u32).to_le_bytes());
    blob.extend_from_slice(&((HEADER_BYTES + AUX_TABLES * 8) as u32).to_le_bytes());
    for _ in 0..AUX_TABLES {
        blob.extend_from_slice(&((HEADER_BYTES + AUX_TABLES * 8) as u32).to_le_bytes());
        blob.extend_from_slice(&0u32.to_le_bytes());
    }
    assert!(validate_blob(&descriptor, &blob).is_ok());
    let mut corrupt = descriptor;
    corrupt.link_offset = u32::MAX;
    assert!(validate_blob(&corrupt, &blob).is_err());
}

#[test]
fn aligned_blob_preserves_bytes_and_zero_pads_the_tail() {
    for length in 0..=16 {
        let bytes: Vec<u8> = (0..length).collect();
        let program = LlvmProgram {
            descriptor: NativeProgramDescriptor::default(),
            blob: bytes.clone(),
        };
        let aligned = program.into_aligned_blob().unwrap();
        let decoded: Vec<u8> = aligned.iter().flat_map(|word| word.to_ne_bytes()).collect();
        assert_eq!(&decoded[..bytes.len()], bytes);
        assert!(decoded[bytes.len()..].iter().all(|&byte| byte == 0));
        assert_eq!(aligned.as_ptr() as usize % std::mem::align_of::<u32>(), 0);
    }
}
