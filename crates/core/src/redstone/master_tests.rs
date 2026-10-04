use super::*;
use crate::plot::{PlotWorld, PLOT_WIDTH};
use crate::redpiler::{Compiler, CompilerOptions};
use crate::world::storage::Chunk;
use mchprs_blocks::blocks::RedstoneRepeater;

fn world() -> PlotWorld {
    let chunks = (0..PLOT_WIDTH)
        .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
        .collect();
    PlotWorld::from_chunks(0, 0, chunks, Default::default())
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
        compiler.compile(
            &world,
            world.get_corners(),
            CompilerOptions::parse(options),
            ticks,
            Default::default(),
        );
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
        compiler.compile(
            &world,
            world.get_corners(),
            CompilerOptions::parse("-O -io"),
            Vec::new(),
            Default::default(),
        );
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
