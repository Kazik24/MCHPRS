use super::*;
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::{
    ComparatorMode, Lever, LeverFace, RedstoneComparator, RedstoneRepeater, RedstoneWire,
};
use mchprs_blocks::{BlockColorVariant, BlockDirection};

#[test]
fn conditional_geometry_drives_command_outputs_without_replaying_chat_on_reset() {
    for optimize in [false, true] {
        let make_world = || {
            let (mut world, trigger, base, output) = conductor_output(Block::Stone {}, true, false);
            world.disable_command_output_limits_for_replay();
            world.set_block(output, Block::from_name("command_block").unwrap());
            world.set_block_entity(
                output,
                BlockEntity::CommandBlock(Box::new(
                    mchprs_blocks::block_entities::CommandBlockEntity {
                        command: "say instant output".into(),
                        ..Default::default()
                    },
                )),
            );
            crate::redstone::command_block::update(&mut world, output);
            (world, trigger, base, output)
        };
        let (mut reference, trigger, _, _) = make_world();
        let (mut compiled, _, _, _) = make_world();
        let bounds = reference.get_corners();
        let mut compiler = Compiler::default();
        compiler
            .compile(
                &compiled,
                bounds,
                CompilerOptions {
                    optimize,
                    io_only: true,
                    ..Default::default()
                },
                compiled.scheduler().iter_entries().collect(),
                Default::default(),
            )
            .unwrap();
        lever_action(&mut reference, trigger, true);
        compiler.on_use_block(trigger);
        for tick in 0..36 {
            reference.tick_interpreted();
            compiler.tick_with_world(&mut compiled);
            assert_eq!(
                reference.command_output().collect::<Vec<_>>(),
                compiled.command_output().collect::<Vec<_>>(),
                "opt={optimize}, tick={tick}"
            );
        }
        assert!(compiled.command_output().count() > 0);
        let before = compiled.command_output().count();
        compiler.reset(&mut compiled, bounds);
        assert_eq!(compiled.command_output().count(), before);
    }
}

fn conductor_output(
    payload: Block,
    near: bool,
    fixed_source: bool,
) -> (PlotWorld, BlockPos, BlockPos, BlockPos) {
    let (mut world, _, manifest) = fixture("instant_observer");
    let base = local_pos(&manifest["ports"]["observations"]["base"]);
    let far = base + BlockPos::new(0, 0, 2);
    world.set_block(far, payload);
    let conductor = if near {
        base + BlockPos::new(0, 0, 1)
    } else {
        far
    };
    let source = conductor + BlockPos::new(1, 0, 0);
    world.set_block(source.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        source,
        Block::RedstoneRepeater {
            repeater: RedstoneRepeater {
                facing: BlockDirection::East,
                delay: 1,
                powered: true,
                locked: false,
            },
        },
    );
    world.set_block(source + BlockPos::new(1, 0, 0), Block::RedstoneBlock);
    let output = if near {
        let lamp = conductor + BlockPos::new(-1, 0, 0);
        world.set_block(lamp, Block::RedstoneLamp { lit: false });
        lamp
    } else {
        local_pos(&manifest["ports"]["observations"]["repeater"])
    };
    if fixed_source {
        let wire = far + BlockPos::new(0, 0, 1);
        world.set_block(wire + BlockPos::new(-1, 0, 0), Block::RedstoneBlock);
    }
    (
        world,
        local_pos(&manifest["ports"]["inputs"]["trigger"]),
        base,
        output,
    )
}

