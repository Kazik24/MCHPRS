use super::*;
use crate::plot::{PLOT_WIDTH, PlotWorld};
use crate::redpiler::scratch::ScratchError;
use crate::world::{World, storage::Chunk};
use mchprs_blocks::blocks::{ButtonFace, Lever, LeverFace, RedstonePiston, StoneButton};
use mchprs_blocks::{BlockDirection, BlockFace, BlockFacing, BlockPos, block_entities::BlockEntity, blocks::Block};
use mchprs_world::{AdvancePhase, TickEntry, TickPriority};
use std::sync::Arc;

#[test]
#[cfg(any(windows, target_os = "linux"))]
fn compiled_pressure_plate_edges_match_native_for_both_backends_and_publication_modes() {
    use mchprs_blocks::blocks::RedstoneObserver;
    let plate = BlockPos::new(15, 15, 15);
    let wire = plate.offset(BlockFace::East);
    let lamp = wire.offset(BlockFace::East);
    let observer = plate.offset(BlockFace::West);
    let observed_lamp = observer.offset(BlockFace::West);
    let bounds = (BlockPos::new(10, 10, 10), BlockPos::new(20, 20, 20));
    for name in ["stone_pressure_plate", "oak_pressure_plate", "polished_blackstone_pressure_plate"] {
        for initially_powered in [false, true] {
            let make = || {
                let (mut world, old_button, old_lamp, _) = button_world(ButtonFace::Floor, BlockDirection::North);
                world.set_block(old_button, Block::Air);
                world.set_block(old_lamp, Block::Air);
                world.set_block(plate, Block::from_name(name).unwrap().with_pressure_plate_power(false).unwrap());
                world.set_block(wire.offset(BlockFace::Bottom), Block::Stone {});
                world.set_block(wire, Block::from_name("redstone_wire").unwrap());
                world.set_block(lamp, Block::RedstoneLamp { lit: false });
                world.set_block(
                    observer,
                    Block::Observer {
                        observer: RedstoneObserver {
                            facing: BlockFacing::East,
                            powered: false,
                        },
                    },
                );
                world.set_block(observed_lamp, Block::RedstoneLamp { lit: false });
                if initially_powered {
                    let block = world.get_block(plate);
                    crate::redstone::set_pressure_plate(&mut world, plate, block, true);
                }
                for _ in 0..6 {
                    world.tick_interpreted();
                }
                world
            };
            for llvm in [false, true] {
                for io_only in [false, true] {
                    let mut native = make();
                    let mut compiled = make();
                    let mut compiler = Compiler::default();
                    compiler
                        .compile(
                            &compiled,
                            bounds,
                            CompilerOptions {
                                llvm,
                                io_only,
                                publication: Some(PublicationMode::All),
                                ..Default::default()
                            },
                            compiled.scheduler().iter_entries().collect(),
                            Default::default(),
                        )
                        .unwrap();
                    // Activation transfers scheduled work to the compiler.
                    compiled.clear_scheduled_ticks();
                    assert!(compiler.accepts_control(plate));
                    compiler.try_on_use_block(plate).unwrap();
                    compiler.try_flush(&mut compiled).unwrap();
                    assert_eq!(compiled.get_block(plate), native.get_block(plate), "right-click must not press a plate");
                    for tick in 0..=30 {
                        let edge = match tick {
                            0 | 1 => Some(!initially_powered),
                            5 => Some(initially_powered),
                            9 => Some(!initially_powered),
                            _ => None,
                        };
                        if let Some(powered) = edge {
                            if native.get_block(plate).pressure_plate_powered() != Some(powered) {
                                let block = native.get_block(plate);
                                crate::redstone::set_pressure_plate(&mut native, plate, block, powered);
                            }
                            compiler.try_set_pressure_plate(plate, powered).unwrap();
                            compiler.try_flush(&mut compiled).unwrap();
                        }
                        for pos in [plate, lamp, observed_lamp]
                            .into_iter()
                            .chain((!io_only).then_some(wire))
                            .chain((!io_only).then_some(observer))
                        {
                            assert_eq!(
                                compiled.get_block(pos),
                                native.get_block(pos),
                                "{name} initial={initially_powered} llvm={llvm} io={io_only} tick={tick} {pos:?}"
                            );
                        }
                        assert!(compiler.is_active());
                        native.tick_interpreted();
                        compiler.tick().unwrap();
                        compiler.try_flush(&mut compiled).unwrap();
                    }
                    compiler.try_reset(&mut compiled, bounds).unwrap();
                    for pos in [plate, wire, lamp, observer, observed_lamp] {
                        assert_eq!(compiled.get_block(pos), native.get_block(pos));
                    }
                    assert_eq!(
                        compiled.scheduler().iter_entries().collect::<Vec<_>>(),
                        native.scheduler().iter_entries().collect::<Vec<_>>(),
                        "{name} initial={initially_powered} llvm={llvm} io={io_only} reset pending work"
                    );
                }
            }
        }
    }
}

#[test]
fn compile_error_recovery_matches_the_failure_type() {
    for (reason, hint) in [
        ("LLVM does not support command blocks", Some("without --llvm")),
        ("scratch entry has piston motion", Some("settle")),
        ("LLVM DLL ABI/version mismatch", Some("rebuild")),
        ("scratch probe read outside captured boundary", Some("entire connected circuit")),
        ("unsupported piston input source", None),
    ] {
        let error = CompileError::Unsupported(ScratchError { position: None, reason });
        match hint {
            Some(hint) => assert!(error.recovery_hint().unwrap().contains(hint)),
            None => assert!(error.recovery_hint().is_none()),
        }
    }
}

