use super::*;

#[test]
fn shared_consumer_channels_merge_regions_but_comparator_inputs_stay_distinct() {
    let (mut world, _, _) = fixture("instant_observer");
    let (second, bounds, _) = fixture("instant_observer");
    let shift = BlockPos::new(64, 0, 48);
    crate::world::for_each_block_optimized(&second, bounds.0, bounds.1, |pos| {
        let block = second.get_block(pos);
        if block != Block::Air {
            world.set_block(pos + shift, block);
        }
    });
    let monitor = Default::default();
    let mut report = analyze_world(&world);
    assert_eq!(
        crate::redpiler::instant::regions::split(&world, &report, &monitor)
            .unwrap()
            .len(),
        2,
        "disconnected circuits begin in separate regions"
    );
    let first = report.ports.outputs[0].clone();
    let other = report
        .ports
        .outputs
        .iter()
        .position(|output| output.consumer.x >= BASE.x + shift.x)
        .unwrap();
    report.ports.outputs[other].consumer = first.consumer;
    report.ports.outputs[other].input = first.input;
    assert_eq!(
        crate::redpiler::instant::regions::split(&world, &report, &monitor)
            .unwrap()
            .len(),
        1,
        "the same consumer input is evaluated in one region"
    );

    report.ports.outputs[other].input =
        if first.input == crate::redpiler::analysis::ports::ConsumerInput::Main {
            crate::redpiler::analysis::ports::ConsumerInput::ComparatorSide
        } else {
            crate::redpiler::analysis::ports::ConsumerInput::Main
        };
    assert_eq!(
        crate::redpiler::instant::regions::split(&world, &report, &monitor)
            .unwrap()
            .len(),
        2,
        "comparator main and side channels remain independent"
    );
}

#[test]
fn empty_ordinary_clock_preserves_stationary_far_context() {
    for assume_instant in [false, true] {
        for optimize in [false, true] {
            let (mut compiled, bounds, manifest) = fixture("counter_basic");
            let clock = analyze_world(&compiled)
                .pistons
                .iter()
                .find(|p| !p.piston.sticky)
                .unwrap()
                .pos;
            let far = clock + BlockPos::new(0, -2, 0);
            assert_eq!(compiled.get_block(far), Block::Air);
            compiled.set_block(far, Block::Glowstone {});
            let (mut native, _, _) = fixture("counter_basic");
            native.set_block(far, Block::Glowstone {});
            let trigger = local_pos(&manifest["ports"]["inputs"]["trigger"]);
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &compiled,
                    compiled.get_corners(),
                    CompilerOptions {
                        assume_instant,
                        optimize,
                        ..Default::default()
                    },
                    vec![],
                    Default::default(),
                )
                .unwrap();
            compiler.on_use_block(trigger);
            lever_action(&mut native, trigger, true);
            let mut poses = [false; 2];
            for tick in 1..=64 {
                native.tick_interpreted();
                compiler.tick_with_world(&mut compiled);
                compiler.flush(&mut compiled);
                assert_eq!(
                    native.get_block(far),
                    Block::Glowstone {},
                    "native tick {tick}"
                );
                assert_eq!(
                    compiled.get_block(far),
                    Block::Glowstone {},
                    "compiled tick {tick}"
                );
                if let Block::Piston { piston } = native.get_block(clock) {
                    poses[usize::from(piston.extended)] = true;
                }
                if tick % 6 == 5 {
                    assert_eq!(
                        repeater_value(
                            &compiled,
                            &manifest["ports"]["observations"]["repeater"],
                            BlockPos::new(0, 0, 0)
                        ),
                        (tick - 5) / 6
                    );
                }
            }
            assert_eq!(
                poses,
                [true, true],
                "exercise native extension and retraction"
            );
            compiler.reset(&mut compiled, bounds);
            assert_eq!(compiled.get_block(far), Block::Glowstone {});
        }
    }
}

