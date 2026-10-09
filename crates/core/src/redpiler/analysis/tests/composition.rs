//! Ordinary traces use the interpreter; instant outputs use the certified runtime model.
use super::*;
use crate::redpiler::analysis::graph::prepare_candidate_graph;
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::{ComparatorMode, RedstoneComparator, RedstoneRepeater, RedstoneWire};
use mchprs_blocks::BlockDirection;

const OSCILLATOR: BlockPos = BlockPos::new(8, 30, 8);

fn callbacks(entries: Vec<Value>) -> Vec<Value> {
    entries
        .into_iter()
        .filter(|entry| entry["kind"] == "callback")
        .collect()
}

fn add_oscillator(world: &mut PlotWorld) -> Vec<BlockPos> {
    let mut positions = Vec::new();
    for offset in [
        BlockPos::new(0, 0, 0),
        BlockPos::new(1, 0, 0),
        BlockPos::new(1, 0, 1),
        BlockPos::new(0, 0, 1),
    ] {
        let pos = OSCILLATOR + offset;
        world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(
            pos,
            if offset == BlockPos::new(0, 0, 0) {
                Block::RedstoneComparator {
                    comparator: RedstoneComparator::new(
                        BlockDirection::West,
                        ComparatorMode::Subtract,
                        false,
                    ),
                }
            } else {
                Block::RedstoneWire {
                    wire: RedstoneWire::default(),
                }
            },
        );
        positions.push(pos);
    }
    world.set_block(OSCILLATOR.offset(BlockFace::West), Block::RedstoneBlock);
    world.set_block_entity(OSCILLATOR, BlockEntity::Comparator { output_strength: 0 });
    for &pos in &positions[1..] {
        let wire = crate::redstone::wire::get_regulated_sides(RedstoneWire::default(), world, pos);
        world.set_block(pos, Block::RedstoneWire { wire });
    }
    crate::redstone::update(world.get_block(OSCILLATOR), world, OSCILLATOR, None);
    positions
}

fn compare_fixture(name: &str, optimize: bool, io_only: bool) {
    let (mut mixed, _, manifest) = fixture(name);
    let positions = add_oscillator(&mut mixed);
    let mut ordinary = empty();
    add_oscillator(&mut ordinary);
    let (mut isolated, _, _) = fixture(name);
    let mut compiler = Compiler::default();
    let mut reference = Compiler::default();
    let options = || CompilerOptions {
        optimize,
        io_only,
        ..Default::default()
    };
    let bounds = mixed.get_corners();
    compiler
        .compile(
            &mixed,
            bounds,
            options(),
            mixed.scheduler().iter_entries().collect(),
            Default::default(),
        )
        .unwrap_or_else(|error| panic!("mixed {name}: {error}"));
    reference
        .compile(
            &isolated,
            isolated.get_corners(),
            options(),
            Vec::new(),
            Default::default(),
        )
        .unwrap_or_else(|error| panic!("isolated {name}: {error}"));
    assert!(compiler.stats().unwrap().graph.native_propagation);
    mixed.clear_scheduled_ticks();
    let controls: Vec<_> = manifest["ports"]["inputs"]
        .as_object()
        .unwrap()
        .values()
        .map(local_pos)
        .collect();
    let outputs: Vec<_> = manifest["ports"]["observations"]
        .as_object()
        .unwrap()
        .values()
        .flat_map(|value| {
            if value[0].is_number() {
                vec![local_pos(value)]
            } else {
                value.as_array().unwrap().iter().map(local_pos).collect()
            }
        })
        .collect();
    let mut previous = 0;
    let mut transitions = 0;
    for tick in 0..96 {
        if tick % 24 == 0 {
            for &pos in &controls {
                compiler.on_use_block(pos);
                reference.on_use_block(pos);
            }
        }
        let expected_callbacks =
            crate::redstone::instant_piston_tests::capture_at(&positions, || {
                ordinary.tick_interpreted()
            });
        let actual_callbacks =
            crate::redstone::instant_piston_tests::capture_at(&positions, || {
                compiler.tick_with_world(&mut mixed)
            });
        assert_eq!(
            callbacks(actual_callbacks),
            callbacks(expected_callbacks),
            "{name} ordinary callback trace at tick {tick}"
        );
        reference.tick_with_world(&mut isolated);
        let strength = compiler
            .ordinary_sources()
            .into_iter()
            .find(|(pos, _)| *pos == OSCILLATOR)
            .unwrap()
            .1;
        assert_eq!(
            strength,
            crate::redstone::source_strength(ordinary.get_block(OSCILLATOR), &ordinary, OSCILLATOR)
        );
        transitions += usize::from(strength != previous);
        previous = strength;
        if tick % 5 == 0 {
            compiler.flush(&mut mixed);
            reference.flush(&mut isolated);
            for &pos in &outputs {
                assert_eq!(
                    mixed.get_block(pos),
                    isolated.get_block(pos),
                    "{name} O={optimize} IO={io_only} tick={tick} output={pos:?}"
                );
            }
        }
    }
    assert!(transitions >= 20, "exercise a live comparator oscillator");
    compiler.reset(&mut mixed, bounds);
    let isolated_bounds = isolated.get_corners();
    reference.reset(&mut isolated, isolated_bounds);
    for &pos in &outputs {
        assert_eq!(
            mixed.get_block(pos),
            isolated.get_block(pos),
            "{name} reset {pos:?}"
        );
    }
    for &pos in &positions {
        assert_eq!(mixed.get_block(pos), ordinary.get_block(pos));
    }
    assert_eq!(
        mixed
            .scheduler()
            .iter_entries()
            .filter(|entry| positions.contains(&entry.pos))
            .collect::<Vec<_>>(),
        ordinary.scheduler().iter_entries().collect::<Vec<_>>()
    );
    for _ in 0..16 {
        mixed.tick_interpreted();
        ordinary.tick_interpreted();
        for &pos in &positions {
            assert_eq!(mixed.get_block(pos), ordinary.get_block(pos));
        }
    }
}

