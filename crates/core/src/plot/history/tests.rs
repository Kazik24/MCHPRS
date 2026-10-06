use super::*;
use crate::redstone;
use crate::world::storage::Chunk;
use crate::world::World;
use mchprs_blocks::block_entities::{BlockEntity, CommandBlockEntity, ContainerType};
use mchprs_blocks::blocks::{Block, RedstonePiston};
use mchprs_blocks::{BlockFace, BlockFacing, BlockPos};
use mchprs_world::{TickEntry, TickPriority};

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
    scheduler: Vec<TickEntry>,
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
        // Relative delays, FIFO contents, priorities and expected types are observable.
        scheduler: world.to_be_ticked.iter_entries().collect(),
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
    world.set_block_entity(sign_pos, BlockEntity::Sign(Box::default()));
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
        .contains("Only 0 game ticks are available to rewind"));
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
fn memory_stats_include_serialized_entity_payloads_and_compressed_storage() {
    let mut world = world();
    let plain = capture_raw(&mut world, &budget::WORK).unwrap().data.len();
    let pos = BlockPos::new(4, 30, 4);
    world.set_block_entity(
        pos,
        BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
            command: "x".repeat(8000),
            last_output: Some("y".repeat(9000)),
            ..Default::default()
        })),
    );
    let command = capture_raw(&mut world, &budget::WORK).unwrap().data.len();
    assert!(command >= plain + 17000);
    world.enable_history(3, false).unwrap();
    world.tick_interpreted();
    assert_eq!(world.history.raw_bytes, command);
    assert!(world.history.heap_bytes < command);
    world.rewind_ticks(1, false).unwrap();
    assert_eq!(world.history.heap_bytes, 0);
}

fn limited_world(limit: usize) -> PlotWorld {
    let mut world = world();
    world.history = TickHistory::with_budgets(Budget::new(limit), Budget::new(8 * 1024 * 1024));
    world
}

#[test]
fn borrowed_chunk_encoding_matches_palettes_and_direct_storage() {
    for states in [16, 300, 1000] {
        let mut chunk = Chunk::empty(0, 0);
        for index in 0..4096 {
            chunk.set_block(index % 16, index / 256, index / 16 % 16, index % states + 1);
        }
        chunk.prepare_history();
        let borrowed = bincode::serialize(&chunk.history_view()).unwrap();
        let owned = bincode::serialize(&chunk.save()).unwrap();
        assert_eq!(borrowed, owned, "palette with {states} states");
    }
}

#[test]
fn shared_dictionary_compresses_repeated_random_data() {
    use rand::{RngCore, SeedableRng};
    let work = Budget::new(1024 * 1024);
    let mut raw = Bytes::zeroed(&work, 60000).unwrap();
    rand::rngs::StdRng::seed_from_u64(22).fill_bytes(&mut raw.data);
    let dictionary = raw.data.clone();
    let without = Encoded::encode(Bytes::copy(&work, &raw.data).unwrap(), &[], &work).unwrap();
    let shared = Encoded::encode(raw, &dictionary, &work).unwrap();
    assert!(!without.compressed);
    assert!(shared.compressed);
    assert!(shared.bytes.data.len() < without.bytes.data.len() / 10);
    let mut decoded = vec![0; shared.raw_len];
    let count =
        lz4_flex::block::decompress_into_with_dict(&shared.bytes.data, &mut decoded, &dictionary)
            .unwrap();
    assert_eq!(count, dictionary.len());
    assert_eq!(decoded, dictionary);
}

#[test]
fn compressed_admission_uses_stored_size_and_accounts_for_dictionary() {
    let mut world = limited_world(256 * 1024);
    world.set_block_entity(
        BlockPos::new(4, 30, 4),
        BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
            command: "repeated pattern ".repeat(65536),
            ..Default::default()
        })),
    );
    world.enable_history(10, false).unwrap();
    world.tick_interpreted();
    assert!(world.history.raw_bytes > world.history.budget.stats().1);
    assert!(world.history.heap_bytes < world.history.raw_bytes / 10);
    let dictionary = world.history.dictionary().to_vec();
    assert_eq!(dictionary.len(), 65535);
    for _ in 0..20 {
        world.tick_interpreted();
    }
    assert_eq!(world.history.dictionary(), dictionary);
    assert_eq!(world.history.len(), 10);
    let before = state(&world);
    world.rewind_ticks(1, false).unwrap();
    world.tick_interpreted();
    assert_eq!(state(&world), before);
    let budget = world.history.budget.clone();
    assert_eq!(budget.stats().0, world.history.memory_bytes());
    world.history.disable();
    assert_eq!(budget.stats().0, 0);
    assert_eq!(world.history.work.stats().0, 0);
}

