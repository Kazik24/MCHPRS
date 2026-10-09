use super::*;
use rayon::ThreadPoolBuilder;

#[test]
fn single_region_fpu_evaluates_responses_in_parallel_and_preserves_handoff() {
    let pool = ThreadPoolBuilder::new().num_threads(4).build().unwrap();
    pool.install(|| {
        let path = root().join("test_data/piston-research/fpu-full-20261009/manifest.json");
        let (mut actual, bounds, manifest) = load_fixture(&path);
        let (mut expected, _, _) = load_fixture(&path);
        let mut compiler = Compiler::default();
        let mut reference = Compiler::default();
        for (compiler, world, parallel) in [(&mut compiler, &actual, true), (&mut reference, &expected, false)] {
            compiler.compile(world, world.get_corners(), CompilerOptions {
                optimize: true, assume_instant: true, budget_multiplier: 8, ..Default::default()
            }, vec![], Default::default()).unwrap();
            assert_eq!(compiler.stats().unwrap().regions.logical_regions, 1);
            compiler.backend.as_mut().unwrap().instant_parallel = Some(parallel);
        }
        let trigger = local_pos(&manifest["observations"]["trigger"][0]);
        let inputs: Vec<_> = ["light_blue_operand_inputs", "red_operand_inputs", "opcode_levers_by_sign_weight_4_2_1"]
            .into_iter().flat_map(|key| manifest["ports"][key].as_array().unwrap()).map(local_pos).collect();
        for step in 0..4 {
            for (compiler, world) in [(&mut compiler, &mut actual), (&mut reference, &mut expected)] {
                compiler.on_use_block(trigger);
                for (index, &pos) in inputs.iter().enumerate() {
                    if (index + step) % 4 == 0 { compiler.on_use_block(pos); }
                }
                for _ in 0..8 { compiler.tick_with_world(world); }
                compiler.flush(world);
            }
            assert_eq!(super::research::compilation_fingerprint(&actual, bounds),
                       super::research::compilation_fingerprint(&expected, bounds));
        }
        assert!(compiler.backend.as_ref().unwrap().parallel_response_batches() > 0);
        assert_eq!(reference.backend.as_ref().unwrap().parallel_response_batches(), 0);
        compiler.reset(&mut actual, bounds);
        reference.reset(&mut expected, bounds);
        for _ in 0..8 {
            actual.tick_interpreted();
            expected.tick_interpreted();
            assert_eq!(super::research::compilation_fingerprint(&actual, bounds),
                       super::research::compilation_fingerprint(&expected, bounds));
        }
    });
}

fn banks(
    optimize: bool,
    io_only: bool,
    parallel: bool,
) -> (PlotWorld, Compiler, Vec<BlockPos>, [BlockPos; 4]) {
    let (mut world, first_cells, data, writer) = super::memory::wire_bank();
    let (second, second_cells, second_data, second_writer) = super::memory::wire_bank();
    let shift = BlockPos::new(64, 0, 48);
    let bounds = second.get_corners();
    crate::world::for_each_block_optimized(&second, bounds.0, bounds.1, |pos| {
        let block = second.get_block(pos);
        if block != Block::Air {
            world.set_block(pos + shift, block);
        }
    });
    let cells = [
        first_cells[0],
        first_cells[1],
        second_cells[0] + shift,
        second_cells[1] + shift,
    ];
    for cell in cells {
        world.set_block(
            cell.offset(BlockFace::East),
            Block::Observer {
                observer: RedstoneObserver {
                    facing: BlockFacing::West,
                    powered: false,
                },
            },
        );
    }
    let mut positions = Vec::new();
    let bounds = world.get_corners();
    crate::world::for_each_block_optimized(&world, bounds.0, bounds.1, |pos| {
        if world.get_block(pos) != Block::Air {
            positions.push(pos);
        }
    });
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &world,
            bounds,
            CompilerOptions {
                optimize,
                io_only,
                ..Default::default()
            },
            vec![],
            Default::default(),
        )
        .unwrap();
    assert_eq!(compiler.stats().unwrap().regions.logical_regions, 2);
    compiler.backend.as_mut().unwrap().instant_parallel = Some(parallel);
    (
        world,
        compiler,
        positions,
        [data, writer, second_data + shift, second_writer + shift],
    )
}

