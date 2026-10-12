//! Graph backend differential and regression tests.
use super::*;
use crate::plot::PLOT_WIDTH;
use crate::plot::PlotWorld;
use crate::plot::worldedit::{WorldEditClipboard, load_schematic, paste_clipboard};
use crate::redpiler::backend::ScheduledBlockTick;
use crate::redstone;
use crate::redstone::instant_piston_tests::capture_at;
use crate::world::storage::Chunk;
use mchprs_blocks::blocks::{
    Block, ButtonFace, ComparatorMode, Lever, LeverFace, RedstoneComparator, RedstoneObserver,
    RedstoneRepeater, StoneButton,
};
use mchprs_blocks::{BlockDirection, BlockFacing};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const BOUNDS: (BlockPos, BlockPos) = (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15));
const LEVER_POS: BlockPos = BlockPos::new(5, 20, 5);
const NEXT_POS: BlockPos = BlockPos::new(6, 20, 5);

fn empty_world() -> PlotWorld {
    PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default())
}

fn lever(powered: bool) -> Block {
    Block::Lever {
        lever: Lever::new(LeverFace::Floor, BlockDirection::North, powered),
    }
}

fn set_native_lever(world: &mut PlotWorld, position: BlockPos, powered: bool) {
    set_native_lever_with_support(world, position, powered, BlockFace::Bottom);
}

fn set_native_lever_with_support(
    world: &mut PlotWorld,
    position: BlockPos,
    powered: bool,
    support: BlockFace,
) {
    let Block::Lever { mut lever } = world.get_block(position) else {
        panic!("expected lever at {position:?}");
    };
    if lever.powered == powered {
        return;
    }
    lever.powered = powered;
    world.set_block(position, Block::Lever { lever });
    redstone::update_surrounding_blocks(world, position);
    redstone::update_surrounding_blocks(world, position.offset(support));
}

fn graph(world: &PlotWorld, ticks: Vec<TickEntry>) -> GraphBackend {
    GraphBackend::compile(world, BOUNDS, ticks, Arc::default()).unwrap()
}

fn state_at(backend: &GraphBackend, position: BlockPos) -> NodeState {
    backend.node_state(backend.positions[&position])
}

fn ticks_at(
    position: BlockPos,
    block: Block,
    count: usize,
    priority: TickPriority,
) -> Vec<TickEntry> {
    (0..count)
        .map(|_| TickEntry {
            ticks_left: 0,
            tick_priority: priority,
            pos: position,
            block_type: Some(block.registry_id()),
        })
        .collect()
}

