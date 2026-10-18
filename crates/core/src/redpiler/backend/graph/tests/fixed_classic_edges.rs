//! Focused witnesses for fixed-circuit literal sources, boundary cuts and live locks.
use super::fixed_classic_tests::{assert_equivalent, attach, compile};
use super::*;
use crate::plot::PlotWorld;
use crate::world::storage::Chunk;
use mchprs_blocks::BlockDirection;
use mchprs_blocks::blocks::{HopperFacing, Lever, LeverFace, RedstoneRepeater};

const BOUNDS: (BlockPos, BlockPos) = (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15));
fn empty() -> PlotWorld {
    PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default())
}
fn wire(world: &mut PlotWorld, position: BlockPos) {
    world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(position, Block::RedstoneWire { wire: Default::default() });
    let mut wire = crate::redstone::wire::get_state_for_placement(world, position);
    wire.power = 0;
    world.set_block(position, Block::RedstoneWire { wire });
}
#[test]
fn fixed_classic_redstone_block_emits_literal_fifteen_with_false_powered_flag() {
    let mut world = empty();
    let source = BlockPos::new(5, 20, 5);
    let dust = BlockPos::new(6, 20, 5);
    world.set_block(source, Block::RedstoneBlock);
    wire(&mut world, dust);
    let mut reference = compile(&world, BOUNDS);
    let source_node = reference.positions[&source];
    assert!(!reference.node_state(source_node).powered);
    assert_eq!(reference.node_state(source_node).output_strength, 15);
    let (mut candidate, _) = attach(&reference);
    let dust_node = reference.positions[&dust];
    let FixedClassicKind::Wire(index) = candidate.fixed_classic.as_ref().unwrap().nodes[dust_node.index()].kind else {
        panic!("dust missing")
    };
    let queries = &candidate.fixed_classic.as_ref().unwrap().wires[index as usize].no_dust;
    assert!(
        queries.iter().any(|query| query.constant == 15 && query.start == query.end),
        "redstone block must compile to literal15"
    );
    reference.evaluate_seed(dust_node, None).unwrap();
    candidate.evaluate_seed(dust_node, None).unwrap();
    assert_equivalent(&reference, &candidate, 0);
    assert_eq!(candidate.wire_state(dust_node).power, 15);
    assert!(candidate.fixed_classic_counts.seed_calls > 0);
}
#[test]
fn fixed_classic_boundary_wire_is_rejected_before_execution() {
    let mut world = empty();
    wire(&mut world, BlockPos::new(5, 20, 5));
    wire(&mut world, BlockPos::new(6, 20, 5));
    let result = GraphBackend::compile_with_options(
        &world,
        (BlockPos::new(6, 0, 0), BlockPos::new(15, 31, 15)),
        vec![],
        &crate::redpiler::CompilerOptions {
            optimization_level: Some(2),
            ..Default::default()
        },
        Arc::new(TaskMonitor::default()),
    );
    // Preparation already refuses an executable wire beyond the selection.
    assert!(matches!(
        result,
        Err(GraphBackendError::Callbacks(callbacks::CallbackError::UninventoriedReceiver { .. }))
    ));
}

const MAIN: BlockPos = BlockPos::new(5, 20, 5);
const LOCK: BlockPos = BlockPos::new(8, 20, 7);
const REPEATER: BlockPos = BlockPos::new(8, 20, 5);
const LAMP: BlockPos = BlockPos::new(10, 20, 5);
const HOPPER: BlockPos = BlockPos::new(9, 20, 6);
fn locked_world() -> PlotWorld {
    let mut world = empty();
    for position in [MAIN, LOCK] {
        world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(
            position,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
            },
        );
    }
    for (position, facing, delay) in [(REPEATER, BlockDirection::West, 2), (BlockPos::new(8, 20, 6), BlockDirection::South, 1)] {
        world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(
            position,
            Block::RedstoneRepeater {
                repeater: RedstoneRepeater {
                    facing,
                    delay,
                    powered: false,
                    locked: false,
                },
            },
        );
    }
    world.set_block(BlockPos::new(9, 20, 5), Block::Stone {});
    world.set_block(LAMP, Block::RedstoneLamp { lit: false });
    world.set_block(
        HOPPER,
        Block::Hopper {
            enabled: true,
            facing: HopperFacing::Down,
        },
    );
    for position in [BlockPos::new(6, 20, 5), BlockPos::new(7, 20, 5)] {
        wire(&mut world, position);
    }
    world
}
fn native_toggle(world: &mut PlotWorld, position: BlockPos) {
    let Block::Lever { mut lever } = world.get_block(position) else {
        panic!("control missing")
    };
    lever.powered = !lever.powered;
    world.set_block(position, Block::Lever { lever });
    crate::redstone::update_surrounding_blocks(world, position);
    crate::redstone::update_surrounding_blocks(world, position.offset(BlockFace::Bottom));
}
fn check_native(candidate: &GraphBackend, native: &PlotWorld) {
    for (&position, &node) in &candidate.positions {
        assert_eq!(candidate.node_block(node), native.get_block(position), "{position:?}");
    }
    assert_eq!(candidate.scheduled_ticks(), native.scheduler().iter_entries().collect::<Vec<_>>());
}
#[test]
fn fixed_classic_locking_repeater_lamp_and_hopper_preserve_native_future() {
    let mut native = locked_world();
    let mut reference = compile(&native, BOUNDS);
    let (mut candidate, _) = attach(&reference);
    let repeater = candidate.positions[&REPEATER];
    let mut locked = false;
    let mut lit = false;
    let mut disabled = false;
    let mut tick = 0;
    for (control, ticks) in [(MAIN, 3), (LOCK, 2), (MAIN, 3), (LOCK, 6), (MAIN, 3), (LOCK, 2), (LOCK, 2), (MAIN, 6)] {
        reference.use_block(control).unwrap();
        candidate.use_block(control).unwrap();
        native_toggle(&mut native, control);
        assert_equivalent(&reference, &candidate, tick);
        check_native(&candidate, &native);
        for _ in 0..ticks {
            reference.tick().unwrap();
            candidate.tick().unwrap();
            native.tick_interpreted();
            tick += 1;
            assert_equivalent(&reference, &candidate, tick);
            check_native(&candidate, &native);
            locked |= candidate.node_state(repeater).repeater_locked;
            lit |= matches!(candidate.node_block(candidate.positions[&LAMP]), Block::RedstoneLamp { lit: true });
            disabled |= matches!(candidate.node_block(candidate.positions[&HOPPER]), Block::Hopper { enabled: false, .. });
        }
    }
    assert!(locked, "fixture must actually exercise a live lock transition");
    assert!(lit, "fixture must actually light the lamp");
    assert!(disabled, "fixture must actually disable the hopper");
    assert!(candidate.fixed_classic_counts.wire_steps > 0);
    assert!(candidate.fixed_classic_counts.gate_ops > 0);
}