fn button_world(face: ButtonFace, facing: BlockDirection) -> (PlotWorld, BlockPos, BlockPos, (BlockPos, BlockPos)) {
    let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
    let mut world = PlotWorld::from_chunks(0, 0, chunks, TickScheduler::default());
    let button_pos = BlockPos::new(15, 15, 15);
    let support = match face {
        ButtonFace::Floor => button_pos.offset(BlockFace::Bottom),
        ButtonFace::Ceiling => button_pos.offset(BlockFace::Top),
        ButtonFace::Wall => button_pos.offset(facing.opposite().block_face()),
    };
    let lamp_pos = match face {
        ButtonFace::Ceiling => button_pos.offset(BlockFace::Bottom),
        _ => button_pos.offset(BlockFace::Top),
    };
    world.set_block(support, Block::Stone {});
    world.set_block(
        button_pos,
        Block::StoneButton {
            button: StoneButton::new(face, facing, false),
        },
    );
    world.set_block(lamp_pos, Block::RedstoneLamp { lit: false });
    let bounds = (button_pos + BlockPos::new(-3, -3, -3), button_pos + BlockPos::new(3, 3, 3));
    (world, button_pos, lamp_pos, bounds)
}

fn compile_button(world: &PlotWorld, bounds: (BlockPos, BlockPos)) -> Compiler {
    let mut compiler = Compiler::default();
    compiler
        .compile(world, bounds, CompilerOptions::default(), Vec::new(), Arc::new(TaskMonitor::default()))
        .unwrap();
    compiler
}

#[test]
fn llvm_handoff_errors_identify_pending_pistons_and_partial_ticks() {
    let (mut world, pos, _, bounds) = button_world(ButtonFace::Floor, BlockDirection::North);
    world.piston_state_mut().events.push_back(mchprs_world::PistonEvent {
        pos,
        sticky: false,
        facing: BlockFace::East,
        action: mchprs_world::PistonAction::Extend,
    });
    let compile = |world: &PlotWorld| {
        Compiler::default().compile(
            world,
            bounds,
            CompilerOptions {
                llvm: true,
                ..Default::default()
            },
            Vec::new(),
            Default::default(),
        )
    };
    let Err(CompileError::Unsupported(error)) = compile(&world) else {
        panic!("pending piston was accepted")
    };
    assert_eq!(error.position, Some(pos));
    world.piston_state_mut().events.clear();
    let moving = pos.offset(BlockFace::East);
    world.piston_state_mut().motions.push_back(mchprs_world::PistonMotion {
        pos: moving,
        identity: 1,
        progress: 0.5,
        previous_progress: 0.0,
        last_tick: 0,
        carried_entity: None,
    });
    let Err(CompileError::Unsupported(error)) = compile(&world) else {
        panic!("moving piston was accepted")
    };
    assert_eq!(error.position, Some(moving));
    world.piston_state_mut().motions.clear();
    world.piston_state_mut().phase = AdvancePhase::ScheduledTicks;
    let Err(CompileError::Unsupported(error)) = compile(&world) else {
        panic!("partial tick was accepted")
    };
    assert_eq!(error.position, None);
    assert!(error.reason.contains("scheduled ticks"));
}

#[test]
#[cfg(any(windows, target_os = "linux"))]
fn llvm_accepts_completed_interpreter_tick_with_scheduler_advanced() {
    let (mut world, _, _, bounds) = button_world(ButtonFace::Floor, BlockDirection::North);
    world.tick_interpreted();
    assert_eq!(world.piston_state().phase, AdvancePhase::BetweenTicks);
    assert!(world.piston_state().scheduled_advanced);
    Compiler::default()
        .compile(
            &world,
            bounds,
            CompilerOptions {
                llvm: true,
                ..Default::default()
            },
            world.scheduler().iter_entries().collect(),
            Default::default(),
        )
        .unwrap();
}

#[test]
fn compiled_button_matches_native_pulse_repeat_and_rearm() {
    let (mut native, pos, lamp, bounds) = button_world(ButtonFace::Floor, BlockDirection::North);
    let (compiled, _, _, _) = button_world(ButtonFace::Floor, BlockDirection::North);
    let mut compiler = compile_button(&compiled, bounds);

    crate::interaction::use_redstone(native.get_block(pos), &mut native, pos, None);
    compiler.try_on_use_block(pos).unwrap();
    let mut compiled = compiled;
    compiler.try_flush(&mut compiled).unwrap();
    assert!(matches!(compiled.get_block(lamp), Block::RedstoneLamp { lit: true }));
    for tick in 1..=22 {
        if tick == 9 {
            crate::interaction::use_redstone(native.get_block(pos), &mut native, pos, None);
            compiler.try_on_use_block(pos).unwrap();
        }
        native.tick_interpreted();
        compiler.tick().unwrap();
        compiler.try_flush(&mut compiled).unwrap();
        assert_eq!(native.get_block(pos), compiled.get_block(pos), "button at tick {tick}");
        assert_eq!(native.get_block(lamp), compiled.get_block(lamp), "lamp at tick {tick}");
    }
    assert_eq!(
        native.get_block(pos),
        Block::StoneButton {
            button: StoneButton::new(ButtonFace::Floor, BlockDirection::North, false)
        }
    );

    crate::interaction::use_redstone(native.get_block(pos), &mut native, pos, None);
    compiler.try_on_use_block(pos).unwrap();
    for tick in 1..=20 {
        native.tick_interpreted();
        compiler.tick().unwrap();
        compiler.try_flush(&mut compiled).unwrap();
        assert_eq!(native.get_block(pos), compiled.get_block(pos), "rearmed button at tick {tick}");
    }
}

