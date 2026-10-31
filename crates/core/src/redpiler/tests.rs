use super::*;
use crate::plot::{PLOT_WIDTH, PlotWorld};
use crate::world::{World, storage::Chunk};
use mchprs_blocks::blocks::{ButtonFace, Lever, LeverFace, RedstonePiston, StoneButton};
use mchprs_blocks::{BlockDirection, BlockFace, BlockFacing};
use mchprs_world::{TickEntry, TickPriority};

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

    compiler.try_flush(&mut world).unwrap();
    assert!(matches!(world.get_block(head), Block::MovingPiston { .. }));
    assert!(matches!(world.get_block(payload), Block::MovingPiston { .. }));
    let first = motions(&world);
    assert_eq!(first.len(), 2);
    assert_eq!(first.iter().map(|motion| motion.0).collect::<Vec<_>>(), [payload, head]);
    assert!(first.iter().all(|motion| motion.2 == 0.5));
    let entities = [head, payload].map(|pos| format!("{:?}", world.get_block_entity(pos)));

    compiler.try_flush(&mut world).unwrap();
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

    compiler.try_flush(&mut world).unwrap();
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

    compiler.try_flush(&mut world).unwrap();
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
    world.set_block(source, Block::RedstoneBlock);
    let bounds = (BlockPos::new(38, 29, 38), BlockPos::new(42, 32, 42));
    let mut compiler = Compiler::default();
    let error = compiler
        .compile(
            &world,
            bounds,
            CompilerOptions::default(),
            Vec::new(),
            Arc::new(TaskMonitor::default()),
        )
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
