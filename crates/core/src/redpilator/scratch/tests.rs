use super::*;
use crate::plot::{PLOT_BLOCK_WIDTH, PlotWorld};
use crate::redstone;
use crate::world::RedstoneUpdateObservation;
use mchprs_blocks::{
    BlockDirection, BlockFace, BlockFacing,
    blocks::{Lever, LeverFace, RedstonePiston, RedstoneWire, RedstoneWireSide},
};

const BOUNDS: (BlockPos, BlockPos) = (BlockPos::new(38, 26, 38), BlockPos::new(44, 34, 42));
const LEVER: BlockPos = BlockPos::new(40, 30, 40);
const WIRE: BlockPos = BlockPos::new(41, 30, 40);
const PISTON: BlockPos = BlockPos::new(42, 30, 40);

fn fixture() -> PlotWorld {
    let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
    let mut world = PlotWorld::from_chunks(0, 0, chunks, TickScheduler::<ScheduledBlockTick>::default());
    world.set_block(BlockPos::new(40, 29, 40), Block::Stone {});
    world.set_block(
        LEVER,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    world.set_block(
        WIRE,
        Block::RedstoneWire {
            wire: RedstoneWire::new(
                RedstoneWireSide::None,
                RedstoneWireSide::None,
                RedstoneWireSide::Side,
                RedstoneWireSide::Side,
                0,
            ),
        },
    );
    world.set_block(
        PISTON,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::East,
                sticky: false,
                extended: false,
            },
        },
    );
    world
}

fn entry(world: &PlotWorld) -> ScratchEntry {
    ScratchEntry {
        scheduled: world.scheduler().iter_entries().collect(),
        piston: world.piston_state().clone(),
    }
}

fn drive(world: &mut PlotWorld) {
    world.set_block(
        LEVER,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, true),
        },
    );
    redstone::update_surrounding_blocks(world, LEVER);
    redstone::update_surrounding_blocks(world, WIRE);
    redstone::update_surrounding_blocks(world, BlockPos::new(40, 29, 40));
}

fn sample(world: &PlotWorld) -> (Vec<Block>, serde_json::Value, Vec<TickEntry>) {
    let blocks = [BlockPos::new(40, 29, 40), LEVER, WIRE, PISTON, PISTON.offset(BlockFace::East)]
        .into_iter()
        .map(|pos| world.get_block(pos))
        .collect();
    (
        blocks,
        serde_json::to_value(world.piston_state()).unwrap(),
        world.scheduler().iter_entries().collect(),
    )
}

#[test]
fn scratch_copy_runs_interpreter_passively_and_is_independent() {
    let host = fixture();
    let host_before = sample(&host);
    let mut source_run = fixture();
    drive(&mut source_run);
    let mut traced = ScratchWorld::from_world(&host, BOUNDS, entry(&host)).unwrap();
    let mut untraced = ScratchWorld::from_world(&host, BOUNDS, entry(&host)).unwrap();
    traced.world_mut().observe_redstone_updates(true);
    drive(traced.world_mut());
    drive(untraced.world_mut());
    traced.finish_probe().unwrap();
    untraced.finish_probe().unwrap();
    assert_eq!(sample(traced.world()), sample(&source_run));
    assert_eq!(sample(traced.world()), sample(untraced.world()));
    let events: Vec<RedstoneUpdateObservation> = traced.world_mut().take_redstone_update_observations();
    assert!(
        events
            .iter()
            .any(|event| event.pos == PISTON && matches!(event.block, Block::Piston { piston } if !piston.extended) && event.direction.is_some()),
        "piston delivery absent: {events:?}"
    );
    assert_eq!(sample(&host), host_before);

    let other = ScratchWorld::from_world(&host, BOUNDS, entry(&host)).unwrap();
    traced.world_mut().set_block(LEVER, Block::Air);
    assert_ne!(traced.world().get_block(LEVER), other.world().get_block(LEVER));
    assert_eq!(sample(&host), host_before);
}

