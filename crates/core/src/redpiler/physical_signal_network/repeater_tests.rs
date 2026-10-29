use super::*;
use crate::plot::PlotWorld;
use crate::redpiler::backend::graph::{
    piston_probe::pending_work,
    wire_probe::{physical_graph, physical_program},
};
use crate::redpiler::program_graph::ComponentState;
use crate::world::{World, storage::Chunk};
use mchprs_blocks::{
    BlockDirection, BlockPos,
    blocks::{Block, Lever, LeverFace, RedstoneRepeater},
};

#[test]
fn repeater_delays_and_pulses_match_graph_at_each_tick() {
    let source = BlockPos::new(5, 8, 8);
    let repeater = BlockPos::new(6, 8, 8);
    let lamp = BlockPos::new(7, 8, 8);
    let bounds = (BlockPos::new(2, 5, 5), BlockPos::new(10, 11, 11));
    for delay in 1..=4 {
        let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
        world.set_block(BlockPos::new(5, 7, 8), Block::Stone {});
        world.set_block(BlockPos::new(6, 7, 8), Block::Stone {});
        world.set_block(
            source,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::East, false),
            },
        );
        world.set_block(
            repeater,
            Block::RedstoneRepeater {
                repeater: RedstoneRepeater {
                    delay,
                    facing: BlockDirection::West,
                    locked: false,
                    powered: false,
                },
            },
        );
        world.set_block(lamp, Block::RedstoneLamp { lit: false });
        let (program, state, _) = physical_program(&world, bounds).unwrap();
        let mut graph = physical_graph(&world, bounds).unwrap();
        let mut network = PhysicalSignalNetwork::compile(&program, &state, &EntryWork::capture(&world, bounds)).unwrap();
        let rep = program.position_index()[&repeater].index();
        let mut imported = state.clone();
        let ComponentState::Electrical(repeater_state) = &mut imported.components[rep] else {
            panic!("repeater missing")
        };
        repeater_state.pending_tick = true;
        assert!(matches!(
            PhysicalSignalNetwork::compile(&program, &imported, &EntryWork::capture(&world, bounds)),
            Err(reason) if reason.contains("inconsistent component pending flag")
        ));
        let light = program.position_index()[&lamp].index();
        let mut input = false;
        for tick in 0..36 {
            for (_, value) in [(0, true), (1, false), (3, true), (12, false), (13, true), (24, false)]
                .into_iter()
                .filter(|(at, _)| *at == tick)
            {
                network.set_lever(source, value).unwrap();
                if input != value {
                    graph.use_block(source).unwrap();
                    input = value;
                }
            }
            network.advance_tick().unwrap();
            graph.tick().unwrap();
            for node in [rep, light] {
                let ComponentState::Electrical(actual) = &network.state().components[node] else {
                    panic!("component missing")
                };
                let ComponentState::Electrical(expected) = &graph.circuit_state().components[node] else {
                    panic!("graph component missing")
                };
                assert_eq!(actual.powered, expected.powered, "delay {delay}, tick {tick}, node {node}");
            }
            let graph_due = pending_work(&graph)
                .scheduled
                .into_iter()
                .find(|(pos, _, _)| *pos == repeater)
                .map(|(_, due, _)| due);
            let network_due = network.scheduler.iter().find(|(node, _, _)| **node == rep).map(|(_, due, _)| due);
            assert_eq!(network_due, graph_due, "delay {delay}, tick {tick}, pending deadline");
        }
    }
}

#[test]
fn scheduled_locking_repeater_runs_before_its_target_in_both_input_orders() {
    let main_source = BlockPos::new(5, 8, 8);
    let lock_source = BlockPos::new(6, 8, 6);
    let main = BlockPos::new(6, 8, 8);
    let lock = BlockPos::new(6, 8, 7);
    let lamp = BlockPos::new(7, 8, 8);
    let bounds = (BlockPos::new(2, 5, 4), BlockPos::new(10, 11, 11));
    let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
    for pos in [main_source, lock_source, main, lock] {
        world.set_block(BlockPos::new(pos.x, pos.y - 1, pos.z), Block::Stone {});
    }
    for pos in [main_source, lock_source] {
        world.set_block(
            pos,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::East, false),
            },
        );
    }
    for (pos, facing) in [(main, BlockDirection::West), (lock, BlockDirection::North)] {
        world.set_block(
            pos,
            Block::RedstoneRepeater {
                repeater: RedstoneRepeater {
                    delay: 1,
                    facing,
                    locked: false,
                    powered: false,
                },
            },
        );
    }
    world.set_block(lamp, Block::RedstoneLamp { lit: false });
    let (program, state, _) = physical_program(&world, bounds).unwrap();
    for sources in [[main_source, lock_source], [lock_source, main_source]] {
        let mut graph = physical_graph(&world, bounds).unwrap();
        let mut network = PhysicalSignalNetwork::compile(&program, &state, &EntryWork::capture(&world, bounds)).unwrap();
        for source in sources {
            network.set_lever(source, true).unwrap();
            graph.use_block(source).unwrap();
        }
        assert_eq!(network.scheduler.iter().count(), 2, "both ticks are scheduled");
        for tick in 0..4 {
            network.advance_tick().unwrap();
            graph.tick().unwrap();
            for pos in [main, lock, lamp] {
                let node = program.position_index()[&pos].index();
                let ComponentState::Electrical(actual) = &network.state().components[node] else {
                    panic!("component missing")
                };
                let ComponentState::Electrical(expected) = &graph.circuit_state().components[node] else {
                    panic!("graph component missing")
                };
                assert_eq!(
                    (actual.powered, actual.repeater_locked),
                    (expected.powered, expected.repeater_locked),
                    "source order {sources:?}, tick {tick}, position {pos}"
                );
            }
            let expected = pending_work(&graph).scheduled;
            let actual: Vec<_> = network
                .scheduler
                .iter()
                .map(|(node, due, priority)| (program.nodes()[*node].position.unwrap(), due, priority))
                .collect();
            assert_eq!(actual, expected, "scheduled work at tick {tick}");
        }
        let main_state = &network.state().components[program.position_index()[&main].index()];
        assert!(
            matches!(main_state, ComponentState::Electrical(state) if state.repeater_locked && !state.powered),
            "the lock takes effect before the main repeater tick"
        );
    }
}

