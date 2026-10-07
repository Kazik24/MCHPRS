use super::*;
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::{
    ComparatorMode, Lever, LeverFace, RedstoneComparator, RedstoneRepeater, RedstoneWire,
};
use mchprs_blocks::{BlockColorVariant, BlockDirection};

#[test]
fn conditional_geometry_drives_command_outputs_without_replaying_chat_on_reset() {
    for optimize in [false, true] {
        let (mut world, trigger, _, output) = conductor_output(Block::Stone {}, true, false);
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
        let bounds = world.get_corners();
        let mut compiler = Compiler::default();
        compiler
            .compile(
                &world,
                bounds,
                CompilerOptions {
                    optimize,
                    io_only: true,
                    ..Default::default()
                },
                world.scheduler().iter_entries().collect(),
                Default::default(),
            )
            .unwrap();
        compiler.on_use_block(trigger);
        for _ in 0..36 {
            compiler.tick_with_world(&mut world);
        }
        assert_eq!(world.command_output().count(), 1);
        let before = world.command_output().count();
        for _ in 0..24 {
            compiler.tick_with_world(&mut world);
        }
        assert_eq!(
            world.command_output().count(),
            before,
            "a held level must not replay a command"
        );
        compiler.reset(&mut world, bounds);
        assert_eq!(world.command_output().count(), before);
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
            for payload in [
                Block::RedstoneBlock,
                Block::Stone {},
                Block::Quartz,
                Block::SmoothQuartz,
            ] {
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
                    let (mut compiled, trigger, output, dust) = make_world();
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
                    compiler.on_use_block(trigger);
                    for _ in 0..24 {
                        compiler.tick();
                        compiler.flush(&mut compiled);
                    }
                    // Retraction removes the mobile main/side supply. The fixed
                    // container remains the main input only in the side case.
                    let expected = if side_input { strength.min(8) } else { 0 };
                    let live_strength = |compiler: &Compiler| {
                        compiler
                            .backend
                            .as_ref()
                            .unwrap()
                            .ordinary_sources()
                            .into_iter()
                            .find(|&(pos, _)| pos == output)
                            .unwrap()
                            .1
                    };
                    assert_eq!(
                        live_strength(&compiler),
                        expected,
                        "{payload:?}, side={side_input}, strength={strength}"
                    );
                    // The side fixture's front dust joins the conditional raw
                    // output cone, so its display is owned and deferred to handoff.
                    if !optimize && !side_input {
                        assert!(matches!(compiled.get_block(dust),
                            Block::RedstoneWire { wire } if wire.power == expected),
                            "{payload:?}, side={side_input}, strength={strength}, dust={dust:?}, actual={:?}",
                            compiled.get_block(dust));
                    }
                    let held = compiled.get_block(dust);
                    for _ in 0..16 {
                        compiler.tick();
                        compiler.flush(&mut compiled);
                        assert_eq!(live_strength(&compiler), expected);
                        assert_eq!(compiled.get_block(dust), held);
                    }
                    let bounds = compiled.get_corners();
                    compiler.reset(&mut compiled, bounds);
                    // Comparator entity strength is materialized at handoff.
                    assert!(matches!(compiled.get_block_entity(output),
                        Some(BlockEntity::Comparator { output_strength }) if *output_strength == expected));
                    if side_input || !optimize {
                        assert!(matches!(compiled.get_block(dust),
                            Block::RedstoneWire { wire } if wire.power == expected),
                            "handoff {payload:?}, side={side_input}, strength={strength}, actual={:?}",
                            compiled.get_block(dust));
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
            "analog override through moving blocks"
        } else {
            "moving payload support"
        };
        assert!(error.contains(expected), "{mutation}: {error}");
        assert!(!compiler.is_active());
        assert_eq!(snapshot(&world, bounds), before, "{mutation}");
    }
}

#[test]
fn ordinary_source_changes_refresh_logical_ports() {
    for optimize in [false, true] {
        for io_only in [false, true] {
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
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &world,
                    world.get_corners(),
                    CompilerOptions {
                        optimize,
                        io_only,
                        ..Default::default()
                    },
                    vec![],
                    Default::default(),
                )
                .unwrap();
            for (input, powered) in [(data, true), (trigger, false)] {
                compiler.on_use_block(input);
                for _ in 0..16 {
                    compiler.tick();
                    compiler.flush(&mut world);
                }
                assert!(
                    matches!(world.get_block(output), Block::RedstoneRepeater { repeater } if repeater.powered == powered)
                );
                assert!(world.piston_state().events.is_empty());
                assert!(world.piston_state().motions.is_empty());
            }
        }
    }
}

