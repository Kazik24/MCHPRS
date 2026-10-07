//! Small executable cases for material/context issues found in FPU and RILAX.
use super::*;
use mchprs_blocks::block_entities::{BlockEntity, ContainerType, InventoryEntry};
use mchprs_blocks::blocks::{ComparatorMode, RedstoneComparator};
use mchprs_blocks::BlockDirection;

fn furnace_inventory(strength: u8) -> BlockEntity {
    let mut remaining = if strength == 0 {
        0
    } else {
        (u16::from(strength - 1) * 192).div_ceil(14).max(1)
    };
    let mut inventory = Vec::new();
    for slot in 0..3 {
        let count = remaining.min(64);
        if count > 0 {
            inventory.push(InventoryEntry {
                id: 1,
                slot,
                count: count as i8,
                nbt: None,
            });
            remaining -= count;
        }
    }
    BlockEntity::Container {
        comparator_override: strength,
        inventory: inventory.into(),
        ty: ContainerType::Furnace,
    }
}

fn furnace_reset(strength: u8) -> (PlotWorld, BlockPos, BlockPos, BlockPos, BlockPos) {
    let (mut world, _, manifest) = fixture("instant_observer");
    let base = local_pos(&manifest["ports"]["observations"]["base"]);
    let cap = base + BlockPos::new(0, 2, 0);
    world.set_block(
        cap,
        Block::Furnace {
            facing: BlockDirection::North,
            lit: false,
        },
    );
    world.set_block_entity(cap, furnace_inventory(strength));
    let comparator = cap.offset(BlockFace::West);
    world.set_block(comparator.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        comparator,
        Block::RedstoneComparator {
            comparator: RedstoneComparator::new(
                BlockDirection::East,
                ComparatorMode::Compare,
                false,
            ),
        },
    );
    world.set_block_entity(comparator, BlockEntity::Comparator { output_strength: 0 });
    world.set_block(
        comparator.offset(BlockFace::West),
        Block::RedstoneLamp { lit: false },
    );
    crate::redstone::update(world.get_block(comparator), &mut world, comparator, None);
    for _ in 0..8 {
        world.tick_interpreted();
    }
    (
        world,
        local_pos(&manifest["ports"]["inputs"]["trigger"]),
        base,
        local_pos(&manifest["ports"]["observations"]["repeater"]),
        cap,
    )
}

#[test]
fn fixed_furnace_reset_preserves_inventory_override_and_consumer_waveforms() {
    for strength in [0, 1, 4, 6, 12, 15] {
        for optimize in [false, true] {
            for io_only in [false, true] {
                let (mut reference, trigger, base, output, cap) = furnace_reset(strength);
                let (mut compiled, _, _, _, _) = furnace_reset(strength);
                let comparator = cap.offset(BlockFace::West);
                let lamp = comparator.offset(BlockFace::West);
                let inventory = json!(compiled.get_block_entity(cap));
                let report = analyze_world(&compiled);
                assert!(report.recognition[0].is_matched());
                assert!(report
                    .ports
                    .reset_exposures
                    .iter()
                    .all(|e| e.consumer != comparator));
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
                lever_action(&mut reference, trigger, true);
                compiler.on_use_block(trigger);
                let mut retracted = false;
                for tick in 1..=24 {
                    reference.tick_interpreted();
                    compiler.tick();
                    compiler.flush(&mut compiled);
                    retracted |= matches!(reference.get_block(base), Block::Piston { piston } if !piston.extended);
                    for pos in [output, comparator, lamp] {
                        assert_eq!(compiled.get_block(pos), reference.get_block(pos), "strength={strength}, opt={optimize}, io={io_only}, tick={tick}, {pos:?}");
                    }
                    assert!(
                        matches!(compiled.get_block_entity(comparator), Some(BlockEntity::Comparator { output_strength }) if *output_strength == strength)
                    );
                    assert_eq!(json!(compiled.get_block_entity(cap)), inventory);
                }
                assert!(retracted, "the reset mechanism must actually fire");
                compiler.reset(&mut compiled, reference.get_corners());
                assert_eq!(json!(compiled.get_block_entity(cap)), inventory);
                assert_eq!(compiled.get_block(cap), reference.get_block(cap));
                for tick in 25..=36 {
                    reference.tick_interpreted();
                    compiled.tick_interpreted();
                    for pos in [output, comparator, lamp] {
                        assert_eq!(compiled.get_block(pos), reference.get_block(pos), "handoff strength={strength}, opt={optimize}, io={io_only}, tick={tick}");
                    }
                    assert_eq!(json!(compiled.get_block_entity(cap)), inventory);
                }
            }
        }
    }
}