#[test]
fn repeater_and_torch_due_together_match_graph_receiver_order() {
    let repeater_source = BlockPos::new(6, 8, 8);
    let repeater = BlockPos::new(7, 8, 8);
    let lamp = BlockPos::new(8, 8, 8);
    let torch = BlockPos::new(8, 8, 7);
    let torch_support = BlockPos::new(8, 7, 7);
    let torch_source = BlockPos::new(7, 7, 7);
    let bounds = (BlockPos::new(3, 4, 4), BlockPos::new(11, 11, 11));
    let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
    world.set_block(BlockPos::new(6, 7, 8), Block::Stone {});
    world.set_block(BlockPos::new(7, 7, 8), Block::Stone {});
    world.set_block(torch_support, Block::Stone {});
    world.set_block(
        repeater_source,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::East, false),
        },
    );
    world.set_block(
        torch_source,
        Block::Lever {
            lever: Lever::new(LeverFace::Wall, BlockDirection::West, false),
        },
    );
    world.set_block(
        repeater,
        Block::RedstoneRepeater {
            repeater: RedstoneRepeater {
                delay: 1,
                facing: BlockDirection::West,
                locked: false,
                powered: false,
            },
        },
    );
    world.set_block(torch, Block::RedstoneTorch { lit: true });
    world.set_block(lamp, Block::RedstoneLamp { lit: true });
    let (program, state, _) = physical_program(&world, bounds).unwrap();
    for sources in [[repeater_source, torch_source], [torch_source, repeater_source]] {
        let mut graph = physical_graph(&world, bounds).unwrap();
        let mut network = PhysicalSignalNetwork::compile(&program, &state, &EntryWork::capture(&world, bounds)).unwrap();
        for source in sources {
            network.set_lever(source, true).unwrap();
            graph.use_block(source).unwrap();
        }
        let scheduled = pending_work(&graph).scheduled;
        assert!(
            scheduled
                .iter()
                .any(|(pos, _, priority)| *pos == repeater && *priority == mchprs_world::TickPriority::High),
            "repeater is scheduled at high priority: {scheduled:?}"
        );
        assert!(
            scheduled
                .iter()
                .any(|(pos, _, priority)| *pos == torch && *priority == mchprs_world::TickPriority::Normal),
            "torch is scheduled at normal priority: {scheduled:?}"
        );
        assert_eq!(
            scheduled.iter().find(|(pos, _, _)| *pos == repeater).unwrap().1,
            scheduled.iter().find(|(pos, _, _)| *pos == torch).unwrap().1,
            "both components are due on the same tick"
        );
        for tick in 0..6 {
            network.advance_tick().unwrap();
            graph.tick().unwrap();
            for pos in [repeater, torch, lamp] {
                let node = program.position_index()[&pos].index();
                let ComponentState::Electrical(actual) = &network.state().components[node] else {
                    panic!("component missing")
                };
                let ComponentState::Electrical(expected) = &graph.circuit_state().components[node] else {
                    panic!("graph component missing")
                };
                assert_eq!(actual.powered, expected.powered, "order {sources:?}, tick {tick}, position {pos}");
            }
            let actual: Vec<_> = network
                .scheduler
                .iter()
                .map(|(node, due, priority)| (program.nodes()[*node].position.unwrap(), due, priority))
                .collect();
            assert_eq!(actual, pending_work(&graph).scheduled, "pending work at tick {tick}");
        }
        let lamp_state = &network.state().components[program.position_index()[&lamp].index()];
        assert!(matches!(lamp_state, ComponentState::Electrical(state) if state.powered && !state.pending_tick));
        let repeater_state = &network.state().components[program.position_index()[&repeater].index()];
        let torch_state = &network.state().components[program.position_index()[&torch].index()];
        assert!(matches!(repeater_state, ComponentState::Electrical(state) if state.powered));
        assert!(matches!(torch_state, ComponentState::Electrical(state) if !state.powered));
    }
}