#[test]
fn shared_near_outputs_use_deterministic_first_owner_geometry() {
    let mut saw_near = false;
    let mut saw_multiple_active = false;
    let mut recompile_errors = Vec::new();
    for assignment in 0..4 {
        let mut variants = Vec::new();
        for assume_instant in [false, true] {
            let (mut world, bounds, manifest) = fixture("or_1");
            let output = BASE + BlockPos::new(1, 0, 7);
            world.set_block(output, Block::RedstoneLamp { lit: false });
            let report = analyze_world(&world);
            let group = report
                .payload_groups
                .iter()
                .find(|group| group.members.len() > 1)
                .unwrap();
            let first = &report.pistons[group.members[0]];
            let far = first.head.offset(first.piston.facing.into());
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &world,
                    world.get_corners(),
                    CompilerOptions {
                        assume_instant,
                        optimize: true,
                        ..Default::default()
                    },
                    vec![],
                    Default::default(),
                )
                .unwrap();
            for (bit, input) in ["IN1", "IN2"].into_iter().enumerate() {
                let pos = local_pos(&manifest["ports"]["inputs"][input]);
                let Block::Lever { lever } = world.get_block(pos) else {
                    unreachable!()
                };
                if lever.powered != (assignment & (1 << bit) != 0) {
                    compiler.on_use_block(pos);
                }
            }
            for _ in 0..24 {
                compiler.tick();
                compiler.flush(&mut world);
            }
            let held = world.get_block(output);
            for _ in 0..16 {
                compiler.tick();
                compiler.flush(&mut world);
            }
            assert_eq!(world.get_block(output), held);
            compiler.reset(&mut world, bounds);
            assert_eq!(world.get_block(output), held);
            let material: Vec<_> = group
                .positions
                .iter()
                .copied()
                .filter(|&pos| world.get_block(pos) == Block::RedstoneBlock)
                .collect();
            assert_eq!(material.len(), 1);
            let first_active = group.members.iter().find(|&&actor| {
                matches!(world.get_block(report.pistons[actor].pos), Block::Piston { piston } if !piston.extended)
            });
            let expected = first_active.map_or(far, |&actor| report.pistons[actor].head);
            assert_eq!(
                material[0], expected,
                "shared payload must use the deterministic first active owner"
            );
            saw_near |= first_active.is_some();
            let active_count = group.members.iter().filter(|&&actor| {
                matches!(world.get_block(report.pistons[actor].pos), Block::Piston { piston } if !piston.extended)
            }).count();
            saw_multiple_active |= active_count > 1;
            assert!(world.piston_state().events.is_empty());
            assert!(world.piston_state().motions.is_empty());
            let restored = snapshot(&world, bounds);
            variants.push(restored.clone());
            let result = compiler.compile(
                &world,
                world.get_corners(),
                CompilerOptions {
                    assume_instant,
                    optimize: true,
                    ..Default::default()
                },
                world.scheduler().iter_entries().collect(),
                Default::default(),
            );
            assert_eq!(
                snapshot(&world, bounds),
                restored,
                "recompilation is read-only"
            );
            if let Err(error) = result {
                assert!(!compiler.is_active());
                recompile_errors.push(format!(
                    "assignment={assignment}, active={active_count}, assume_instant={assume_instant}: {error}"
                ));
                continue;
            }
            for _ in 0..8 {
                compiler.tick();
                compiler.flush(&mut world);
                assert_eq!(world.get_block(output), held);
            }
            compiler.reset(&mut world, bounds);
            let rerecorded = snapshot(&world, bounds);
            assert_eq!(rerecorded["cells"], restored["cells"]);
            assert_eq!(rerecorded["ticks"], restored["ticks"]);
            assert!(world.piston_state().events.is_empty());
            assert!(world.piston_state().motions.is_empty());
        }
        assert_eq!(variants[0], variants[1], "assignment {assignment}");
    }
    assert!(saw_near, "exercise shared near ownership");
    assert!(
        saw_multiple_active,
        "exercise both active shared-payload branches"
    );
    assert!(
        recompile_errors.is_empty(),
        "settled shared-payload recompilation failures:\n{}",
        recompile_errors.join("\n")
    );
}