#[test]
fn graph_route_powers_lamp_synchronously_and_flush_only_projects_state() {
    let lamp_pos = LEVER_POS.offset(BlockFace::East);
    let mut native = empty_world();
    let mut graph_world = empty_world();
    for world in [&mut native, &mut graph_world] {
        world.set_block(LEVER_POS.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(LEVER_POS, lever(false));
        world.set_block(lamp_pos, Block::RedstoneLamp { lit: false });
    }
    let mut backend = graph(&graph_world, vec![]);
    let lever_node = backend.positions[&LEVER_POS];
    let lamp_node = backend.positions[&lamp_pos];

    backend.use_block(LEVER_POS).unwrap();
    set_native_lever(&mut native, LEVER_POS, true);
    assert!(state_at(&backend, lamp_pos).powered);
    assert!(matches!(
        native.get_block(lamp_pos),
        Block::RedstoneLamp { lit: true }
    ));

    let trace = backend.trace();
    let source_change = trace
        .iter()
        .position(|item| item.starts_with(&format!("state {lever_node:?}")))
        .unwrap();
    let lamp_change = trace
        .iter()
        .position(|item| item.starts_with(&format!("state {lamp_node:?}")))
        .unwrap();
    let delivery = trace
        .iter()
        .position(|item| item.starts_with("route "))
        .unwrap();
    assert!(
        source_change < delivery && delivery < lamp_change,
        "{trace:#?}"
    );

    let state = backend.state.clone();
    let scheduled = backend.scheduled_ticks();
    let pending = backend.pending.clone();
    let clock = backend.clock;
    backend.flush(&mut graph_world);
    assert_eq!(backend.state, state);
    assert_eq!(backend.scheduled_ticks(), scheduled);
    assert_eq!(backend.pending, pending);
    assert_eq!(backend.clock.logical_tick, clock.logical_tick);
    assert_eq!(backend.clock.phase, clock.phase);
    assert_eq!(backend.clock.scheduled_advanced, clock.scheduled_advanced);
    assert_eq!(graph_world.get_block(lamp_pos), native.get_block(lamp_pos));
}

#[test]
fn external_torch_boundary_supplies_power_to_selected_lamp() {
    let lamp_pos = BlockPos::new(10, 20, 5);
    let torch_pos = lamp_pos.offset(BlockFace::East);
    let bounds = (BlockPos::new(0, 0, 0), BlockPos::new(10, 31, 10));
    let mut native = empty_world();
    let mut graph_world = empty_world();
    for world in [&mut native, &mut graph_world] {
        world.set_block(lamp_pos, Block::RedstoneLamp { lit: false });
        world.set_block(torch_pos, Block::RedstoneTorch { lit: true });
    }
    let mut backend = GraphBackend::compile(&graph_world, bounds, vec![], Arc::default()).unwrap();
    let lamp = backend.positions[&lamp_pos];
    assert!(!backend.positions.contains_key(&torch_pos));
    assert_eq!(backend.input_strength(lamp, InputPort::Main), 15);

    backend.update_lamp(lamp, None).unwrap();
    redstone::update(native.get_block(lamp_pos), &mut native, lamp_pos, None);
    backend.flush(&mut graph_world);
    assert_eq!(graph_world.get_block(lamp_pos), native.get_block(lamp_pos));
    assert!(matches!(
        graph_world.get_block(lamp_pos),
        Block::RedstoneLamp { lit: true }
    ));
}

#[test]
fn observer_filters_direction_suppresses_pending_and_uses_two_tick_edges() {
    let observer_pos = NEXT_POS;
    let mut native = empty_world();
    let mut graph_world = empty_world();
    for world in [&mut native, &mut graph_world] {
        world.set_block(LEVER_POS.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(LEVER_POS, lever(false));
        world.set_block(
            observer_pos,
            Block::Observer {
                observer: RedstoneObserver {
                    facing: BlockFacing::West,
                    powered: false,
                },
            },
        );
    }
    let mut backend = graph(&graph_world, vec![]);
    let observer = backend.positions[&observer_pos];
    backend
        .update_component(
            observer,
            DeliveryKind::Update {
                direction: Some(BlockFace::North),
            },
            None,
        )
        .unwrap();
    assert!(
        !backend.is_pending(observer),
        "wrong watched face must be ignored"
    );

    backend.use_block(LEVER_POS).unwrap();
    set_native_lever(&mut native, LEVER_POS, true);
    assert!(backend.is_pending(observer));
    assert!(native.pending_tick_at(observer_pos));
    backend
        .update_component(
            observer,
            DeliveryKind::Update {
                direction: Some(BlockFace::West),
            },
            None,
        )
        .unwrap();
    assert_eq!(
        backend.scheduled_ticks().len(),
        1,
        "a duplicate callback is suppressed"
    );

    for (tick, expected_powered) in [(1, false), (2, true), (3, true), (4, false)] {
        backend.tick().unwrap();
        native.tick_interpreted();
        backend.flush(&mut graph_world);
        assert_eq!(
            state_at(&backend, observer_pos).powered,
            expected_powered,
            "tick {tick}"
        );
        assert_eq!(
            graph_world.get_block(observer_pos),
            native.get_block(observer_pos),
            "tick {tick}"
        );
        assert_eq!(
            backend.clock.logical_tick,
            native.piston_state().logical_tick
        );
    }

    let mut recovery_world = empty_world();
    recovery_world.set_block(
        observer_pos,
        Block::Observer {
            observer: RedstoneObserver {
                facing: BlockFacing::West,
                powered: true,
            },
        },
    );
    let mut recovery = graph(&recovery_world, vec![]);
    let recovery_node = recovery.positions[&observer_pos];
    recovery
        .update_component(
            recovery_node,
            DeliveryKind::Update { direction: None },
            None,
        )
        .unwrap();
    assert!(
        recovery.is_pending(recovery_node),
        "powered/None recovery schedules the off edge"
    );
}

#[test]
fn shape_delivery_does_not_sample_observers() {
    let source = NEXT_POS;
    let away = source.offset(BlockFace::West);
    let toward = source.offset(BlockFace::East);
    let observer = |facing| Block::Observer {
        observer: RedstoneObserver {
            facing,
            powered: false,
        },
    };
    let mut native = empty_world();
    let mut graph_world = empty_world();
    for world in [&mut native, &mut graph_world] {
        world.set_block(source, Block::Stone {});
        world.set_block(away, observer(BlockFacing::West));
        world.set_block(toward, observer(BlockFacing::West));
    }
    let mut backend = graph(&graph_world, vec![]);

    for (position, shape_direction, update_direction) in [
        (away, BlockFace::West, BlockFace::East),
        (toward, BlockFace::East, BlockFace::West),
    ] {
        let node = backend.positions[&position];
        crate::interaction::change(
            native.get_block(position),
            &mut native,
            position,
            shape_direction,
        );
        backend
            .update_component(
                node,
                DeliveryKind::Shape {
                    direction: shape_direction,
                },
                None,
            )
            .unwrap();
        assert!(!backend.is_pending(node));
        assert!(!native.pending_tick_at(position));

        redstone::update(
            native.get_block(position),
            &mut native,
            position,
            Some(update_direction),
        );
        backend
            .update_component(
                node,
                DeliveryKind::Update {
                    direction: Some(update_direction),
                },
                None,
            )
            .unwrap();
        assert_eq!(backend.is_pending(node), native.pending_tick_at(position));
    }
    assert!(!backend.is_pending(backend.positions[&away]));
    assert!(backend.is_pending(backend.positions[&toward]));
}

#[test]
fn imported_ticks_keep_duplicates_priority_fifo_and_reject_stale_types() {
    let first = BlockPos::new(5, 20, 5);
    let second = BlockPos::new(8, 20, 5);
    let button = Block::StoneButton {
        button: StoneButton::new(ButtonFace::Floor, BlockDirection::North, true),
    };
    let entries = vec![
        TickEntry {
            pos: second,
            block_type: Some(button.registry_id()),
            ticks_left: 2,
            tick_priority: TickPriority::High,
        },
        TickEntry {
            pos: first,
            block_type: Some(button.registry_id()),
            ticks_left: 2,
            tick_priority: TickPriority::High,
        },
        TickEntry {
            pos: second,
            block_type: Some(button.registry_id()),
            ticks_left: 2,
            tick_priority: TickPriority::High,
        },
        TickEntry {
            pos: second,
            block_type: Some(Block::Air.registry_id()),
            ticks_left: 2,
            tick_priority: TickPriority::High,
        },
        TickEntry {
            pos: first,
            block_type: Some(button.registry_id()),
            ticks_left: 2,
            tick_priority: TickPriority::Normal,
        },
    ];
    let scheduler = entries
        .clone()
        .into_iter()
        .collect::<TickScheduler<ScheduledBlockTick>>();
    let mut native = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], scheduler);
    native.set_block(first, button);
    native.set_block(second, button);
    let mut backend = graph(&native, entries);
    assert_eq!(
        backend.scheduled_ticks().len(),
        5,
        "duplicates and stale typed work are imported"
    );

    backend.tick().unwrap();
    native.tick_interpreted();
    assert_eq!(backend.scheduled_ticks().len(), 5);
    backend.tick().unwrap();
    native.tick_interpreted();
    assert!(backend.scheduled_ticks().is_empty());
    assert!(matches!(native.get_block(first), Block::StoneButton { button } if !button.powered));
    assert!(matches!(native.get_block(second), Block::StoneButton { button } if !button.powered));

    let dispatches: Vec<_> = backend
        .trace()
        .iter()
        .filter_map(|line| {
            backend.positions.iter().find_map(|(position, node)| {
                (line == &format!("dispatch {node:?}")).then_some(*position)
            })
        })
        .collect();
    assert_eq!(dispatches, [second, first, second, first]);
    assert!(
        backend
            .trace()
            .iter()
            .any(|line| line.starts_with("stale "))
    );
    assert_eq!(
        backend
            .trace()
            .iter()
            .filter(|line| line.as_str() == "clock.advance")
            .count(),
        2
    );
}