#[test]
#[cfg(any(windows, target_os = "linux"))]
fn compiled_non_control_use_is_noop_for_both_backends() {
    for llvm in [false, true] {
        let (mut world, button, lamp, bounds) = button_world(ButtonFace::Floor, BlockDirection::North);
        let mut compiler = Compiler::default();
        compiler
            .compile(&world, bounds, CompilerOptions { llvm, ..Default::default() }, vec![], Default::default())
            .unwrap();
        for pos in [lamp, bounds.1 + BlockPos::new(1, 1, 1)] {
            assert!(!compiler.accepts_control(pos));
            compiler.try_on_use_block(pos).unwrap();
        }
        compiler.try_flush(&mut world).unwrap();
        assert!(compiler.is_active());
        assert!(compiler.runtime.as_ref().unwrap().pending_entries().is_empty());
        assert_eq!(world.get_block(lamp), Block::RedstoneLamp { lit: false });
        compiler.try_on_use_block(button).unwrap();
        compiler.try_flush(&mut world).unwrap();
        assert_eq!(world.get_block(lamp), Block::RedstoneLamp { lit: true });
    }
}

#[test]
fn llvm_dll_executes_button_lamp_and_hands_off_reset_tick() {
    if std::env::var_os("MCHPRS_LLVM_DLL").is_none()
        && !std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/native_engine.dll")
            .exists()
    {
        return;
    }
    let (mut reference, button, lamp, bounds) = button_world(ButtonFace::Floor, BlockDirection::North);
    let (mut candidate, _, _, _) = button_world(ButtonFace::Floor, BlockDirection::North);
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &candidate,
            bounds,
            CompilerOptions {
                llvm: true,
                ..Default::default()
            },
            Vec::new(),
            Arc::new(TaskMonitor::default()),
        )
        .unwrap();
    assert!(compiler.is_active());
    crate::interaction::use_redstone(reference.get_block(button), &mut reference, button, None);
    compiler.try_on_use_block(button).unwrap();
    for tick in 1..=7 {
        reference.tick_interpreted();
        compiler.tick().unwrap();
        compiler.try_flush(&mut candidate).unwrap();
        assert_eq!(reference.get_block(button), candidate.get_block(button), "button tick {tick}");
        assert_eq!(reference.get_block(lamp), candidate.get_block(lamp), "lamp tick {tick}");
    }
    compiler.try_reset(&mut candidate, bounds).unwrap();
    assert_eq!(
        reference.scheduler().iter_entries().collect::<Vec<_>>(),
        candidate.scheduler().iter_entries().collect::<Vec<_>>()
    );
    for tick in 8..=22 {
        reference.tick_interpreted();
        candidate.tick_interpreted();
        assert_eq!(reference.get_block(button), candidate.get_block(button), "reset button tick {tick}");
        assert_eq!(reference.get_block(lamp), candidate.get_block(lamp), "reset lamp tick {tick}");
    }
}

#[test]
fn llvm_imports_pending_button_release_and_hands_it_back() {
    if std::env::var_os("MCHPRS_LLVM_DLL").is_none()
        && !std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/native_engine.dll")
            .exists()
    {
        return;
    }
    let (mut reference, button, lamp, bounds) = button_world(ButtonFace::Floor, BlockDirection::North);
    let (mut candidate, _, _, _) = button_world(ButtonFace::Floor, BlockDirection::North);
    crate::interaction::use_redstone(reference.get_block(button), &mut reference, button, None);
    crate::interaction::use_redstone(candidate.get_block(button), &mut candidate, button, None);
    let pending = candidate.scheduler().iter_entries().collect();
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &candidate,
            bounds,
            CompilerOptions {
                llvm: true,
                ..Default::default()
            },
            pending,
            Arc::new(TaskMonitor::default()),
        )
        .unwrap();
    candidate.clear_scheduled_ticks();
    for tick in 1..=7 {
        reference.tick_interpreted();
        compiler.tick().unwrap();
        compiler.try_flush(&mut candidate).unwrap();
        assert_eq!(reference.get_block(button), candidate.get_block(button), "button tick {tick}");
        assert_eq!(reference.get_block(lamp), candidate.get_block(lamp), "lamp tick {tick}");
    }
    compiler.try_reset(&mut candidate, bounds).unwrap();
    assert_eq!(
        reference.scheduler().iter_entries().collect::<Vec<_>>(),
        candidate.scheduler().iter_entries().collect::<Vec<_>>()
    );
    for tick in 8..=22 {
        reference.tick_interpreted();
        candidate.tick_interpreted();
        assert_eq!(reference.get_block(button), candidate.get_block(button), "button tick {tick}");
        assert_eq!(reference.get_block(lamp), candidate.get_block(lamp), "lamp tick {tick}");
    }
}