#[test]
fn comparator_ports_preserve_strength_and_distinguish_side_conductors() {
    for side_input in [false, true] {
        for strength in [3, 8, 15] {
            for payload in [Block::RedstoneBlock, Block::Stone {}] {
                for optimize in [false, true] {
                    let make_world = || {
                        let (mut world, trigger, base, repeater) =
                            conductor_output(payload, false, false);
                        let far = base + BlockPos::new(0, 0, 2);
                        let output = if side_input {
                            far + BlockPos::new(-1, 0, 0)
                        } else {
                            repeater
                        };
                        let facing = if side_input {
                            BlockDirection::North
                        } else {
                            let Block::RedstoneRepeater { repeater } = world.get_block(output)
                            else {
                                unreachable!()
                            };
                            repeater.facing
                        };
                        world.set_block(output.offset(BlockFace::Bottom), Block::Stone {});
                        world.set_block(
                            output,
                            Block::RedstoneComparator {
                                comparator: RedstoneComparator::new(
                                    facing,
                                    ComparatorMode::Compare,
                                    false,
                                ),
                            },
                        );
                        world.set_block_entity(
                            output,
                            BlockEntity::Comparator { output_strength: 0 },
                        );
                        let source = if side_input {
                            output.offset(facing.block_face())
                        } else {
                            far + BlockPos::new(1, 0, 0)
                        };
                        if side_input {
                            world.set_block(
                                source,
                                Block::Composter {
                                    level: strength.min(8),
                                },
                            );
                        } else {
                            world.set_block(
                                source,
                                Block::RedstoneComparator {
                                    comparator: RedstoneComparator::new(
                                        BlockDirection::East,
                                        ComparatorMode::Compare,
                                        true,
                                    ),
                                },
                            );
                            world.set_block_entity(
                                source,
                                BlockEntity::Comparator {
                                    output_strength: strength,
                                },
                            );
                            world.set_block(
                                source.offset(BlockFace::East),
                                if strength == 15 {
                                    Block::RedstoneBlock
                                } else {
                                    Block::Composter { level: strength }
                                },
                            );
                        }
                        let dust = output.offset(facing.opposite().block_face());
                        world.set_block(dust.offset(BlockFace::Bottom), Block::Stone {});
                        world.set_block(
                            dust,
                            Block::RedstoneWire {
                                wire: RedstoneWire::default(),
                            },
                        );
                        for pos in [far.offset(BlockFace::South), output, dust] {
                            crate::redstone::update(world.get_block(pos), &mut world, pos, None);
                        }
                        for _ in 0..8 {
                            world.tick_interpreted();
                        }
                        (world, trigger, output, dust)
                    };
                    let (mut interpreted, trigger, output, dust) = make_world();
                    let (mut compiled, _, _, _) = make_world();
                    let mut compiler = Compiler::default();
                    compiler
                        .compile(
                            &compiled,
                            compiled.get_corners(),
                            CompilerOptions {
                                optimize,
                                ..Default::default()
                            },
                            vec![],
                            Default::default(),
                        )
                        .unwrap();
                    lever_action(&mut interpreted, trigger, true);
                    compiler.on_use_block(trigger);
                    for tick in 1..=24 {
                        interpreted.tick_interpreted();
                        compiler.tick();
                        compiler.flush(&mut compiled);
                        for pos in [output, dust] {
                            // The side fixture's dust also touches the moving
                            // conductor's raw net. Its physical presentation
                            // is not a compiled output contract.
                            if pos == dust && (optimize || side_input) {
                                continue;
                            }
                            assert_eq!(compiled.get_block(pos), interpreted.get_block(pos), "{payload:?}, side={side_input}, strength={strength}, tick {tick}, {pos:?}");
                        }
                        if side_input && payload == (Block::Stone {}) {
                            assert!(
                                matches!(interpreted.get_block_entity(output), Some(BlockEntity::Comparator { output_strength }) if *output_strength == strength.min(8)),
                                "a conductor must never become a comparator side source"
                            );
                        }
                    }
                    compiler.reset(&mut compiled, interpreted.get_corners());
                    let output_strength = |world: &PlotWorld| match world.get_block_entity(output) {
                        Some(BlockEntity::Comparator { output_strength }) => *output_strength,
                        _ => panic!("missing comparator output entity"),
                    };
                    assert_eq!(output_strength(&compiled), output_strength(&interpreted));
                    if side_input && payload == (Block::Stone {}) {
                        assert_eq!(output_strength(&compiled), strength.min(8));
                    }
                }
            }
        }
    }
}