#[test]
fn imported_tick_validation_rejects_out_of_ring_and_invalid_zero_delay() {
    let mut world = empty_world();
    world.set_block(LEVER_POS, lever(false));
    let mut backend = graph(&world, vec![]);
    let tick = |ticks_left, tick_priority| TickEntry {
        pos: LEVER_POS,
        block_type: Some(lever(false).registry_id()),
        ticks_left,
        tick_priority,
    };

    assert!(matches!(
        backend.import_ticks(
            &world,
            vec![tick(SCHEDULER_BUCKETS as u32, TickPriority::Normal)]
        ),
        Err(GraphBackendError::TickOutsideScheduler { delay })
            if delay == SCHEDULER_BUCKETS as u32
    ));
    assert!(matches!(
        backend.import_ticks(&world, vec![tick(0, TickPriority::Highest)]),
        Err(GraphBackendError::InvalidZeroDelayPriority {
            priority: TickPriority::Highest
        })
    ));
}

#[test]
fn stale_tick_at_uncompiled_air_position_remains_until_dispatch() {
    let position = BlockPos::new(9, 20, 9);
    let stale_block = Block::StoneButton {
        button: StoneButton::new(ButtonFace::Floor, BlockDirection::North, true),
    };
    let entries = vec![TickEntry {
        pos: position,
        block_type: Some(stale_block.registry_id()),
        ticks_left: 2,
        tick_priority: TickPriority::High,
    }];
    let scheduler = entries
        .clone()
        .into_iter()
        .collect::<TickScheduler<ScheduledBlockTick>>();
    let mut native = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], scheduler);
    let mut backend = graph(&native, entries);

    assert_eq!(
        backend.scheduled_ticks(),
        native.scheduler().iter_entries().collect::<Vec<_>>()
    );
    backend.tick().unwrap();
    native.tick_interpreted();
    assert_eq!(
        backend.scheduled_ticks(),
        native.scheduler().iter_entries().collect::<Vec<_>>()
    );
    assert_eq!(backend.scheduled_ticks()[0].ticks_left, 1);

    backend.tick().unwrap();
    native.tick_interpreted();
    assert!(backend.scheduled_ticks().is_empty());
    assert!(native.scheduler().iter_entries().next().is_none());
    assert!(
        backend
            .trace()
            .iter()
            .any(|line| line == &format!("stale {position:?} skipped"))
    );
}

#[test]
fn due_at_entry_comparator_repeats_same_value_notification() {
    let comparator_pos = NEXT_POS;
    let lamp_pos = comparator_pos.offset(BlockFace::West);
    let source_pos = comparator_pos.offset(BlockFace::East);
    let comparator = Block::RedstoneComparator {
        comparator: RedstoneComparator::new(BlockDirection::East, ComparatorMode::Compare, true),
    };
    let ticks = ticks_at(comparator_pos, comparator, 1, TickPriority::NanoTick);

    let mut graph_world = empty_world();
    graph_world.set_block(LEVER_POS.offset(BlockFace::Bottom), Block::Stone {});
    graph_world.set_block(source_pos, lever(true));
    graph_world.set_block(comparator_pos, comparator);
    graph_world.set_block_entity(
        comparator_pos,
        BlockEntity::Comparator {
            output_strength: 15,
        },
    );
    graph_world.set_block(lamp_pos, Block::RedstoneLamp { lit: false });
    let mut backend = graph(&graph_world, ticks.clone());

    let scheduler = ticks
        .into_iter()
        .collect::<TickScheduler<ScheduledBlockTick>>();
    let mut native = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], scheduler);
    native.set_block(LEVER_POS.offset(BlockFace::Bottom), Block::Stone {});
    native.set_block(source_pos, lever(true));
    native.set_block(comparator_pos, comparator);
    native.set_block_entity(
        comparator_pos,
        BlockEntity::Comparator {
            output_strength: 15,
        },
    );
    native.set_block(lamp_pos, Block::RedstoneLamp { lit: false });

    backend.tick().unwrap();
    native.tick_interpreted();
    backend.flush(&mut graph_world);
    assert!(
        state_at(&backend, lamp_pos).powered,
        "unchanged comparator output still delivers its callback"
    );
    assert!(matches!(
        native.get_block(lamp_pos),
        Block::RedstoneLamp { lit: true }
    ));
    let comparator_node = backend.positions[&comparator_pos];
    let route = backend.program.nodes()[comparator_node.index()]
        .output_route
        .unwrap();
    assert!(
        backend
            .trace()
            .iter()
            .any(|item| item.starts_with(&format!("route {}:", route.index())))
    );
    assert_eq!(
        backend
            .trace()
            .iter()
            .filter(|line| line.as_str() == "clock.advance")
            .count(),
        1
    );
    assert!(
        backend
            .trace()
            .iter()
            .position(|line| line.starts_with("dispatch "))
            .unwrap()
            < backend
                .trace()
                .iter()
                .position(|line| line == "clock.advance")
                .unwrap()
    );
    assert_eq!(graph_world.get_block(lamp_pos), native.get_block(lamp_pos));
    assert_eq!(state_at(&backend, comparator_pos).output_strength, 15);
}