#[test]
fn llvm_imported_ticks_preserve_priority_and_duplicates() {
    if std::env::var_os("MCHPRS_LLVM_DLL").is_none()
        && !std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/native_engine.dll")
            .exists()
    {
        return;
    }
    let (mut world, button, _, bounds) = button_world(ButtonFace::Floor, BlockDirection::North);
    let kind = Some(world.get_block(button).registry_id());
    for (delay, priority) in [
        (5, TickPriority::Normal),
        (3, TickPriority::High),
        (5, TickPriority::Highest),
        (5, TickPriority::Highest),
    ] {
        world.restore_scheduled_tick(TickEntry {
            ticks_left: delay,
            tick_priority: priority,
            pos: button,
            block_type: kind,
        });
    }
    let expected: Vec<_> = world.scheduler().iter_entries().collect();
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &world,
            bounds,
            CompilerOptions {
                llvm: true,
                ..Default::default()
            },
            expected.clone(),
            Arc::new(TaskMonitor::default()),
        )
        .unwrap();
    world.clear_scheduled_ticks();
    compiler.try_reset(&mut world, bounds).unwrap();
    assert_eq!(expected, world.scheduler().iter_entries().collect::<Vec<_>>());
    let mut stale = expected[0].clone();
    stale.block_type = None;
    compiler
        .compile(
            &world,
            bounds,
            CompilerOptions {
                llvm: true,
                ..Default::default()
            },
            vec![stale.clone()],
            Arc::new(TaskMonitor::default()),
        )
        .unwrap();
    world.clear_scheduled_ticks();
    compiler.try_reset(&mut world, bounds).unwrap();
    assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), vec![stale]);
}

#[test]
#[cfg(any(windows, target_os = "linux"))]
fn compiled_pending_ticks_preserve_stale_passive_and_large_initial_queues() {
    let (mut reference, button, lamp, bounds) = button_world(ButtonFace::Floor, BlockDirection::North);
    let (mut candidate, _, _, _) = button_world(ButtonFace::Floor, BlockDirection::North);
    let plate = button + BlockPos::new(2, 0, 0);
    for world in [&mut reference, &mut candidate] {
        world.set_block(plate, Block::StonePressurePlate { powered: false });
        for _ in 0..128 {
            world.restore_scheduled_tick(TickEntry {
                pos: button,
                block_type: Some(world.get_block(button).registry_id()),
                ticks_left: 30,
                tick_priority: TickPriority::High,
            });
        }
        for (pos, block_type, ticks_left) in [
            (button, None, 2),
            (button, Some(world.get_block(lamp).registry_id()), 3),
            (bounds.0, Some(Block::Stone {}.registry_id()), 7),
            (button.offset(BlockFace::Bottom), Some(Block::Stone {}.registry_id()), 8),
            (plate, Some(world.get_block(plate).registry_id()), 9),
        ] {
            world.restore_scheduled_tick(TickEntry {
                pos,
                block_type,
                ticks_left,
                tick_priority: TickPriority::Normal,
            });
        }
    }
    let initial_count = reference.scheduler().iter_entries().count();
    let mut rust = Compiler::default();
    let mut native = Compiler::default();
    for (compiler, world, llvm) in [(&mut rust, &mut reference, false), (&mut native, &mut candidate, true)] {
        compiler
            .compile(
                world,
                bounds,
                CompilerOptions { llvm, ..Default::default() },
                world.scheduler().iter_entries().collect(),
                Default::default(),
            )
            .unwrap();
        world.clear_scheduled_ticks();
        compiler.try_on_use_block(button).unwrap();
    }
    for tick in 0..=18 {
        if tick != 0 {
            if rust.is_active() {
                rust.tick().unwrap();
                native.tick().unwrap();
            } else {
                reference.tick_interpreted();
                candidate.tick_interpreted();
            }
        }
        if rust.is_active() {
            rust.try_flush(&mut reference).unwrap();
            native.try_flush(&mut candidate).unwrap();
            let expected = rust.runtime.as_ref().unwrap();
            let actual = native.runtime.as_ref().unwrap();
            assert_eq!(actual.pending_entries(), expected.pending_entries(), "pending tick {tick}");
            assert_eq!(actual.projected_blocks(), expected.projected_blocks(), "snapshot tick {tick}");
            if tick == 0 {
                assert_eq!(expected.pending_entries().len(), initial_count + 1);
            }
        }
        for pos in [button, lamp, plate] {
            assert_eq!(reference.get_block(pos), candidate.get_block(pos), "block {pos:?} tick {tick}");
        }
        if tick == 5 {
            rust.try_reset(&mut reference, bounds).unwrap();
            native.try_reset(&mut candidate, bounds).unwrap();
        }
        if !rust.is_active() {
            assert_eq!(
                reference.scheduler().iter_entries().collect::<Vec<_>>(),
                candidate.scheduler().iter_entries().collect::<Vec<_>>(),
                "handoff tick {tick}"
            );
        }
    }
}

#[test]
fn llvm_scheduler_wraps_and_reuses_slots_without_changing_pulses() {
    if std::env::var_os("MCHPRS_LLVM_DLL").is_none()
        && !std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/native_engine.dll")
            .exists()
    {
        return;
    }
    let (mut reference, button, lamp, bounds) = button_world(ButtonFace::Floor, BlockDirection::North);
    let (mut candidate, _, _, _) = button_world(ButtonFace::Floor, BlockDirection::North);
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &candidate,
            bounds,
            CompilerOptions {
                llvm: true,
                ..Default::default()
            },
            Vec::new(),
            Arc::new(TaskMonitor::default()),
        )
        .unwrap();
    for tick in 0..257 {
        if tick % 25 == 0 {
            crate::interaction::use_redstone(reference.get_block(button), &mut reference, button, None);
            compiler.try_on_use_block(button).unwrap();
        }
        reference.tick_interpreted();
        compiler.tick().unwrap();
        compiler.try_flush(&mut candidate).unwrap();
        assert_eq!(reference.get_block(button), candidate.get_block(button), "button tick {tick}");
        assert_eq!(reference.get_block(lamp), candidate.get_block(lamp), "lamp tick {tick}");
    }
    compiler.try_reset(&mut candidate, bounds).unwrap();
    assert_eq!(
        reference.scheduler().iter_entries().collect::<Vec<_>>(),
        candidate.scheduler().iter_entries().collect::<Vec<_>>()
    );
    for tick in 0..32 {
        reference.tick_interpreted();
        candidate.tick_interpreted();
        assert_eq!(reference.get_block(button), candidate.get_block(button), "reset button tick {tick}");
        assert_eq!(reference.get_block(lamp), candidate.get_block(lamp), "reset lamp tick {tick}");
    }
}

