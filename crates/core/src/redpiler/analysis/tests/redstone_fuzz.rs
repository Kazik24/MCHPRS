//! Deterministic differential fuzzing. MCHPRS_REDSTONE_FUZZ_SEED selects the first
//! case; MCHPRS_REDSTONE_FUZZ_CASES sets the count (defaults: 0 and 64).
use super::*;
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::{
    ComparatorMode, Lever, LeverFace, RedstoneComparator, RedstoneRepeater, RedstoneWire,
};
use mchprs_blocks::BlockDirection;
use rand::{rngs::StdRng, Rng, SeedableRng};

const WIDTH: i32 = 8;
const DEPTH: i32 = 6;

// shortcut: flat layouts and lever inputs only, expand for vertical wiring and other controls.
fn circuit(rng: &mut StdRng) -> Vec<(BlockPos, Block)> {
    let mut blocks = Vec::new();
    for z in 0..DEPTH {
        for x in 0..WIDTH {
            let pos = BASE + BlockPos::new(x, 0, z);
            let block = if x == 0 {
                Block::Lever {
                    lever: Lever::new(LeverFace::Floor, BlockDirection::North, rng.gen()),
                }
            } else if x == WIDTH - 1 {
                Block::RedstoneLamp { lit: false }
            } else {
                let facing = BlockDirection::from_id(rng.gen_range(0..4));
                match rng.gen_range(0..10) {
                    0 => Block::Air,
                    1 => Block::RedstoneTorch { lit: true },
                    2 | 3 => Block::RedstoneRepeater {
                        repeater: RedstoneRepeater {
                            facing,
                            delay: rng.gen_range(1..=4),
                            powered: false,
                            locked: false,
                        },
                    },
                    4 | 5 => Block::RedstoneComparator {
                        comparator: RedstoneComparator::new(
                            facing,
                            if rng.gen() {
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

fn compare(seed: u64, optimize: bool) -> Result<(), String> {
    let mut rng = StdRng::seed_from_u64(seed);
    let blocks = circuit(&mut rng);
    let warmup = rng.gen_range(0..=16);
    let actions: Vec<_> = (0..64)
        .map(|_| {
            (
                rng.gen_range(0..DEPTH),
                rng.gen::<bool>(),
                rng.gen_range(0..=8),
            )
        })
        .collect();
    let mut native = load(&blocks, warmup);
    let mut compiled = load(&blocks, warmup);
    let bounds = compiled.get_corners();
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &compiled,
            bounds,
            CompilerOptions {
                optimize,
                ..Default::default()
            },
            compiled.scheduler().iter_entries().collect(),
            Default::default(),
        )
        .map_err(|error| format!("seed={seed} optimize={optimize} compile error: {error}"))?;
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
                compiler.tick_with_world(&mut compiled);
            }
            compiler.flush(&mut compiled);
            for &(pos, block) in &blocks {
                // Optimization omits dust display nodes; compare component states.
                if !matches!(block, Block::Air | Block::RedstoneWire { .. }) {
                    let actual = compiled.get_block(pos);
                    let expected = native.get_block(pos);
                    if actual != expected {
                        return Err(format!(
                            "seed={seed} optimize={optimize} warmup={warmup} step={step} tick={tick} input=({z},{powered}) pos={pos:?} actual={actual:?} expected={expected:?}; circuit={blocks:?}; actions={:?}",
                            &actions[..=step],
                        ));
                    }
                }
            }
        }
    }
    // Comparator entities receive the backend's analog strength on reset.
    compiler.reset(&mut compiled, bounds);
    for &(pos, block) in &blocks {
        if matches!(block, Block::RedstoneComparator { .. }) {
            let actual = crate::redstone::source_strength(block, &compiled, pos);
            let expected = crate::redstone::source_strength(block, &native, pos);
            if actual != expected {
                return Err(format!(
                    "seed={seed} optimize={optimize} comparator strength at {pos:?} actual={actual} expected={expected}; circuit={blocks:?}; actions={actions:?}",
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
    assert!(
        cases > 0,
        "the fuzz campaign must execute at least one case"
    );

    let mut failures = 0;
    for offset in 0..cases {
        let seed = first_seed.wrapping_add(offset);
        for optimize in [false, true] {
            if let Err(error) = compare(seed, optimize) {
                eprintln!("{error}");
                failures += 1;
            }
        }
        if (offset + 1) % 1000 == 0 {
            eprintln!(
                "Fuzz progress: {} circuits checked, {failures} failing runs",
                offset + 1
            );
        }
    }
    eprintln!("Fuzz summary: {cases} circuits, optimization off/on, {failures} failing runs");
    assert_eq!(failures, 0, "see the replayable failures above");
}

#[test]
fn coalesced_repeaters_preserve_display_and_pending_work_on_reset() {
    let control = BASE;
    let dust = BASE + BlockPos::new(1, 0, 0);
    let repeaters = [dust.offset(BlockFace::North), dust.offset(BlockFace::South)];
    let lamps = [
        repeaters[0].offset(BlockFace::North),
        repeaters[1].offset(BlockFace::South),
    ];
    let blocks = [
        (
            control,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
            },
        ),
        (
            dust,
            Block::RedstoneWire {
                wire: RedstoneWire::default(),
            },
        ),
        (
            repeaters[0],
            Block::RedstoneRepeater {
                repeater: RedstoneRepeater {
                    facing: BlockDirection::South,
                    delay: 2,
                    powered: false,
                    locked: false,
                },
            },
        ),
        (
            repeaters[1],
            Block::RedstoneRepeater {
                repeater: RedstoneRepeater {
                    facing: BlockDirection::North,
                    delay: 2,
                    powered: false,
                    locked: false,
                },
            },
        ),
        (lamps[0], Block::RedstoneLamp { lit: false }),
        (lamps[1], Block::RedstoneLamp { lit: false }),
    ];
    for optimize in [false, true] {
        for io_only in [false, true] {
            for reset_at in [2, 4] {
                let mut native = load(&blocks, 6);
                let mut compiled = load(&blocks, 6);
                let bounds = compiled.get_corners();
                let mut compiler = Compiler::default();
                compiler
                    .compile(
                        &compiled,
                        bounds,
                        CompilerOptions {
                            optimize,
                            io_only,
                            update: true,
                            ..Default::default()
                        },
                        compiled.scheduler().iter_entries().collect(),
                        Default::default(),
                    )
                    .unwrap();
                compiled.clear_scheduled_ticks();
                lever_action(&mut native, control, true);
                compiler.on_use_block(control);
                for tick in 1..=reset_at {
                    native.tick_interpreted();
                    compiler.tick_with_world(&mut compiled);
                    compiler.flush(&mut compiled);
                    for pos in repeaters.into_iter().chain(lamps) {
                        if !io_only || lamps.contains(&pos) {
                            assert_eq!(compiled.get_block(pos), native.get_block(pos),
                                "optimize={optimize} io_only={io_only} reset_at={reset_at} tick={tick} pos={pos:?}");
                        }
                    }
                }
                compiler.reset(&mut compiled, bounds);
                for pos in repeaters.into_iter().chain(lamps) {
                    assert_eq!(
                        compiled.get_block(pos),
                        native.get_block(pos),
                        "reset at {pos:?}"
                    );
                }
                lever_action(&mut native, control, false);
                lever_action(&mut compiled, control, false);
                for tick in 1..=14 {
                    native.tick_interpreted();
                    compiled.tick_interpreted();
                    for pos in repeaters.into_iter().chain(lamps) {
                        assert_eq!(compiled.get_block(pos), native.get_block(pos),
                            "handoff optimize={optimize} io_only={io_only} reset_at={reset_at} tick={tick} pos={pos:?}");
                    }
                }
            }
        }
    }
}
