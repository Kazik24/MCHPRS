use super::*;
use crate::plot::{PLOT_WIDTH, PlotWorld};
use crate::world::storage::Chunk;
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::{Block, ComparatorMode, Lever, LeverFace, RedstoneComparator, RedstoneObserver};
use mchprs_blocks::{BlockDirection, BlockFace, BlockFacing};

const BULB: BlockPos = BlockPos::new(40, 30, 40);
const INPUT: BlockPos = BlockPos::new(40, 30, 39);
const DIRECT: BlockPos = BlockPos::new(40, 30, 41);
const FAR: BlockPos = BlockPos::new(42, 30, 40);
const OBSERVER: BlockPos = BlockPos::new(39, 30, 40);

fn world() -> PlotWorld {
    let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
    PlotWorld::from_chunks(0, 0, chunks, Default::default())
}

fn circuit(name: &str, lit: bool, powered: bool) -> PlotWorld {
    let mut world = world();
    world.set_block(BULB, Block::from_name(name).unwrap().with_copper_bulb_state(lit, powered).unwrap());
    for pos in [INPUT, DIRECT, FAR] {
        world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
    }
    world.set_block(
        INPUT,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    world.set_block(BULB.offset(BlockFace::East), Block::Stone {});
    for (pos, facing) in [(DIRECT, BlockDirection::North), (FAR, BlockDirection::West)] {
        world.set_block(
            pos,
            Block::RedstoneComparator {
                comparator: RedstoneComparator::new(facing, ComparatorMode::Compare, false),
            },
        );
        world.set_block_entity(pos, BlockEntity::Comparator { output_strength: 0 });
        world.set_block(pos.offset(facing.opposite().block_face()), Block::RedstoneLamp { lit: false });
    }
    world.set_block(
        OBSERVER,
        Block::Observer {
            observer: RedstoneObserver {
                facing: BlockFacing::East,
                powered: false,
            },
        },
    );
    world
}

#[test]
fn every_bulb_state_has_opaque_cube_geometry_without_electrical_output() {
    let mut world = world();
    let mut count = 0;
    for &(name, _, first, last, _) in mchprs_blocks::generated::BLOCKS {
        if !name.ends_with("copper_bulb") {
            continue;
        }
        for id in first..=last {
            let block = Block::from_id(id);
            let (lit, powered) = block.copper_bulb_state().unwrap();
            count += 1;
            assert!(block.is_cube() && !block.is_solid() && !block.is_transparent());
            assert!(!block.has_block_entity());
            assert!(crate::redstone::has_neighbor_update(block));
            assert_eq!(block.with_copper_bulb_state(lit, powered), Some(block));
            world.set_block(BULB, block);
            assert_eq!(
                crate::redstone::comparator::get_override(block, &world, BULB),
                crate::redstone::bool_to_ss(lit)
            );
            for face in BlockFace::values() {
                assert_eq!(crate::redstone::get_redstone_power(block, &world, BULB, face), 0);
            }
            assert_eq!(block.with_copper_bulb_state(!lit, !powered).unwrap().get_name(), name);
        }
    }
    assert_eq!(count, 32);
    assert!(Block::Stone {}.copper_bulb_state().is_none());
}

#[test]
fn stale_callbacks_do_not_toggle_twice_or_schedule_a_bulb_tick() {
    let mut world = world();
    let bulb = Block::from_name("waxed_copper_bulb").unwrap();
    world.set_block(BULB, bulb);
    world.set_block(INPUT, Block::RedstoneBlock);
    for _ in 0..3 {
        crate::redstone::update(bulb, &mut world, BULB, None);
    }
    assert_eq!(world.get_block(BULB).copper_bulb_state(), Some((true, true)));
    assert!(!world.pending_tick_at(BULB));
    world.set_block(INPUT, Block::Air);
    crate::redstone::update(bulb, &mut world, BULB, None);
    assert_eq!(world.get_block(BULB).copper_bulb_state(), Some((true, false)));
    world.set_block(INPUT, Block::RedstoneBlock);
    crate::redstone::update(bulb, &mut world, BULB, None);
    assert_eq!(world.get_block(BULB).copper_bulb_state(), Some((false, true)));
}

#[test]
fn powered_placement_and_same_type_state_edits_have_distinct_behavior() {
    let mut world = world();
    world.set_block(INPUT, Block::RedstoneBlock);
    let bulb = Block::from_name("copper_bulb").unwrap();
    crate::interaction::place_in_world(bulb, &mut world, BULB, &None);
    assert_eq!(world.get_block(BULB).copper_bulb_state(), Some((true, true)));
    crate::interaction::place_in_world(bulb, &mut world, BULB, &None);
    assert_eq!(world.get_block(BULB).copper_bulb_state(), Some((false, false)));
}

#[test]
fn interpreter_bulbs_match_java_1_21_5() {
    let reference: serde_json::Value = serde_json::from_str(include_str!("../../../../../test_data/copper-bulbs/java-1.21.5.json")).unwrap();
    assert_eq!(reference["version"], "1.21.5");
    for case in reference["cases"].as_array().unwrap() {
        let mut world = circuit(
            case["variant"].as_str().unwrap(),
            case["lit"].as_bool().unwrap(),
            case["powered"].as_bool().unwrap(),
        );
        let mut powered = false;
        for sample in case["trace"].as_array().unwrap() {
            let target = sample["input"].as_bool().unwrap();
            if target != powered {
                let Block::Lever { mut lever } = world.get_block(INPUT) else {
                    unreachable!()
                };
                lever.powered = target;
                world.set_block(INPUT, Block::Lever { lever });
                crate::redstone::update_surrounding_blocks(&mut world, INPUT);
                crate::redstone::update_surrounding_blocks(&mut world, INPUT.offset(BlockFace::Bottom));
                powered = target;
            }
            for _ in 0..sample["ticks"].as_u64().unwrap() {
                world.tick_interpreted();
            }
            let (lit, powered) = world.get_block(BULB).copper_bulb_state().unwrap();
            let mut got = vec![serde_json::json!(lit), serde_json::json!(powered)];
            for pos in [DIRECT, FAR] {
                let Some(BlockEntity::Comparator { output_strength }) = world.get_block_entity(pos) else {
                    unreachable!()
                };
                got.push(serde_json::json!(output_strength));
            }
            got.push(serde_json::json!(
                matches!(world.get_block(OBSERVER), Block::Observer { observer } if observer.powered)
            ));
            assert_eq!(serde_json::Value::Array(got), sample["state"], "{case}");
        }
    }
}

#[test]
fn compiled_copper_bulb_is_rejected_without_changing_native_state() {
    use crate::redpiler::{Compiler, CompilerOptions};

    let mut world = circuit("copper_bulb", false, false);
    world.schedule_tick(DIRECT, 5, mchprs_world::TickPriority::Normal);
    let positions = [BULB, INPUT, DIRECT, FAR, OBSERVER];
    let blocks = positions.map(|pos| world.get_block(pos));
    let entities = [DIRECT, FAR].map(|pos| format!("{:?}", world.get_block_entity(pos)));
    let ticks = world.scheduler().iter_entries().collect::<Vec<_>>();
    let piston_state = format!("{:?}", world.piston_state());

    let mut compiler = Compiler::default();
    assert!(
        compiler
            .compile(&world, world.get_corners(), CompilerOptions::default(), ticks.clone(), Default::default())
            .is_err()
    );
    assert!(!compiler.is_active());
    assert_eq!(positions.map(|pos| world.get_block(pos)), blocks);
    assert_eq!([DIRECT, FAR].map(|pos| format!("{:?}", world.get_block_entity(pos))), entities);
    assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), ticks);
    assert_eq!(format!("{:?}", world.piston_state()), piston_state);
}