#[test]
fn llvm_control_work_counts_toward_the_next_scheduled_tick_limit() {
    if std::env::var_os("MCHPRS_LLVM_DLL").is_none()
        && !std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/native_engine.dll")
            .exists()
    {
        return;
    }
    let (mut world, lever, _, bounds) = button_world(ButtonFace::Floor, BlockDirection::North);
    world.set_block(
        lever,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &world,
            bounds,
            CompilerOptions {
                llvm: true,
                ..Default::default()
            },
            Vec::new(),
            Arc::new(TaskMonitor::default()),
        )
        .unwrap();
    compiler.try_on_use_block(lever).unwrap();
    compiler.tick().unwrap();
    compiler.try_on_use_block(lever).unwrap();
    for _ in 0..3 {
        compiler.tick().unwrap();
    }
    for _ in 0..500_000 {
        compiler.try_on_use_block(lever).unwrap();
    }
    assert!(compiler.tick().is_err());
}

#[test]
fn compiled_buttons_validate_every_supported_attachment() {
    let attachments = [
        (ButtonFace::Floor, BlockDirection::North),
        (ButtonFace::Ceiling, BlockDirection::South),
        (ButtonFace::Wall, BlockDirection::North),
        (ButtonFace::Wall, BlockDirection::East),
        (ButtonFace::Wall, BlockDirection::South),
        (ButtonFace::Wall, BlockDirection::West),
    ];
    for (face, facing) in attachments {
        let (mut native, pos, lamp, bounds) = button_world(face, facing);
        let (mut world, _, _, _) = button_world(face, facing);
        let mut compiler = compile_button(&world, bounds);
        crate::interaction::use_redstone(native.get_block(pos), &mut native, pos, None);
        compiler.try_on_use_block(pos).unwrap();
        for tick in 1..=20 {
            native.tick_interpreted();
            compiler.tick().unwrap();
            compiler.try_flush(&mut world).unwrap();
            assert_eq!(native.get_block(pos), world.get_block(pos), "{face:?}/{facing:?} button tick {tick}");
            assert_eq!(native.get_block(lamp), world.get_block(lamp), "{face:?}/{facing:?} lamp tick {tick}");
        }
    }
}

#[test]
fn reset_preserves_button_state_and_pending_release() {
    let (mut native, pos, lamp, bounds) = button_world(ButtonFace::Wall, BlockDirection::West);
    let (mut compiled, _, _, _) = button_world(ButtonFace::Wall, BlockDirection::West);
    crate::interaction::use_redstone(native.get_block(pos), &mut native, pos, None);
    crate::interaction::use_redstone(compiled.get_block(pos), &mut compiled, pos, None);
    let pending = compiled.scheduler().iter_entries().collect();
    let mut compiler = Compiler::default();
    compiler
        .compile(&compiled, bounds, CompilerOptions::default(), pending, Arc::new(TaskMonitor::default()))
        .unwrap();
    compiled.clear_scheduled_ticks();
    compiler.try_on_use_block(pos).unwrap(); // A loaded powered button ignores another press.
    compiler.try_flush(&mut compiled).unwrap();
    compiler.try_reset(&mut compiled, bounds).unwrap();
    assert!(matches!(compiled.get_block(pos), Block::StoneButton { button } if button.powered));
    assert_eq!(
        native.scheduler().iter_entries().collect::<Vec<_>>(),
        compiled.scheduler().iter_entries().collect::<Vec<_>>()
    );
    for tick in 1..=20 {
        native.tick_interpreted();
        compiled.tick_interpreted();
        assert_eq!(native.get_block(pos), compiled.get_block(pos), "reset button at tick {tick}");
        assert_eq!(native.get_block(lamp), compiled.get_block(lamp), "reset lamp at tick {tick}");
    }
}

#[test]
fn headless_pistons_preserve_entry_and_match_native_motion_and_handoff() {
    check_piston_motion_and_handoff(false);
}

#[test]
fn llvm_pistons_preserve_entry_motion_and_reset_handoff() {
    if std::env::var_os("MCHPRS_LLVM_DLL").is_none()
        && !std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/native_engine.dll")
            .exists()
    {
        return;
    }
    check_piston_motion_and_handoff(true);
}