#[test]
fn fixed_classic_whole_decline_keeps_local_classic_net_and_native_future() {
    let mut native = empty();
    native.set_block(MAIN.offset(BlockFace::Bottom), Block::Stone {});
    native.set_block(
        MAIN,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    let receiver = BlockPos::new(8, 20, 5);
    native.set_block(receiver.offset(BlockFace::Bottom), Block::Stone {});
    native.set_block(
        receiver,
        Block::RedstoneRepeater {
            repeater: RedstoneRepeater {
                facing: BlockDirection::West,
                delay: 2,
                powered: false,
                locked: false,
            },
        },
    );
    for position in [BlockPos::new(6, 20, 5), BlockPos::new(7, 20, 5)] {
        wire(&mut native, position);
    }
    native.set_block(
        BlockPos::new(13, 25, 13),
        Block::Observer {
            observer: mchprs_blocks::blocks::RedstoneObserver {
                facing: BlockFace::North.into(),
                powered: false,
            },
        },
    );
    let mut reference = compile(&native, BOUNDS);
    let mut candidate = GraphBackend::compile_with_options(
        &native,
        BOUNDS,
        vec![],
        &crate::redpiler::CompilerOptions {
            optimization_level: Some(2),
            classic_redstone: true,
            ..Default::default()
        },
        Arc::new(TaskMonitor::default()),
    )
    .unwrap();
    assert!(
        candidate.program.fixed_classic().is_none(),
        "unsupported Observer must decline the whole fixed engine"
    );
    assert!(candidate.fixed_classic.is_none());
    assert!(
        !candidate.program.classic().nets.is_empty(),
        "the unrelated Observer must not reject the local classic net"
    );
    let mut tick = 0;
    for ticks in [0, 1, 3, 0, 2, 4] {
        reference.use_block(MAIN).unwrap();
        candidate.use_block(MAIN).unwrap();
        native_toggle(&mut native, MAIN);
        for step in 0..=ticks {
            if step > 0 {
                reference.tick().unwrap();
                candidate.tick().unwrap();
                native.tick_interpreted();
                tick += 1;
            }
            check_native(&candidate, &native);
            assert_eq!(candidate.scheduled_ticks(), reference.scheduled_ticks(), "FIFO at {tick}");
            assert_eq!(candidate.future.pending, reference.future.pending);
            assert_eq!(candidate.future.pending_nodes, reference.future.pending_nodes);
            assert_eq!(candidate.future.clock.logical_tick, reference.future.clock.logical_tick);
            assert_eq!(candidate.future.clock.phase, reference.future.clock.phase);
            assert_eq!(candidate.future.clock.scheduled_advanced, reference.future.clock.scheduled_advanced);
            for (index, node) in reference.program.nodes().iter().enumerate() {
                let id = NodeId::new(index as u32);
                if matches!(node.kind, CircuitNodeKind::Wire(_)) {
                    assert_eq!(candidate.wire_state(id), reference.wire_state(id), "reconstructed dust at {tick}");
                } else {
                    assert_eq!(candidate.state.components[index], reference.state.components[index], "actor at {tick}");
                }
            }
        }
    }
    assert!(candidate.counts.classic_net_invocations > 0);
    assert_eq!(candidate.counts.wire_walks, 0);
}