#[test]
fn probe_workers_preserve_quiet_basis_results_order_and_cancellation() {
    for threads in [1, 4] {
        rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap().install(|| {
            let host = fixture();
            let host_before = sample(&host);
            let mut scratch = ScratchWorld::from_world(&host, BOUNDS, entry(&host)).unwrap();
            scratch.world_mut().set_block(LEVER, Block::Air);
            scratch.checkpoint().unwrap();
            let quiet = sample(scratch.world());
            drive(scratch.world_mut());
            scratch.world_mut().tick_interpreted();
            let driven = sample(scratch.world());
            scratch.revert();
            let jobs: Vec<_> = (0..512).collect();
            let results = scratch
                .probe_jobs(&jobs, None, |probe, &index| {
                    assert!(rayon::current_thread_index().is_some());
                    assert_eq!(sample(probe.world()), quiet);
                    if index % 2 == 0 {
                        drive(probe.world_mut());
                        probe.world_mut().tick_interpreted();
                    }
                    probe.finish_probe()?;
                    Ok((index, sample(probe.world())))
                })
                .unwrap();
            for (expected_index, (index, state)) in results.into_iter().enumerate() {
                assert_eq!(index, expected_index);
                assert_eq!(state, if index % 2 == 0 { driven.clone() } else { quiet.clone() });
            }
            let error = scratch
                .probe_jobs(&jobs, None, |_, &index| {
                    if index == 7 {
                        Err(ScratchError {
                            position: Some(LEVER),
                            reason: "probe failed",
                        })
                    } else {
                        Ok(index)
                    }
                })
                .unwrap_err();
            assert_eq!(error.reason, "probe failed");
            let monitor = super::super::TaskMonitor::default();
            monitor.cancel();
            let error = scratch
                .probe_jobs(&jobs, Some(&monitor), |_, _| -> Result<(), ScratchError> {
                    panic!("cancelled probe must not run")
                })
                .unwrap_err();
            assert_eq!(error.reason, "compilation cancelled");
            assert_eq!(sample(&host), host_before);
        });
    }
}

#[test]
fn checkpoint_reverts_blocks_entities_and_work_to_the_new_basis() {
    use mchprs_blocks::block_entities::BlockEntity;
    use mchprs_world::TickPriority;
    let host = fixture();
    let host_before = sample(&host);
    let mut scratch = ScratchWorld::from_world(&host, BOUNDS, entry(&host)).unwrap();
    let comparator = BlockPos::new(43, 30, 40);
    scratch.world_mut().set_block(LEVER, Block::Air);
    scratch.world_mut().set_block(comparator, Block::from_name("comparator").unwrap());
    scratch
        .world_mut()
        .set_block_entity(comparator, BlockEntity::Comparator { output_strength: 7 });
    scratch.world_mut().schedule_tick(comparator, 3, TickPriority::High);
    scratch.world_mut().piston_state_mut().logical_tick = 91;
    scratch.checkpoint().unwrap();
    let expected = sample(scratch.world());
    let expected_entity = serde_json::to_value(scratch.world().get_block_entity(comparator)).unwrap();
    for _ in 0..2 {
        scratch.world_mut().set_block(LEVER, Block::Stone {});
        scratch.world_mut().set_block(comparator, Block::Air);
        scratch.world_mut().delete_block_entity(comparator);
        drive(scratch.world_mut());
        scratch.world_mut().tick_interpreted();
        let _ = scratch.world().get_block(BlockPos::new(-2, 30, 40));
        scratch.revert();
        assert_eq!(sample(scratch.world()), expected);
        assert_eq!(scratch.world().get_block(LEVER), Block::Air);
        assert_eq!(
            serde_json::to_value(scratch.world().get_block_entity(comparator)).unwrap(),
            expected_entity
        );
        assert_eq!(scratch.world().get_block(comparator), Block::from_name("comparator").unwrap());
        scratch.finish_probe().unwrap();
        assert!(scratch.world_mut().take_redstone_update_observations().is_empty());
    }
    assert_eq!(sample(&host), host_before);
}

#[test]
fn scratch_reports_unknown_boundary_and_pending_entry_work() {
    let host = fixture();
    let scratch = ScratchWorld::from_world(&host, BOUNDS, entry(&host)).unwrap();
    let outside = BlockPos::new(-2, BOUNDS.1.y, BOUNDS.1.z);
    let _ = scratch.world().get_block(outside);
    assert_eq!(
        scratch.finish_probe().unwrap_err(),
        ScratchError {
            position: Some(outside),
            reason: "scratch probe read outside captured boundary"
        }
    );

    let mut pending = entry(&host);
    pending.piston.events.push_back(mchprs_world::PistonEvent {
        pos: PISTON,
        sticky: false,
        facing: BlockFace::East,
        action: mchprs_world::PistonAction::Extend,
    });
    let error = match ScratchWorld::from_world(&host, BOUNDS, pending) {
        Ok(_) => panic!("pending piston work was accepted"),
        Err(error) => error,
    };
    assert_eq!(
        error,
        ScratchError {
            position: Some(PISTON),
            reason: "scratch entry has queued piston work"
        }
    );
}

