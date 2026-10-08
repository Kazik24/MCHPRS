use super::memory::{generator_cell, wire_bank};
use super::*;

fn assert_pose(world: &PlotWorld, base: BlockPos, retracted: bool, payload: Block) {
    let Block::Piston { piston } = world.get_block(base) else {
        panic!("memory base must remain a piston");
    };
    assert_eq!(piston.extended, !retracted);
    let near = base.offset(piston.facing.into());
    let far = near.offset(piston.facing.into());
    assert_eq!(
        world.get_block(near),
        if retracted {
            payload
        } else {
            Block::PistonHead {
                head: RedstonePistonHead {
                    facing: piston.facing,
                    sticky: piston.sticky,
                    short: false,
                },
            }
        }
    );
    assert_eq!(
        world.get_block(far),
        if retracted { Block::Air } else { payload }
    );
}

#[test]
fn independent_bud_display_follows_samples_and_respects_suppression() {
    for assume_instant in [false, true] {
        for optimize in [false, true] {
            for io_only in [false, true] {
                for screen_only in [false, true] {
                    let (mut world, cell, data, _, sample) = generator_cell(false);
                    world.set_screen_only(screen_only);
                    let bounds = world.get_corners();
                    let mut compiler = Compiler::default();
                    compiler
                        .compile(
                            &world,
                            bounds,
                            CompilerOptions {
                                assume_instant,
                                optimize,
                                io_only,
                                ..Default::default()
                            },
                            vec![],
                            Default::default(),
                        )
                        .unwrap();
                    compiler.tick_with_world(&mut world);
                    compiler.flush(&mut world);
                    assert_pose(&world, cell, false, Block::RedstoneBlock);
                    compiler.on_use_block(data);
                    compiler.flush(&mut world);
                    assert_pose(&world, cell, false, Block::RedstoneBlock);
                    compiler.on_use_block(data);
                    compiler.on_use_block(sample);
                    compiler.flush(&mut world);
                    assert_pose(&world, cell, !io_only, Block::RedstoneBlock);
                    let (chunk_x, chunk_z) = (cell.x >> 4, cell.z >> 4);
                    let saved = world.get_chunk_mut(chunk_x, chunk_z).unwrap().save();
                    let reloaded = Chunk::load(chunk_x, chunk_z, saved);
                    for offset in 0..=2 {
                        let pos = cell + BlockPos::new(0, -offset, 0);
                        assert_eq!(
                            Block::from_id(reloaded.get_block(
                                (pos.x & 15) as u32,
                                pos.y as u32,
                                (pos.z & 15) as u32,
                            )),
                            world.get_block(pos),
                            "chunk reload must retain the published pose"
                        );
                    }
                    compiler.on_use_block(data);
                    compiler.flush(&mut world);
                    assert_pose(&world, cell, !io_only, Block::RedstoneBlock);

                    let state = compiler.backend.as_ref().unwrap().logical_stats();
                    world.flush_block_changes();
                    let records = world.visual_update_counts().2;
                    for _ in 0..3 {
                        compiler.flush(&mut world);
                        world.flush_block_changes();
                        assert_eq!(compiler.backend.as_ref().unwrap().logical_stats(), state);
                        assert_eq!(world.visual_update_counts().2, records);
                    }
                    compiler.on_use_block(sample);
                    compiler.flush(&mut world);
                    assert_pose(&world, cell, false, Block::RedstoneBlock);
                    compiler.reset(&mut world, bounds);
                    assert_pose(&world, cell, false, Block::RedstoneBlock);
                    assert!(world.piston_state().events.is_empty());
                    assert!(world.piston_state().motions.is_empty());
                }
            }
        }
    }
}

#[test]
fn independent_bud_display_preserves_inert_payload_material() {
    let payload = Block::Wool {
        color: mchprs_blocks::BlockColorVariant::Gray,
    };
    for assume_instant in [false, true] {
        for optimize in [false, true] {
            let (mut world, cell, data, _, sample) = generator_cell(false);
            world.set_block(cell + BlockPos::new(0, -2, 0), payload);
            let bounds = world.get_corners();
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &world,
                    bounds,
                    CompilerOptions {
                        assume_instant,
                        optimize,
                        ..Default::default()
                    },
                    vec![],
                    Default::default(),
                )
                .unwrap();
            assert_pose(&world, cell, false, payload);
            compiler.on_use_block(sample);
            compiler.flush(&mut world);
            assert_pose(&world, cell, true, payload);
            compiler.on_use_block(data);
            compiler.flush(&mut world);
            assert_pose(&world, cell, true, payload);
            compiler.on_use_block(sample);
            compiler.flush(&mut world);
            assert_pose(&world, cell, false, payload);
            compiler.reset(&mut world, bounds);
            assert_pose(&world, cell, false, payload);
            assert!(world.piston_state().events.is_empty());
            assert!(world.piston_state().motions.is_empty());
        }
    }
}