#[test]
fn comparator_oscillator_compiles_with_an_active_instant_circuit() {
    for optimize in [false, true] {
        for io_only in [false, true] {
            compare_fixture("instant_observer", optimize, io_only);
        }
    }
}

#[test]
fn private_assembly_wires_have_no_ordinary_graph_nodes() {
    let (mut world, _, manifest) = fixture("instant_chain");
    let report = analyze_world(&world);
    let options = CompilerOptions {
        assume_instant: true,
        optimize: true,
        ..Default::default()
    };
    let (graph, programs) = crate::redpiler::instant::program::prepare(
        &world,
        &report,
        &world.scheduler().iter_entries().collect::<Vec<_>>(),
        &options,
        std::sync::Arc::new(crate::redpiler::TaskMonitor::default()),
    )
    .unwrap();
    let private_wires: Vec<_> = programs
        .iter()
        .flat_map(|program| {
            program
                .logic
                .wires
                .iter()
                .filter(|wire| !program.propagation_wires.contains(wire))
                .copied()
        })
        .collect();
    assert!(
        !private_wires.is_empty(),
        "fixture needs private assembly dust"
    );
    for &pos in &private_wires {
        assert!(
            graph
                .node_weights()
                .all(|node| !node.block.is_some_and(|(node_pos, _)| node_pos == pos)),
            "private assembly wire {pos:?} became an ordinary graph node"
        );
    }

    let controls: Vec<_> = manifest["ports"]["inputs"]
        .as_object()
        .unwrap()
        .values()
        .map(local_pos)
        .collect();
    let mut compiler = Compiler::default();
    let bounds = world.get_corners();
    compiler
        .compile(
            &world,
            bounds,
            options,
            world.scheduler().iter_entries().collect(),
            Default::default(),
        )
        .unwrap();
    for &pos in &private_wires {
        assert!(
            !compiler.native_owns(pos),
            "private assembly wire {pos:?} became natively executable"
        );
    }
    let private_states: Vec<_> = private_wires
        .iter()
        .map(|&pos| (pos, world.get_block(pos)))
        .collect();
    for tick in 0..32 {
        if tick % 4 == 0 {
            for &control in &controls {
                compiler.on_use_block(control);
            }
        }
        compiler.tick_with_world(&mut world);
        compiler.flush(&mut world);
        for &(pos, block) in &private_states {
            assert_eq!(
                world.get_block(pos),
                block,
                "private assembly wire {pos:?} received a physical update at tick {tick}"
            );
        }
    }
}

