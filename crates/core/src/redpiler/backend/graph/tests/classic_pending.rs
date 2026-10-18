//! Typed tick identity and duplicate multiplicity survive the dense pending index.
use super::*;
use crate::{plot::PlotWorld, world::storage::Chunk};

const TORCH: BlockPos = BlockPos::new(8, 20, 8);
const AIR: BlockPos = BlockPos::new(12, 20, 12);

fn world() -> PlotWorld {
    let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
    world.set_block(TORCH.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(TORCH, Block::RedstoneTorch { lit: false });
    world
}

fn pair(ticks: Vec<TickEntry>) -> [GraphBackend; 2] {
    let world = world();
    let dense = GraphBackend::compile(&world, (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15)), ticks.clone(), Arc::default()).unwrap();
    let mut general = GraphBackend::compile(&world, (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15)), ticks, Arc::default()).unwrap();
    general.test_recorder.general_pending_queries = true;
    [dense, general]
}

fn assert_same(engines: &[GraphBackend; 2]) {
    let [dense, general] = engines;
    assert_eq!(dense.state, general.state);
    assert_eq!(dense.scheduled_ticks(), general.scheduled_ticks());
    assert_eq!(dense.future.pending, general.future.pending);
    assert_eq!(dense.future.pending_nodes, general.future.pending_nodes);
    for index in 0..dense.program.nodes().len() {
        let node = NodeId::new(index as u32);
        assert_eq!(dense.is_pending(node), dense.pending_from_general_map(node));
        assert_eq!(dense.is_pending(node), general.is_pending(node));
    }
}

#[test]
fn canonical_pending_counts_preserve_duplicates_and_imported_identities() {
    let registry = Block::RedstoneTorch { lit: false }.registry_id();
    let entry = |block_type, ticks_left, tick_priority| TickEntry {
        pos: TORCH,
        block_type,
        ticks_left,
        tick_priority,
    };
    let ticks = vec![
        entry(Some(registry), 1, TickPriority::High),
        entry(Some(registry), 3, TickPriority::Normal),
        entry(None, 1, TickPriority::Higher),
        entry(Some(Block::Stone {}.registry_id()), 2, TickPriority::Normal),
        TickEntry {
            pos: AIR,
            block_type: Some(Block::Stone {}.registry_id()),
            ticks_left: 2,
            tick_priority: TickPriority::Normal,
        },
    ];
    let mut engines = pair(ticks);
    let node = engines[0].positions[&TORCH];
    assert_eq!(engines[0].future.pending_nodes[node.index()], 2);
    assert_same(&engines);
    for engine in &mut engines {
        engine.tick().unwrap();
    }
    assert_same(&engines);
    assert_eq!(
        engines[0].future.pending_nodes[node.index()],
        1,
        "one typed duplicate survives the first pop"
    );
    for engine in &mut engines {
        let before = engine.scheduled_ticks();
        engine.schedule(node, 1, TickPriority::Normal);
        assert_eq!(engine.scheduled_ticks(), before, "remaining duplicate suppresses a new canonical event");
    }
    for engine in &mut engines {
        engine.tick().unwrap();
    }
    assert_same(&engines);
    assert_eq!(
        engines[0].future.pending_nodes[node.index()],
        1,
        "stale work does not consume the typed duplicate"
    );
    for engine in &mut engines {
        engine.tick().unwrap();
    }
    assert_same(&engines);
    assert_eq!(engines[0].future.pending_nodes[node.index()], 0);
}

#[test]
fn untyped_and_stale_work_allow_new_typed_tick_then_reset_clears_dense_counts() {
    let mut engines = pair(vec![
        TickEntry {
            pos: TORCH,
            block_type: None,
            ticks_left: 5,
            tick_priority: TickPriority::Higher,
        },
        TickEntry {
            pos: TORCH,
            block_type: Some(Block::Stone {}.registry_id()),
            ticks_left: 4,
            tick_priority: TickPriority::High,
        },
    ]);
    let node = engines[0].positions[&TORCH];
    assert!(!engines[0].is_pending(node));
    for engine in &mut engines {
        engine.schedule_half_tick(node, 0, TickPriority::NanoTick);
        engine.schedule_half_tick(node, 0, TickPriority::NanoTick);
    }
    assert_same(&engines);
    assert_eq!(engines[0].future.pending_nodes[node.index()], 1);
    for engine in &mut engines {
        engine.tick().unwrap();
    }
    assert_same(&engines);
    for engine in &mut engines {
        engine.schedule(node, 2, TickPriority::Normal);
    }
    let mut projected = [world(), world()];
    for (engine, world) in engines.iter_mut().zip(&mut projected) {
        engine.reset(world);
    }
    assert_same(&engines);
    assert!(engines[0].future.pending_nodes.iter().all(|count| *count == 0));
    for _ in 0..5 {
        for world in &mut projected {
            world.tick_interpreted();
        }
        assert_eq!(projected[0].get_block(TORCH), projected[1].get_block(TORCH));
        assert_eq!(
            projected[0].scheduler().iter_entries().collect::<Vec<_>>(),
            projected[1].scheduler().iter_entries().collect::<Vec<_>>()
        );
    }
}
