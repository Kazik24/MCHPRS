//! Small executable cases for material/context issues found in FPU and RILAX.
use super::*;
use mchprs_blocks::block_entities::{BlockEntity, ContainerType, SignalStrength};
use mchprs_blocks::blocks::{ComparatorMode, RedstoneComparator};
use mchprs_blocks::items::ItemStack;
use mchprs_blocks::BlockDirection;

fn container_inventory(ty: ContainerType, strength: u8) -> BlockEntity {
    let item = ItemStack::container_with_ss(ty, SignalStrength::new(strength).unwrap());
    if let Some(nbt::Value::Compound(entity)) =
        item.nbt.as_ref().and_then(|nbt| nbt.get("BlockEntityTag"))
    {
        BlockEntity::from_nbt(entity).unwrap()
    } else {
        BlockEntity::Container {
            ty,
            comparator_override: 0,
            inventory: Default::default(),
        }
    }
}

fn container_reset(
    kind: ContainerType,
    strength: u8,
) -> (PlotWorld, BlockPos, BlockPos, BlockPos, BlockPos) {
    let (mut world, _, manifest) = fixture("instant_observer");
    let base = local_pos(&manifest["ports"]["observations"]["base"]);
    let cap = base + BlockPos::new(0, 2, 0);
    world.set_block(
        cap,
        Block::from_name(kind.to_string().trim_start_matches("minecraft:")).unwrap(),
    );
    world.set_block_entity(cap, container_inventory(kind, strength));
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
fn fixed_container_context_preserves_inventory_override_and_logical_consumer_levels() {
    for (kind, strength) in [ContainerType::Furnace, ContainerType::Barrel]
        .into_iter()
        .flat_map(|kind| (0..=15).map(move |strength| (kind, strength)))
    {
        for optimize in [false, true] {
            for io_only in [false, true] {
                let (mut compiled, trigger, _, output, cap) = container_reset(kind, strength);
                let comparator = cap.offset(BlockFace::West);
                let lamp = comparator.offset(BlockFace::West);
                let inventory = json!(compiled.get_block_entity(cap));
                let material = compiled.get_block(cap);
                let (mut native, _, _, _, _) = container_reset(kind, strength);
                lever_action(&mut native, trigger, true);
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
                compiler.on_use_block(trigger);
                for tick in 1..=24 {
                    native.tick_interpreted();
                    compiler.tick();
                    compiler.flush(&mut compiled);
                    assert_eq!(compiled.get_block(cap), material);
                    assert_eq!(native.get_block(cap), material);
                    assert_eq!(json!(compiled.get_block_entity(cap)), inventory);
                    assert_eq!(json!(native.get_block_entity(cap)), inventory);
                    assert_eq!(
                        json!(compiled.get_block_entity(comparator)),
                        json!(native.get_block_entity(comparator))
                    );
                    assert_eq!(compiled.get_block(lamp), native.get_block(lamp));
                    assert_eq!(
                        compiled.get_block(output),
                        native.get_block(output),
                        "output waveform at tick {tick}, container {kind:?}, strength {strength}, optimize {optimize}, io_only {io_only}"
                    );
                }
                assert!(matches!(compiled.get_block_entity(comparator),
                    Some(BlockEntity::Comparator { output_strength }) if *output_strength == strength));
                assert!(
                    matches!(compiled.get_block(lamp), Block::RedstoneLamp { lit } if lit == (strength > 0))
                );
                assert_eq!(json!(compiled.get_block_entity(cap)), inventory);
                let bounds = compiled.get_corners();
                compiler.reset(&mut compiled, bounds);
                assert_eq!(json!(compiled.get_block_entity(cap)), inventory);
                assert_eq!(compiled.get_block(cap), material);
                assert!(matches!(compiled.get_block_entity(comparator),
                    Some(BlockEntity::Comparator { output_strength }) if *output_strength == strength));
                for _ in 0..12 {
                    native.tick_interpreted();
                    compiled.tick_interpreted();
                    assert_eq!(compiled.get_block(output), native.get_block(output));
                    assert_eq!(compiled.get_block(lamp), native.get_block(lamp));
                    assert_eq!(compiled.get_block(cap), material);
                    assert_eq!(json!(compiled.get_block_entity(cap)), inventory);
                    assert_eq!(
                        json!(compiled.get_block_entity(comparator)),
                        json!(native.get_block_entity(comparator))
                    );
                }
            }
        }
    }
}

#[test]
fn fixed_inventory_main_input_ignores_conditional_power_above_the_container() {
    for (kind, strength) in [ContainerType::Furnace, ContainerType::Barrel]
        .into_iter()
        .flat_map(|kind| [1, 6, 15].map(|strength| (kind, strength)))
    {
        for optimize in [false, true] {
            for io_only in [false, true] {
                let make_world = || {
                    let (mut world, _, manifest) = fixture("instant_observer");
                    let wire = local_pos(&manifest["ports"]["observations"]["raw_output"]);
                    let container = wire.offset(BlockFace::Bottom);
                    world.set_block(
                        container,
                        Block::from_name(kind.to_string().trim_start_matches("minecraft:"))
                            .unwrap(),
                    );
                    world.set_block_entity(container, container_inventory(kind, strength));
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
                let (mut compiled, trigger, comparator, wire) = make_world();
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
                let inventory = json!(compiled.get_block_entity(wire.offset(BlockFace::Bottom)));
                compiler.on_use_block(trigger);
                for _ in 0..24 {
                    compiler.tick();
                    compiler.flush(&mut compiled);
                }
                assert!(matches!(compiled.get_block_entity(comparator),
                    Some(BlockEntity::Comparator { output_strength }) if *output_strength == strength));
                let bounds = compiled.get_corners();
                compiler.reset(&mut compiled, bounds);
                assert_eq!(
                    json!(compiled.get_block_entity(wire.offset(BlockFace::Bottom))),
                    inventory
                );
                assert!(matches!(compiled.get_block_entity(comparator),
                    Some(BlockEntity::Comparator { output_strength }) if *output_strength == strength));
                for _ in 0..12 {
                    compiled.tick_interpreted();
                    assert_eq!(
                        json!(compiled.get_block_entity(wire.offset(BlockFace::Bottom))),
                        inventory
                    );
                    assert!(matches!(compiled.get_block_entity(comparator),
                        Some(BlockEntity::Comparator { output_strength }) if *output_strength == strength));
                }
            }
        }
    }
}

#[test]
fn fixed_container_requires_matching_inert_conducting_material() {
    let mut world = empty();
    for kind in [
        ContainerType::Furnace,
        ContainerType::Barrel,
        ContainerType::Chest,
        ContainerType::Hopper,
    ] {
        let block = Block::from_name(kind.to_string().trim_start_matches("minecraft:")).unwrap();
        world.set_block(BASE, block);
        world.set_block_entity(BASE, container_inventory(kind, 4));
        assert_eq!(
            families::fixed_container(&world, BASE),
            matches!(kind, ContainerType::Furnace | ContainerType::Barrel)
        );
        world.set_block_entity(BASE, BlockEntity::Comparator { output_strength: 4 });
        assert!(!families::fixed_container(&world, BASE));
        let wrong_kind = if kind == ContainerType::Furnace {
            ContainerType::Barrel
        } else {
            ContainerType::Furnace
        };
        world.set_block_entity(BASE, container_inventory(wrong_kind, 4));
        assert!(!families::fixed_container(&world, BASE));
    }
}

#[test]
fn container_support_exception_keeps_entity_movement_and_reset_guards() {
    use families::RecognitionFailure;
    for (kind, mutation) in [ContainerType::Furnace, ContainerType::Barrel]
        .into_iter()
        .flat_map(|kind| {
            ["wrong block", "wrong entity", "moving support", "pending"]
                .map(|mutation| (kind, mutation))
        })
    {
        let (mut world, _, base, _, cap) = container_reset(kind, 4);
        let expected = match mutation {
            "wrong block" => {
                world.set_block(cap, Block::Stone {});
                world.set_block_entity(cap, container_inventory(kind, 4));
                RecognitionFailure::BlockEntity { pos: cap }
            }
            "wrong entity" => {
                let mut entity = container_inventory(kind, 4);
                if let BlockEntity::Container { ty, .. } = &mut entity {
                    *ty = if kind == ContainerType::Furnace {
                        ContainerType::Barrel
                    } else {
                        ContainerType::Furnace
                    };
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
fn logical_conductors_accept_retained_geometry_and_reject_entities_and_bad_heads() {
    for mutation in ["entity", "missing head", "short head", "retracted"] {
        let (mut world, bounds, manifest) = fixture("instant_observer");
        let base = local_pos(&manifest["ports"]["observations"]["base"]);
        let head_pos = base.offset(BlockFace::South);
        let payload = head_pos.offset(BlockFace::South);
        world.set_block(payload, Block::Quartz);
        let expected = match mutation {
            "entity" => {
                world.set_block_entity(payload, container_inventory(ContainerType::Furnace, 1));
                "unsupported block entity in its movement area"
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
                "matching stationary head"
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
        if mutation == "retracted" {
            compiler
                .compile(
                    &world,
                    world.get_corners(),
                    Default::default(),
                    vec![],
                    Default::default(),
                )
                .unwrap();
            assert!(!compiler
                .backend
                .as_ref()
                .unwrap()
                .logical_stats()
                .is_empty());
            assert_eq!(snapshot(&world, bounds), before);
            let trigger = local_pos(&manifest["ports"]["inputs"]["trigger"]);
            compiler.on_use_block(trigger);
            for _ in 0..24 {
                compiler.tick();
            }
            compiler.reset(&mut world, bounds);
            assert!(!compiler.is_active());
            let payload_count = [head_pos, payload]
                .into_iter()
                .filter(|&pos| {
                    world.get_block(pos) == Block::Quartz
                        || matches!(world.get_block_entity(pos),
                            Some(BlockEntity::MovingPiston(entity))
                                if entity.block_state == Block::Quartz.get_id())
                })
                .count();
            assert_eq!(payload_count, 1);
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