#[test]
fn ordinary_feedback_preserves_all_mandatory_instant_fixtures() {
    for name in [
        "instant_chain",
        "instant_torch",
        "adder_1bit",
        "counter_basic",
        "bud_noninstantinputs",
        "bud_instantmemorycellobserverupdate",
    ] {
        for optimize in [false, true] {
            for io_only in [false, true] {
                compare_fixture(name, optimize, io_only);
            }
        }
    }
}

fn feedback_fixture() -> (PlotWorld, [BlockPos; 2]) {
    use mchprs_blocks::blocks::RedstoneRepeater;
    let (mut world, _, manifest) = fixture("instant_observer");
    let input = local_pos(&manifest["ports"]["inputs"]["trigger"]);
    let output = local_pos(&manifest["ports"]["observations"]["repeater"]);
    for (pos, strength) in [(input, 0), (output, 15)] {
        world.set_block(
            pos,
            Block::RedstoneComparator {
                comparator: RedstoneComparator::new(
                    BlockDirection::North,
                    ComparatorMode::Compare,
                    strength > 0,
                ),
            },
        );
        world.set_block_entity(
            pos,
            BlockEntity::Comparator {
                output_strength: strength,
            },
        );
    }
    let repeaters = [
        (0, 10, BlockDirection::North),
        (4, 6, BlockDirection::South),
        (0, -1, BlockDirection::North),
    ];
    let wires: Vec<_> = (0..=4)
        .map(|x| (x, 11))
        .chain((7..=10).map(|z| (4, z)))
        .chain((-2..=5).map(|z| (4, z)))
        .chain((0..4).map(|x| (x, -2)))
        .collect();
    for &(x, z) in &wires {
        let pos = input + BlockPos::new(x, 0, z);
        world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(
            pos,
            Block::RedstoneWire {
                wire: RedstoneWire::default(),
            },
        );
    }
    for (x, z, facing) in repeaters {
        let pos = input + BlockPos::new(x, 0, z);
        world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(
            pos,
            Block::RedstoneRepeater {
                repeater: RedstoneRepeater {
                    facing,
                    delay: 4,
                    powered: false,
                    locked: false,
                },
            },
        );
    }
    for (x, z) in wires {
        let pos = input + BlockPos::new(x, 0, z);
        let wire = crate::redstone::wire::get_regulated_sides(RedstoneWire::default(), &world, pos);
        world.set_block(pos, Block::RedstoneWire { wire });
    }
    world.schedule_half_tick(output, 1, TickPriority::Normal);
    (world, [input, output])
}

#[test]
fn delayed_ordinary_feedback_through_an_assembly_is_event_driven_and_flush_independent() {
    let run = |optimize, io_only, flush_every| {
        let (mut world, channels) = feedback_fixture();
        let bounds = world.get_corners();
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
                world.scheduler().iter_entries().collect(),
                Default::default(),
            )
            .unwrap();
        world.clear_scheduled_ticks();
        let mut trace = Vec::new();
        for tick in 0..256 {
            compiler.tick_with_world(&mut world);
            if flush_every != 0 && tick % flush_every == 0 {
                compiler.flush(&mut world);
            }
            let sources = compiler.ordinary_sources();
            trace.push(
                channels.map(|channel| sources.iter().find(|(pos, _)| *pos == channel).unwrap().1),
            );
        }
        assert!(
            trace.windows(2).filter(|pair| pair[0] != pair[1]).count() >= 8,
            "the connected loop must actually oscillate: {trace:?}"
        );
        // Imported callback at 1, three delay-four repeaters (8 each), comparator (2).
        // The returning edge then crosses the input torch (2) and output comparator (2).
        for half_tick in 1..=40 {
            assert_eq!(
                trace[half_tick - 1],
                [
                    if half_tick < 27 { 0 } else { 15 },
                    if half_tick < 31 { 15 } else { 0 },
                ],
                "independent connected-loop timing at half tick {half_tick}"
            );
        }
        compiler.reset(&mut world, bounds);
        let pending: Vec<_> = world.scheduler().iter_entries().collect();
        assert!(
            !pending.is_empty(),
            "delayed feedback retains pending work at handoff"
        );
        (trace, pending)
    };
    let expected = run(false, false, 1);
    for optimize in [false, true] {
        for io_only in [false, true] {
            for flush_every in [0, 1, 5] {
                assert_eq!(run(optimize, io_only, flush_every), expected);
            }
        }
    }
}