#[test]
fn fixed_inventory_main_input_ignores_conditional_power_above_the_container() {
    for strength in [1, 6, 15] {
        for optimize in [false, true] {
            for io_only in [false, true] {
                let make_world = || {
                    let (mut world, _, manifest) = fixture("instant_observer");
                    let wire = local_pos(&manifest["ports"]["observations"]["raw_output"]);
                    let container = wire.offset(BlockFace::Bottom);
                    world.set_block(
                        container,
                        Block::Furnace {
                            facing: BlockDirection::North,
                            lit: false,
                        },
                    );
                    world.set_block_entity(container, furnace_inventory(strength));
                    let comparator = container.offset(BlockFace::West);
                    world.set_block(comparator.offset(BlockFace::Bottom), Block::Stone {});
                    world.set_block(
                        comparator,
                        Block::RedstoneComparator {
                            comparator: RedstoneComparator::new(
                                BlockDirection::East,
                                ComparatorMode::Compare,
                                false,
                            ),
                        },
                    );
                    world.set_block_entity(
                        comparator,
                        BlockEntity::Comparator { output_strength: 0 },
                    );
                    world.set_block(
                        comparator.offset(BlockFace::West),
                        Block::RedstoneLamp { lit: false },
                    );
                    crate::redstone::update(
                        world.get_block(comparator),
                        &mut world,
                        comparator,
                        None,
                    );
                    for _ in 0..8 {
                        world.tick_interpreted();
                    }
                    (
                        world,
                        local_pos(&manifest["ports"]["inputs"]["trigger"]),
                        comparator,
                        wire,
                    )
                };
                let (mut reference, trigger, comparator, wire) = make_world();
                let (mut compiled, _, _, _) = make_world();
                let report = analyze_world(&compiled);
                assert!(report
                    .ports
                    .outputs
                    .iter()
                    .all(|port| port.consumer != comparator
                        || port.input != ports::ConsumerInput::Main));
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
                lever_action(&mut reference, trigger, true);
                compiler.on_use_block(trigger);
                let mut lost_power = false;
                for tick in 1..=24 {
                    reference.tick_interpreted();
                    compiler.tick();
                    compiler.flush(&mut compiled);
                    lost_power |= matches!(reference.get_block(wire), Block::RedstoneWire { wire } if wire.power == 0);
                    assert_eq!(
                        compiled.get_block(comparator),
                        reference.get_block(comparator),
                        "strength={strength}, opt={optimize}, io={io_only}, tick={tick}"
                    );
                    assert!(
                        matches!(compiled.get_block_entity(comparator), Some(BlockEntity::Comparator { output_strength }) if *output_strength == strength)
                    );
                }
                assert!(
                    lost_power,
                    "the container's electrical power must actually change"
                );
                compiler.reset(&mut compiled, reference.get_corners());
                assert_eq!(
                    json!(compiled.get_block_entity(wire.offset(BlockFace::Bottom))),
                    json!(reference.get_block_entity(wire.offset(BlockFace::Bottom)))
                );
            }
        }
    }
}

#[test]
fn furnace_support_exception_keeps_entity_movement_and_reset_guards() {
    use families::RecognitionFailure;
    for mutation in [
        "wrong block",
        "wrong entity",
        "moving support",
        "pending",
    ] {
        let (mut world, _, base, _, cap) = furnace_reset(4);
        let expected = match mutation {
            "wrong block" => {
                world.set_block(cap, Block::Stone {});
                world.set_block_entity(cap, furnace_inventory(4));
                RecognitionFailure::BlockEntity { pos: cap }
            }
            "wrong entity" => {
                let mut entity = furnace_inventory(4);
                if let BlockEntity::Container { ty, .. } = &mut entity {
                    *ty = ContainerType::Barrel;
                }
                world.set_block_entity(cap, entity);
                RecognitionFailure::BlockEntity { pos: cap }
            }
            "moving support" => {
                let owner = cap + BlockPos::new(-2, 0, 0);
                world.set_block(
                    owner,
                    Block::Piston {
                        piston: RedstonePiston {
                            facing: BlockFacing::East,
                            sticky: true,
                            extended: true,
                        },
                    },
                );
                world.set_block(
                    owner.offset(BlockFace::East),
                    Block::PistonHead {
                        head: RedstonePistonHead {
                            facing: BlockFacing::East,
                            sticky: true,
                            short: false,
                        },
                    },
                );
                RecognitionFailure::MovableSupport { pos: cap }
            }
            "writer" => {
                let pos = cap.offset(BlockFace::East);
                world.set_block(
                    pos,
                    Block::Lever {
                        lever: mchprs_blocks::blocks::Lever::new(
                            mchprs_blocks::blocks::LeverFace::Wall,
                            BlockDirection::East,
                            false,
                        ),
                    },
                );
                RecognitionFailure::AdditionalResetWriter { pos }
            }
            "pending" => {
                world.schedule_tick(base.offset(BlockFace::Top), 2, TickPriority::Normal);
                RecognitionFailure::PendingReset {
                    pos: base.offset(BlockFace::Top),
                }
            }
            _ => unreachable!(),
        };
        let report = analyze_world(&world);
        let actor = report.pistons.iter().position(|p| p.pos == base).unwrap();
        assert!(
            report.recognition[actor].failures.contains(&expected),
            "{mutation}: {:?}",
            report.recognition[actor].failures
        );
        let bounds = (base - BlockPos::new(4, 2, 6), base + BlockPos::new(4, 5, 6));
        let before = snapshot(&world, bounds);
        let mut compiler = Compiler::default();
        assert!(
            compiler
                .compile(
                    &world,
                    world.get_corners(),
                    Default::default(),
                    world.scheduler().iter_entries().collect(),
                    Default::default()
                )
                .is_err(),
            "{mutation}"
        );
        assert!(!compiler.is_active());
        assert_eq!(snapshot(&world, bounds), before, "{mutation}");
    }
}

