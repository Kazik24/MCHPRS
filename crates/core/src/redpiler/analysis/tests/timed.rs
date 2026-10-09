//! Production acceptance uses native timing; ideal-plan tests keep separate oracles.
use super::*;

fn value(world: &PlotWorld, compiler: Option<&Compiler>, ports: &Value) -> u16 {
    let Some(compiler) = compiler else {
        return regions::repeater_value(world, ports, BlockPos::new(0, 0, 0));
    };
    let sources = compiler.ordinary_sources();
    let ports: Vec<_> = if ports[0].is_number() {
        vec![ports]
    } else {
        ports.as_array().unwrap().iter().collect()
    };
    ports.iter().enumerate().fold(0, |bits, (bit, port)| {
        let pos = local_pos(port);
        let strength = sources.iter().find(|(source, _)| *source == pos).unwrap().1;
        bits | (u16::from(strength == 0) << bit)
    })
}

#[test]
fn production_adders_preserve_native_arithmetic_windows() {
    for name in ["adder_1bit", "adder_11bits"] {
        let (_, _, manifest) = fixture(name);
        for case in manifest["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|case| case["expectation"].is_object())
        {
            for optimize in [false, true] {
                for assume_instant in [false, true] {
                    let (mut native, _, _) = fixture(name);
                    let (mut world, _, _) = fixture(name);
                    let mut compiler = Compiler::default();
                    compiler
                        .compile(
                            &world,
                            world.get_corners(),
                            CompilerOptions {
                                optimize,
                                assume_instant,
                                ..Default::default()
                            },
                            vec![],
                            Default::default(),
                        )
                        .unwrap();
                    for action in case["actions"].as_array().unwrap() {
                        if action["op"] == "lever" {
                            let pos = local_pos(&action["pos"]);
                            let desired = action["powered"].as_bool().unwrap();
                            lever_action(&mut native, pos, desired);
                            if matches!(world.get_block(pos),Block::Lever { lever } if lever.powered != desired)
                            {
                                compiler.on_use_block(pos);
                                compiler.flush(&mut world);
                            }
                        } else if action["op"] == "wait_ready" {
                            for _ in 0..action["ticks"].as_u64().unwrap() {
                                native.tick_interpreted();
                                compiler.tick_with_world(&mut world);
                                compiler.flush(&mut world);
                            }
                        }
                    }
                    for tick in 1..=24 {
                        native.tick_interpreted();
                        compiler.tick_with_world(&mut world);
                        compiler.flush(&mut world);
                        let ports = &manifest["ports"]["observations"]["sum_repeater"];
                        let actual = value(&world, Some(&compiler), ports);
                        assert_eq!(
                            actual,
                            value(&native, None, ports),
                            "{name} {} tick={tick} O={optimize} A={assume_instant}",
                            case["id"]
                        );
                        if (3..=7).contains(&tick) {
                            assert_eq!(
                                actual,
                                case["expectation"]["sum"].as_u64().unwrap() as u16,
                                "{name} {} sum tick={tick}",
                                case["id"]
                            );
                        }
                        if name == "adder_1bit" && (5..=9).contains(&tick) {
                            assert_eq!(
                                value(
                                    &world,
                                    Some(&compiler),
                                    &manifest["ports"]["observations"]["carry_repeater"]
                                ),
                                case["expectation"]["carry"].as_u64().unwrap() as u16
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn production_counter_counts_in_native_consumer_windows() {
    for optimize in [false, true] {
        for assume_instant in [false, true] {
            let (mut native, _, manifest) = fixture("counter_basic");
            let (mut world, _, _) = fixture("counter_basic");
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &world,
                    world.get_corners(),
                    CompilerOptions {
                        optimize,
                        assume_instant,
                        ..Default::default()
                    },
                    vec![],
                    Default::default(),
                )
                .unwrap();
            let trigger = local_pos(&manifest["ports"]["inputs"]["trigger"]);
            lever_action(&mut native, trigger, true);
            compiler.on_use_block(trigger);
            for tick in 1..=102 {
                native.tick_interpreted();
                compiler.tick_with_world(&mut world);
                compiler.flush(&mut world);
                let ports = &manifest["ports"]["observations"]["repeater"];
                let actual = value(&world, Some(&compiler), ports);
                assert_eq!(
                    actual,
                    value(&native, None, ports),
                    "counter tick={tick} O={optimize} A={assume_instant}"
                );
                if tick >= 11 && matches!(tick % 6, 5 | 0 | 1 | 2) {
                    assert_eq!(
                        actual,
                        ((tick - 5) / 6) as u16,
                        "count consumer tick={tick}"
                    );
                }
            }
        }
    }
}

#[test]
fn production_handoff_keeps_unchanged_container_entities() {
    use mchprs_blocks::block_entities::{BlockEntity, ContainerType};
    let (mut world, cell, _, _, _) = memory::generator_cell(false);
    let pos = cell + BlockPos::new(0, 0, -2);
    world.set_block(
        pos,
        Block::Barrel {
            facing: BlockFacing::Up,
            open: true,
        },
    );
    let bounds = world.get_corners();
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &world,
            bounds,
            CompilerOptions {
                io_only: true,
                ..Default::default()
            },
            vec![],
            Default::default(),
        )
        .unwrap();
    world.set_block(
        pos,
        Block::Barrel {
            facing: BlockFacing::Up,
            open: false,
        },
    );
    world.set_block_entity(
        pos,
        BlockEntity::Container {
            ty: ContainerType::Barrel,
            inventory: Default::default(),
            comparator_override: 7,
        },
    );
    compiler.reset(&mut world, bounds);
    assert!(matches!(
        world.get_block(pos),
        Block::Barrel { open: false, .. }
    ));
    assert!(matches!(
        world.get_block_entity(pos),
        Some(BlockEntity::Container {
            comparator_override: 7,
            ..
        })
    ));
}

#[test]
fn production_timed_commands_keep_synchronous_conditional_chain_results() {
    use mchprs_blocks::block_entities::{BlockEntity, CommandBlockEntity};
    let setup = || {
        let (mut world, cell, _, _, _) = memory::generator_cell(false);
        let first = cell + BlockPos::new(2, 1, 0);
        for (pos, name, conditional, command) in [
            (first, "command_block", false, "say timed first"),
            (
                first.offset(BlockFace::East),
                "chain_command_block",
                true,
                "say timed chain",
            ),
        ] {
            let mut block = Block::from_name(name).unwrap();
            block.set_properties(std::collections::HashMap::from([
                ("facing", "east"),
                ("conditional", if conditional { "true" } else { "false" }),
            ]));
            world.set_block(pos, block);
            world.set_block_entity(
                pos,
                BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
                    command: command.into(),
                    automatic: true,
                    ..Default::default()
                })),
            );
        }
        crate::redstone::update(world.get_block(first), &mut world, first, None);
        (world, first)
    };
    for io_only in [false, true] {
        let (mut native, first) = setup();
        let (mut world, _) = setup();
        let mut compiler = Compiler::default();
        compiler
            .compile(
                &world,
                world.get_corners(),
                CompilerOptions {
                    io_only,
                    ..Default::default()
                },
                world.scheduler().iter_entries().collect(),
                Default::default(),
            )
            .unwrap();
        world.native_scheduler().clear();
        for tick in 1..=4 {
            native.tick_interpreted();
            compiler.tick_with_world(&mut world);
            compiler.flush(&mut world);
            assert_eq!(
                world.command_output().collect::<Vec<_>>(),
                native.command_output().collect::<Vec<_>>(),
                "command tick={tick} io_only={io_only}"
            );
            for pos in [first, first.offset(BlockFace::East)] {
                assert_eq!(
                    json!(world.get_block_entity(pos)),
                    json!(native.get_block_entity(pos)),
                    "command entity tick={tick} io_only={io_only}"
                );
            }
        }
        assert_eq!(world.command_output().count(), 2);
    }
}

#[test]
fn production_assumption_keeps_movement_and_sampling_guards() {
    for mutation in [
        "unknown payload",
        "payload entity",
        "short head",
        "moving support",
        "no sampler",
    ] {
        let (mut world, cell, _, generator, _) = memory::generator_cell(false);
        let far = cell + BlockPos::new(0, -2, 0);
        match mutation {
            "unknown payload" => {
                world.set_block(far, Block::Unknown { id: 1_000_000 });
            }
            "payload entity" => {
                world.set_block_entity(
                    far,
                    mchprs_blocks::block_entities::BlockEntity::Comparator { output_strength: 7 },
                );
            }
            "short head" => {
                let pos = cell.offset(BlockFace::Bottom);
                let Block::PistonHead { mut head } = world.get_block(pos) else {
                    unreachable!()
                };
                head.short = true;
                world.set_block(pos, Block::PistonHead { head });
            }
            "moving support" => {
                world.set_block(
                    far.offset(BlockFace::Top),
                    Block::RedstoneTorch { lit: false },
                );
            }
            "no sampler" => {
                world.set_block(generator, Block::Air);
                world.set_block(generator.offset(BlockFace::West), Block::Air);
            }
            _ => unreachable!(),
        }
        let bounds = world.get_corners();
        let before = snapshot(
            &world,
            (cell - BlockPos::new(4, 4, 4), cell + BlockPos::new(4, 4, 4)),
        );
        let mut compiler = Compiler::default();
        assert!(
            compiler
                .compile(
                    &world,
                    bounds,
                    CompilerOptions {
                        assume_instant: true,
                        ..Default::default()
                    },
                    vec![],
                    Default::default()
                )
                .is_err(),
            "{mutation}"
        );
        assert!(!compiler.is_active());
        assert_eq!(
            snapshot(
                &world,
                (cell - BlockPos::new(4, 4, 4), cell + BlockPos::new(4, 4, 4))
            ),
            before
        );
    }
}