#[test]
fn scratch_reads_authoritative_air_in_probe_context() {
    let mut host = fixture();
    let bounds = (BlockPos::new(40, 30, 38), BlockPos::new(44, 32, 42));
    host.set_block(BlockPos::new(42, 32, 40), Block::Stone {});
    let outside_air = BlockPos::new(42, 33, 40);
    assert_eq!(host.get_block(outside_air), Block::Air);

    let scratch = ScratchWorld::from_world(&host, bounds, entry(&host)).unwrap();
    assert_eq!(scratch.world().get_block(outside_air), Block::Air);
    discover_piston_sources(scratch.world(), PISTON, BlockFacing::East).unwrap();
    scratch.finish_probe().unwrap();
}

#[test]
fn scratch_plot_edge_context_uses_the_world_air_contract() {
    let host = fixture();
    let bounds = (BlockPos::new(0, 30, 40), BlockPos::new(2, 32, 42));
    let scratch = ScratchWorld::from_world(&host, bounds, entry(&host)).unwrap();
    let outside_plot = BlockPos::new(-1, 31, 41);
    assert_eq!(host.get_block(outside_plot), Block::Air);
    assert_eq!(scratch.world().get_block(outside_plot), Block::Air);
    assert!(scratch.world().get_block_entity(outside_plot).is_none());
    scratch.finish_probe().unwrap();

    let plot_max_x = PLOT_BLOCK_WIDTH - 1;
    let edge_bounds = (BlockPos::new(plot_max_x - 2, 30, 40), BlockPos::new(plot_max_x, 32, 42));
    let edge_scratch = ScratchWorld::from_world(&host, edge_bounds, entry(&host)).unwrap();
    let outside_plot = BlockPos::new(plot_max_x + 1, 31, 41);
    assert_eq!(host.get_block(outside_plot), Block::Air);
    assert_eq!(edge_scratch.world().get_block(outside_plot), Block::Air);
    assert!(edge_scratch.world().get_block_entity(outside_plot).is_none());
    edge_scratch.finish_probe().unwrap();
}

#[test]
fn watched_wire_discovery_separates_dependencies_from_notifications() {
    let mut world = fixture();
    let discovery = discover_watched_wire_sources(&world, BOUNDS).unwrap();
    assert!(discovery.watchers.contains(&WireWatcher {
        wire: WIRE,
        piston: PISTON,
        head: false
    }));
    assert!(discovery.sources.contains(&WireSource {
        wire: WIRE,
        source: LEVER,
        attenuation: 0
    }));
    let saved = match world.get_block(WIRE) {
        Block::RedstoneWire { wire } => wire.power,
        _ => unreachable!(),
    };
    assert_eq!(discovered_wire_strength(&world, &discovery, WIRE), saved);

    // A nearby watcher is only a candidate. Without an interpreter delivery,
    // discovering an electrical dependency does not manufacture a callback.
    world.observe_redstone_updates(true);
    assert!(world.take_redstone_update_observations().is_empty());
    assert!(wire_strength_changed(14, 15));
    assert!(!wire_strength_changed(14, 14));
}

#[test]
fn changed_watched_wire_is_the_notification_condition() {
    let discovery = WireDiscovery {
        watchers: vec![WireWatcher {
            wire: WIRE,
            piston: PISTON,
            head: false,
        }],
        sources: vec![],
    };
    assert_eq!(changed_wire_watchers(&discovery, &[(WIRE, 0)], &[(WIRE, 1)]), discovery.watchers);
}

#[test]
fn masked_source_change_does_not_change_wire_or_notify() {
    let discovery = WireDiscovery {
        watchers: vec![WireWatcher {
            wire: WIRE,
            piston: PISTON,
            head: false,
        }],
        sources: vec![],
    };
    let before = max_attenuated_strength([(15, 1), (14, 2)]);
    let after = max_attenuated_strength([(15, 1), (12, 2)]);
    assert_eq!((before, after), (14, 14));
    assert!(changed_wire_watchers(&discovery, &[(WIRE, before)], &[(WIRE, after)]).is_empty());
}

#[test]
fn positive_to_positive_strength_change_remains_observable() {
    let before = max_attenuated_strength([(15, 1)]);
    let after = max_attenuated_strength([(14, 1)]);
    assert_eq!((before, after), (14, 13));
    assert!(wire_strength_changed(before, after));
}

#[test]
fn nearby_power_dependency_without_wire_change_does_not_notify() {
    let discovery = WireDiscovery {
        watchers: vec![WireWatcher {
            wire: WIRE,
            piston: PISTON,
            head: false,
        }],
        sources: vec![],
    };
    assert!(changed_wire_watchers(&discovery, &[(WIRE, 7)], &[(WIRE, 7)]).is_empty());
}