#[test]
fn repeater_keeps_short_pulse_and_delayed_off_transition() {
    let lever_pos = BlockPos::new(8, 20, 5);
    let repeater_pos = BlockPos::new(7, 20, 5);
    let lamp_pos = BlockPos::new(6, 20, 5);
    let repeater = Block::RedstoneRepeater {
        repeater: RedstoneRepeater {
            facing: BlockDirection::East,
            ..Default::default()
        },
    };
    let mut native = empty_world();
    let mut graph_world = empty_world();
    for world in [&mut native, &mut graph_world] {
        world.set_block(lever_pos.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(lever_pos, lever(false));
        world.set_block(repeater_pos, repeater);
        world.set_block(lamp_pos, Block::RedstoneLamp { lit: false });
    }
    let mut backend = graph(&graph_world, vec![]);
    backend.use_block(lever_pos).unwrap();
    set_native_lever(&mut native, lever_pos, true);
    backend.tick().unwrap();
    native.tick_interpreted();
    assert!(!state_at(&backend, repeater_pos).powered);

    backend.use_block(lever_pos).unwrap();
    set_native_lever(&mut native, lever_pos, false);
    for expected in [true, true, false] {
        backend.tick().unwrap();
        native.tick_interpreted();
        backend.flush(&mut graph_world);
        assert_eq!(state_at(&backend, repeater_pos).powered, expected);
        assert_eq!(
            graph_world.get_block(repeater_pos),
            native.get_block(repeater_pos)
        );
        assert_eq!(graph_world.get_block(lamp_pos), native.get_block(lamp_pos));
    }
}

#[test]
fn captured_repeater_lock_change_uses_captured_powered_state() {
    let repeater_pos = NEXT_POS;
    let side_pos = repeater_pos.offset(BlockFace::South);
    let repeater = Block::RedstoneRepeater {
        repeater: RedstoneRepeater {
            facing: BlockDirection::East,
            powered: true,
            ..Default::default()
        },
    };
    let side = Block::RedstoneComparator {
        comparator: RedstoneComparator::new(BlockDirection::South, ComparatorMode::Compare, true),
    };
    let mut native = empty_world();
    let mut graph_world = empty_world();
    for world in [&mut native, &mut graph_world] {
        world.set_block(repeater_pos, repeater);
        world.set_block(side_pos, side);
        world.set_block_entity(
            side_pos,
            BlockEntity::Comparator {
                output_strength: 15,
            },
        );
    }
    let mut backend = graph(&graph_world, vec![]);
    let node = backend.positions[&repeater_pos];
    assert!(backend.repeater_locked(node));
    assert!(state_at(&backend, repeater_pos).powered);

    let captured = ObservedSite {
        profile: None,
        receiver: Some(CallbackReceiver::Component(node)),
        properties: ObservedProperties::Electrical {
            powered: false,
            locked: false,
        },
    };
    backend.update_repeater(node, Some(&captured)).unwrap();

    let captured_repeater = RedstoneRepeater {
        powered: false,
        locked: false,
        ..match repeater {
            Block::RedstoneRepeater { repeater } => repeater,
            _ => unreachable!(),
        }
    };
    crate::redstone::repeater::on_neighbor_updated(captured_repeater, &mut native, repeater_pos);
    let graph_state = state_at(&backend, repeater_pos);
    assert!(!graph_state.powered);
    assert_eq!(graph_state.output_strength, 0);
    assert!(graph_state.repeater_locked);
    assert_eq!(
        backend.node_block(node),
        native.get_block(repeater_pos),
        "captured powered and locked properties match native handling"
    );
}

#[test]
fn copper_bulb_uses_power_latch_separately_from_lit_state() {
    let bulb_pos = NEXT_POS;
    let mut native = empty_world();
    let mut graph_world = empty_world();
    let bulb = Block::from_name("copper_bulb").unwrap();
    for world in [&mut native, &mut graph_world] {
        world.set_block(LEVER_POS.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(LEVER_POS, lever(false));
        world.set_block(bulb_pos, bulb);
    }
    let mut backend = graph(&graph_world, vec![]);
    for (powered, lit) in [(true, true), (false, true), (true, false)] {
        backend.use_block(LEVER_POS).unwrap();
        set_native_lever(&mut native, LEVER_POS, powered);
        backend.flush(&mut graph_world);
        assert_eq!(graph_world.get_block(bulb_pos), native.get_block(bulb_pos));
        assert_eq!(
            graph_world.get_block(bulb_pos).copper_bulb_state(),
            Some((lit, powered))
        );
        assert_eq!(state_at(&backend, bulb_pos).powered, powered);
        assert_eq!(
            state_at(&backend, bulb_pos).output_strength,
            if lit { 15 } else { 0 }
        );
    }
}

#[test]
fn bulb_far_comparator_callbacks_match_native_order_and_count() {
    let bulb_pos = BlockPos::new(7, 20, 5);
    let lever_pos = bulb_pos.offset(BlockFace::East);
    let support_pos = lever_pos.offset(BlockFace::Bottom);
    let spacer_pos = bulb_pos.offset(BlockFace::West);
    let comparator_pos = spacer_pos.offset(BlockFace::West);
    let comparator = Block::RedstoneComparator {
        comparator: RedstoneComparator::new(BlockDirection::East, ComparatorMode::Compare, false),
    };
    let bulb = Block::from_name("copper_bulb").unwrap();
    let mut native = empty_world();
    let mut graph_world = empty_world();
    for world in [&mut native, &mut graph_world] {
        world.set_block(support_pos, Block::Stone {});
        world.set_block(lever_pos, lever(false));
        world.set_block(bulb_pos, bulb);
        world.set_block(spacer_pos, Block::Stone {});
        world.set_block(comparator_pos, comparator);
        world.set_block_entity(
            comparator_pos,
            BlockEntity::Comparator { output_strength: 0 },
        );
    }
    let mut backend = graph(&graph_world, vec![]);
    let comparator_node = backend.positions[&comparator_pos];

    backend.use_block(lever_pos).unwrap();
    let native_trace =
        crate::redstone::instant_piston_tests::capture_at(&[bulb_pos, comparator_pos], || {
            set_native_lever(&mut native, lever_pos, true)
        });
    let native_callbacks: Vec<_> = native_trace
        .iter()
        .filter(|entry| entry["kind"] == "callback")
        .map(|entry| {
            (
                serde_json::from_value(entry["data"]["pos"].clone()).unwrap(),
                serde_json::from_value(entry["data"]["dir"].clone()).unwrap(),
            )
        })
        .collect();
    let graph_callbacks: Vec<_> = backend
        .callback_order
        .iter()
        .filter(|(position, _)| [bulb_pos, comparator_pos].contains(position))
        .map(|(position, kind)| {
            let DeliveryKind::Update { direction } = kind else {
                unreachable!()
            };
            (*position, *direction)
        })
        .collect();
    assert_eq!(graph_callbacks, native_callbacks);
    assert_eq!(
        native_callbacks
            .iter()
            .filter(|(pos, _)| *pos == comparator_pos)
            .count(),
        1
    );
    assert_eq!(
        backend
            .comparator_updates
            .iter()
            .filter(|&&position| position == comparator_pos)
            .count(),
        1
    );
    assert_eq!(
        backend.scheduled_ticks(),
        native.scheduler().iter_entries().collect::<Vec<_>>()
    );
    assert_eq!(
        backend.comparator_updates,
        [comparator_pos],
        "far comparator is notified by the compiled bulb route exactly once"
    );
    assert!(matches!(
        native.get_block(bulb_pos),
        block if block.copper_bulb_state() == Some((true, true))
    ));
    assert!(state_at(&backend, bulb_pos).powered);
    assert_eq!(state_at(&backend, comparator_pos).output_strength, 0);
    assert_eq!(backend.positions[&comparator_pos], comparator_node);
}

#[test]
fn torch_uses_compiled_conductor_query_and_scheduled_edges() {
    let torch_pos = NEXT_POS;
    let support_pos = torch_pos.offset(BlockFace::Bottom);
    let lever_pos = support_pos.offset(BlockFace::West);
    let wall_lever = Block::Lever {
        lever: Lever::new(LeverFace::Wall, BlockDirection::West, false),
    };
    let mut native = empty_world();
    let mut graph_world = empty_world();
    for world in [&mut native, &mut graph_world] {
        world.set_block(support_pos, Block::Stone {});
        world.set_block(torch_pos, Block::RedstoneTorch { lit: true });
        world.set_block(lever_pos.offset(BlockFace::East), Block::Stone {});
        world.set_block(lever_pos, wall_lever);
    }
    let mut backend = graph(&graph_world, vec![]);

    backend.use_block(lever_pos).unwrap();
    set_native_lever_with_support(&mut native, lever_pos, true, BlockFace::East);
    for expected in [true, false] {
        backend.tick().unwrap();
        native.tick_interpreted();
        backend.flush(&mut graph_world);
        assert_eq!(state_at(&backend, torch_pos).powered, expected);
        assert_eq!(
            graph_world.get_block(torch_pos),
            native.get_block(torch_pos)
        );
    }

    backend.use_block(lever_pos).unwrap();
    set_native_lever_with_support(&mut native, lever_pos, false, BlockFace::East);
    for expected in [false, true] {
        backend.tick().unwrap();
        native.tick_interpreted();
        backend.flush(&mut graph_world);
        assert_eq!(state_at(&backend, torch_pos).powered, expected);
        assert_eq!(
            graph_world.get_block(torch_pos),
            native.get_block(torch_pos)
        );
    }
}

fn frozen_clipboard(name: &str) -> WorldEditClipboard {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("test_data/instant-pistons-io");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(directory.join("download-manifest.json")).unwrap())
            .unwrap();
    let entry = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == name)
        .expect("fixture belongs to frozen manifest");
    let bytes = std::fs::read(directory.join(name)).unwrap();
    assert_eq!(entry["bytes"].as_u64().unwrap() as usize, bytes.len());
    assert_eq!(entry["sha256"], format!("{:x}", Sha256::digest(&bytes)));
    load_schematic(std::io::Cursor::new(bytes)).unwrap()
}

