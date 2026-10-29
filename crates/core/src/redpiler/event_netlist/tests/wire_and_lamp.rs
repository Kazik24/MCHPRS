    #[test]
    fn lever_wire_piston_lamp_repeated_cycles_match_physical_graph() {
        compare_wire_piston_lamp(false);
    }

    #[test]
    fn dot_wire_piston_lamp_repeated_cycles_match_physical_graph() {
        compare_wire_piston_lamp(true);
    }

    #[test]
    fn blocked_destination_is_declined_or_left_unchanged() {
        use crate::redpiler::backend::graph::{
            piston_probe::tests::{BOUNDS, FAR, LEVER, fixture},
            wire_probe::physical_program,
        };
        use crate::world::World;
        use mchprs_blocks::blocks::Block;
        let mut world = fixture();
        let blocker = Block::from_name("obsidian").unwrap();
        world.set_block(FAR, blocker);
        let (program, state, _) = match physical_program(&world, BOUNDS) {
            Ok(graph) => graph,
            Err(reason) => {
                assert!(!reason.is_empty());
                eprintln!("physical graph declined blocked lane: {reason}");
                return;
            }
        };
        let mut runtime = match CompiledPistonActors::compile(&program, &state, &crate::redpiler::physical_signal_network::EntryWork::capture(&world, BOUNDS)) {
            Ok(runtime) => runtime,
            Err(reason) => {
                assert!(!reason.is_empty());
                eprintln!("compiled actors declined blocked lane: {reason}");
                return;
            }
        };
        let original_resources = runtime.signals.state().resources.clone();
        runtime.begin_computation_cycle();
        runtime.set_lever(LEVER, true).unwrap();
        for _ in 0..6 {
            runtime.advance_tick().unwrap();
        }
        assert_eq!(runtime.successful_moves.iter().sum::<u64>(), 0);
        assert_eq!(runtime.signals.state().resources, original_resources);
        assert!(runtime.consumed.iter().all(|used| !used));
        assert_eq!(world.get_block(FAR), blocker);
    }

    fn compare_wire_piston_lamp(dot: bool) {
        use crate::redpiler::backend::graph::{
            piston_probe::tests::{ACTOR, BOUNDS, FAR, NEAR, fixture},
            wire_probe::{physical_graph, physical_program},
        };
        use crate::redpiler::program_graph::ComponentState;
        use crate::world::World;
        use mchprs_blocks::{
            BlockDirection, BlockPos,
            blocks::{Block, Lever, LeverFace},
        };
        let mut world = fixture();
        let source = BlockPos::new(3, 20, 5);
        let wire = BlockPos::new(4, 20, 5);
        let lamp = BlockPos::new(8, 20, 5);
        world.set_block(
            source,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::East, false),
            },
        );
        world.set_block(BlockPos::new(3, 19, 5), Block::Stone {});
        world.set_block(BlockPos::new(4, 19, 5), Block::Stone {});
        world.set_block(
            wire,
            Block::RedstoneWire {
                wire: mchprs_blocks::blocks::RedstoneWire {
                    east: mchprs_blocks::blocks::RedstoneWireSide::Side,
                    west: mchprs_blocks::blocks::RedstoneWireSide::Side,
                    ..Default::default()
                },
            },
        );
        if dot {
            world.set_block(wire, Block::RedstoneWire { wire: Default::default() });
        }
        world.set_block(lamp, Block::RedstoneLamp { lit: false });
        let (program, state, _) = physical_program(&world, BOUNDS).unwrap();
        let mut graph = physical_graph(&world, BOUNDS).unwrap();
        let mut runtime = CompiledPistonActors::compile(&program, &state, &crate::redpiler::physical_signal_network::EntryWork::capture(&world, BOUNDS)).unwrap();
        let piston = program.pistons().iter().position(|p| p.position == ACTOR).unwrap();
        let resource = runtime.actors[piston].as_ref().unwrap().resource.index();
        let lamp_id = program.position_index()[&lamp].index();
        let wire_id = program.position_index()[&wire].index();
        for (cycle, powered) in [true, false, true].into_iter().enumerate() {
            runtime.begin_computation_cycle();
            runtime.set_lever(source, powered).unwrap();
            graph.use_block(source).unwrap();
            assert_eq!(
                runtime.signals.state().components[wire_id],
                graph.circuit_state().components[wire_id],
                "wire immediately after lever cycle {cycle}"
            );
            for _ in 0..8 {
                runtime.advance_tick().unwrap();
                graph.tick().unwrap();
            }
            let actual = runtime.signals.state();
            let expected = graph.circuit_state();
            let ComponentState::Electrical(actual_lamp) = &actual.components[lamp_id] else {
                panic!("lamp missing")
            };
            let ComponentState::Electrical(graph_lamp) = &expected.components[lamp_id] else {
                panic!("graph lamp missing")
            };
            assert_eq!(
                (actual_lamp.powered, actual_lamp.pending_tick),
                (graph_lamp.powered, graph_lamp.pending_tick),
                "lamp cycle {cycle}"
            );
            assert_eq!(actual_lamp.powered, powered, "lamp follows the moved block at cycle {cycle}");
            assert_eq!(actual.pistons[piston], expected.pistons[piston], "piston cycle {cycle}");
            assert_eq!(
                (actual.resources[resource].at, actual.resources[resource].enabled),
                (expected.resources[resource].at, expected.resources[resource].enabled),
                "resource cycle {cycle}"
            );
            assert!(
                runtime.pending.is_empty() && runtime.deferred.is_empty(),
                "runtime pending work at cycle {cycle}"
            );
            assert!(
                !actual
                    .components
                    .iter()
                    .any(|state| matches!(state, ComponentState::Electrical(node) if node.pending_tick)),
                "runtime scheduled work at cycle {cycle}"
            );
            let graph_pending = crate::redpiler::backend::graph::piston_probe::pending_work(&graph);
            assert!(
                graph_pending.scheduled.is_empty() && graph_pending.requests.is_empty() && graph_pending.deferred.is_empty(),
                "graph pending work at cycle {cycle}: {graph_pending:?}"
            );
            assert_eq!(runtime.successful_moves[piston], (cycle + 1) as u64, "actual movements at cycle {cycle}");
            assert_eq!(
                program.contacts()[actual.resources[resource].at.index()].position,
                if powered { FAR } else { NEAR }
            );
        }
    }

    #[test]
    #[ignore = "bounded release fixture timing"]
    fn connected_piston_fixture_release_comparison() {
        use crate::redpiler::backend::graph::{piston_probe::tests::{fixture, BOUNDS, LEVER}, wire_probe::{physical_graph, physical_program}};
        use crate::redpiler::program_graph::ComponentState;
        use crate::world::World;
        use mchprs_blocks::{BlockPos, blocks::Block};
        let mut world = fixture();
        let lamp = BlockPos::new(8, 20, 5);
        world.set_block(lamp, Block::RedstoneLamp { lit: false });
        let (program, state, _) = physical_program(&world, BOUNDS).unwrap();
        let mut runtime = CompiledPistonActors::compile(&program, &state, &crate::redpiler::physical_signal_network::EntryWork::capture(&world, BOUNDS)).unwrap();
        let mut graph = physical_graph(&world, BOUNDS).unwrap();
        let lamp_id = program.position_index()[&lamp].index();
        let mut runtime_outputs = Vec::new();
        let start = std::time::Instant::now();
        for frame in 0..256 {
            runtime.begin_computation_cycle();
            runtime.set_lever(LEVER, frame % 2 == 0).unwrap();
            for _ in 0..8 { runtime.advance_tick().unwrap(); }
            let ComponentState::Electrical(lamp) = &runtime.signals.state().components[lamp_id] else { panic!("lamp missing") };
            runtime_outputs.push((lamp.powered, runtime.signals.state().resources[0].at, runtime.signals.state().resources[0].enabled));
        }
        let runtime_time = start.elapsed();
        let mut graph_outputs = Vec::new();
        let start = std::time::Instant::now();
        for _ in 0..256 {
            graph.use_block(LEVER).unwrap();
            for _ in 0..8 { graph.tick().unwrap(); }
            let ComponentState::Electrical(lamp) = &graph.circuit_state().components[lamp_id] else { panic!("lamp missing") };
            graph_outputs.push((lamp.powered, graph.circuit_state().resources[0].at, graph.circuit_state().resources[0].enabled));
        }
        let graph_time = start.elapsed();
        assert_eq!(runtime_outputs, graph_outputs);
        assert_eq!(runtime.successful_moves.iter().sum::<u64>(), 256);
        eprintln!("fixture ticks=2048 frames=256 runtime={runtime_time:?} graph={graph_time:?} resource_moves={} actions={} graph_counts={:?}", runtime.successful_moves.iter().sum::<u64>(), runtime.successful_actions.iter().sum::<u64>(), graph.execution_counts());
    }