#[test]
fn initially_off_pressure_plate_is_a_dependency() {
    let mut world = fixture();
    let pos = BlockPos::new(41, 30, 41);
    world.set_block(pos, Block::StonePressurePlate { powered: false });
    let discovery = discover_watched_wire_sources(&world, BOUNDS).unwrap();
    assert!(discovery.sources.iter().any(|source| source.source == pos));
}

#[test]
fn direct_source_strength_matches_interpreter_dust() {
    let mut world = fixture();
    let discovery = discover_watched_wire_sources(&world, BOUNDS).unwrap();
    assert!(discovery.sources.contains(&WireSource {
        wire: WIRE,
        source: LEVER,
        attenuation: 0
    }));
    let Block::RedstoneWire { wire } = world.get_block(WIRE) else {
        panic!("wire missing")
    };
    assert_eq!(discovered_wire_strength(&world, &discovery, WIRE), wire.power);
    drive(&mut world);
    let Block::RedstoneWire { wire } = world.get_block(WIRE) else {
        panic!("wire missing")
    };
    assert_eq!(wire.power, 15);
    assert_eq!(discovered_wire_strength(&world, &discovery, WIRE), wire.power);
}

#[test]
fn directional_sources_match_native_power_queries() {
    use mchprs_blocks::blocks::{RedstoneObserver, RedstoneRepeater};
    let source = WIRE.offset(BlockFace::North);
    for facing in [BlockDirection::North, BlockDirection::South] {
        let mut world = fixture();
        let block = Block::RedstoneRepeater {
            repeater: RedstoneRepeater {
                facing,
                powered: true,
                ..Default::default()
            },
        };
        world.set_block(source, block);
        let discovery = discover_watched_wire_sources(&world, BOUNDS).unwrap();
        assert_eq!(
            discovered_wire_strength(&world, &discovery, WIRE),
            redstone::get_redstone_power(block, &world, source, BlockFace::North)
        );
    }
    for facing in [BlockFacing::North, BlockFacing::South] {
        let mut world = fixture();
        let block = Block::Observer {
            observer: RedstoneObserver { facing, powered: true },
        };
        world.set_block(source, block);
        let discovery = discover_watched_wire_sources(&world, BOUNDS).unwrap();
        assert_eq!(
            discovered_wire_strength(&world, &discovery, WIRE),
            redstone::get_redstone_power(block, &world, source, BlockFace::North)
        );
    }
}

#[test]
fn dust_distance_fourteen_and_unpowered_continuation_match_interpreter() {
    let mut world = fixture();
    world.set_block(LEVER, Block::Air);
    world.set_block(WIRE, Block::Air);
    world.set_block(PISTON, Block::Air);
    let source = BlockPos::new(39, 30, 40);
    let mut lever = Lever::new(LeverFace::Floor, BlockDirection::North, false);
    world.set_block(source, Block::Lever { lever });
    for x in 40..=55 {
        world.set_block(
            BlockPos::new(x, 30, 40),
            Block::RedstoneWire {
                wire: RedstoneWire::new(
                    RedstoneWireSide::None,
                    RedstoneWireSide::None,
                    RedstoneWireSide::Side,
                    RedstoneWireSide::Side,
                    0,
                ),
            },
        );
    }
    let bounds = (BlockPos::new(37, 26, 37), BlockPos::new(58, 34, 43));
    let discovery = discover_watched_wire_sources(&world, bounds).unwrap();
    for x in 40..=55 {
        let wire = BlockPos::new(x, 30, 40);
        assert_eq!(discovered_wire_strength(&world, &discovery, wire), 0);
        assert_eq!(discovery.sources.iter().any(|link| link.wire == wire && link.source == source), x <= 54);
    }
    lever.powered = true;
    world.set_block(source, Block::Lever { lever });
    redstone::update_surrounding_blocks(&mut world, source);
    for x in 40..=55 {
        let pos = BlockPos::new(x, 30, 40);
        let Block::RedstoneWire { wire } = world.get_block(pos) else {
            panic!("wire missing")
        };
        assert_eq!(discovered_wire_strength(&world, &discovery, pos), wire.power, "wire at {pos:?}");
    }
}