fn frozen_component(name: &str, matches: impl Fn(Block) -> bool) -> Block {
    let clipboard = frozen_clipboard(name);
    let mut index = 0;
    for y in 0..clipboard.size_y {
        for z in 0..clipboard.size_z {
            for x in 0..clipboard.size_x {
                let block = Block::from_id(clipboard.data.get_entry(index));
                index += 1;
                if matches(block) {
                    println!("frozen witness: {name} @ ({x},{y},{z}): {block:?}");
                    return block;
                }
            }
        }
    }
    panic!("{name} has no requested fixed component");
}

fn fixture_torch(lit: bool) -> Block {
    match frozen_component("INSTANT_TORCH.schem", |block| {
        matches!(
            block,
            Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. }
        )
    }) {
        Block::RedstoneTorch { .. } => Block::RedstoneTorch { lit },
        Block::RedstoneWallTorch { facing, .. } => Block::RedstoneWallTorch { facing, lit },
        _ => unreachable!(),
    }
}

#[test]
fn boundary_torch_power_is_not_silently_compiled_as_zero() {
    let lamp = NEXT_POS;
    let source = lamp.offset(BlockFace::North);
    let mut world = empty_world();
    world.set_block(lamp, Block::RedstoneLamp { lit: false });
    world.set_block(source, fixture_torch(true));
    // A wall torch excludes one horizontal face; use a face that actually emits.
    let source = if redstone::redstone_lamp_should_be_lit(&world, lamp) {
        source
    } else {
        let torch = world.get_block(source);
        world.set_block(source, Block::Air);
        let source = lamp.offset(BlockFace::South);
        world.set_block(source, torch);
        source
    };
    assert!(redstone::redstone_lamp_should_be_lit(&world, lamp));
    let full = graph(&world, vec![]);
    assert_eq!(
        full.input_strength(full.positions[&lamp], InputPort::Main),
        15
    );
    match GraphBackend::compile(&world, (lamp, lamp), vec![], Arc::default()) {
        Err(error) => assert!(
            error.to_string().contains(&format!("{source:?}")),
            "external-source rejection must identify the source: {error}"
        ),
        Ok(mut backend) => {
            let node = backend.positions[&lamp];
            assert_eq!(
                backend.input_strength(node, InputPort::Main),
                15,
                "an admitted external torch must not become zero power"
            );
            backend
                .update_component(node, DeliveryKind::Update { direction: None }, None)
                .unwrap();
            redstone::update(world.get_block(lamp), &mut world, lamp, None);
            assert_eq!(backend.node_block(node), world.get_block(lamp));
        }
    }
}