fn state(world: &PlotWorld, positions: &[BlockPos]) -> Value {
    json!({
        "blocks": positions.iter().map(|&pos| (world.get_block_raw(pos), world.get_block_entity(pos))).collect::<Vec<_>>(),
        "pistons": world.piston_state(),
        "ticks": world.scheduler().iter_entries().collect::<Vec<_>>()
    })
}

#[test]
fn parallel_sampling_and_observers_match_serial_execution_and_handoff() {
    for workers in [1, 2, 4] {
        let pool = ThreadPoolBuilder::new()
            .num_threads(workers)
            .build()
            .unwrap();
        pool.install(|| {
            for optimize in [false, true] {
                for io_only in [false, true] {
                    let (mut actual, mut compiler, positions, controls) =
                        banks(optimize, io_only, true);
                    let (mut expected, mut reference, _, _) = banks(optimize, io_only, false);
                    compiler.flush(&mut actual);
                    reference.flush(&mut expected);
                    assert_eq!(state(&actual, &positions), state(&expected, &positions));
                    for tick in 0..48 {
                        if tick % 3 == 0 {
                            let order = if tick % 2 == 0 {
                                [3, 1, 0, 2]
                            } else {
                                [0, 2, 1, 3]
                            };
                            for index in order {
                                compiler.on_use_block(controls[index]);
                                reference.on_use_block(controls[index]);
                                assert_eq!(
                                    compiler.backend.as_ref().unwrap().logical_stats(),
                                    reference.backend.as_ref().unwrap().logical_stats()
                                );
                            }
                        }
                        let observed =
                            crate::redstone::instant_piston_tests::capture_at(&positions, || {
                                compiler.tick_with_world(&mut actual)
                            });
                        let serial =
                            crate::redstone::instant_piston_tests::capture_at(&positions, || {
                                reference.tick_with_world(&mut expected)
                            });
                        assert_eq!(observed, serial, "workers={workers}, tick={tick}");
                        if tick % 5 == 0 {
                            compiler.flush(&mut actual);
                            reference.flush(&mut expected);
                        }
                        assert_eq!(state(&actual, &positions), state(&expected, &positions));
                        assert_eq!(
                            compiler.backend.as_ref().unwrap().logical_stats(),
                            reference.backend.as_ref().unwrap().logical_stats()
                        );
                    }
                    assert_eq!(
                        compiler.backend.as_ref().unwrap().instant_parallel_batches > 0,
                        workers > 1
                    );
                    let bounds = actual.get_corners();
                    compiler.reset(&mut actual, bounds);
                    reference.reset(&mut expected, bounds);
                    assert_eq!(state(&actual, &positions), state(&expected, &positions));
                    for _ in 0..8 {
                        actual.tick_interpreted();
                        expected.tick_interpreted();
                        assert_eq!(state(&actual, &positions), state(&expected, &positions));
                    }
                    compiler
                        .compile(
                            &actual,
                            bounds,
                            Default::default(),
                            actual.scheduler().iter_entries().collect(),
                            Default::default(),
                        )
                        .unwrap();
                    reference
                        .compile(
                            &expected,
                            bounds,
                            Default::default(),
                            expected.scheduler().iter_entries().collect(),
                            Default::default(),
                        )
                        .unwrap();
                    compiler.backend.as_mut().unwrap().instant_parallel = Some(true);
                    reference.backend.as_mut().unwrap().instant_parallel = Some(false);
                    compiler.tick_with_world(&mut actual);
                    reference.tick_with_world(&mut expected);
                    compiler.flush(&mut actual);
                    reference.flush(&mut expected);
                    assert_eq!(state(&actual, &positions), state(&expected, &positions));
                }
            }
        });
    }
}