#[test]
fn competing_levers_mask_a_source_change_in_native_dust() {
    let mut world = fixture();
    let second = WIRE.offset(BlockFace::North);
    let mut lever = Lever::new(LeverFace::Floor, BlockDirection::North, false);
    world.set_block(second, Block::Lever { lever });
    let discovery = discover_watched_wire_sources(&world, BOUNDS).unwrap();
    for source in [LEVER, second] {
        assert!(
            discovery
                .sources
                .iter()
                .any(|link| link.wire == WIRE && link.source == source && link.attenuation == 0)
        );
    }
    drive(&mut world);
    lever.powered = true;
    world.set_block(second, Block::Lever { lever });
    redstone::update_surrounding_blocks(&mut world, second);
    let before = world.get_block(WIRE);
    let Block::RedstoneWire { wire } = before else { panic!("wire missing") };
    assert_eq!(discovered_wire_strength(&world, &discovery, WIRE), wire.power);
    lever.powered = false;
    world.set_block(second, Block::Lever { lever });
    redstone::update_surrounding_blocks(&mut world, second);
    assert_eq!(world.get_block(WIRE), before);
    assert_eq!(discovered_wire_strength(&world, &discovery, WIRE), wire.power);
}

#[test]
fn wire_only_discovery_keeps_shortest_of_competing_paths() {
    let mut world = fixture();
    world.set_block(PISTON, Block::Air);
    // Two paths from the first wire to the target: two edges and four edges.
    for (x, z) in [(42, 40), (43, 40), (41, 41), (42, 41), (43, 41)] {
        world.set_block(
            BlockPos::new(x, 30, z),
            Block::RedstoneWire {
                wire: RedstoneWire::default(),
            },
        );
    }
    let target = BlockPos::new(43, 30, 40);
    let scratch = ScratchWorld::from_world(&world, BOUNDS, entry(&world)).unwrap();
    let sources = discover_wire_sources(scratch.world(), &[target]).unwrap();
    scratch.finish_probe().unwrap();
    assert_eq!(
        sources,
        vec![WireSource {
            wire: target,
            source: LEVER,
            attenuation: 2
        }]
    );
    assert_eq!(
        world.get_block(LEVER),
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false)
        }
    );
}

#[test]
fn wire_only_discovery_respects_strong_conduction_direction() {
    let conductor = WIRE;
    let target = PISTON;
    for face in [BlockFace::Top, BlockFace::North] {
        let mut world = fixture();
        world.set_block(LEVER, Block::Air);
        world.set_block(conductor, Block::Stone {});
        world.set_block(
            target,
            Block::RedstoneWire {
                wire: RedstoneWire::default(),
            },
        );
        let source = conductor.offset(face);
        let block = Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        };
        world.set_block(source, block);
        let scratch = ScratchWorld::from_world(&world, BOUNDS, entry(&world)).unwrap();
        let sources = discover_wire_sources(scratch.world(), &[target]).unwrap();
        scratch.finish_probe().unwrap();
        let emits = crate::redstone::power::emits_strong_power(block, &world, source, face, false);
        assert_eq!(emits, face == BlockFace::Top);
        assert_eq!(
            sources.contains(&WireSource {
                wire: target,
                source,
                attenuation: 0
            }),
            emits
        );
    }
}

#[test]
fn ascending_and_descending_dust_match_interpreter() {
    for source_above in [false, true] {
        let mut world = fixture();
        world.set_block(LEVER, Block::Air);
        world.set_block(PISTON, Block::Stone {});
        let lower = WIRE;
        let upper = PISTON.offset(BlockFace::Top);
        let source = if source_above { BlockPos::new(43, 31, 40) } else { LEVER };
        let mut lever = Lever::new(LeverFace::Floor, BlockDirection::North, false);
        world.set_block(source, Block::Lever { lever });
        for pos in [lower, upper] {
            world.set_block(
                pos,
                Block::RedstoneWire {
                    wire: RedstoneWire::new(
                        RedstoneWireSide::None,
                        RedstoneWireSide::None,
                        RedstoneWireSide::Side,
                        RedstoneWireSide::Side,
                        0,
                    ),
                },
            );
        }
        let discovery = discover_watched_wire_sources(&world, BOUNDS).unwrap();
        let destination = if source_above { lower } else { upper };
        assert!(
            discovery
                .sources
                .iter()
                .any(|link| link.wire == destination && link.source == source && link.attenuation == 1)
        );
        for pos in [lower, upper] {
            assert_eq!(discovered_wire_strength(&world, &discovery, pos), 0);
        }
        lever.powered = true;
        world.set_block(source, Block::Lever { lever });
        redstone::update_surrounding_blocks(&mut world, source);
        for pos in [lower, upper] {
            let Block::RedstoneWire { wire } = world.get_block(pos) else {
                panic!("wire missing")
            };
            assert_eq!(
                discovered_wire_strength(&world, &discovery, pos),
                wire.power,
                "wire at {pos:?}, source_above={source_above}"
            );
        }
    }
}