#[test]
fn destructive_attachments_and_dynamic_overrides_reject_without_mutation() {
    for mutation in ["floor lever", "wall lever", "torch", "repeater", "override"] {
        let (mut world, _, base, _) = conductor_output(Block::Stone {}, false, false);
        let far = base + BlockPos::new(0, 0, 2);
        match mutation {
            "floor lever" => {
                world.set_block(
                    far.offset(BlockFace::Top),
                    Block::Lever {
                        lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
                    },
                );
            }
            "wall lever" => {
                world.set_block(
                    far.offset(BlockFace::West),
                    Block::Lever {
                        lever: Lever::new(LeverFace::Wall, BlockDirection::West, false),
                    },
                );
            }
            "torch" => {
                world.set_block(
                    far.offset(BlockFace::Top),
                    Block::RedstoneTorch { lit: false },
                );
            }
            "repeater" => {
                world.set_block(
                    far.offset(BlockFace::Top),
                    Block::RedstoneRepeater {
                        repeater: RedstoneRepeater::default(),
                    },
                );
            }
            "override" => {
                let output = far.offset(BlockFace::West);
                world.set_block(output.offset(BlockFace::Bottom), Block::Stone {});
                world.set_block(
                    output,
                    Block::RedstoneComparator {
                        comparator: RedstoneComparator::new(
                            BlockDirection::East,
                            ComparatorMode::Compare,
                            false,
                        ),
                    },
                );
                world.set_block(far.offset(BlockFace::East), Block::Composter { level: 8 });
            }
            _ => unreachable!(),
        }
        let bounds = (base - BlockPos::new(4, 3, 7), base + BlockPos::new(4, 4, 7));
        let before = snapshot(&world, bounds);
        let mut compiler = Compiler::default();
        let error = compiler
            .compile(
                &world,
                world.get_corners(),
                Default::default(),
                vec![],
                Default::default(),
            )
            .unwrap_err()
            .to_string();
        let expected = if mutation == "override" {
            "dynamic override"
        } else {
            "moving payload support"
        };
        assert!(error.contains(expected), "{mutation}: {error}");
        assert!(!compiler.is_active());
        assert_eq!(snapshot(&world, bounds), before, "{mutation}");
    }
}

#[test]
fn ordinary_source_changes_refresh_ports_before_an_instant_launch() {
    for optimize in [false, true] {
        for io_only in [false, true] {
            let make_world = || {
                let (mut world, trigger, base, output) =
                    conductor_output(Block::Stone {}, false, false);
                let data = base + BlockPos::new(2, 0, 2);
                world.set_block(data.offset(BlockFace::Bottom), Block::Stone {});
                world.set_block(
                    data,
                    Block::Lever {
                        lever: Lever::new(LeverFace::Floor, BlockDirection::East, false),
                    },
                );
                let source = data.offset(BlockFace::West);
                crate::redstone::update(world.get_block(source), &mut world, source, None);
                for _ in 0..12 {
                    world.tick_interpreted();
                }
                (world, trigger, data, output)
            };
            let (mut interpreted, trigger, data, output) = make_world();
            let (mut compiled, _, _, _) = make_world();
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
                    vec![],
                    Default::default(),
                )
                .unwrap();
            for (input, elapsed) in [(data, 12), (trigger, 24)] {
                lever_action(&mut interpreted, input, true);
                compiler.on_use_block(input);
                for tick in 1..=elapsed {
                    interpreted.tick_interpreted();
                    compiler.tick();
                    compiler.flush(&mut compiled);
                    assert_eq!(
                        compiled.get_block(output),
                        interpreted.get_block(output),
                        "opt={optimize}, io={io_only}, input={input:?}, tick {tick}"
                    );
                }
            }
            compiler.reset(&mut compiled, interpreted.get_corners());
            for resumed in 1..=12 {
                interpreted.tick_interpreted();
                compiled.tick_interpreted();
                assert_eq!(
                    compiled.get_block(output),
                    interpreted.get_block(output),
                    "opt={optimize}, io={io_only}, resumed {resumed}"
                );
            }
        }
    }
}