#[test]
fn imported_stale_and_inert_ticks_are_retained_or_explicitly_rejected() {
    let repeater = frozen_component("ADDER_1BIT.schem", |block| {
        matches!(block, Block::RedstoneRepeater { .. })
    });
    let removed = NEXT_POS;
    let inert = NEXT_POS.offset(BlockFace::East);
    let entries = vec![
        TickEntry {
            pos: removed,
            block_type: Some(repeater.registry_id()),
            ticks_left: 4,
            tick_priority: TickPriority::High,
        },
        TickEntry {
            pos: inert,
            block_type: Some(Block::Stone {}.registry_id()),
            ticks_left: 4,
            tick_priority: TickPriority::Normal,
        },
    ];
    let scheduler = entries
        .clone()
        .into_iter()
        .collect::<TickScheduler<ScheduledBlockTick>>();
    let mut native = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], scheduler);
    native.set_block(LEVER_POS, lever(false));
    native.set_block(LEVER_POS.offset(BlockFace::Bottom), Block::Stone {});
    native.set_block(inert, Block::Stone {});
    assert_eq!(native.get_block(removed), Block::Air);
    let before = native.scheduler().iter_entries().collect::<Vec<_>>();
    assert_eq!(before, entries);
    match GraphBackend::compile(&native, BOUNDS, entries.clone(), Arc::default()) {
        Err(error) => {
            let message = error.to_string();
            assert!(
                message.contains("tick")
                    && entries
                        .iter()
                        .any(|entry| message.contains(&format!("{:?}", entry.pos))),
                "rejection must identify the unbound scheduled target: {message}"
            );
            assert_eq!(
                native.scheduler().iter_entries().collect::<Vec<_>>(),
                before
            );
        }
        Ok(mut backend) => {
            assert_eq!(
                backend.scheduled_ticks(),
                before,
                "stale/inert work is still pending until its dispatch boundary"
            );
            for _ in 0..4 {
                backend.tick().unwrap();
                native.tick_interpreted();
                assert_eq!(
                    backend.scheduled_ticks(),
                    native.scheduler().iter_entries().collect::<Vec<_>>()
                );
            }
        }
    }
}