#[test]
fn logical_conductor_outputs_preserve_material_and_fixed_contributors() {
    for payload in [
        Block::Wool {
            color: BlockColorVariant::White,
        },
        Block::Concrete {
            color: BlockColorVariant::Black,
        },
        Block::Stone {},
        Block::Sandstone {},
        Block::Quartz,
        Block::SmoothQuartz,
    ] {
        for (near, fixed_source) in [(false, false), (false, true), (true, false)] {
            for optimize in [false, true] {
                for io_only in [false, true] {
                    let (mut world, trigger, _, output) =
                        conductor_output(payload, near, fixed_source);
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
                            vec![],
                            Default::default(),
                        )
                        .unwrap();
                    compiler.on_use_block(trigger);
                    for _ in 0..24 {
                        compiler.tick();
                        compiler.flush(&mut world);
                    }
                    if near {
                        assert!(
                            matches!(world.get_block(output), Block::RedstoneLamp { lit: true }),
                            "{payload:?}"
                        );
                    } else {
                        assert!(
                            matches!(world.get_block(output), Block::RedstoneRepeater { repeater } if repeater.powered == fixed_source),
                            "{payload:?}, fixed={fixed_source}"
                        );
                    }
                    let held = world.get_block(output);
                    for _ in 0..16 {
                        compiler.tick();
                        compiler.flush(&mut world);
                    }
                    assert_eq!(world.get_block(output), held);
                    compiler.reset(&mut world, bounds);
                    assert_eq!(world.get_block(output), held);
                    assert!(world.piston_state().events.is_empty());
                    assert!(world.piston_state().motions.is_empty());
                }
            }
        }
    }
}

#[test]
fn certified_gates_use_the_same_logical_executor_with_and_without_trust() {
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
            let mut variants = Vec::new();
            for assume_instant in [false, true] {
                let (mut world, bounds, _) = fixture(name);
                let mut compiler = Compiler::default();
                compiler
                    .compile(
                        &world,
                        world.get_corners(),
                        CompilerOptions {
                            assume_instant,
                            optimize: true,
                            io_only: true,
                            ..Default::default()
                        },
                        vec![],
                        Default::default(),
                    )
                    .unwrap_or_else(|error| panic!("{name}: {error}"));
                assert!(!compiler
                    .backend
                    .as_ref()
                    .unwrap()
                    .logical_stats()
                    .is_empty());
                apply_adder_actions(&mut world, &mut compiler, case);
                for _ in 0..24 {
                    compiler.tick();
                    compiler.flush(&mut world);
                }
                let ports: Vec<_> = ["repeater", "lamp"]
                    .into_iter()
                    .filter_map(|port| {
                        let pos = &manifest["ports"]["observations"][port];
                        pos.is_array().then(|| local_pos(pos))
                    })
                    .collect();
                let held: Vec<_> = ports.iter().map(|&pos| world.get_block(pos)).collect();
                for _ in 0..16 {
                    compiler.tick();
                    compiler.flush(&mut world);
                }
                assert_eq!(
                    ports
                        .iter()
                        .map(|&pos| world.get_block(pos))
                        .collect::<Vec<_>>(),
                    held
                );
                compiler.reset(&mut world, bounds);
                assert!(world.piston_state().events.is_empty());
                assert!(world.piston_state().motions.is_empty());
                variants.push(snapshot(&world, bounds));
            }
            assert_eq!(variants[0], variants[1], "{name} {}", case["id"]);
        }
    }
}

#[test]
fn one_bit_adder_logical_outputs_match_every_prepared_assignment() {
    for optimize in [false, true] {
        for io_only in [false, true] {
            let (_, _, manifest) = fixture("adder_1bit");
            for case in manifest["cases"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|case| case["expectation"].is_object())
            {
                let (mut world, bounds, _) = fixture("adder_1bit");
                let mut compiler = Compiler::default();
                compiler
                    .compile(
                        &world,
                        world.get_corners(),
                        CompilerOptions {
                            optimize,
                            io_only,
                            ..Default::default()
                        },
                        vec![],
                        Default::default(),
                    )
                    .unwrap();
                apply_adder_actions(&mut world, &mut compiler, case);
                for _ in 0..16 {
                    compiler.tick();
                    compiler.flush(&mut world);
                }
                for _ in 0..8 {
                    for (port, bit) in [("sum_repeater", "sum"), ("carry_repeater", "carry")] {
                        assert_eq!(
                            regions::repeater_value(
                                &world,
                                &manifest["ports"]["observations"][port],
                                BlockPos::new(0, 0, 0)
                            ) as u64,
                            case["expectation"][bit].as_u64().unwrap(),
                            "{} {bit}",
                            case["id"]
                        );
                    }
                    compiler.tick();
                    compiler.flush(&mut world);
                }
                compiler.reset(&mut world, bounds);
                assert!(world.piston_state().events.is_empty());
                assert!(world.piston_state().motions.is_empty());
            }
        }
    }
}
