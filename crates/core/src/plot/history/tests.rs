use super::*;
use crate::redstone;
use crate::world::World;
use mchprs_blocks::block_entities::{CommandBlockEntity, ContainerType, SignBlockEntity};
use mchprs_blocks::blocks::{Block, RedstonePiston};
use mchprs_blocks::{BlockFace, BlockFacing};
use mchprs_world::TickPriority;

fn world() -> PlotWorld {
    // One chunk is sufficient for these circuits and keeps snapshots small.
    PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default())
}

#[derive(Debug, PartialEq)]
struct State {
    blocks: Vec<u32>,
    entities: Vec<(BlockPos, Vec<u8>)>,
    counts: Vec<u32>,
    piston: Vec<u8>,
    scheduler: String,
}

fn state(world: &PlotWorld) -> State {
    let chunk = &world.chunks[0];
    let blocks = (0..256)
        .flat_map(|y| (0..16).flat_map(move |z| (0..16).map(move |x| chunk.get_block(x, y, z))))
        .collect();
    let mut entities: Vec<_> = chunk
        .block_entities
        .iter()
        .map(|(&pos, entity)| (pos, bincode::serialize(entity).unwrap()))
        .collect();
    entities.sort_by_key(|(pos, _)| (pos.x, pos.y, pos.z));
    State {
        blocks,
        entities,
        counts: chunk
            .sections
            .iter()
            .map(|section| section.block_count())
            .collect(),
        piston: bincode::serialize(&world.piston_state).unwrap(),
        // Includes the actual bucket cursor, FIFO contents, priorities and expected types.
        scheduler: format!("{:?}", world.to_be_ticked),
    }
}

#[test]
fn ring_wrap_eviction_rewind_and_new_future() {
    let pos = BlockPos::new(4, 30, 4);
    for capacity in [1, 3, 8] {
        let mut world = world();
        world.enable_history(capacity, false).unwrap();
        let mut before = Vec::new();
        let mut after = Vec::new();
        for tick in 0..12 {
            let block = if tick % 2 == 0 {
                Block::GoldBlock {}
            } else {
                Block::Glass {}
            };
            world.set_block(pos, block);
            before.push(state(&world));
            world.tick_interpreted();
            after.push(state(&world));
        }
        assert_eq!(world.history.len(), capacity);
        let unchanged = state(&world);
        let memory = world.history.memory_bytes();
        assert!(world.rewind_ticks(capacity + 1, false).is_err());
        assert!(world.rewind_ticks(0, false).is_err());
        assert_eq!(state(&world), unchanged);
        assert_eq!(world.history.memory_bytes(), memory);
        world.rewind_ticks(capacity, false).unwrap();
        assert_eq!(state(&world), before[12 - capacity]);
        assert_eq!(world.history.len(), 0);
        world.tick_interpreted();
        assert_eq!(state(&world), after[12 - capacity]);
        assert_eq!(world.history.len(), 1);
        world.rewind_ticks(1, false).unwrap();
        assert_eq!(state(&world), before[12 - capacity]);
        assert_eq!(world.history.heap_bytes, 0);
    }
}

#[test]
fn one_step_then_multiple_steps_across_wrapped_slots() {
    let mut world = world();
    world.enable_history(4, false).unwrap();
    for _ in 0..7 {
        world.tick_interpreted();
    }
    world.rewind_ticks(1, false).unwrap();
    assert_eq!(world.piston_state.logical_tick, 6);
    assert_eq!(world.history.len(), 3);
    world.rewind_ticks(2, false).unwrap();
    assert_eq!(world.piston_state.logical_tick, 4);
    assert_eq!(world.history.len(), 1);
    for _ in 0..5 {
        world.tick_interpreted();
    }
    world.rewind_ticks(4, false).unwrap();
    assert_eq!(world.piston_state.logical_tick, 5);
}