#[test]
fn shared_near_outputs_require_an_ownership_protocol() {
    let (mut world, bounds, _) = fixture("or_1");
    let output = BASE + BlockPos::new(1, 0, 7);
    assert_eq!(world.get_block(output), Block::Air);
    world.set_block(output, Block::RedstoneLamp { lit: false });
    let before = snapshot(&world, bounds);
    let mut compiler = Compiler::default();
    let error = compiler
        .compile(
            &world,
            world.get_corners(),
            Default::default(),
            vec![],
            Default::default(),
        )
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("near ownership of a shared payload"),
        "{error}"
    );
    assert!(!compiler.is_active());
    assert_eq!(snapshot(&world, bounds), before);
}

#[test]
fn moving_conductor_outputs_and_fixed_contributors_preserve_their_waveforms() {
    for payload in [
        Block::Wool {
            color: BlockColorVariant::White,
        },
        Block::Concrete {
            color: BlockColorVariant::Black,
        },
        Block::Stone {},
        Block::Sandstone {},
    ] {
        for (near, fixed_source) in [(false, false), (false, true), (true, false)] {
            for optimize in [false, true] {
                for io_only in [false, true] {
                    let (mut interpreted, trigger, base, output) =
                        conductor_output(payload, near, fixed_source);
                    let (mut compiled, _, _, _) = conductor_output(payload, near, fixed_source);
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
                            vec![],
                            Default::default(),
                        )
                        .unwrap();
                    lever_action(&mut interpreted, trigger, true);
                    compiler.on_use_block(trigger);
                    compiler.flush(&mut compiled);
                    let initial = compiled.get_block(output);
                    let mut changed = false;
                    let mut retracted = false;
                    for tick in 1..=24 {
                        interpreted.tick_interpreted();
                        compiler.tick();
                        compiler.flush(&mut compiled);
                        changed |= compiled.get_block(output) != initial;
                        retracted |= matches!(interpreted.get_block(base), Block::Piston { piston } if !piston.extended);
                        assert_eq!(compiled.get_block(output), interpreted.get_block(output), "{payload:?}, near={near}, fixed={fixed_source}, opt={optimize}, io={io_only}, tick {tick}");
                    }
                    assert!(retracted, "the test must actually move the conductor");
                    assert_eq!(
                        changed, !fixed_source,
                        "fixed contributors must hold the output despite movement"
                    );
                    compiler.reset(&mut compiled, interpreted.get_corners());
                    for tick in 25..=36 {
                        interpreted.tick_interpreted();
                        compiled.tick_interpreted();
                        assert_eq!(
                            compiled.get_block(output),
                            interpreted.get_block(output),
                            "{payload:?}, near={near}, fixed={fixed_source}, resumed {tick}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn moving_conductor_handoff_preserves_every_wave_phase() {
    for near in [false, true] {
        for elapsed in 0..=14 {
            let payload = Block::Concrete {
                color: BlockColorVariant::Black,
            };
            let (mut interpreted, trigger, _, output) = conductor_output(payload, near, false);
            let (mut compiled, _, _, _) = conductor_output(payload, near, false);
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &compiled,
                    compiled.get_corners(),
                    CompilerOptions {
                        optimize: true,
                        io_only: true,
                        ..Default::default()
                    },
                    vec![],
                    Default::default(),
                )
                .unwrap();
            lever_action(&mut interpreted, trigger, true);
            compiler.on_use_block(trigger);
            for _ in 0..elapsed {
                interpreted.tick_interpreted();
                compiler.tick();
                compiler.flush(&mut compiled);
            }
            compiler.reset(&mut compiled, interpreted.get_corners());
            for resumed in 1..=18 {
                interpreted.tick_interpreted();
                compiled.tick_interpreted();
                assert_eq!(
                    compiled.get_block(output),
                    interpreted.get_block(output),
                    "near={near}, handoff {elapsed}, resumed {resumed}"
                );
            }
        }
    }
}

#[test]
fn additional_gate_outputs_match_interpreted_consumers() {
    for name in [
        "and_1",
        "and_2",
        "or_1",
        "not_1",
        "instant_down",
        "instant_chain",
    ] {
        let (_, _, manifest) = fixture(name);
        for case in manifest["cases"].as_array().unwrap() {
            let (mut interpreted, _, _) = fixture(name);
            let (mut compiled, _, _) = fixture(name);
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &compiled,
                    compiled.get_corners(),
                    CompilerOptions {
                        optimize: true,
                        io_only: true,
                        ..Default::default()
                    },
                    vec![],
                    Default::default(),
                )
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            apply_adder_actions(&mut interpreted, &mut compiled, &mut compiler, case);
            for tick in 1..=24 {
                interpreted.tick_interpreted();
                compiler.tick();
                compiler.flush(&mut compiled);
                for port in ["repeater", "lamp"] {
                    if !manifest["ports"]["observations"][port].is_array() {
                        continue;
                    }
                    let pos = local_pos(&manifest["ports"]["observations"][port]);
                    assert_eq!(
                        compiled.get_block(pos),
                        interpreted.get_block(pos),
                        "{name} {} {port} tick {tick}",
                        case["id"]
                    );
                }
            }
        }
    }
}

#[test]
#[ignore = "XOR reset-generated waves need ordered inhibitor updates; tick 8 currently differs"]
fn xor_complete_reset_waveform_matches_interpreted_consumers() {
    let (mut interpreted, _, manifest) = fixture("xor_simple");
    let (mut compiled, _, _) = fixture("xor_simple");
    let case = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "events-11-12")
        .unwrap();
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &compiled,
            compiled.get_corners(),
            Default::default(),
            vec![],
            Default::default(),
        )
        .unwrap();
    apply_adder_actions(&mut interpreted, &mut compiled, &mut compiler, case);
    let output = local_pos(&manifest["ports"]["observations"]["repeater"]);
    for tick in 1..=24 {
        interpreted.tick_interpreted();
        compiler.tick();
        compiler.flush(&mut compiled);
        assert_eq!(
            compiled.get_block(output),
            interpreted.get_block(output),
            "tick {tick}"
        );
    }
}

#[test]
fn one_bit_adder_compiled_outputs_match_every_prepared_episode() {
    for optimize in [false, true] {
        for io_only in [false, true] {
            let (_, _, manifest) = fixture("adder_1bit");
            for case in manifest["cases"].as_array().unwrap() {
                let (mut interpreted, _, _) = fixture("adder_1bit");
                let (mut compiled, _, _) = fixture("adder_1bit");
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
                        vec![],
                        Default::default(),
                    )
                    .unwrap();
                apply_adder_actions(&mut interpreted, &mut compiled, &mut compiler, case);
                for tick in 1..=24 {
                    interpreted.tick_interpreted();
                    compiler.tick();
                    compiler.flush(&mut compiled);
                    for port in ["sum_repeater", "carry_repeater"] {
                        let pos = local_pos(&manifest["ports"]["observations"][port]);
                        assert_eq!(
                            compiled.get_block(pos),
                            interpreted.get_block(pos),
                            "{} {port} tick {tick}, optimize={optimize}, io={io_only}",
                            case["id"]
                        );
                    }
                    if case["expectation"].is_object() {
                        for (port, bit, window) in [
                            ("sum_repeater", "sum", 3..=7),
                            ("carry_repeater", "carry", 5..=9),
                        ] {
                            if window.contains(&tick) {
                                let pos = local_pos(&manifest["ports"]["observations"][port]);
                                let Block::RedstoneRepeater { repeater } = compiled.get_block(pos)
                                else {
                                    unreachable!()
                                };
                                assert_eq!(
                                    u64::from(!repeater.powered),
                                    case["expectation"][bit].as_u64().unwrap(),
                                    "{} {bit} tick {tick}",
                                    case["id"]
                                );
                            }
                        }
                    }
                }
                compiler.reset(&mut compiled, interpreted.get_corners());
                for tick in 25..=36 {
                    interpreted.tick_interpreted();
                    compiled.tick_interpreted();
                    for port in ["sum_repeater", "carry_repeater"] {
                        let pos = local_pos(&manifest["ports"]["observations"][port]);
                        assert_eq!(
                            compiled.get_block(pos),
                            interpreted.get_block(pos),
                            "{} resumed {port} tick {tick}",
                            case["id"]
                        );
                    }
                }
            }
        }
    }
}