#[test]
fn removed_repeater_tick_keeps_its_deadline_until_native_stale_dispatch() {
    let repeater = frozen_component("ADDER_1BIT.schem", |block| {
        matches!(block, Block::RedstoneRepeater { .. })
    });
    let removed = NEXT_POS;
    let entry = TickEntry {
        pos: removed,
        block_type: Some(repeater.registry_id()),
        ticks_left: 4,
        tick_priority: TickPriority::High,
    };
    let scheduler = vec![entry]
        .into_iter()
        .collect::<TickScheduler<ScheduledBlockTick>>();
    let mut native = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], scheduler);
    native.set_block(LEVER_POS, lever(false));
    native.set_block(LEVER_POS.offset(BlockFace::Bottom), Block::Stone {});
    let before = native.scheduler().iter_entries().collect::<Vec<_>>();
    match GraphBackend::compile(&native, BOUNDS, before.clone(), Arc::default()) {
        Err(error) => {
            let message = error.to_string();
            assert!(
                message.contains("tick") && message.contains(&format!("{removed:?}")),
                "{message}"
            );
            assert_eq!(
                native.scheduler().iter_entries().collect::<Vec<_>>(),
                before
            );
        }
        Ok(mut backend) => {
            assert_eq!(backend.scheduled_ticks(), before);
            let mut dispatches = Vec::new();
            for _ in 0..4 {
                let records = capture_at(&[removed], || native.tick_interpreted());
                dispatches.push(
                    records
                        .iter()
                        .filter(|record| record["kind"] == "scheduled_tick")
                        .count(),
                );
                backend.tick().unwrap();
                assert_eq!(
                    backend.scheduled_ticks(),
                    native.scheduler().iter_entries().collect::<Vec<_>>()
                );
                assert_eq!(
                    backend.clock.logical_tick,
                    native.piston_state().logical_tick
                );
            }
            assert_eq!(dispatches, [0, 0, 0, 1]);
            assert_eq!(native.get_block(removed), Block::Air);
        }
    }
}

#[test]
fn observer_shape_then_explicit_update_matches_native_for_every_orientation() {
    let template = frozen_component("INSTANT_OBSERVER.schem", |block| {
        matches!(block, Block::Observer { .. })
    });
    let Block::Observer { observer: template } = template else {
        unreachable!()
    };
    let position = BlockPos::new(7, 20, 7);
    let mut mismatches = Vec::new();
    let original_facing = BlockFace::from(template.facing);
    let facings = std::iter::once(original_facing).chain(
        BlockFace::values()
            .into_iter()
            .filter(|face| *face != original_facing),
    );
    for facing in facings {
        for watches_changed_cell in [false, true] {
            let mut native = empty_world();
            let observer = Block::Observer {
                observer: RedstoneObserver {
                    facing: facing.into(),
                    powered: false,
                },
            };
            native.set_block(position, observer);
            let mut backend = graph(&native, vec![]);
            let node = backend.positions[&position];
            let shape_direction = if watches_changed_cell {
                facing.opposite()
            } else {
                facing
            };
            crate::interaction::change(observer, &mut native, position, shape_direction);
            backend
                .update_component(
                    node,
                    DeliveryKind::Shape {
                        direction: shape_direction,
                    },
                    None,
                )
                .unwrap();
            if backend.is_pending(node) != native.pending_tick_at(position) {
                mismatches.push((facing, watches_changed_cell, "after_shape", 0));
            }
            let update_direction = shape_direction.opposite();
            redstone::update(
                native.get_block(position),
                &mut native,
                position,
                Some(update_direction),
            );
            backend
                .update_component(
                    node,
                    DeliveryKind::Update {
                        direction: Some(update_direction),
                    },
                    None,
                )
                .unwrap();
            if backend.is_pending(node) != native.pending_tick_at(position) {
                mismatches.push((facing, watches_changed_cell, "after_update", 0));
            }
            for tick in 1..=4 {
                native.tick_interpreted();
                backend.tick().unwrap();
                if backend.node_block(node) != native.get_block(position) {
                    mismatches.push((facing, watches_changed_cell, "pulse", tick));
                }
            }
        }
    }
    assert!(
        mismatches.is_empty(),
        "only the explicit watched-face Update starts the pulse: {mismatches:?}"
    );
}

#[test]
fn torch_shape_does_not_sample_power_before_the_update_phase() {
    let position = BlockPos::new(7, 20, 7);
    let torch = fixture_torch(true);
    let mut native = empty_world();
    native.set_block(position, torch);
    let (support, _) = crate::interaction::attachment_support(torch, position).unwrap();
    native.set_block(support, Block::Stone {});
    let control_face = [
        BlockFace::North,
        BlockFace::South,
        BlockFace::East,
        BlockFace::West,
    ]
    .into_iter()
    .find(|face| support.offset(*face) != position)
    .unwrap();
    native.set_block(
        support.offset(control_face),
        Block::Lever {
            lever: Lever::new(LeverFace::Wall, control_face.unwrap_direction(), true),
        },
    );
    let mut backend = graph(&native, vec![]);
    let node = backend.positions[&position];
    assert_eq!(backend.input_strength(node, InputPort::Main), 15);
    assert!(crate::interaction::is_valid_position(
        torch, &native, position
    ));
    crate::interaction::change(torch, &mut native, position, BlockFace::East);
    backend
        .update_component(
            node,
            DeliveryKind::Shape {
                direction: BlockFace::East,
            },
            None,
        )
        .unwrap();
    assert_eq!(
        backend.is_pending(node),
        native.pending_tick_at(position),
        "a valid Shape callback must not enqueue an electrical recheck"
    );
    redstone::update(torch, &mut native, position, Some(BlockFace::East));
    backend
        .update_component(
            node,
            DeliveryKind::Update {
                direction: Some(BlockFace::East),
            },
            None,
        )
        .unwrap();
    assert_eq!(backend.is_pending(node), native.pending_tick_at(position));
}