#[test]
fn copper_bulb_waxing_and_scraping_preserve_the_latch() {
    for &(name, _, first, last, _) in mchprs_blocks::generated::BLOCKS {
        if !name.ends_with("copper_bulb") {
            continue;
        }
        for state in first..=last {
            let block = Block::from_id(state);
            if name.starts_with("waxed_") {
                assert!(item_transform(block, "honeycomb").is_none());
                let (unwaxed, sound, effect) = item_transform(block, "iron_axe").unwrap();
                assert_eq!(unwaxed.get_name(), name.strip_prefix("waxed_").unwrap());
                assert_eq!(unwaxed.copper_bulb_state(), block.copper_bulb_state());
                assert_eq!((sound, effect), (Some("item.axe.wax_off"), 3004));
            } else {
                let (waxed, _, effect) = item_transform(block, "honeycomb").unwrap();
                assert_eq!(waxed.get_name(), format!("waxed_{name}"));
                assert_eq!(waxed.copper_bulb_state(), block.copper_bulb_state());
                assert_eq!(effect, 3003);
                if block.copper_oxidation().unwrap() > 0 {
                    let (scraped, sound, effect) = item_transform(block, "diamond_axe").unwrap();
                    assert_eq!(scraped.copper_oxidation(), Some(block.copper_oxidation().unwrap() - 1));
                    assert_eq!(scraped.copper_bulb_state(), block.copper_bulb_state());
                    assert_eq!((sound, effect), (Some("item.axe.scrape"), 3005));
                } else {
                    assert!(item_transform(block, "iron_axe").is_none());
                }
            }
            assert!(item_transform(block, "stick").is_none());
        }
    }
}

