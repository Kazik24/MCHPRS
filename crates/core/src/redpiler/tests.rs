use super::*;
use crate::plot::{PLOT_WIDTH, PlotWorld};
use crate::world::{World, storage::Chunk};
use mchprs_blocks::blocks::{Lever, LeverFace, RedstonePiston};
use mchprs_blocks::{BlockDirection, BlockFace, BlockFacing};
use mchprs_world::{TickEntry, TickPriority};

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
