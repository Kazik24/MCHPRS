use super::*;
use crate::plot::{PLOT_WIDTH, PlotWorld};
use crate::redpiler::{Compiler, CompilerOptions};
use crate::world::storage::Chunk;
use mchprs_blocks::blocks::{Lever, LeverFace, RedstoneRepeater, RedstoneWire, SlabType};

fn world() -> PlotWorld {
    let chunks = (0..PLOT_WIDTH)
        .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
        .collect();
    PlotWorld::from_chunks(0, 0, chunks, Default::default())
}

#[test]
fn threshold_power_matches_analog_queries_for_every_registry_state_and_face() {
    let mut w = world();
    let pos = BlockPos::new(40, 30, 40);
    // The early positive strong input is followed by an out-of-range comparator
    // output. Numeric maxima must still inspect it and preserve the raw byte.
    for output_strength in [0, 255] {
        w.set_block_entity(pos, BlockEntity::Comparator { output_strength });
        for side in BlockFace::values() {
            let neighbor = pos.offset(side);
            w.set_block(
                neighbor,
                Block::RedstoneComparator {
                    comparator: mchprs_blocks::blocks::RedstoneComparator::new(
                        if side.is_horizontal() {
                            side.unwrap_direction()
                        } else {
                            BlockDirection::North
                        },
                        mchprs_blocks::blocks::ComparatorMode::Compare,
                        false,
                    ),
                },
            );
            w.set_block_entity(neighbor, BlockEntity::Comparator { output_strength });
        }
        if output_strength != 0 {
            w.set_block(
                pos.offset(BlockFace::Top),
                Block::RedstoneWire {
                    wire: RedstoneWire {
                        power: 15,
                        ..Default::default()
                    },
                },
            );
            assert_eq!(
                get_redstone_power(Block::Stone {}, &w, pos, BlockFace::Top),
                255
            );
        }
        for id in 0..mchprs_blocks::generated::STATE_PROPERTIES.len() as u32 {
            let block = Block::from_id(id);
            for side in BlockFace::values() {
                assert_eq!(
                    has_redstone_power(block, &w, pos, side),
                    get_redstone_power(block, &w, pos, side) > 0,
                    "state {id}, side {side:?}, comparator output {output_strength}",
                );
            }
        }
    }
}

#[test]
fn wire_shape_keeps_raw_cross_guard_and_climbing_connection() {
    use mchprs_blocks::blocks::RedstoneWireSide::{None, Side, Up};
    let mut w = world();
    let pos = BlockPos::new(40, 30, 40);
    let dot = RedstoneWire {
        power: 7,
        ..Default::default()
    };
    let cross = RedstoneWire {
        north: Side,
        south: Side,
        east: Side,
        west: Side,
        power: 7,
    };
    assert_eq!(wire::get_regulated_sides(dot, &w, pos), dot);
    assert_eq!(wire::get_regulated_sides(cross, &w, pos), cross);
    let north = pos.offset(BlockFace::North);
    w.set_block(north, Block::RedstoneWire { wire: dot });
    let straight = RedstoneWire {
        north: Side,
        south: Side,
        east: None,
        west: None,
        power: 7,
    };
    assert_eq!(wire::get_regulated_sides(dot, &w, pos), straight);
    // Recomputing the raw south side yields None, even though regulation adds
    // a south arm. The original cross must be retained in this case.
    assert_eq!(
        wire::on_neighbor_changed(cross, &w, pos, BlockFace::North),
        cross
    );
    assert_eq!(
        wire::on_neighbor_changed(cross, &w, pos, BlockFace::South),
        straight
    );
    w.set_block(north, Block::Stone {});
    w.set_block(
        north.offset(BlockFace::Top),
        Block::RedstoneWire { wire: dot },
    );
    let climbing = RedstoneWire {
        north: Up,
        ..straight
    };
    assert_eq!(
        wire::on_neighbor_changed(cross, &w, pos, BlockFace::South),
        climbing
    );
    w.set_block(pos.offset(BlockFace::Top), Block::Stone {});
    assert_eq!(wire::get_regulated_sides(dot, &w, pos), dot);
}