#[test]
fn copper_bulbs_toggle_on_the_aggregate_input_edge() {
    let second = BULB.offset(BlockFace::Top);
    let mut world = circuit("waxed_copper_bulb", false, false);
    world.set_block(
        second,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    for (pos, state) in [
        (INPUT, (true, true)),
        (second, (true, true)),
        (INPUT, (true, true)),
        (second, (true, false)),
        (INPUT, (false, true)),
        (INPUT, (false, false)),
    ] {
        let Block::Lever { mut lever } = world.get_block(pos) else {
            unreachable!()
        };
        lever.powered = !lever.powered;
        world.set_block(pos, Block::Lever { lever });
        crate::redstone::update_surrounding_blocks(&mut world, pos);
        assert_eq!(world.get_block(BULB).copper_bulb_state(), Some(state));
    }
}

#[test]
fn copper_bulb_variants_do_not_age_during_native_simulation() {
    for &(name, _, _, _, _) in mchprs_blocks::generated::BLOCKS {
        if !name.ends_with("copper_bulb") {
            continue;
        }
        let mut world = circuit(name, false, false);
        world.set_block(INPUT, Block::RedstoneBlock);
        crate::redstone::update_surrounding_blocks(&mut world, INPUT);
        for _ in 0..64 {
            world.tick_interpreted();
            assert_eq!(world.get_block(BULB).get_name(), name);
        }
        assert_eq!(world.get_block(BULB).copper_bulb_state(), Some((true, true)));
    }
}

#[test]
fn copper_bulb_light_updates_and_removal_cross_chunk_boundaries() {
    let mut world = world();
    let source = BlockPos::new(31, 30, 31);
    for (name, emission) in [
        ("copper_bulb", 15),
        ("exposed_copper_bulb", 12),
        ("weathered_copper_bulb", 8),
        ("oxidized_copper_bulb", 4),
        ("waxed_copper_bulb", 15),
    ] {
        let bulb = Block::from_name(name).unwrap().with_copper_bulb_state(true, false).unwrap();
        world.set_block(source, bulb);
        world.flush_block_changes();
        assert_eq!(world.bulb_light_at(source), emission);
        assert_eq!(world.bulb_light_at(source.offset(BlockFace::East)), emission - 1);
        assert_eq!(world.bulb_light_at(source.offset(BlockFace::South)), emission - 1);
        world.set_block(source.offset(BlockFace::East), Block::Stone {});
        world.flush_block_changes();
        assert_eq!(world.bulb_light_at(source.offset(BlockFace::East)), 0);
        world.set_block(source.offset(BlockFace::East), Block::Air);
        world.set_block(source, bulb.with_copper_bulb_state(false, true).unwrap());
        world.flush_block_changes();
        assert_eq!(world.bulb_light_at(source), 0);
        assert_eq!(world.bulb_light_at(source.offset(BlockFace::East)), 0);
    }
}

#[test]
fn copper_bulb_save_load_retains_states() {
    let mut chunk = Chunk::empty(2, 2);
    let mut states = Vec::new();
    for &(name, _, first, last, _) in mchprs_blocks::generated::BLOCKS {
        if name.ends_with("copper_bulb") {
            states.extend(first..=last);
        }
    }
    for (index, &state) in states.iter().enumerate() {
        chunk.set_block(index as u32 & 15, 30, index as u32 >> 4, state);
    }
    let saved = chunk.save();
    let loaded = Chunk::load(2, 2, saved);
    for (index, &state) in states.iter().enumerate() {
        assert_eq!(loaded.get_block(index as u32 & 15, 30, index as u32 >> 4), state);
    }
}

#[test]
fn copper_bulbs_move_physically_under_the_interpreter() {
    use mchprs_blocks::blocks::RedstonePiston;
    let mut world = world();
    let piston = BULB.offset(BlockFace::West);
    let bulb = Block::from_name("waxed_oxidized_copper_bulb")
        .unwrap()
        .with_copper_bulb_state(true, false)
        .unwrap();
    world.set_block(BULB, bulb);
    world.set_block(
        piston,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::East,
                sticky: false,
                extended: false,
            },
        },
    );
    world.set_block(piston.offset(BlockFace::North), Block::RedstoneBlock);
    crate::redstone::update(world.get_block(piston), &mut world, piston, None);
    for _ in 0..8 {
        world.tick_interpreted();
    }
    assert_eq!(world.get_block(BULB.offset(BlockFace::East)), bulb);
}