#[test]
fn disconnected_assembly_preserves_local_ordinary_execution_planning() {
    for optimize in [false, true] {
        let mut ordinary = empty();
        add_oscillator(&mut ordinary);
        let (mut mixed, _, _) = fixture("instant_observer");
        add_oscillator(&mut mixed);
        for world in [&mut ordinary, &mut mixed] {
            let pos = OSCILLATOR + BlockPos::new(0, 0, 20);
            world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
            world.set_block(
                pos,
                Block::Lever {
                    lever: mchprs_blocks::blocks::Lever::default(),
                },
            );
            world.set_block(
                pos.offset(BlockFace::East),
                Block::RedstoneLamp { lit: false },
            );
        }
        let plan = |world: &PlotWorld| {
            let candidate = prepare_candidate_graph(
                world,
                world.get_corners(),
                &world.scheduler().iter_entries().collect::<Vec<_>>(),
                &CompilerOptions {
                    optimize,
                    ..Default::default()
                },
                Default::default(),
            )
            .unwrap();
            let mut nodes: Vec<_> = candidate
                .graph
                .node_weights()
                .filter_map(|node| {
                    node.block
                        .filter(|(pos, _)| pos.x < 20)
                        .map(|(pos, _)| (pos, node.native))
                })
                .collect();
            nodes.sort_by_key(|(pos, _)| (pos.x, pos.y, pos.z));
            nodes
        };
        assert_eq!(plan(&ordinary), plan(&mixed));
    }
}

#[test]
fn unchanged_comparator_outputs_deliver_the_same_callbacks_through_unchanged_dust() {
    let make = |mixed| {
        let mut world = if mixed {
            fixture("instant_observer").0
        } else {
            empty()
        };
        for index in 0..3 {
            let source = OSCILLATOR + BlockPos::new(0, 0, index * 5);
            world.set_block(source.offset(BlockFace::Bottom), Block::Stone {});
            world.set_block(
                source,
                Block::RedstoneComparator {
                    comparator: RedstoneComparator::new(
                        BlockDirection::West,
                        ComparatorMode::Compare,
                        true,
                    ),
                },
            );
            world.set_block_entity(
                source,
                BlockEntity::Comparator {
                    output_strength: 15,
                },
            );
            world.set_block(source.offset(BlockFace::West), Block::RedstoneBlock);
            let dust = source.offset(BlockFace::East);
            world.set_block(dust.offset(BlockFace::Bottom), Block::Stone {});
            let wire =
                crate::redstone::wire::get_regulated_sides(RedstoneWire::default(), &world, dust);
            world.set_block(
                dust,
                Block::RedstoneWire {
                    wire: RedstoneWire { power: 15, ..wire },
                },
            );
            world.schedule_half_tick(source, 2, TickPriority::Normal);
        }
        world
    };
    for optimize in [false, true] {
        let mut ordinary = make(false);
        let mut mixed = make(true);
        let mut compiler = Compiler::default();
        compiler
            .compile(
                &mixed,
                mixed.get_corners(),
                CompilerOptions {
                    optimize,
                    ..Default::default()
                },
                mixed.scheduler().iter_entries().collect(),
                Default::default(),
            )
            .unwrap();
        mixed.clear_scheduled_ticks();
        let positions: Vec<_> = (0..3)
            .flat_map(|index| {
                let source = OSCILLATOR + BlockPos::new(0, 0, index * 5);
                [source, source.offset(BlockFace::East)]
            })
            .collect();
        let expected = callbacks(crate::redstone::instant_piston_tests::capture_at(
            &positions,
            || {
                for _ in 0..8 {
                    ordinary.tick_interpreted();
                }
            },
        ));
        let actual = callbacks(crate::redstone::instant_piston_tests::capture_at(
            &positions,
            || {
                for _ in 0..8 {
                    compiler.tick_with_world(&mut mixed);
                }
            },
        ));
        assert!(
            expected.len() >= 6,
            "exercise repeated unchanged-output callbacks"
        );
        assert_eq!(actual, expected);
        compiler.flush(&mut mixed);
        assert_eq!(
            mixed.get_block(positions[1]),
            ordinary.get_block(positions[1])
        );
    }
}

