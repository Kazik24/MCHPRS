//! Deterministic differential fuzzing. MCHPRS_REDSTONE_FUZZ_SEED selects the first
//! case; MCHPRS_REDSTONE_FUZZ_CASES sets the count (defaults: 0 and 64).
//! Adapted from e8ea360's analysis/tests/redstone_fuzz.rs for the sampled backend.
use super::*;
use crate::plot::{PLOT_WIDTH, PlotWorld};
use crate::world::{World, storage::Chunk};
use mchprs_blocks::BlockDirection;
use mchprs_blocks::BlockFace;
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::{ComparatorMode, Lever, LeverFace, RedstoneComparator, RedstoneRepeater, RedstoneWire};
use rand::{RngExt, SeedableRng, rngs::StdRng};

const WIDTH: i32 = 8;
const DEPTH: i32 = 6;
const BASE: BlockPos = BlockPos::new(8, 8, 8);

fn empty() -> PlotWorld {
    PlotWorld::from_chunks(
        0,
        0,
        (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
        Default::default(),
    )
}

fn lever_action(world: &mut PlotWorld, pos: BlockPos, powered: bool) {
    if matches!(world.get_block(pos), Block::Lever { lever } if lever.powered != powered) {
        crate::interaction::use_redstone(world.get_block(pos), world, pos, None);
    }
}

fn comparator_output(world: &PlotWorld, pos: BlockPos) -> u8 {
    let Some(BlockEntity::Comparator { output_strength }) = world.get_block_entity(pos) else {
        panic!("missing comparator entity at {pos:?}")
    };
    *output_strength
}

// shortcut: flat layouts and lever inputs only, expand for vertical wiring and other controls.
fn circuit(rng: &mut StdRng) -> Vec<(BlockPos, Block)> {
    let mut blocks = Vec::new();
    for z in 0..DEPTH {
        for x in 0..WIDTH {
            let pos = BASE + BlockPos::new(x, 0, z);
            let block = if x == 0 {
                Block::Lever {
                    lever: Lever::new(LeverFace::Floor, BlockDirection::North, rng.random()),
                }
            } else if x == WIDTH - 1 {
                Block::RedstoneLamp { lit: false }
            } else {
                let facing = BlockDirection::from_id(rng.random_range(0..4));
                match rng.random_range(0..10) {
                    0 => Block::Air,
                    1 => Block::RedstoneTorch { lit: true },
                    2 | 3 => Block::RedstoneRepeater {
                        repeater: RedstoneRepeater {
                            facing,
                            delay: rng.random_range(1..=4),
                            powered: false,
                            locked: false,
                        },
                    },
                    4 | 5 => Block::RedstoneComparator {
                        comparator: RedstoneComparator::new(
                            facing,
                            if rng.random() {
                                ComparatorMode::Compare
                            } else {
                                ComparatorMode::Subtract
                            },
                            false,
                        ),
                    },
                    _ => Block::RedstoneWire {
                        wire: RedstoneWire::default(),
                    },
                }
            };
            blocks.push((pos, block));
        }
    }
    blocks
}

fn load(blocks: &[(BlockPos, Block)], warmup: u32) -> PlotWorld {
    let mut world = empty();
    for &(pos, block) in blocks {
        world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(pos, block);
        if matches!(block, Block::RedstoneComparator { .. }) {
            world.set_block_entity(pos, BlockEntity::Comparator { output_strength: 0 });
        }
    }
    // Derive dust connections from the complete geometry before delivering updates.
    for &(pos, block) in blocks {
        if let Block::RedstoneWire { wire } = block {
            let wire = crate::redstone::wire::get_regulated_sides(wire, &world, pos);
            world.set_block(pos, Block::RedstoneWire { wire });
        }
    }
    for &(pos, _) in blocks {
        crate::redstone::update(world.get_block(pos), &mut world, pos, None);
    }
    for _ in 0..warmup {
        world.tick_interpreted();
    }
    world
}

fn compare(seed: u64) -> Result<(), String> {
    let mut rng = StdRng::seed_from_u64(seed);
    let blocks = circuit(&mut rng);
    let warmup = rng.random_range(0..=16);
    let actions: Vec<_> = (0..64)
        .map(|_| (rng.random_range(0..DEPTH), rng.random::<bool>(), rng.random_range(0..=8)))
        .collect();
    let mut native = load(&blocks, warmup);
    let mut compiled = load(&blocks, warmup);
    let bounds = (BASE + BlockPos::new(-2, -2, -2), BASE + BlockPos::new(WIDTH + 2, 2, DEPTH + 2));
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &compiled,
            bounds,
            CompilerOptions::default(),
            compiled.scheduler().iter_entries().collect(),
            Default::default(),
        )
        .map_err(|error| format!("seed={seed} compile error: {error}"))?;
    compiled.clear_scheduled_ticks();

    for (step, &(z, powered, wait)) in actions.iter().enumerate() {
        let input = BASE + BlockPos::new(0, 0, z);
        lever_action(&mut native, input, powered);
        if matches!(compiled.get_block(input), Block::Lever { lever } if lever.powered != powered) {
            compiler.on_use_block(input);
        }
        for tick in 0..=wait {
            if tick > 0 {
                native.tick_interpreted();
                compiler.tick().unwrap();
            }
            compiler.try_flush_complete(&mut compiled).unwrap();
            for &(pos, block) in &blocks {
                // Compare dust as well as receiver state after every input and tick.
                if !matches!(block, Block::Air) {
                    let actual = compiled.get_block(pos);
                    let expected = native.get_block(pos);
                    if actual != expected {
                        return Err(format!(
                            "seed={seed} warmup={warmup} step={step} tick={tick} input=({z},{powered}) pos={pos:?} actual={actual:?} expected={expected:?}; circuit={blocks:?}; actions={:?}",
                            &actions[..=step],
                        ));
                    }
                }
                if matches!(block, Block::RedstoneComparator { .. }) {
                    assert_eq!(
                        comparator_output(&compiled, pos),
                        comparator_output(&native, pos),
                        "seed={seed} step={step} tick={tick} comparator entity at {pos:?}"
                    );
                }
            }
        }
    }
    compiler.try_reset(&mut compiled, bounds).unwrap();
    for tick in 1..=12 {
        native.tick_interpreted();
        compiled.tick_interpreted();
        for &(pos, _) in &blocks {
            assert_eq!(
                compiled.get_block(pos),
                native.get_block(pos),
                "seed={seed} handoff tick={tick} pos={pos:?}"
            );
        }
    }
    for &(pos, block) in &blocks {
        if matches!(block, Block::RedstoneComparator { .. }) {
            let actual = comparator_output(&compiled, pos);
            let expected = comparator_output(&native, pos);
            if actual != expected {
                return Err(format!(
                    "seed={seed} comparator strength at {pos:?} actual={actual} expected={expected}; circuit={blocks:?}; actions={actions:?}",
                ));
            }
        }
    }
    Ok(())
}