#[test]
fn byte_limit_evicts_oldest_ticks_without_gaps_and_oversized_capture_stops() {
    let mut world = limited_world(1024 * 1024);
    world.enable_history(8, false).unwrap();
    let work = world.history.work.clone();
    let raw = capture_raw(&mut world, &work).unwrap();
    let sample = Encoded::encode(raw, world.history.dictionary(), &world.history.work).unwrap();
    let limit = world.history.memory_bytes() + sample.bytes.data.len() * 2 + 8;
    drop(sample);
    world.history.budget.set_limit(limit, || Ok(())).unwrap();
    for _ in 0..20 {
        world.tick_interpreted();
    }
    let depth = world.history.len();
    assert!(depth > 0 && depth < 8);
    assert!(world.history.memory_bytes() <= limit);
    world.rewind_ticks(depth, false).unwrap();
    assert_eq!(world.piston_state.logical_tick, 20 - depth as u64);
    // Grow the circuit so one raw-fallback record cannot fit, even with no older ticks.
    let mut rng = rand::rngs::StdRng::seed_from_u64(19);
    use rand::{Rng, SeedableRng};
    let command: String = (0..8000).map(|_| rng.gen_range('a'..='z')).collect();
    world.set_block_entity(
        BlockPos::new(4, 30, 4),
        BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
            command,
            ..Default::default()
        })),
    );
    let tick = world.piston_state.logical_tick;
    world.tick_interpreted();
    assert_eq!(world.piston_state.logical_tick, tick + 1);
    assert!(!world.history.enabled());
    assert_eq!(world.history.budget.stats().0, 0);
}

#[test]
fn shared_limits_release_on_drop_and_reenable_rejection_preserves_history() {
    let budget = Budget::new(1024 * 1024);
    let work = Budget::new(1024 * 1024);
    let mut first = world();
    let mut second = world();
    first.history = TickHistory::with_budgets(budget.clone(), work.clone());
    second.history = TickHistory::with_budgets(budget.clone(), work.clone());
    first.enable_history(3, false).unwrap();
    second.enable_history(3, false).unwrap();
    first.tick_interpreted();
    second.tick_interpreted();
    assert_eq!(
        budget.stats().0,
        first.history.memory_bytes() + second.history.memory_bytes()
    );
    let memory = first.history.memory_bytes();
    let before = state(&first);
    budget.set_limit(budget.stats().0, || Ok(())).unwrap();
    assert!(first.enable_history(4, false).is_err());
    assert_eq!(first.history.memory_bytes(), memory);
    assert_eq!(state(&first), before);
    assert_eq!(first.history.len(), 1);
    assert!(first.enable_history(i32::MAX as usize, true).is_err());
    drop(second);
    assert_eq!(budget.stats().0, memory);
    drop(first);
    assert_eq!(budget.stats().0, 0);
    assert_eq!(work.stats().0, 0);
}

#[test]
fn corrupt_snapshot_or_exhausted_workspace_preserves_rewind_state() {
    let mut world = limited_world(1024 * 1024);
    world.enable_history(3, false).unwrap();
    world.tick_interpreted();
    let before = state(&world);
    let memory = world.history.memory_bytes();
    let slot = world.history.slots[0].as_mut().unwrap();
    slot.checksum ^= 1;
    assert!(world.rewind_ticks(1, false).is_err());
    assert_eq!(state(&world), before);
    assert_eq!(world.history.len(), 1);
    assert_eq!(world.history.memory_bytes(), memory);
    world.history.slots[0].as_mut().unwrap().checksum ^= 1;
    world.history.work.set_limit(0, || Ok(())).unwrap();
    assert!(world.rewind_ticks(1, false).is_err());
    assert_eq!(state(&world), before);
    assert_eq!(world.history.len(), 1);
    world.tick_interpreted(); // Stops recording and frees it rather than leaving a missing tick.
    assert!(!world.history.enabled());
    assert_eq!(world.history.budget.stats().0, 0);
}

#[test]
fn raw_fallback_and_concurrent_reservations_obey_exact_limits() {
    use rand::{RngCore, SeedableRng};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Barrier,
    };
    let work = Budget::new(16384);
    let mut raw = Bytes::zeroed(&work, 2048).unwrap();
    rand::rngs::StdRng::seed_from_u64(21).fill_bytes(&mut raw.data);
    let encoded = Encoded::encode(raw, &[], &work).unwrap();
    assert!(!encoded.compressed);
    let stored = Budget::new(2048);
    assert!(Bytes::copy(&stored, &encoded.bytes.data).is_ok());
    let held = Bytes::copy(&stored, &encoded.bytes.data).unwrap();
    assert!(stored.reserve(1).is_err());
    assert!(stored.set_limit(2047, || Ok(())).is_err());
    assert!(stored.set_limit(4096, || Err("disk error".into())).is_err());
    assert_eq!(stored.stats().1, 2048);
    drop(held);
    stored.set_limit(0, || Ok(())).unwrap();
    assert!(stored.reserve(1).is_err());

    let budget = Budget::new(4);
    let barrier = Arc::new(Barrier::new(17));
    let accepted = Arc::new(AtomicUsize::new(0));
    let threads: Vec<_> = (0..16)
        .map(|_| {
            let budget = budget.clone();
            let barrier = barrier.clone();
            let accepted = accepted.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let held = budget.reserve(1).ok();
                if held.is_some() {
                    accepted.fetch_add(1, Ordering::Relaxed);
                }
                barrier.wait();
                barrier.wait();
                drop(held);
            })
        })
        .collect();
    barrier.wait();
    barrier.wait();
    assert_eq!(budget.stats().0, 4);
    assert_eq!(accepted.load(Ordering::Relaxed), 4);
    barrier.wait();
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(budget.stats().0, 0);
}
