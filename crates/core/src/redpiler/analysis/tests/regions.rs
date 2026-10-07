use super::*;

#[test]
fn unsupported_ordinary_payload_is_rejected_instead_of_becoming_empty() {
    for assume_instant in [false, true] {
        let mut world = empty();
        world.set_block(BASE, Block::Piston { piston: RedstonePiston { facing: BlockFacing::East, sticky: false, extended: false } });
        world.set_block(BASE.offset(BlockFace::East), Block::Furnace { facing: mchprs_blocks::BlockDirection::North, lit: false });
        let bounds = (BASE, BASE + BlockPos::new(2,0,0));
        let before = snapshot(&world, bounds);
        let mut compiler = Compiler::default();
        let error = compiler.compile(&world, world.get_corners(), CompilerOptions { assume_instant, ..Default::default() }, vec![], Default::default()).unwrap_err().to_string();
        assert!(error.contains("unsupported sampled payload minecraft:furnace"), "{error}");
        assert!(!compiler.is_active());
        assert_eq!(snapshot(&world, bounds), before);
    }
}

#[test]
fn sampled_extension_destination_does_not_resample_quasi_powered_memory() {
    for assume_instant in [false, true] {
        let mut world = empty();
        let source = BASE;
        let memory = BASE + BlockPos::new(2, -1, 0);
        let lever = BASE.offset(BlockFace::North);
        world.set_block(source, Block::Piston { piston: RedstonePiston { facing: BlockFacing::East, sticky: true, extended: false } });
        world.set_block(source.offset(BlockFace::East), Block::RedstoneBlock);
        world.set_block(memory, Block::Piston { piston: RedstonePiston { facing: BlockFacing::Down, sticky: true, extended: true } });
        world.set_block(memory.offset(BlockFace::Bottom), Block::PistonHead { head: RedstonePistonHead { facing: BlockFacing::Down, sticky: true, short: false } });
        world.set_block(memory + BlockPos::new(0, -2, 0), Block::RedstoneBlock);
        world.set_block(lever.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(lever, Block::Lever { lever: mchprs_blocks::blocks::Lever::new(mchprs_blocks::blocks::LeverFace::Floor, mchprs_blocks::BlockDirection::North, false) });
        assert!(crate::redstone::piston::should_piston_extend(&world, BlockFacing::Down, memory));
        let mut compiler = Compiler::default();
        compiler.compile(&world, world.get_corners(), CompilerOptions { assume_instant, ..Default::default() }, vec![], Default::default()).unwrap();
        compiler.on_use_block(lever);
        lever_action(&mut world, lever, true);
        for tick in 0..6 {
            compiler.tick();
            world.tick_interpreted();
            let actors = compiler.backend.as_ref().unwrap().sampled_pistons();
            assert_eq!(actors[&memory].0, false, "memory tick {tick}, assume={assume_instant}");
            assert!(matches!(world.get_block(memory), Block::Piston { piston } if piston.extended));
        }
    }
}

#[test]
fn independent_adders_and_counters_share_a_plot_with_staggered_waves_and_handoff() {
    for optimize in [false, true] {
        for io_only in [false, true] {
            let mut interpreted = empty();
            let mut compiled = empty();
            let mut triggers = Vec::new();
            let mut outputs = Vec::new();
            for (name, shift, launch) in [
                ("counter_basic", BlockPos::new(0, 0, 0), 0),
                ("counter_basic", BlockPos::new(80, 0, 0), 7),
                ("adder_1bit", BlockPos::new(0, 0, 65), 17),
                ("adder_11bits", BlockPos::new(80, 0, 65), 23),
            ] {
                let (source, bounds, manifest) = fixture(name);
                crate::world::for_each_block_optimized(&source, bounds.0, bounds.1, |pos| {
                    let block = source.get_block(pos);
                    if block == Block::Air {
                        return;
                    }
                    for world in [&mut interpreted, &mut compiled] {
                        world.set_block(pos + shift, block);
                        if let Some(entity) = source.get_block_entity(pos) {
                            world.set_block_entity(pos + shift, entity.clone());
                        }
                    }
                });
                let counter = name == "counter_basic";
                triggers.push((
                    launch,
                    local_pos(&manifest["ports"]["inputs"]["trigger"]) + shift,
                    counter,
                ));
                for key in ["repeater", "sum_repeater", "carry_repeater"] {
                    let ports = &manifest["ports"]["observations"][key];
                    if ports.is_null() {
                        continue;
                    }
                    if ports[0].is_number() {
                        outputs.push(local_pos(ports) + shift);
                    } else {
                        outputs.extend(
                            ports
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|p| local_pos(p) + shift),
                        );
                    }
                }
                if !counter {
                    let case = manifest["cases"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|c| {
                            c["id"]
                                == if name == "adder_1bit" {
                                    "prepared-1-1-0"
                                } else {
                                    "prepared-31-1-0"
                                }
                        })
                        .unwrap();
                    for action in case["actions"].as_array().unwrap() {
                        if action["op"] == "lever"
                            && action["pos"] != manifest["ports"]["inputs"]["trigger"]
                        {
                            for world in [&mut interpreted, &mut compiled] {
                                lever_action(
                                    world,
                                    local_pos(&action["pos"]) + shift,
                                    action["powered"].as_bool().unwrap(),
                                );
                            }
                        }
                    }
                }
            }
            for _ in 0..32 {
                interpreted.tick_interpreted();
                compiled.tick_interpreted();
            }
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &compiled,
                    compiled.get_corners(),
                    CompilerOptions {
                        optimize,
                        io_only,
                        ..Default::default()
                    },
                    compiled.scheduler().iter_entries().collect(),
                    Default::default(),
                )
                .unwrap();
            let mut changed = vec![false; outputs.len()];
            let initial: Vec<_> = outputs
                .iter()
                .map(|&pos| interpreted.get_block(pos))
                .collect();
            for tick in 0..124 {
                for &(launch, pos, powered) in &triggers {
                    if tick == launch {
                        lever_action(&mut interpreted, pos, powered);
                        compiler.on_use_block(pos);
                        compiler.flush(&mut compiled);
                    }
                }
                interpreted.tick_interpreted();
                compiler.tick();
                compiler.flush(&mut compiled);
                for (index, &pos) in outputs.iter().enumerate() {
                    assert_eq!(
                        compiled.get_block(pos),
                        interpreted.get_block(pos),
                        "tick {tick}, output {pos:?}, optimize={optimize}, io={io_only}"
                    );
                    changed[index] |= interpreted.get_block(pos) != initial[index];
                }
            }
            // Both counters, the one-bit carry, and the eleven-bit sum fired.
            assert!(changed[..16].iter().any(|&v| v));
            assert!(changed[16..32].iter().any(|&v| v));
            assert!(changed[33]);
            assert!(changed[34..].iter().any(|&v| v));
            compiler.reset(&mut compiled, interpreted.get_corners());
            for tick in 0..24 {
                interpreted.tick_interpreted();
                compiled.tick_interpreted();
                for &pos in &outputs {
                    assert_eq!(
                        compiled.get_block(pos),
                        interpreted.get_block(pos),
                        "handoff tick {tick}, output {pos:?}, optimize={optimize}, io={io_only}"
                    );
                }
            }
        }
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

fn repeater_value(world: &PlotWorld, ports: &Value, shift: BlockPos) -> u16 {
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
                    for action in case["actions"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|a| a["op"] == "lever")
                    {
                        let pos = local_pos(&action["pos"]) + *shift;
                        let Block::Lever { lever } = world.get_block(pos) else {
                            panic!("missing control")
                        };
                        if lever.powered != action["powered"].as_bool().unwrap() {
                            compiler.on_use_block(pos);
                            compiler.flush(&mut world);
                        }
                    }
                    logical_ticks(&mut compiler, &mut world, &controls, &mut elapsed, 12);
                    for _ in 0..8 {
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
                        logical_ticks(&mut compiler, &mut world, &controls, &mut elapsed, 1);
                    }
                }
            }
            // Stop one clock. Its bank holds while the other clock keeps sampling.
            compiler.on_use_block(controls[0]);
            logical_ticks(&mut compiler, &mut world, &controls, &mut elapsed, 12);
            let held: Vec<_> = counters
                .iter()
                .map(|(m, shift)| {
                    repeater_value(&world, &m["ports"]["observations"]["repeater"], *shift)
                })
                .collect();
            logical_ticks(&mut compiler, &mut world, &controls, &mut elapsed, 24);
            assert_eq!(
                repeater_value(
                    &world,
                    &counters[0].0["ports"]["observations"]["repeater"],
                    counters[0].1
                ),
                held[0]
            );
            assert_eq!(
                repeater_value(
                    &world,
                    &counters[1].0["ports"]["observations"]["repeater"],
                    counters[1].1
                ),
                held[1].wrapping_add(4)
            );
            // Restart is a new sampling episode, without clearing stored data.
            compiler.on_use_block(controls[0]);
            logical_ticks(&mut compiler, &mut world, &controls, &mut elapsed, 24);
            assert_eq!(
                repeater_value(
                    &world,
                    &counters[0].0["ports"]["observations"]["repeater"],
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
                    repeater_value(&world, &m["ports"]["observations"]["repeater"], *shift)
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
                // This schematic's output bank reads the previous sampled count.
                assert_eq!(
                    stored,
                    expected.wrapping_add(1),
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
    let Block::PistonHead { head: mut saved_head } = world.get_block(head) else { unreachable!() };
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