#[test]
fn piston_motion_entities_and_typed_queue_round_trip() {
    let mut world = world();
    let base = BlockPos::new(4, 30, 4);
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
    let sign_pos = base.offset(BlockFace::East);
    world.set_block(sign_pos, Block::from_name("oak_sign").unwrap());
    world.set_block_entity(
        sign_pos,
        BlockEntity::Sign(Box::new(SignBlockEntity::default())),
    );
    let observer = BlockPos::new(10, 30, 4);
    world.set_block(observer, Block::from_name("observer").unwrap());
    world.schedule_half_tick(observer, 7, TickPriority::High);
    world.schedule_half_tick(BlockPos::new(11, 30, 4), 7, TickPriority::High);
    world.set_block(base.offset(BlockFace::Bottom), Block::RedstoneBlock {});
    redstone::update(world.get_block(base), &mut world, base, None);
    world.enable_history(12, false).unwrap();
    let initial = state(&world);
    let mut expected = Vec::new();
    for _ in 0..6 {
        world.tick_interpreted();
        expected.push(state(&world));
    }
    world.rewind_ticks(5, false).unwrap();
    assert_eq!(state(&world), expected[0]);
    assert!(!world.piston_state.motions.is_empty());
    for target in expected.iter().skip(1) {
        world.tick_interpreted();
        assert_eq!(&state(&world), target);
    }
    world.rewind_ticks(6, false).unwrap();
    assert_eq!(state(&world), initial);
    // Retraction and short-pulse decisions retain the same exact movement state too.
    world.set_block(base.offset(BlockFace::Bottom), Block::Air);
    redstone::update(world.get_block(base), &mut world, base, None);
    let retract = state(&world);
    for _ in 0..4 {
        world.tick_interpreted();
    }
    let finished = state(&world);
    world.rewind_ticks(4, false).unwrap();
    assert_eq!(state(&world), retract);
    for _ in 0..4 {
        world.tick_interpreted();
    }
    assert_eq!(state(&world), finished);
}

#[test]
fn command_blocks_restore_mutable_data_without_reinitializing_or_replaying_output() {
    let mut world = world();
    let pos = BlockPos::new(4, 30, 4);
    world.set_block(pos, Block::from_name("repeating_command_block").unwrap());
    world.set_block_entity(
        pos,
        BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
            automatic: true,
            command: "say rewind test".into(),
            ..Default::default()
        })),
    );
    redstone::command_block::update(&mut world, pos);
    world.enable_history(8, false).unwrap();
    let initial = state(&world);
    for _ in 0..4 {
        world.tick_interpreted();
    }
    let expected = state(&world);
    assert_eq!(world.command_messages.len(), 4);
    world.rewind_ticks(4, false).unwrap();
    assert!(world.command_messages.is_empty());
    assert_eq!(state(&world), initial);
    for _ in 0..4 {
        world.tick_interpreted();
    }
    assert_eq!(state(&world), expected);
    assert_eq!(world.command_messages.len(), 4);
}

#[test]
fn edits_and_entity_only_changes_restore_and_snapshot_capture_preserves_packets() {
    let mut world = world();
    let pos = BlockPos::new(4, 30, 4);
    world.set_block(pos, Block::from_name("barrel").unwrap());
    world.set_block_entity(
        pos,
        BlockEntity::Container {
            comparator_override: 3,
            inventory: Default::default(),
            ty: ContainerType::Barrel,
        },
    );
    world.enable_history(2, false).unwrap();
    let initial = state(&world);
    world.tick_interpreted();
    let expected_id = world.get_block_raw(pos);
    let packets: Vec<_> = world.chunks[0].multi_blocks().collect();
    assert_eq!(packets.len(), 1);
    assert_eq!(packets[0].records.len(), 1);
    assert_eq!(packets[0].records[0].block_id, expected_id);
    if let Some(BlockEntity::Container {
        comparator_override,
        ..
    }) = world.get_block_entity_mut(pos)
    {
        *comparator_override = 12;
    }
    world.set_block(BlockPos::new(5, 30, 4), Block::Glass {});
    world.rewind_ticks(1, false).unwrap();
    assert_eq!(state(&world), initial);
    assert!(world.chunks[0].multi_blocks().next().is_none());
}