fn check_piston_motion_and_handoff(llvm: bool) {
    for (sticky, payload) in [
        (false, Block::Air),
        (true, Block::Air),
        (true, Block::Stone {}),
        (true, Block::RedstoneBlock),
    ] {
        for handoff in [false, true] {
            let create = || {
                let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
                let mut world = PlotWorld::from_chunks(0, 0, chunks, TickScheduler::default());
                let lever = BlockPos::new(15, 15, 15);
                let base = lever.offset(BlockFace::East);
                world.set_block(lever.offset(BlockFace::Bottom), Block::Stone {});
                world.set_block(
                    lever,
                    Block::Lever {
                        lever: Lever::new(LeverFace::Floor, BlockDirection::North, true),
                    },
                );
                world.set_block(
                    base,
                    Block::Piston {
                        piston: RedstonePiston {
                            facing: BlockFacing::East,
                            sticky,
                            extended: true,
                        },
                    },
                );
                world.set_block(base.offset(BlockFace::East).offset(BlockFace::East), payload);
                world
            };
            let mut native = create();
            let mut compiled = create();
            let lever = BlockPos::new(15, 15, 15);
            let base = lever.offset(BlockFace::East);
            let home = base.offset(BlockFace::East);
            let far = home.offset(BlockFace::East);
            let positions = [lever, base, home, far];
            let bounds = (BlockPos::new(10, 10, 10), BlockPos::new(22, 20, 20));
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &compiled,
                    bounds,
                    CompilerOptions { llvm, ..Default::default() },
                    vec![],
                    Arc::new(TaskMonitor::default()),
                )
                .unwrap();
            compiler.try_flush_complete(&mut compiled).unwrap();
            assert_eq!(compiled.get_block(base), native.get_block(base));
            assert_eq!(compiled.get_block(home), Block::Air, "initial publication must not invent a head");
            for phase in 0..2 {
                crate::interaction::use_redstone(native.get_block(lever), &mut native, lever, None);
                if compiler.is_active() {
                    compiler.try_on_use_block(lever).unwrap();
                } else {
                    crate::interaction::use_redstone(compiled.get_block(lever), &mut compiled, lever, None);
                }
                for tick in 1..=6 {
                    native.tick_interpreted();
                    if compiler.is_active() {
                        compiler.tick().unwrap();
                        compiler.try_flush_complete(&mut compiled).unwrap();
                        if handoff && phase == 0 && tick == 1 {
                            compiler.try_reset(&mut compiled, bounds).unwrap();
                            assert!(!compiler.is_active());
                        }
                    } else {
                        compiled.tick_interpreted();
                    }
                    for pos in positions {
                        assert_eq!(
                            compiled.get_block(pos),
                            native.get_block(pos),
                            "sticky={sticky} payload={payload:?} handoff={handoff} phase={phase} tick={tick} at {pos}"
                        );
                    }
                }
                assert!(matches!(compiled.get_block(base), Block::Piston { piston } if piston.extended == (phase == 1)));
            }
        }
    }
}

#[test]
fn flush_and_reset_preserve_an_in_flight_piston_payload() {
    fn motions(world: &PlotWorld) -> Vec<(BlockPos, u64, f32, u64)> {
        world
            .piston_state()
            .motions
            .iter()
            .map(|motion| (motion.pos, motion.identity, motion.progress, motion.last_tick))
            .collect()
    }

    let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
    let mut world = PlotWorld::from_chunks(0, 0, chunks, TickScheduler::default());
    let lever = BlockPos::new(15, 15, 15);
    let base = lever.offset(BlockFace::East);
    let head = base.offset(BlockFace::East);
    let payload = head.offset(BlockFace::East);
    world.set_block(lever.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        lever,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
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
    world.set_block(head, Block::RedstoneBlock {});

    let mut compiler = Compiler::default();
    compiler
        .compile(
            &world,
            (BlockPos::new(10, 10, 10), BlockPos::new(22, 20, 20)),
            CompilerOptions::default(),
            Vec::new(),
            Arc::new(TaskMonitor::default()),
        )
        .unwrap();
    compiler.try_on_use_block(lever).unwrap();
    compiler.tick().unwrap();

    compiler.try_flush_complete(&mut world).unwrap();
    assert!(matches!(world.get_block(head), Block::MovingPiston { .. }));
    assert!(matches!(world.get_block(payload), Block::MovingPiston { .. }));
    let first = motions(&world);
    assert_eq!(first.len(), 2);
    assert_eq!(first.iter().map(|motion| motion.0).collect::<Vec<_>>(), [payload, head]);
    assert!(first.iter().all(|motion| motion.2 == 0.5));
    let entities = [head, payload].map(|pos| format!("{:?}", world.get_block_entity(pos)));

    compiler.try_flush_complete(&mut world).unwrap();
    assert_eq!(motions(&world), first, "repeated publication keeps motion identities and timing");
    assert_eq!([head, payload].map(|pos| format!("{:?}", world.get_block_entity(pos))), entities);
    compiler
        .try_reset(&mut world, (BlockPos::new(10, 10, 10), BlockPos::new(22, 20, 20)))
        .unwrap();
    assert!(!compiler.is_active());
    assert_eq!(motions(&world), first, "reset returns the active motion without settling it");
    assert_eq!([head, payload].map(|pos| format!("{:?}", world.get_block_entity(pos))), entities);

    world.tick_interpreted();
    world.tick_interpreted();
    assert!(matches!(world.get_block(head), Block::PistonHead { .. }));
    assert_eq!(world.get_block(payload), Block::RedstoneBlock {});
}

#[test]
fn flush_and_reset_export_only_the_unfinished_interrupted_motion_part() {
    let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
    let mut world = PlotWorld::from_chunks(0, 0, chunks, TickScheduler::default());
    let lever = BlockPos::new(15, 15, 15);
    let base = lever.offset(BlockFace::East);
    let home = base.offset(BlockFace::East);
    world.set_block(lever.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        lever,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    world.set_block(
        base,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::East,
                sticky: false,
                extended: false,
            },
        },
    );

    let bounds = (BlockPos::new(10, 10, 10), BlockPos::new(22, 20, 20));
    let mut compiler = Compiler::default();
    compiler
        .compile(&world, bounds, CompilerOptions::default(), Vec::new(), Arc::new(TaskMonitor::default()))
        .unwrap();
    compiler.try_on_use_block(lever).unwrap();
    compiler.tick().unwrap();
    compiler.try_on_use_block(lever).unwrap();
    compiler.tick().unwrap();

    compiler.try_flush_complete(&mut world).unwrap();
    let moving_positions: Vec<_> = world.piston_state().motions.iter().map(|motion| motion.pos).collect();
    assert_eq!(moving_positions, [base]);
    assert!(matches!(world.get_block(base), Block::MovingPiston { .. }));
    assert_eq!(world.get_block(home), Block::Air);

    compiler.try_reset(&mut world, bounds).unwrap();
    assert!(!compiler.is_active());
    assert_eq!(
        world.piston_state().motions.iter().map(|motion| motion.pos).collect::<Vec<_>>(),
        moving_positions
    );
    assert_eq!(world.get_block(home), Block::Air);
}