#[test]
fn captured_repeater_lock_rewrite_keeps_native_output_strength_and_consumers() {
    let template = frozen_component("ADDER_1BIT.schem", |block| {
        matches!(block, Block::RedstoneRepeater { .. })
    });
    let Block::RedstoneRepeater { repeater: template } = template else {
        unreachable!()
    };
    let position = BlockPos::new(7, 20, 7);
    let mut mismatches = Vec::new();
    for captured_powered in [false, true] {
        let mut native = empty_world();
        native.set_block(
            position,
            Block::RedstoneRepeater {
                repeater: RedstoneRepeater {
                    powered: !captured_powered,
                    locked: false,
                    ..template
                },
            },
        );
        let side = template.facing.rotate();
        native.set_block(
            position.offset(side.block_face()),
            Block::RedstoneRepeater {
                repeater: RedstoneRepeater {
                    facing: side,
                    powered: true,
                    locked: false,
                    ..Default::default()
                },
            },
        );
        let lamp = position.offset(template.facing.opposite().block_face());
        native.set_block(lamp, Block::RedstoneLamp { lit: false });
        let mut backend = graph(&native, vec![]);
        let node = backend.positions[&position];
        let observed = ObservedSite {
            profile: None,
            receiver: Some(CallbackReceiver::Component(node)),
            properties: ObservedProperties::Electrical {
                powered: captured_powered,
                locked: false,
            },
        };
        redstone::repeater::on_neighbor_updated(
            RedstoneRepeater {
                powered: captured_powered,
                locked: false,
                ..template
            },
            &mut native,
            position,
        );
        backend
            .update_component(
                node,
                DeliveryKind::Update {
                    direction: Some(BlockFace::Top),
                },
                Some(&observed),
            )
            .unwrap();
        let native_strength =
            redstone::source_strength(native.get_block(position), &native, position);
        redstone::update(native.get_block(lamp), &mut native, lamp, None);
        let lamp_node = backend.positions[&lamp];
        backend
            .update_component(lamp_node, DeliveryKind::Update { direction: None }, None)
            .unwrap();
        let actual = (
            backend.node_block(node),
            backend.node_state(node).output_strength,
            backend.node_block(lamp_node),
        );
        let expected = (
            native.get_block(position),
            native_strength,
            native.get_block(lamp),
        );
        if actual != expected {
            mismatches.push((captured_powered, actual, expected));
        }
    }
    assert!(
        mismatches.is_empty(),
        "captured lock rewrites must preserve the emitted strength and downstream lamp: {mismatches:?}"
    );
}

#[test]
fn bulb_far_comparator_callbacks_match_native_order_and_multiplicity() {
    let bulb = BlockPos::new(7, 20, 7);
    let control = bulb.offset(BlockFace::Top);
    let mut native = empty_world();
    native.set_block(bulb, Block::from_name("copper_bulb").unwrap());
    native.set_block(control, lever(false));
    let faces = [
        BlockFace::North,
        BlockFace::South,
        BlockFace::East,
        BlockFace::West,
    ];
    let mut comparators = Vec::new();
    for face in faces {
        let support = bulb.offset(face);
        native.set_block(support, Block::Stone {});
        let position = support.offset(face);
        native.set_block(
            position,
            Block::RedstoneComparator {
                comparator: RedstoneComparator::new(
                    face.opposite().unwrap_direction(),
                    ComparatorMode::Compare,
                    false,
                ),
            },
        );
        native.set_block_entity(position, BlockEntity::Comparator { output_strength: 0 });
        comparators.push(position);
    }
    let mut backend = graph(&native, vec![]);
    let records = capture_at(&comparators, || {
        set_native_lever(&mut native, control, true)
    });
    let expected: Vec<BlockPos> = records
        .iter()
        .filter(|entry| entry["kind"] == "callback")
        .map(|entry| {
            assert!(entry["data"]["dir"].is_null());
            serde_json::from_value(entry["data"]["pos"].clone()).unwrap()
        })
        .collect();
    assert_eq!(expected, comparators);
    backend.use_block(control).unwrap();
    let actual: Vec<_> = backend
        .comparator_updates
        .iter()
        .copied()
        .filter(|position| comparators.contains(position))
        .collect();
    assert_eq!(
        actual, expected,
        "same final output does not justify additional far callbacks"
    );
    let delivered: Vec<_> = backend
        .callback_order
        .iter()
        .filter_map(|(position, kind)| comparators.contains(position).then_some((*position, *kind)))
        .collect();
    let expected_deliveries: Vec<_> = expected
        .into_iter()
        .map(|position| (position, DeliveryKind::Update { direction: None }))
        .collect();
    assert_eq!(delivered, expected_deliveries);
}

#[test]
fn piston_schematics_require_an_unsupported_component_error() {
    for name in [
        "INSTANT_TORCH.schem",
        "INSTANT_OBSERVER.schem",
        "ADDER_1BIT.schem",
    ] {
        let clipboard = frozen_clipboard(name);
        let mut world = PlotWorld::from_chunks(
            0,
            0,
            (0..PLOT_WIDTH)
                .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
                .collect(),
            Default::default(),
        );
        let origin = BlockPos::new(40, 30, 40);
        paste_clipboard(
            &mut world,
            &clipboard,
            origin + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z),
            false,
        );
        let result = GraphBackend::compile(&world, world.get_corners(), vec![], Arc::default());
        assert!(
            matches!(
                result,
                Err(GraphBackendError::UnsupportedComponent {
                    kind: "piston transition",
                    ..
                })
            ),
            "{name} must not claim graph execution before piston/dust steps"
        );
    }
}