#[test]
fn unsupported_ordinary_payload_is_rejected_instead_of_becoming_empty() {
    for assume_instant in [false, true] {
        let mut world = empty();
        world.set_block(
            BASE,
            Block::Piston {
                piston: RedstonePiston {
                    facing: BlockFacing::East,
                    sticky: false,
                    extended: false,
                },
            },
        );
        world.set_block(
            BASE.offset(BlockFace::East),
            Block::Furnace {
                facing: mchprs_blocks::BlockDirection::North,
                lit: false,
            },
        );
        let bounds = (BASE, BASE + BlockPos::new(2, 0, 0));
        let before = snapshot(&world, bounds);
        let mut compiler = Compiler::default();
        let error = compiler
            .compile(
                &world,
                world.get_corners(),
                CompilerOptions {
                    assume_instant,
                    ..Default::default()
                },
                vec![],
                Default::default(),
            )
            .unwrap_err()
            .to_string();
        assert!(error.contains("logical piston admission failed"), "{error}");
        assert!(!compiler.is_active());
        assert_eq!(snapshot(&world, bounds), before);
    }
}

#[test]
fn ordinary_payload_cannot_treat_an_occupied_far_base_as_empty_context() {
    for assume_instant in [false, true] {
        let mut world = empty();
        let base = Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::East,
                sticky: false,
                extended: false,
            },
        };
        world.set_block(BASE, base);
        world.set_block(BASE.offset(BlockFace::East), Block::RedstoneBlock);
        world.set_block(BASE + BlockPos::new(2, 0, 0), base);
        let bounds = (BASE, BASE + BlockPos::new(4, 0, 0));
        let before = snapshot(&world, bounds);
        let mut compiler = Compiler::default();
        let error = compiler
            .compile(
                &world,
                world.get_corners(),
                CompilerOptions {
                    assume_instant,
                    ..Default::default()
                },
                vec![],
                Default::default(),
            )
            .unwrap_err()
            .to_string();
        assert!(error.contains("logical piston admission failed"), "{error}");
        assert!(!compiler.is_active());
        assert_eq!(snapshot(&world, bounds), before);
    }
}

fn logical_ticks(
    compiler: &mut Compiler,
    world: &mut PlotWorld,
    controls: &[BlockPos],
    elapsed: &mut usize,
    ticks: usize,
) {
    for _ in 0..ticks {
        for (index, &pos) in controls.iter().enumerate() {
            if *elapsed == index * 7 {
                compiler.on_use_block(pos);
            }
        }
        compiler.tick();
        compiler.flush(world);
        *elapsed += 1;
    }
}

pub(super) fn repeater_value(world: &PlotWorld, ports: &Value, shift: BlockPos) -> u16 {
    let ports: Vec<_> = if ports[0].is_number() {
        vec![ports]
    } else {
        ports.as_array().unwrap().iter().collect()
    };
    ports.iter().enumerate().fold(0, |bits, (bit, port)| {
        let Block::RedstoneRepeater { repeater } = world.get_block(local_pos(port) + shift) else {
            panic!("missing output")
        };
        bits | (u16::from(!repeater.powered) << bit)
    })
}

fn stored_value(compiler: &Compiler, ports: &Value, shift: BlockPos) -> u16 {
    let banks = compiler.backend.as_ref().unwrap().logical_stats();
    ports
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .fold(0, |word, (bit, port)| {
            let cell = local_pos(port) + shift;
            let stored = banks
                .iter()
                .flat_map(|(_, _, cells)| cells)
                .find(|(pos, _)| *pos == cell)
                .unwrap()
                .1;
            word | (u16::from(stored) << bit)
        })
}