#[test]
fn default_custom_capacity_memory_disable_and_invalid_enable() {
    let mut world = world();
    assert!(!world.history.enabled());
    world.tick_interpreted();
    assert_eq!(world.history.memory_bytes(), 0);
    let projected = world.enable_history(DEFAULT_HISTORY_TICKS, false).unwrap();
    assert_eq!(world.history.capacity(), DEFAULT_HISTORY_TICKS);
    assert_eq!(world.history.len(), 0);
    let empty = world.history.memory_bytes();
    assert!(empty > 0 && projected > empty);
    world.tick_interpreted();
    let recorded = world.history.memory_bytes();
    assert!(recorded > empty);
    for invalid in [0, usize::MAX] {
        assert!(world.enable_history(invalid, false).is_err());
        assert_eq!(world.history.capacity(), DEFAULT_HISTORY_TICKS);
        assert_eq!(world.history.len(), 1);
        assert_eq!(world.history.memory_bytes(), recorded);
    }
    assert_eq!(world.history.disable(), recorded);
    assert_eq!(world.history.slots.capacity(), 0);
    assert_eq!(world.history.memory_bytes(), 0);
    assert!(world.rewind_ticks(1, false).is_err());
    world.enable_history(2, false).unwrap();
    for _ in 0..6 {
        world.tick_interpreted();
    }
    assert_eq!(world.history.len(), 2);
    world.enable_history(1, false).unwrap();
    assert_eq!(world.history.len(), 0);
}

#[test]
fn normal_limit_and_admin_override_preserve_existing_history_on_rejection() {
    let mut world = world();
    world.enable_history(NORMAL_HISTORY_LIMIT, false).unwrap();
    assert_eq!(world.history.capacity(), NORMAL_HISTORY_LIMIT);
    world.tick_interpreted();
    let before = state(&world);
    let memory = world.history.memory_bytes();
    for ticks in [NORMAL_HISTORY_LIMIT + 1, usize::MAX] {
        assert!(world
            .enable_history(ticks, false)
            .unwrap_err()
            .contains(UNLIMITED_HISTORY_PERMISSION));
        assert!(world
            .rewind_ticks(ticks, false)
            .unwrap_err()
            .contains(UNLIMITED_HISTORY_PERMISSION));
        assert_eq!(world.history.capacity(), NORMAL_HISTORY_LIMIT);
        assert_eq!(world.history.len(), 1);
        assert_eq!(world.history.memory_bytes(), memory);
        assert_eq!(state(&world), before);
    }
    world
        .enable_history(NORMAL_HISTORY_LIMIT + 1, true)
        .unwrap();
    assert_eq!(world.history.capacity(), NORMAL_HISTORY_LIMIT + 1);
    assert_eq!(world.history.len(), 0);
    // Admin bypasses the policy cap, but not depth validation or representable capacity.
    assert!(world
        .rewind_ticks(NORMAL_HISTORY_LIMIT + 1, true)
        .unwrap_err()
        .contains("Only 0"));
    assert!(world.enable_history(usize::MAX, true).is_err());
    assert_eq!(world.history.capacity(), NORMAL_HISTORY_LIMIT + 1);
}

#[test]
fn partial_stepping_is_excluded_and_partial_enable_is_rejected() {
    let mut world = world();
    let pos = BlockPos::new(4, 30, 4);
    world.schedule_half_tick(pos, 1, TickPriority::Normal);
    world.picotick_advance(1);
    assert_ne!(world.piston_state.phase, AdvancePhase::BetweenTicks);
    assert!(world.enable_history(2, false).is_err());
    world.tick_interpreted();
    world.enable_history(2, false).unwrap();
    let before = state(&world);
    world.nanotick_advance(4);
    world.picotick_advance(4);
    assert_eq!(state(&world), before);
    assert_eq!(world.history.len(), 0);
}

#[test]
fn memory_estimate_includes_owned_entity_payloads() {
    let mut world = world();
    let plain = Snapshot::capture(&mut world).heap_bytes;
    let pos = BlockPos::new(4, 30, 4);
    world.set_block_entity(
        pos,
        BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
            command: "x".repeat(8000),
            last_output: Some("y".repeat(9000)),
            ..Default::default()
        })),
    );
    let command = Snapshot::capture(&mut world).heap_bytes;
    assert!(command >= plain + 17000);
    world.enable_history(3, false).unwrap();
    world.tick_interpreted();
    assert_eq!(world.history.heap_bytes, command);
    world.rewind_ticks(1, false).unwrap();
    assert_eq!(world.history.heap_bytes, 0);
}