#[test]
fn private_geometry_does_not_notify_external_dust_or_remove_decorations() {
    for optimize in [false, true] {
        for io_only in [false, true] {
            let (mut mixed, trigger, base, old_output) =
                super::outputs::conductor_output(Block::Stone {}, false, false);
            let far = base + BlockPos::new(0, 0, 2);
            let sign = base + BlockPos::new(1, 0, 1);
            let main = far.offset(BlockFace::East);
            let side = far.offset(BlockFace::South);
            let consumer = main.offset(BlockFace::South);
            mixed.set_block(old_output, Block::Air);
            mixed.set_block(far + BlockPos::new(2, 0, 0), Block::Air);
            mixed.set_block(
                sign,
                Block::WallSign {
                    sign_type: mchprs_blocks::SignType(0),
                    facing: BlockDirection::East,
                },
            );
            mixed.set_block_entity(sign, BlockEntity::Sign(Box::default()));
            let sign_state = mixed.get_block(sign);
            let sign_entity = json!(mixed.get_block_entity(sign));
            let source = far.offset(BlockFace::West);
            mixed.set_block(source.offset(BlockFace::Bottom), Block::Stone {});
            mixed.set_block(
                source,
                Block::RedstoneRepeater {
                    repeater: RedstoneRepeater {
                        facing: BlockDirection::West,
                        delay: 1,
                        powered: true,
                        locked: false,
                    },
                },
            );
            mixed.set_block(source.offset(BlockFace::West), Block::RedstoneBlock);
            for pos in [main, side, consumer] {
                mixed.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
            }
            mixed.set_block(
                consumer,
                Block::RedstoneComparator {
                    comparator: RedstoneComparator::new(
                        BlockDirection::North,
                        ComparatorMode::Subtract,
                        false,
                    ),
                },
            );
            mixed.set_block_entity(consumer, BlockEntity::Comparator { output_strength: 0 });
            for pos in [main, side] {
                let wire = crate::redstone::wire::get_regulated_sides(
                    RedstoneWire::default(),
                    &mixed,
                    pos,
                );
                mixed.set_block(
                    pos,
                    Block::RedstoneWire {
                        wire: RedstoneWire { power: 15, ..wire },
                    },
                );
            }
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &mixed,
                    mixed.get_corners(),
                    CompilerOptions {
                        optimize,
                        io_only,
                        ..Default::default()
                    },
                    vec![],
                    Default::default(),
                )
                .unwrap();
            compiler.on_use_block(trigger);
            let positions = [main, side, consumer];
            for tick in 1..=32 {
                let actual = callbacks(crate::redstone::instant_piston_tests::capture_at(
                    &positions,
                    || {
                        compiler.tick_with_world(&mut mixed);
                    },
                ));
                assert!(
                    actual.is_empty(),
                    "private geometry notified external blocks at {tick}, optimize {optimize}, IO {io_only}"
                );
                if tick % 5 == 0 {
                    compiler.flush(&mut mixed);
                    if !io_only {
                        assert_eq!(mixed.get_block(sign), sign_state);
                        assert_eq!(json!(mixed.get_block_entity(sign)), sign_entity);
                    }
                }
            }
        }
    }
}