#[test]
fn ideal_mode_supports_repeated_arithmetic_independent_clocks_and_stored_handoff() {
    for optimize in [false, true] {
        for io_only in [false, true] {
            let mut world = empty();
            let mut adders = Vec::new();
            let mut counters = Vec::new();
            for (name, shift) in [
                ("counter_basic", BlockPos::new(0, 0, 0)),
                ("counter_basic", BlockPos::new(80, 0, 0)),
                ("adder_1bit", BlockPos::new(0, 0, 65)),
                ("adder_11bits", BlockPos::new(80, 0, 65)),
            ] {
                let (source, bounds, manifest) = fixture(name);
                crate::world::for_each_block_optimized(&source, bounds.0, bounds.1, |pos| {
                    let block = source.get_block(pos);
                    if block != Block::Air {
                        world.set_block(pos + shift, block);
                        if let Some(entity) = source.get_block_entity(pos) {
                            world.set_block_entity(pos + shift, entity.clone());
                        }
                    }
                });
                if name == "counter_basic" {
                    counters.push((manifest, shift));
                } else {
                    adders.push((manifest, shift));
                }
            }
            let options = CompilerOptions {
                assume_instant: true,
                optimize,
                io_only,
                ..Default::default()
            };
            let candidate = graph::prepare_candidate_graph(
                &world,
                world.get_corners(),
                &[],
                &options,
                Default::default(),
            )
            .unwrap();
            assert!(candidate.summary().compiled_outputs > 0);
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &world,
                    world.get_corners(),
                    options,
                    Vec::new(),
                    Default::default(),
                )
                .unwrap();
            let controls: Vec<_> = counters
                .iter()
                .map(|(m, shift)| local_pos(&m["ports"]["inputs"]["trigger"]) + *shift)
                .collect();
            let mut elapsed = 0;
            for (manifest, shift) in &adders {
                for case in manifest["cases"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|c| c["expectation"].is_object())
                {
                    let trigger = local_pos(&manifest["ports"]["inputs"]["trigger"]) + *shift;
                    if matches!(world.get_block(trigger), Block::Lever { lever } if !lever.powered)
                    {
                        compiler.on_use_block(trigger);
                        compiler.flush(&mut world);
                    }
                    logical_ticks(&mut compiler, &mut world, &controls, &mut elapsed, 32);
                    for action in case["actions"].as_array().unwrap().iter() {
                        if action["op"] == "wait_ready" {
                            logical_ticks(
                                &mut compiler,
                                &mut world,
                                &controls,
                                &mut elapsed,
                                action["ticks"].as_u64().unwrap() as usize,
                            );
                            continue;
                        }
                        assert_eq!(action["op"], "lever");
                        let pos = local_pos(&action["pos"]) + *shift;
                        let Block::Lever { lever } = world.get_block(pos) else {
                            panic!("missing control")
                        };
                        if lever.powered != action["powered"].as_bool().unwrap() {
                            compiler.on_use_block(pos);
                            compiler.flush(&mut world);
                        }
                    }
                    // Sample both consumers in the fixture's common response window.
                    for tick in 1..=24 {
                        logical_ticks(&mut compiler, &mut world, &controls, &mut elapsed, 1);
                        if !(5..=7).contains(&tick) {
                            continue;
                        }
                        assert_eq!(
                            repeater_value(
                                &world,
                                &manifest["ports"]["observations"]["sum_repeater"],
                                *shift
                            ) as u64,
                            case["expectation"]["sum"].as_u64().unwrap(),
                            "{} at {elapsed}",
                            case["id"]
                        );
                        if !manifest["ports"]["observations"]["carry_repeater"].is_null() {
                            assert_eq!(
                                repeater_value(
                                    &world,
                                    &manifest["ports"]["observations"]["carry_repeater"],
                                    *shift
                                ) as u64,
                                case["expectation"]["carry"].as_u64().unwrap(),
                                "{} carry",
                                case["id"]
                            );
                        }
                    }
                }
            }
            // Stop one clock. Its bank holds while the other clock keeps sampling.
            compiler.on_use_block(controls[0]);
            logical_ticks(&mut compiler, &mut world, &controls, &mut elapsed, 12);
            let held: Vec<_> = counters
                .iter()
                .map(|(m, shift)| {
                    stored_value(&compiler, &m["ports"]["observations"]["memory"], *shift)
                })
                .collect();
            logical_ticks(&mut compiler, &mut world, &controls, &mut elapsed, 24);
            assert_eq!(
                stored_value(
                    &compiler,
                    &counters[0].0["ports"]["observations"]["memory"],
                    counters[0].1
                ),
                held[0]
            );
            assert_eq!(
                stored_value(
                    &compiler,
                    &counters[1].0["ports"]["observations"]["memory"],
                    counters[1].1
                ),
                held[1].wrapping_add(4)
            );
            // Restart is a new sampling episode, without clearing stored data.
            compiler.on_use_block(controls[0]);
            logical_ticks(&mut compiler, &mut world, &controls, &mut elapsed, 24);
            assert_eq!(
                stored_value(
                    &compiler,
                    &counters[0].0["ports"]["observations"]["memory"],
                    counters[0].1
                ),
                held[0].wrapping_add(4)
            );
            for &pos in &controls {
                compiler.on_use_block(pos);
            }
            logical_ticks(&mut compiler, &mut world, &controls, &mut elapsed, 12);
            let counts: Vec<_> = counters
                .iter()
                .map(|(m, shift)| {
                    stored_value(&compiler, &m["ports"]["observations"]["memory"], *shift)
                })
                .collect();
            let bounds = world.get_corners();
            compiler.reset(&mut world, bounds);
            for ((manifest, shift), expected) in counters.iter().zip(counts) {
                let stored = manifest["ports"]["observations"]["memory"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .enumerate()
                    .fold(0u16, |count, (bit, pos)| {
                        let Block::Piston { piston } = world.get_block(local_pos(pos) + *shift)
                        else {
                            panic!("missing memory")
                        };
                        count | (u16::from(!piston.extended) << bit)
                    });
                assert_eq!(
                    stored, expected,
                    "stored handoff, optimize={optimize}, io={io_only}"
                );
            }
        }
    }
}