#[test]
fn copper_bud_oxidation_uses_committed_occupancy_with_all_display_policies() {
    let payload = Block::from_name("copper_block").unwrap();
    for assume_instant in [false, true] {
        for optimize in [false, true] {
            for io_only in [false, true] {
                for frequent in [false, true] {
                    let (mut world, cell, _, _, sample) = generator_cell(false);
                    world.set_block(cell + BlockPos::new(0, -2, 0), payload);
                    let bulb = cell + BlockPos::new(0, -6, 0);
                    world.set_block(bulb, Block::from_name("exposed_copper_bulb").unwrap());
                    world.set_random_tick_speed(0);
                    let bounds = world.get_corners();
                    let mut compiler = Compiler::default();
                    compiler
                        .compile(
                            &world,
                            bounds,
                            CompilerOptions {
                                assume_instant,
                                optimize,
                                io_only,
                                ..Default::default()
                            },
                            vec![],
                            Default::default(),
                        )
                        .unwrap();
                    compiler.flush(&mut world);
                    let initial = compiler.backend.as_ref().unwrap().logical_stats();
                    compiler.oxidize_bulb(&mut world, bulb, 0.0, 0.0);
                    assert_eq!(world.get_block(bulb).get_name(), "exposed_copper_bulb");
                    assert_eq!(compiler.backend.as_ref().unwrap().logical_stats(), initial);

                    compiler.on_use_block(sample);
                    if frequent {
                        compiler.flush(&mut world);
                    }
                    assert_pose(&world, cell, frequent && !io_only, payload);
                    if !frequent || io_only {
                        assert!(
                            crate::redstone::copper_bulb::oxidation_state(&world, bulb, 0.0, 0.0)
                                .is_none(),
                            "the unpublished far copper still blocks a raw world read"
                        );
                    }
                    let committed = compiler.backend.as_ref().unwrap().logical_stats();
                    compiler.oxidize_bulb(&mut world, bulb, 0.0, 0.0);
                    assert_eq!(world.get_block(bulb).get_name(), "weathered_copper_bulb");
                    assert_eq!(
                        compiler.backend.as_ref().unwrap().logical_stats(),
                        committed
                    );
                    compiler.reset(&mut world, bounds);
                    assert!(world.scheduler().iter_entries().next().is_none());
                    assert!(world.piston_state().events.is_empty());
                    assert!(world.piston_state().motions.is_empty());
                }
            }
        }
    }
}

#[test]
fn shared_clock_display_tracks_each_committed_bank_with_all_flags() {
    for assume_instant in [false, true] {
        for optimize in [false, true] {
            for io_only in [false, true] {
                let (mut world, _, manifest) = fixture("counter_basic");
                let bounds = world.get_corners();
                let options = || CompilerOptions {
                    assume_instant,
                    optimize,
                    io_only,
                    ..Default::default()
                };
                let mut compiler = Compiler::default();
                compiler
                    .compile(&world, bounds, options(), vec![], Default::default())
                    .unwrap();
                let cells: Vec<_> = manifest["ports"]["observations"]["memory"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(local_pos)
                    .collect();
                let trigger = local_pos(&manifest["ports"]["inputs"]["trigger"]);
                compiler.on_use_block(trigger);
                for _ in 0..42 {
                    compiler.tick_with_world(&mut world);
                    compiler.flush(&mut world);
                    for (_, _, memory) in compiler.backend.as_ref().unwrap().logical_stats() {
                        for (cell, bit) in memory {
                            assert!(cells.contains(&cell));
                            assert_pose(&world, cell, !io_only && bit, Block::RedstoneBlock);
                        }
                    }
                }
                compiler.on_use_block(trigger);
                for _ in 0..18 {
                    compiler.tick_with_world(&mut world);
                }
                compiler.flush(&mut world);
                let stored = compiler.backend.as_ref().unwrap().logical_stats();
                compiler.reset(&mut world, bounds);
                for (_, _, memory) in &stored {
                    for &(cell, bit) in memory {
                        assert_pose(&world, cell, bit, Block::RedstoneBlock);
                    }
                }
                compiler
                    .compile(&world, bounds, options(), vec![], Default::default())
                    .unwrap();
                compiler.flush(&mut world);
                for (_, samples, memory) in compiler.backend.as_ref().unwrap().logical_stats() {
                    assert_eq!(samples, 0, "recompilation must not sample a stopped bank");
                    for (cell, bit) in memory {
                        assert_pose(&world, cell, bit, Block::RedstoneBlock);
                    }
                }
            }
        }
    }
}

#[test]
fn deferred_display_preserves_memory_observers_and_scheduled_work() {
    for optimize in [false, true] {
        let mut reference = None;
        for frequent in [false, true] {
            let (mut world, cells, data, sample) = wire_bank();
            let observer = cells[0] + BlockPos::new(0, -3, 0);
            let lamp = observer.offset(BlockFace::Bottom);
            world.set_block(
                observer,
                Block::Observer {
                    observer: RedstoneObserver {
                        facing: BlockFacing::Up,
                        powered: false,
                    },
                },
            );
            world.set_block(lamp, Block::RedstoneLamp { lit: false });
            let bounds = world.get_corners();
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &world,
                    bounds,
                    CompilerOptions {
                        optimize,
                        ..Default::default()
                    },
                    vec![],
                    Default::default(),
                )
                .unwrap();
            let mut commits = Vec::new();
            for action in [sample, data, sample, data, sample] {
                compiler.on_use_block(action);
                for _ in 0..3 {
                    compiler.tick_with_world(&mut world);
                    if frequent {
                        compiler.flush(&mut world);
                    }
                    commits.push(compiler.backend.as_ref().unwrap().logical_stats());
                }
            }
            compiler.flush(&mut world);
            compiler.reset(&mut world, bounds);
            let result = (
                commits,
                snapshot(
                    &world,
                    (
                        BASE + BlockPos::new(-3, -10, -2),
                        BASE + BlockPos::new(12, 5, 5),
                    ),
                ),
                world.scheduler().iter_entries().collect::<Vec<_>>(),
            );
            if let Some(reference) = &reference {
                assert_eq!(&result, reference);
            } else {
                reference = Some(result);
            }
        }
    }
}