#[test]
fn normal_redstone_matches_interpreter() {
    let first_seed = std::env::var("MCHPRS_REDSTONE_FUZZ_SEED")
        .map(|value| value.parse::<u64>().expect("invalid fuzz seed"))
        .unwrap_or(0);
    let cases = std::env::var("MCHPRS_REDSTONE_FUZZ_CASES")
        .map(|value| value.parse::<u64>().expect("invalid fuzz case count"))
        .unwrap_or(64);
    assert!(cases > 0, "the fuzz campaign must execute at least one case");

    let mut failures = 0;
    for offset in 0..cases {
        let seed = first_seed.wrapping_add(offset);
        if let Err(error) = compare(seed) {
            eprintln!("{error}");
            failures += 1;
        }
        if (offset + 1) % 1000 == 0 {
            eprintln!("Fuzz progress: {} circuits checked, {failures} failing runs", offset + 1);
        }
    }
    eprintln!("Fuzz summary: {cases} circuits, {failures} failing runs");
    assert_eq!(failures, 0, "see the replayable failures above");
}

#[test]
fn comparator_keeps_its_native_fifo_position() {
    compare(10).unwrap();
}

#[test]
#[ignore = "known context-dependent comparator ordering mismatch; seed 410 reproduces"]
fn comparator_order_with_other_active_analog_sources() {
    compare(410).unwrap();
}