#[test]
fn sampled_conductors_preserve_saved_geometry_and_reject_entities_and_bad_heads() {
    for mutation in ["entity", "missing head", "short head", "retracted"] {
        let (mut world, bounds, manifest) = fixture("instant_observer");
        let base = local_pos(&manifest["ports"]["observations"]["base"]);
        let head_pos = base.offset(BlockFace::South);
        let payload = head_pos.offset(BlockFace::South);
        world.set_block(payload, Block::Quartz);
        let expected = match mutation {
            "entity" => {
                world.set_block_entity(payload, furnace_inventory(1));
                "moving-context entity"
            }
            "missing head" => {
                world.set_block(head_pos, Block::Air);
                "matching stationary head"
            }
            "short head" => {
                let Block::PistonHead { mut head } = world.get_block(head_pos) else {
                    unreachable!()
                };
                head.short = true;
                world.set_block(head_pos, Block::PistonHead { head });
                "incompatible saved head"
            }
            "retracted" => {
                let Block::Piston { mut piston } = world.get_block(base) else {
                    unreachable!()
                };
                piston.extended = false;
                world.set_block(base, Block::Piston { piston });
                world.set_block(head_pos, Block::Quartz);
                world.set_block(payload, Block::Air);
                "ready, powered, extended"
            }
            _ => unreachable!(),
        };
        let before = snapshot(&world, bounds);
        let mut compiler = Compiler::default();
        if matches!(mutation, "missing head" | "retracted") {
            let mut reference = empty();
            crate::world::for_each_block_optimized(&world, bounds.0, bounds.1, |pos| {
                reference.set_block(pos, world.get_block(pos));
                if let Some(entity) = world.get_block_entity(pos) { reference.set_block_entity(pos, entity.clone()); }
            });
            compiler.compile(&world, world.get_corners(), Default::default(), vec![], Default::default()).unwrap();
            assert_eq!(snapshot(&world, bounds), before);
            let trigger = local_pos(&manifest["ports"]["inputs"]["trigger"]);
            compiler.on_use_block(trigger);
            lever_action(&mut reference, trigger, true);
            for _ in 0..24 {
                compiler.tick();
                reference.tick_interpreted();
                check_sampled_state(&compiler, &reference);
            }
            compiler.reset(&mut world, reference.get_corners());
            for tick in 0..24 {
                world.tick_interpreted();
                reference.tick_interpreted();
                let mut differences = Vec::new();
                crate::world::for_each_block_optimized(&reference, bounds.0, bounds.1, |pos| {
                    if world.get_block(pos) != reference.get_block(pos) { differences.push((pos, world.get_block(pos), reference.get_block(pos))); }
                });
                assert!(differences.is_empty(), "handoff {mutation}, tick {tick}: {differences:?}");
            }
            continue;
        }
        let message = compiler
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
            message.contains(expected) && message.contains(&format!("{base:?}")),
            "{mutation}: {message}"
        );
        if mutation.contains("head") {
            assert!(message.contains(&format!("{head_pos:?}")), "{message}");
        }
        assert!(!compiler.is_active());
        assert_eq!(snapshot(&world, bounds), before, "{mutation}");
    }
}