#[test]
fn repeater_short_pulse_ends_after_selected_delay() {
    for delay in 1..=4 {
        let mut world = world();
        let pos = BlockPos::new(40, 30, 40);
        world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
        let repeater = RedstoneRepeater {
            delay,
            facing: BlockDirection::West,
            locked: false,
            powered: false,
        };
        world.set_block(pos, Block::RedstoneRepeater { repeater });
        world.set_block(pos.offset(BlockFace::West), Block::RedstoneBlock {});
        update(world.get_block(pos), &mut world, pos, None);
        world.set_block(pos.offset(BlockFace::West), Block::Air);
        update(world.get_block(pos), &mut world, pos, None);
        let mut trace = Vec::new();
        for _ in 0..delay * 4 + 2 {
            world.tick_interpreted();
            trace.push(matches!(world.get_block(pos), Block::RedstoneRepeater { repeater } if repeater.powered));
        }
        assert_eq!(
            trace.iter().filter(|&&v| v).count(),
            delay as usize * 2,
            "delay {delay}: {trace:?}"
        );
        assert_eq!(trace.last(), Some(&false));
    }
}

#[test]
fn compiled_handoff_preserves_pending_short_pulse() {
    for options in ["", "-O", "-O -io"] {
        let mut world = world();
        let pos = BlockPos::new(40, 30, 40);
        world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(
            pos,
            Block::RedstoneRepeater {
                repeater: RedstoneRepeater {
                    delay: 2,
                    facing: BlockDirection::West,
                    locked: false,
                    powered: false,
                },
            },
        );
        let output = pos.offset(BlockFace::East);
        world.set_block(output, Block::RedstoneLamp { lit: false });
        // Input disappeared before the already scheduled on tick fired.
        world.schedule_tick(pos, 2, TickPriority::High);
        let ticks = world.scheduler().iter_entries().collect();
        let mut compiler = Compiler::default();
        compiler
            .compile(
                &world,
                world.get_corners(),
                CompilerOptions::parse(options).unwrap(),
                ticks,
                Default::default(),
            )
            .unwrap();
        let mut trace = Vec::new();
        for _ in 0..12 {
            compiler.tick();
            compiler.flush(&mut world);
            trace.push(world.get_block(output) == Block::RedstoneLamp { lit: true });
        }
        assert!(
            trace.contains(&true),
            "pulse was optimized away: {options}: {trace:?}"
        );
        assert_eq!(
            trace.last(),
            Some(&false),
            "pulse stuck on: {options}: {trace:?}"
        );
    }
}

#[test]
fn missing_container_entity_is_empty_and_frame_eye_is_full_signal() {
    let world = world();
    let pos = BlockPos::new(40, 30, 40);
    for name in ["barrel", "furnace", "hopper"] {
        let block = Block::from_name(name).unwrap();
        assert!(comparator::has_override(block));
        assert_eq!(comparator::get_override(block, &world, pos), 0);
    }
    let mut frame = Block::from_name("end_portal_frame").unwrap();
    assert!(comparator::has_override(frame));
    assert_eq!(comparator::get_override(frame, &world, pos), 0);
    frame.set_properties(std::collections::HashMap::from([("eye", "true")]));
    assert_eq!(comparator::get_override(frame, &world, pos), 15);
}

#[test]
fn ground_torch_never_weakly_powers_its_support() {
    let world = world();
    let pos = BlockPos::new(40, 30, 40);
    for side in BlockFace::values() {
        assert_eq!(
            get_weak_power(Block::RedstoneTorch { lit: true }, &world, pos, side, false),
            if side == BlockFace::Top { 0 } else { 15 }
        );
    }
}

