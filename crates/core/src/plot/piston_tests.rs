use super::*;
use mchprs_blocks::blocks::RedstonePiston;
use mchprs_blocks::BlockFacing;

fn world() -> PlotWorld {
    let chunks = (0..PLOT_WIDTH)
        .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
        .collect();
    PlotWorld::from_chunks(0, 0, chunks, Default::default())
}
fn copy_saved(world: &mut PlotWorld) -> PlotWorld {
    let data = PlotData {
        tps: Tps::Limited(20),
        world_send_rate: WorldSendRate(60),
        chunk_data: world.chunks.iter_mut().map(Chunk::save).collect(),
        pending_ticks: world.scheduler().iter_entries().collect(),
        piston_state: world.piston_state.clone(),
    };
    let bytes = bincode::serialize(&data).unwrap();
    let data: PlotData<PLOT_SECTIONS> = bincode::deserialize(&bytes).unwrap();
    let chunks = data
        .chunk_data
        .into_iter()
        .enumerate()
        .map(|(i, c)| Chunk::load(i as i32 / PLOT_WIDTH, i as i32 % PLOT_WIDTH, c))
        .collect();
    let mut resumed =
        PlotWorld::from_chunks(0, 0, chunks, data.pending_ticks.into_iter().collect());
    resumed.piston_state = data.piston_state;
    resumed
}
fn assert_same(a: &mut PlotWorld, b: &mut PlotWorld) {
    for x in 40..=43 {
        let pos = BlockPos::new(x, 30, 40);
        assert_eq!(a.get_block_raw(pos), b.get_block_raw(pos));
        assert_eq!(
            a.get_block_entity(pos)
                .map(|e| bincode::serialize(e).unwrap()),
            b.get_block_entity(pos)
                .map(|e| bincode::serialize(e).unwrap())
        );
    }
    assert_eq!(
        bincode::serialize(&a.piston_state).unwrap(),
        bincode::serialize(&b.piston_state).unwrap()
    );
    assert_eq!(
        a.scheduler().iter_entries().collect::<Vec<_>>(),
        b.scheduler().iter_entries().collect::<Vec<_>>()
    );
}

#[test]
fn restart_preserves_event_motion_and_partial_step_state() {
    for paused_after in 0..=9 {
        let mut original = world();
        let base = BlockPos::new(40, 30, 40);
        original.set_block(
            base,
            Block::Piston {
                piston: RedstonePiston {
                    facing: BlockFacing::East,
                    sticky: true,
                    extended: false,
                },
            },
        );
        original.set_block(base.offset(BlockFace::East), Block::Stone {});
        original.set_block(base.offset(BlockFace::Bottom), Block::RedstoneBlock {});
        redstone::update(original.get_block(base), &mut original, base, None);
        original.picotick_advance(paused_after);
        let mut resumed = copy_saved(&mut original);
        assert_same(&mut original, &mut resumed);
        for _ in 0..12 {
            original.picotick_advance(1);
            resumed.picotick_advance(1);
            assert_same(&mut original, &mut resumed);
        }
        for w in [&mut original, &mut resumed] {
            w.set_block(base.offset(BlockFace::Bottom), Block::Air);
            redstone::update(w.get_block(base), w, base, None);
        }
        for _ in 0..8 {
            original.tick_interpreted();
            resumed.tick_interpreted();
        }
        assert_same(&mut original, &mut resumed);
    }
}

#[test]
fn old_ticks_bind_once_and_remove_legacy_movement_locks() {
    let mut initial = world();
    let pos = BlockPos::new(40, 30, 40);
    initial.set_block(
        pos,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::East,
                sticky: true,
                extended: false,
            },
        },
    );
    let ticks = [mchprs_world::TickEntry {
        pos,
        ticks_left: 3,
        tick_priority: TickPriority::Normal,
        block_type: None,
    }];
    let resumed = PlotWorld::from_chunks(0, 0, initial.chunks, ticks.into_iter().collect());
    let tick = resumed.scheduler().iter_entries().next().unwrap();
    assert_eq!(tick.ticks_left, 0);
    assert_eq!(tick.block_type, Some(resumed.get_block(pos).registry_id()));
}

#[test]
fn stale_snapshot_cannot_tick_new_motion_at_same_position() {
    let mut world = world();
    let pos = BlockPos::new(40, 30, 40);
    let moving = Block::MovingPiston {
        moving: mchprs_blocks::blocks::RedstoneMovingPiston {
            facing: BlockFacing::East,
            sticky: true,
        },
    };
    let entity = mchprs_blocks::block_entities::MovingPistonEntity {
        block_state: Block::Stone {}.get_id(),
        ..Default::default()
    };
    world.set_block(pos, moving);
    world.set_block_entity(pos, BlockEntity::MovingPiston(entity));
    let old = world.piston_state.motions[0].identity;
    world.piston_state.phase = AdvancePhase::MovingEntities;
    world.piston_state.movement_work = vec![(pos, old)];
    world.set_block_entity(pos, BlockEntity::MovingPiston(entity));
    assert_ne!(world.piston_state.motions[0].identity, old);
    world.picotick_advance(1);
    assert_eq!(world.piston_state.motions[0].progress, 0.0);
    world.tick_interpreted(); // finish the paused phase
    world.tick_interpreted();
    assert_eq!(world.piston_state.motions[0].progress, 0.5);
}