#[test]
#[ignore = "exports the saved Chungus fixture for replay"]
fn export_chungus_case() {
    #[path = "../../test_support/chungus.rs"]
    mod chungus;
    use serde_json::json;
    let state = |block: Block| {
        let properties: std::collections::BTreeMap<_, _> = block.properties().into_iter().collect();
        format!(
            "{}[{}]",
            block.get_name(),
            properties
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join(",")
        )
    };
    let mut world = chungus::init_world();
    let bounds = world.get_corners();
    let mut snapshot = Vec::new();
    let mut comparators = Vec::new();
    crate::world::for_each_block_optimized(&world, bounds.0, bounds.1, |pos| {
        let block = world.get_block(pos);
        if block != Block::Air {
            snapshot.push(json!({"pos":[pos.x,pos.y,pos.z],"state":state(block)}));
        }
        if matches!(block, Block::RedstoneComparator { .. }) {
            comparators.push(json!({"pos":[pos.x,pos.y,pos.z],"output":comparator_output(&world,pos)}));
        }
    });
    let pending: Vec<_> = world
        .scheduler()
        .iter_entries()
        .map(|entry| json!({"pos":[entry.pos.x,entry.pos.y,entry.pos.z],"delay":entry.ticks_left,"priority":entry.tick_priority as u8}))
        .collect();
    let positions = [[160, 78, 121], [168, 78, 121], [172, 78, 121], [173, 79, 121]];
    let mut frames = Vec::new();
    for tick in 0..=512 {
        if tick > 0 {
            world.tick_interpreted();
        }
        let states: Vec<_> = positions
            .iter()
            .map(|p| state(world.get_block(BlockPos::new(p[0], p[1], p[2]))))
            .collect();
        frames.push(json!({"step":0,"tick":tick,"states":states,"outputs":[null,null,null,null]}));
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/redpiler-reference-cases/classic-9999.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        path,
        serde_json::to_vec(
            &json!({"blocks":snapshot,"comparators":comparators,"pending":pending,"positions":positions,"actions":[[-1,false,512]],"frames":frames}),
        )
        .unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "exports native fixtures for replay"]
fn export_classic_cases() {
    use serde_json::json;
    let state = |block: Block| {
        let properties: std::collections::BTreeMap<_, _> = block.properties().into_iter().collect();
        format!(
            "{}[{}]",
            block.get_name(),
            properties
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join(",")
        )
    };
    for seed in [10, 410] {
        let mut rng = StdRng::seed_from_u64(seed);
        let blocks = circuit(&mut rng);
        let warmup = rng.random_range(0..=16);
        let actions: Vec<_> = (0..64)
            .map(|_| (rng.random_range(0..DEPTH), rng.random::<bool>(), rng.random_range(0..=8)))
            .collect();
        let mut world = load(&blocks, warmup);
        let mut snapshot = Vec::new();
        crate::world::for_each_block_optimized(
            &world,
            BASE + BlockPos::new(-2, -2, -2),
            BASE + BlockPos::new(WIDTH + 2, 2, DEPTH + 2),
            |pos| {
                let block = world.get_block(pos);
                if block != Block::Air {
                    snapshot.push(json!({"pos":[pos.x,pos.y,pos.z], "state":state(block)}));
                }
            },
        );
        let comparators: Vec<_> = blocks
            .iter()
            .filter(|(_, block)| matches!(block, Block::RedstoneComparator { .. }))
            .map(|&(pos, _)| json!({"pos":[pos.x,pos.y,pos.z], "output":comparator_output(&world,pos)}))
            .collect();
        let pending: Vec<_> = world
            .scheduler()
            .iter_entries()
            .map(|entry| json!({"pos":[entry.pos.x,entry.pos.y,entry.pos.z], "delay":entry.ticks_left, "priority":entry.tick_priority as u8}))
            .collect();
        let mut frames = Vec::new();
        for (step, &(z, powered, wait)) in actions.iter().enumerate() {
            lever_action(&mut world, BASE + BlockPos::new(0, 0, z), powered);
            for tick in 0..=wait {
                if tick > 0 {
                    world.tick_interpreted();
                }
                let states: Vec<_> = blocks.iter().map(|&(pos, _)| state(world.get_block(pos))).collect();
                let outputs: Vec<_> = blocks
                    .iter()
                    .map(|&(pos, block)| {
                        if matches!(block, Block::RedstoneComparator { .. }) {
                            Some(comparator_output(&world, pos))
                        } else {
                            None
                        }
                    })
                    .collect();
                frames.push(json!({"step":step,"tick":tick,"states":states,"outputs":outputs}));
            }
        }
        let positions: Vec<_> = blocks.iter().map(|(pos, _)| [pos.x, pos.y, pos.z]).collect();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../target/redpiler-reference-cases/classic-{seed}.json"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, serde_json::to_vec(&json!({"seed":seed,"blocks":snapshot,"comparators":comparators,"pending":pending,"positions":positions,"actions":actions,"frames":frames})).unwrap()).unwrap();
    }
}

#[test]
fn repeater_reads_dust_power_through_its_rear_solid_block() {
    for facing in [BlockDirection::North, BlockDirection::South, BlockDirection::East, BlockDirection::West] {
        for delay in 1..=4 {
            let wire = BASE + BlockPos::new(3, 2, 3);
            let input = wire.offset(facing.rotate().block_face());
            let repeater = wire.offset(BlockFace::Bottom).offset(facing.opposite().block_face());
            let lamp = repeater.offset(facing.opposite().block_face());
            let blocks = [
                (
                    input,
                    Block::Lever {
                        lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
                    },
                ),
                (
                    wire,
                    Block::RedstoneWire {
                        wire: RedstoneWire::default(),
                    },
                ),
                (
                    repeater,
                    Block::RedstoneRepeater {
                        repeater: RedstoneRepeater {
                            facing,
                            delay,
                            powered: false,
                            locked: false,
                        },
                    },
                ),
                (lamp, Block::RedstoneLamp { lit: false }),
            ];
            let mut native = load(&blocks, 0);
            let mut compiled = load(&blocks, 0);
            let bounds = (BASE, BASE + BlockPos::new(7, 4, 7));
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &compiled,
                    bounds,
                    Default::default(),
                    compiled.scheduler().iter_entries().collect(),
                    Default::default(),
                )
                .unwrap();
            compiled.clear_scheduled_ticks();
            for powered in [true, false, true] {
                lever_action(&mut native, input, powered);
                compiler.try_on_use_block(input).unwrap();
                for tick in 0..=12 {
                    if tick > 0 {
                        native.tick_interpreted();
                        compiler.tick().unwrap();
                    }
                    compiler.try_flush_complete(&mut compiled).unwrap();
                    for &(pos, _) in &blocks {
                        assert_eq!(
                            compiled.get_block(pos),
                            native.get_block(pos),
                            "facing={facing:?} delay={delay} powered={powered} tick={tick} pos={pos:?}"
                        );
                    }
                }
            }
        }
    }
}
