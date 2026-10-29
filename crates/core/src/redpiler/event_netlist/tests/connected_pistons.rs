
    #[test]
    fn connected_shared_pistons_follow_real_lever_inputs() {
        use crate::plot::PlotWorld;
        use crate::redpiler::backend::graph::{
            piston_probe::pending_work,
            wire_probe::{physical_graph, physical_program},
        };
        use crate::redpiler::program_graph::ComponentState;
        use crate::world::{World, storage::Chunk};
        use mchprs_blocks::{
            BlockDirection, BlockFacing, BlockPos,
            blocks::{Block, Lever, LeverFace, RedstonePiston},
        };
        let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
        let bases = [BlockPos::new(5, 20, 5), BlockPos::new(5, 20, 9)];
        let levers = [BlockPos::new(4, 20, 5), BlockPos::new(4, 20, 9)];
        let lamp = BlockPos::new(6, 20, 6);
        for (index, facing) in [BlockFacing::South, BlockFacing::North].into_iter().enumerate() {
            let piston = RedstonePiston {
                facing,
                sticky: true,
                extended: true,
            };
            world.set_block(bases[index], Block::Piston { piston });
            world.set_block(bases[index].offset(facing.into()), Block::PistonHead { head: piston.into() });
            world.set_block(
                levers[index],
                Block::Lever {
                    lever: Lever::new(LeverFace::Floor, BlockDirection::East, true),
                },
            );
            world.set_block(BlockPos::new(4, 19, bases[index].z), Block::Stone {});
        }
        world.set_block(BlockPos::new(5, 20, 7), Block::RedstoneBlock);
        world.set_block(lamp, Block::RedstoneLamp { lit: false });
        let bounds = (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15));
        let (program, state, _) = physical_program(&world, bounds).expect("physical graph admission");
        let resource = program.resource_for_piston(super::super::program_graph::PistonId::new(0)).unwrap();
        assert_eq!(program.resource_for_piston(super::super::program_graph::PistonId::new(1)), Some(resource));
        let lamp_id = program.position_index()[&lamp].index();
        for order in [[0usize, 1usize], [1, 0]] {
            let mut runtime = CompiledPistonActors::compile(&program, &state, &crate::redpiler::physical_signal_network::EntryWork::capture(&world, bounds)).expect("compiled actor admission");
            let mut graph = physical_graph(&world, bounds).unwrap();
            let mut inputs = [true, true];
            for (step, (input, power)) in [(order[0], false), (order[1], false), (order[0], true), (order[1], true), (order[1], true)]
                .into_iter()
                .enumerate()
            {
                runtime.begin_computation_cycle();
                runtime.set_lever(levers[input], power).unwrap();
                if inputs[input] != power {
                    graph.use_block(levers[input]).unwrap();
                    inputs[input] = power;
                }
                for _ in 0..8 {
                    runtime.advance_tick().unwrap();
                    graph.tick().unwrap();
                }
                let actual = runtime.signals.state();
                let expected = graph.circuit_state();
                let ComponentState::Electrical(actual_lamp) = &actual.components[lamp_id] else {
                    panic!("lamp missing")
                };
                let ComponentState::Electrical(expected_lamp) = &expected.components[lamp_id] else {
                    panic!("graph lamp missing")
                };
                assert_eq!(
                    (actual_lamp.powered, actual_lamp.pending_tick),
                    (expected_lamp.powered, expected_lamp.pending_tick),
                    "lamp order {order:?} step {step}"
                );
                assert_eq!(actual.pistons, expected.pistons, "pistons order {order:?} step {step}");
                assert_eq!(actual.resources, expected.resources, "resources order {order:?} step {step}");
                if step == 1 {
                    assert_eq!(runtime.successful_actions.iter().sum::<u64>(), 2, "both pistons retract");
                    assert_eq!(runtime.successful_moves.iter().sum::<u64>(), 1, "empty retraction moves no payload");
                    assert!(runtime.consumed.iter().all(|used| !used), "empty retraction consumes no resource in its cycle");
                }
                let pending = pending_work(&graph);
                assert!(
                    runtime.pending.is_empty()
                        && runtime.deferred.is_empty()
                        && pending.scheduled.is_empty()
                        && pending.requests.is_empty()
                        && pending.deferred.is_empty(),
                    "pending order {order:?} step {step}"
                );
            }
            assert!(runtime.successful_moves.iter().sum::<u64>() > 0, "compiled executor moved a resource");
        }
    }