#[test]
fn ideal_mode_skips_reset_certification_but_keeps_payload_and_geometry_guards() {
    let (mut world, _, _) = fixture("instant_observer");
    let piston = analyze_world(&world).pistons[0].pos;
    world.set_block(piston + BlockPos::new(0, 2, 0), Block::Glass {});
    assert!(Compiler::default()
        .compile(
            &world,
            world.get_corners(),
            Default::default(),
            Vec::new(),
            Default::default()
        )
        .is_err());
    let options = CompilerOptions {
        assume_instant: true,
        ..Default::default()
    };
    Compiler::default()
        .compile(
            &world,
            world.get_corners(),
            options,
            Vec::new(),
            Default::default(),
        )
        .unwrap();
    let head = analyze_world(&world).pistons[0].head;
    let Block::PistonHead {
        head: mut saved_head,
    } = world.get_block(head)
    else {
        unreachable!()
    };
    saved_head.short = true;
    world.set_block(head, Block::PistonHead { head: saved_head });
    let options = CompilerOptions {
        assume_instant: true,
        ..Default::default()
    };
    assert!(Compiler::default()
        .compile(
            &world,
            world.get_corners(),
            options,
            Vec::new(),
            Default::default()
        )
        .is_err());

    let (mut world, _, manifest) = fixture("counter_basic");
    let clock = analyze_world(&world)
        .pistons
        .iter()
        .find(|p| !p.piston.sticky)
        .unwrap()
        .pos;
    world.set_block(clock + BlockPos::new(0, 2, 0), Block::Glass {});
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &world,
            world.get_corners(),
            CompilerOptions {
                assume_instant: true,
                ..Default::default()
            },
            Vec::new(),
            Default::default(),
        )
        .unwrap();
    compiler.on_use_block(local_pos(&manifest["ports"]["inputs"]["trigger"]));
    for _ in 0..20 {
        compiler.tick();
        compiler.flush(&mut world);
    }
    assert!(
        repeater_value(
            &world,
            &manifest["ports"]["observations"]["repeater"],
            BlockPos::new(0, 0, 0)
        ) > 0
    );
}