#[test]
fn all_binary_plate_variants_power_and_survive_compiler_flush() {
    for name in [
        "stone_pressure_plate",
        "polished_blackstone_pressure_plate",
        "oak_pressure_plate",
        "spruce_pressure_plate",
        "birch_pressure_plate",
        "jungle_pressure_plate",
        "acacia_pressure_plate",
        "dark_oak_pressure_plate",
        "mangrove_pressure_plate",
        "bamboo_pressure_plate",
        "cherry_pressure_plate",
        "crimson_pressure_plate",
        "warped_pressure_plate",
        "pale_oak_pressure_plate",
    ] {
        let mut world = world();
        let pos = BlockPos::new(40, 30, 40);
        let plate = Block::from_name(name).unwrap();
        assert_eq!(plate.pressure_plate_powered(), Some(false), "{name}");
        let powered = plate.with_pressure_plate_power(true).unwrap();
        assert_eq!(powered.get_name(), name);
        assert_eq!(
            get_weak_power(powered, &world, pos, BlockFace::West, false),
            15
        );
        assert_eq!(
            get_strong_power(powered, &world, pos, BlockFace::Top, false),
            15
        );
        world.set_block(pos, plate);
        let mut compiler = Compiler::default();
        compiler
            .compile(
                &world,
                world.get_corners(),
                CompilerOptions::parse("-O -io").unwrap(),
                Vec::new(),
                Default::default(),
            )
            .unwrap();
        compiler.set_pressure_plate(pos, true);
        compiler.flush(&mut world);
        assert_eq!(
            world.get_block(pos).pressure_plate_powered(),
            Some(true),
            "{name}"
        );
        assert_eq!(world.get_block(pos).get_name(), name);
    }
}

#[test]
fn new_sign_species_require_support_and_keep_block_entities() {
    let mut world = world();
    let pos = BlockPos::new(40, 30, 40);
    for name in ["bamboo_sign", "cherry_sign", "mangrove_sign"] {
        let block = Block::from_name(name).unwrap();
        assert!(block.is_sign() && block.has_block_entity());
        assert!(!crate::interaction::is_valid_position(block, &world, pos));
        world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
        assert!(crate::interaction::is_valid_position(block, &world, pos));
        world.set_block(pos.offset(BlockFace::Bottom), Block::Air);
    }
}

#[test]
fn slab_tops_support_dust_without_conducting_to_the_lamp_below() {
    let mut world = world();
    let support = BlockPos::new(40, 30, 40);
    let dust = support.offset(BlockFace::Top);
    let lamp = support.offset(BlockFace::Bottom);
    world.set_block(dust.offset(BlockFace::East), Block::RedstoneBlock {});
    world.set_block(
        dust,
        Block::RedstoneWire {
            wire: RedstoneWire {
                power: 15,
                ..Default::default()
            },
        },
    );
    world.set_block(lamp, Block::RedstoneLamp { lit: false });

    for name in [
        "smooth_stone_slab",
        "quartz_slab",
        "oak_slab",
        "pale_oak_slab",
        "cut_copper_slab",
        "tuff_slab",
    ] {
        let slab = Block::from_name(name)
            .unwrap()
            .with_slab_type(SlabType::Top)
            .unwrap();
        world.set_block(support, slab);
        assert!(
            crate::interaction::is_valid_position(world.get_block(dust), &world, dust),
            "{name}"
        );
        assert_eq!(
            get_redstone_power(slab, &world, support, BlockFace::Bottom),
            0,
            "{name}"
        );
        assert!(!redstone_lamp_should_be_lit(&world, lamp), "{name}");
    }
    for name in ["iron_block", "dirt", "netherite_block"] {
        let block = Block::from_name(name).unwrap();
        world.set_block(support, block);
        assert_eq!(
            get_redstone_power(block, &world, support, BlockFace::Bottom),
            15,
            "{name}"
        );
        assert!(redstone_lamp_should_be_lit(&world, lamp), "{name}");
    }
}