#[test]
fn interrupted_payload_survives_flush_reset_and_native_continuation() {
    let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
    let mut world = PlotWorld::from_chunks(0, 0, chunks, TickScheduler::default());
    let lever = BlockPos::new(15, 15, 15);
    let base = lever.offset(BlockFace::East);
    let home = base.offset(BlockFace::East);
    let far = home.offset(BlockFace::East);
    let opposing_base = far.offset(BlockFace::East);
    world.set_block(lever.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        lever,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    world.set_block(
        base,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::East,
                sticky: false,
                extended: false,
            },
        },
    );
    world.set_block(home, Block::Stone {});
    world.set_block(
        opposing_base,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::West,
                sticky: false,
                extended: false,
            },
        },
    );

    let bounds = (BlockPos::new(10, 10, 10), BlockPos::new(22, 20, 20));
    let mut compiler = Compiler::default();
    compiler
        .compile(&world, bounds, CompilerOptions::default(), Vec::new(), Arc::new(TaskMonitor::default()))
        .unwrap();
    compiler.try_on_use_block(lever).unwrap();
    compiler.tick().unwrap();
    compiler.try_on_use_block(lever).unwrap();
    compiler.tick().unwrap();

    compiler.try_flush_complete(&mut world).unwrap();
    let mut moving_positions: Vec<_> = world.piston_state().motions.iter().map(|motion| motion.pos).collect();
    moving_positions.sort_by_key(|pos| (pos.x, pos.y, pos.z));
    let mut expected_positions = vec![base, far];
    expected_positions.sort_by_key(|pos| (pos.x, pos.y, pos.z));
    assert_eq!(moving_positions, expected_positions);
    assert!(world.piston_state().motions.iter().any(|motion| motion.pos == far));
    assert!(world.piston_state().motions.iter().any(|motion| motion.pos == base));
    assert_eq!(world.get_block(home), Block::Air);

    compiler.try_reset(&mut world, bounds).unwrap();
    assert!(!compiler.is_active());
    assert!(matches!(world.get_block(base), Block::MovingPiston { .. }));
    assert!(matches!(world.get_block(far), Block::MovingPiston { .. }));

    world.tick_interpreted();
    assert_eq!(world.get_block(far), Block::Stone {});
    assert_eq!(world.get_block(home), Block::Air);
    assert_eq!(world.piston_state().motions.len(), 1, "the payload settles before the retracting base");

    world.tick_interpreted();
    assert_eq!(world.get_block(far), Block::Stone {});
    assert_eq!(world.get_block(home), Block::Air);
    assert!(matches!(world.get_block(base), Block::Piston { piston } if !piston.extended));
    assert!(world.piston_state().motions.is_empty());
}

#[test]
fn stale_imported_ticks_are_ignored_like_native() {
    let pos = BlockPos::new(20, 20, 20);
    let bounds = (BlockPos::new(15, 15, 15), BlockPos::new(25, 25, 25));
    let chunks = || (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
    let mut native = PlotWorld::from_chunks(0, 0, chunks(), TickScheduler::default());
    let mut sampled = PlotWorld::from_chunks(0, 0, chunks(), TickScheduler::default());
    let lamp = Block::RedstoneLamp { lit: true };
    let stale = TickEntry {
        pos,
        ticks_left: 1,
        tick_priority: TickPriority::Normal,
        block_type: Some(Block::Stone {}.registry_id()),
    };
    for world in [&mut native, &mut sampled] {
        world.set_block(pos, lamp);
        world.restore_scheduled_tick(stale.clone());
    }

    let ticks = sampled.scheduler().iter_entries().collect();
    let mut compiler = Compiler::default();
    compiler
        .compile(&sampled, bounds, CompilerOptions::default(), ticks, Arc::new(TaskMonitor::default()))
        .unwrap();
    native.tick_interpreted();
    compiler.tick().unwrap();
    compiler.try_flush(&mut sampled).unwrap();
    assert_eq!(sampled.get_block(pos), native.get_block(pos));
}

#[test]
fn compiler_uses_non_origin_plot_coordinates() {
    let (plot_x, plot_z) = (1, -1);
    let chunks = (0..PLOT_WIDTH)
        .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(plot_x * PLOT_WIDTH + x, plot_z * PLOT_WIDTH + z)))
        .collect();
    let mut world = PlotWorld::from_chunks(plot_x, plot_z, chunks, TickScheduler::default());
    let pos = BlockPos::new(
        plot_x * crate::plot::PLOT_BLOCK_WIDTH + 20,
        20,
        plot_z * crate::plot::PLOT_BLOCK_WIDTH + 20,
    );
    world.set_block(pos, Block::RedstoneLamp { lit: false });

    let mut compiler = Compiler::default();
    compiler
        .compile(
            &world,
            (pos - BlockPos::new(3, 3, 3), pos + BlockPos::new(3, 3, 3)),
            CompilerOptions::default(),
            Vec::new(),
            Arc::new(TaskMonitor::default()),
        )
        .unwrap();
    assert!(compiler.is_active());
}