#[test]
fn downward_piston_dust_step_only_transmits_upward() {
    use mchprs_blocks::blocks::RedstonePiston;
    // Exercise ordinary power calculation at placement and the subsequent Turbo
    // walk, for all horizontal directions and both piston variants/states.
    for direction in [
        BlockFace::North,
        BlockFace::South,
        BlockFace::West,
        BlockFace::East,
    ] {
        for sticky in [false, true] {
            for extended in [false, true] {
                for upward in [false, true] {
                    let mut world = world();
                    let step = BlockPos::new(40, 30, 40);
                    let high = step.offset(BlockFace::Top);
                    let low = step.offset(direction);
                    world.set_block(
                        step,
                        Block::Piston {
                            piston: RedstonePiston {
                                facing: BlockFacing::Down,
                                sticky,
                                extended,
                            },
                        },
                    );
                    world.set_block(low.offset(BlockFace::Bottom), Block::Stone {});
                    let (input, output) = if upward { (low, high) } else { (high, low) };
                    let source = if upward {
                        input.offset(direction)
                    } else {
                        input.offset(direction.opposite())
                    };
                    world.set_block(source, Block::RedstoneBlock {});
                    world.set_block(
                        input,
                        Block::RedstoneWire {
                            wire: wire::get_state_for_placement(&world, input),
                        },
                    );
                    world.set_block(
                        output,
                        Block::RedstoneWire {
                            wire: wire::get_state_for_placement(&world, output),
                        },
                    );
                    assert!(crate::interaction::is_valid_position(
                        world.get_block(high),
                        &world,
                        high
                    ));
                    let expected = if upward { 14 } else { 0 };
                    assert!(
                        matches!(world.get_block(output), Block::RedstoneWire { wire } if wire.power == expected),
                        "placement: {direction:?}, sticky={sticky}, extended={extended}, upward={upward}"
                    );
                    // Toggle the actual input, rather than merely inspecting
                    // prepowered dust; this exercises cached propagation too.
                    for power in [Block::Air, Block::RedstoneBlock {}, Block::Air] {
                        world.set_block(source, power);
                        update(world.get_block(input), &mut world, input, None);
                        assert_eq!(
                            get_redstone_power(
                                world.get_block(step),
                                &world,
                                step,
                                BlockFace::Bottom
                            ),
                            0,
                            "the piston must not conduct dust power through its base"
                        );
                        assert!(
                            matches!(world.get_block(output), Block::RedstoneWire { wire }
                            if wire.power == if power == Block::Air { 0 } else { expected }),
                            "Turbo: {direction:?}, sticky={sticky}, extended={extended}, upward={upward}, source={power:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn dust_steps_over_opaque_blocks_and_powers_the_block_below_in_both_backends() {
    for name in ["iron_block", "stone", "dirt", "netherite_block"] {
        for backend in ["interpreted", "", "-O", "-O -io"] {
            let mut world = world();
            let step = BlockPos::new(40, 30, 40);
            let high_dust = step.offset(BlockFace::Top);
            let low_dust = step.offset(BlockFace::West);
            let support = low_dust.offset(BlockFace::Bottom);
            let lamp = support.offset(BlockFace::Bottom);
            let source = high_dust.offset(BlockFace::East);
            world.set_block(step, Block::from_name(name).unwrap());
            world.set_block(
                source,
                Block::Lever {
                    lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
                },
            );
            world.set_block(
                high_dust,
                Block::RedstoneWire {
                    wire: RedstoneWire::default(),
                },
            );
            world.set_block(
                low_dust,
                Block::RedstoneWire {
                    wire: RedstoneWire::default(),
                },
            );
            world.set_block(support, Block::from_name(name).unwrap());
            world.set_block(lamp, Block::RedstoneLamp { lit: false });

            if backend == "interpreted" {
                world.set_block(
                    source,
                    Block::Lever {
                        lever: Lever::new(LeverFace::Floor, BlockDirection::North, true),
                    },
                );
                update_surrounding_blocks(&mut world, source);
                for _ in 0..4 {
                    world.tick_interpreted();
                }
            } else {
                let mut compiler = Compiler::default();
                compiler
                    .compile(
                        &world,
                        world.get_corners(),
                        CompilerOptions::parse(backend).unwrap(),
                        Vec::new(),
                        Default::default(),
                    )
                    .unwrap();
                compiler.on_use_block(source);
                for _ in 0..4 {
                    compiler.tick();
                }
                compiler.flush(&mut world);
            }
            if backend == "interpreted" || backend.is_empty() {
                assert!(
                    matches!(world.get_block(low_dust), Block::RedstoneWire { wire } if wire.power == 14),
                    "{name}: {backend}"
                );
            }
            assert_eq!(
                world.get_block(lamp),
                Block::RedstoneLamp { lit: true },
                "{name}: {backend}"
            );
        }
    }
}