#[test]
fn compile_rejection_preserves_pending_ticks_and_piston_state() {
    use mchprs_blocks::block_entities::CommandBlockEntity;

    let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
    let mut world = PlotWorld::from_chunks(0, 0, chunks, TickScheduler::default());
    let pos = BlockPos::new(15, 15, 15);
    world.set_block(pos, Block::from_name("command_block").unwrap());
    world.set_block_entity(
        pos,
        BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
            command: "say pending".into(),
            ..Default::default()
        })),
    );
    world.schedule_tick(pos, 7, TickPriority::High);
    let malformed = pos.offset(BlockFace::East);
    world.set_block(malformed, Block::from_name("comparator").unwrap());
    world.set_block_entity(malformed, BlockEntity::Comparator { output_strength: 255 });
    let before_block = world.get_block(pos);
    let before_entity = format!("{:?}", world.get_block_entity(pos));
    let before_ticks = world.scheduler().iter_entries().collect::<Vec<_>>();
    let before_pistons = format!("{:?}", world.piston_state());

    let mut compiler = Compiler::default();
    assert!(
        compiler
            .compile(
                &world,
                (BlockPos::new(10, 10, 10), BlockPos::new(20, 20, 20)),
                CompilerOptions::default(),
                before_ticks.clone(),
                Arc::new(TaskMonitor::default()),
            )
            .is_err()
    );
    assert!(!compiler.is_active());
    assert_eq!(world.get_block(pos), before_block);
    assert_eq!(format!("{:?}", world.get_block_entity(pos)), before_entity);
    assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), before_ticks);
    assert_eq!(format!("{:?}", world.piston_state()), before_pistons);
}

#[test]
fn compile_success_preserves_host_blocks_entities_and_pending_work() {
    let (mut world, button, _, bounds) = button_world(ButtonFace::Floor, BlockDirection::North);
    let sign = button + BlockPos::new(1, 0, 0);
    world.set_block(sign, Block::from_name("oak_sign").unwrap());
    world.set_block_entity(sign, mchprs_blocks::block_entities::BlockEntity::Sign(Default::default()));
    world.schedule_tick(button, 10, TickPriority::Normal);

    let before_button = world.get_block(button);
    let before_sign = world.get_block(sign);
    let before_entity = format!("{:?}", world.get_block_entity(sign));
    let before_ticks = world.scheduler().iter_entries().collect::<Vec<_>>();
    let before_pistons = format!("{:?}", world.piston_state());
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &world,
            bounds,
            CompilerOptions::default(),
            before_ticks.clone(),
            Arc::new(TaskMonitor::default()),
        )
        .unwrap();

    assert!(compiler.is_active());
    assert_eq!(world.get_block(button), before_button);
    assert_eq!(world.get_block(sign), before_sign);
    assert_eq!(format!("{:?}", world.get_block_entity(sign)), before_entity);
    assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), before_ticks);
    assert_eq!(format!("{:?}", world.piston_state()), before_pistons);
}

#[test]
fn powered_source_in_probe_context_rejects_compilation() {
    let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
    let mut world = PlotWorld::from_chunks(0, 0, chunks, TickScheduler::default());
    let piston = BlockPos::new(40, 30, 40);
    let source = BlockPos::new(40, 33, 40);
    world.set_block(
        piston,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::East,
                sticky: false,
                extended: false,
            },
        },
    );
    world.set_block(BlockPos::new(40, 32, 40), Block::Stone {});
    world.set_block(
        source,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, true),
        },
    );
    let bounds = (BlockPos::new(38, 29, 38), BlockPos::new(42, 32, 42));
    let mut compiler = Compiler::default();
    let error = compiler
        .compile(&world, bounds, CompilerOptions::default(), Vec::new(), Arc::new(TaskMonitor::default()))
        .unwrap_err();
    assert!(matches!(
        error,
        CompileError::Unsupported(ScratchError {
            position: Some(position),
            reason: "unrepresented discovered wire source"
        }) if position == source
    ));
    assert!(!compiler.is_active());
}

#[test]
fn missing_source_chunk_is_not_captured_as_air() {
    let world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], TickScheduler::default());
    let missing = BlockPos::new(32, 30, 32);
    let mut compiler = Compiler::default();
    let error = compiler
        .compile(
            &world,
            (missing, BlockPos::new(34, 32, 34)),
            CompilerOptions::default(),
            Vec::new(),
            Arc::new(TaskMonitor::default()),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        CompileError::Unsupported(ScratchError {
            position: Some(position),
            reason: "scratch capture contains an unloaded chunk"
        }) if position == missing
    ));
    assert!(!compiler.is_active());
}
