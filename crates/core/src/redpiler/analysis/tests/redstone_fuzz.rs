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
    compare_mixed(seed, optimize, None)
}

fn attach_assembly(world: &mut PlotWorld, name: &str) {
    let (source, bounds, _) = fixture(name);
    let shift = BlockPos::new(80, 0, 80);
    crate::world::for_each_block_optimized(&source, bounds.0, bounds.1, |pos| {
        world.set_block(pos + shift, source.get_block(pos));
        if let Some(entity) = source.get_block_entity(pos) {
            world.set_block_entity(pos + shift, entity.clone());
        }
    });
}

fn compare_mixed(seed: u64, optimize: bool, assembly: Option<&str>) -> Result<(), String> {
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
    if let Some(name) = assembly {
        attach_assembly(&mut native, name);
        attach_assembly(&mut compiled, name);
    }
    let bounds = compiled.get_corners();
    let mut compiler = Compiler::default();
    match compiler.compile(
        &compiled,
        bounds,
        CompilerOptions {
            optimize,
            ..Default::default()
        },
        compiled.scheduler().iter_entries().collect(),
        Default::default(),
    ) {
        Ok(()) => (),
        Err(error) => {
            return Err(format!(
                "seed={seed} optimize={optimize} compile error: {error}"
            ))
        }
    }
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
            for (pos, actual) in compiler.ordinary_sources() {
                if matches!(native.get_block(pos), Block::RedstoneComparator { .. }) {
                    let expected =
                        crate::redstone::source_strength(native.get_block(pos), &native, pos);
                    if actual != expected {
                        return Err(format!(
                            "seed={seed} optimize={optimize} warmup={warmup} step={step} tick={tick} comparator strength at {pos:?} actual={actual} expected={expected}; circuit={blocks:?}; actions={:?}",
                            &actions[..=step],
                        ));
                    }
                }
            }
            for &(pos, block) in &blocks {
                // The native executor retains dust even when optimization is requested.
                if block != Block::Air
                    && (compiler.native_owns(pos) || !matches!(block, Block::RedstoneWire { .. }))
                {
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
            match compare(seed, optimize) {
                Ok(()) => (),
                Err(error) => {
                    eprintln!("{error}");
                    failures += 1;
                }
            }
        }
        if (offset + 1) % 1000 == 0 {
            eprintln!(
                "Fuzz progress: {} circuits checked, {failures} failing runs",
                offset + 1
            );
        }
    }
    eprintln!("Fuzz summary: {cases} circuits, optimization off/on, {failures} failing runs (compile errors count as failures)");
    assert_eq!(failures, 0, "see the replayable failures above");
}

#[test]
fn feedback_fuzz_seeds_compile_and_match_native_callback_order() {
    for seed in [121, 844, 2791, 7604, 8039, 9037, 9545] {
        for optimize in [false, true] {
            compare(seed, optimize).unwrap();
        }
    }
}

#[test]
fn disconnected_assembly_preserves_ordinary_ordering_seeds() {
    for seed in [121, 844, 2791, 7604, 8039, 9037, 9545] {
        for optimize in [false, true] {
            compare_mixed(seed, optimize, Some("instant_observer")).unwrap();
        }
    }
}

#[test]
fn known_valid_assemblies_extend_generated_ordinary_networks() {
    for name in [
        "instant_chain",
        "instant_observer",
        "instant_torch",
        "adder_1bit",
        "counter_basic",
        "bud_noninstantinputs",
        "bud_instantmemorycellobserverupdate",
    ] {
        for seed in 0..8 {
            for optimize in [false, true] {
                compare_mixed(seed, optimize, Some(name))
                    .unwrap_or_else(|error| panic!("{name}: {error}"));
            }
        }
    }
}

#[test]
fn native_feedback_preserves_hidden_state_and_scheduler_on_reset() {
    let mut rng = StdRng::seed_from_u64(121);
    let blocks = circuit(&mut rng);
    let warmup = rng.gen_range(0..=16);
    let control = BASE + BlockPos::new(0, 0, 1);
    for optimize in [false, true] {
        for io_only in [false, true] {
            for flush_every in [0, 1, 5] {
                for reset_at in [1, 4, 7] {
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
                                io_only,
                                ..Default::default()
                            },
                            compiled.scheduler().iter_entries().collect(),
                            Default::default(),
                        )
                        .unwrap();
                    assert!(compiler.stats().unwrap().graph.native_propagation);
                    assert!(compiler.stats().unwrap().graph.passes[3..9]
                        .iter()
                        .all(|pass| !pass.enabled));
                    compiled.clear_scheduled_ticks();
                    compiler.on_use_block(control);
                    let Block::Lever { lever } = native.get_block(control) else {
                        panic!()
                    };
                    lever_action(&mut native, control, !lever.powered);
                    for tick in 1..=reset_at {
                        native.tick_interpreted();
                        if flush_every == 1 {
                            compiler.tick();
                        } else {
                            compiler.tick_with_world(&mut compiled);
                        }
                        if flush_every > 0 && tick % flush_every == 0 {
                            compiler.flush(&mut compiled);
                        }
                        for (pos, strength) in compiler.ordinary_sources() {
                            assert_eq!(
                                strength,
                                crate::redstone::source_strength(
                                    native.get_block(pos),
                                    &native,
                                    pos
                                )
                            );
                        }
                    }
                    compiler.reset(&mut compiled, bounds);
                    for &(pos, _) in &blocks {
                        assert_eq!(
                            compiled.get_block(pos),
                            native.get_block(pos),
                            "reset at {pos:?}"
                        );
                        assert_eq!(
                            crate::redstone::source_strength(
                                compiled.get_block(pos),
                                &compiled,
                                pos
                            ),
                            crate::redstone::source_strength(native.get_block(pos), &native, pos)
                        );
                    }
                    assert_eq!(
                        compiled.scheduler().iter_entries().collect::<Vec<_>>(),
                        native.scheduler().iter_entries().collect::<Vec<_>>()
                    );
                    for _ in 0..12 {
                        native.tick_interpreted();
                        compiled.tick_interpreted();
                        for &(pos, _) in &blocks {
                            assert_eq!(compiled.get_block(pos), native.get_block(pos));
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn torch_oscillator_compiles_and_keeps_oscillating_after_reset() {
    fn oscillator() -> PlotWorld {
        let mut world = empty();
        world.set_block(BASE, Block::Stone {});
        world.set_block(
            BASE.offset(BlockFace::East),
            Block::RedstoneWallTorch {
                lit: true,
                facing: BlockDirection::East,
            },
        );
        for pos in [BASE.offset(BlockFace::Top), BASE + BlockPos::new(1, 1, 0)] {
            world.set_block(
                pos,
                Block::RedstoneWire {
                    wire: RedstoneWire::default(),
                },
            );
        }
        for pos in [BASE.offset(BlockFace::Top), BASE + BlockPos::new(1, 1, 0)] {
            let wire =
                crate::redstone::wire::get_regulated_sides(RedstoneWire::default(), &world, pos);
            world.set_block(pos, Block::RedstoneWire { wire });
            crate::redstone::update(world.get_block(pos), &mut world, pos, None);
        }
        world
    }
    for optimize in [false, true] {
        for io_only in [false, true] {
            let mut native = oscillator();
            let mut compiled = oscillator();
            let bounds = compiled.get_corners();
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &compiled,
                    bounds,
                    CompilerOptions {
                        optimize,
                        io_only,
                        ..Default::default()
                    },
                    compiled.scheduler().iter_entries().collect(),
                    Default::default(),
                )
                .unwrap();
            assert!(compiler.stats().unwrap().graph.native_propagation);
            compiled.clear_scheduled_ticks();
            let torch = BASE.offset(BlockFace::East);
            let mut changes = 0;
            let mut previous = native.get_block(torch);
            for tick in 1..=24 {
                native.tick_interpreted();
                compiler.tick_with_world(&mut compiled);
                if tick % 5 == 0 {
                    compiler.flush(&mut compiled);
                }
                changes += usize::from(previous != native.get_block(torch));
                previous = native.get_block(torch);
                let (_, actual) = compiler
                    .ordinary_sources()
                    .into_iter()
                    .find(|(pos, _)| *pos == torch)
                    .unwrap();
                assert_eq!(
                    actual,
                    crate::redstone::source_strength(native.get_block(torch), &native, torch)
                );
            }
            assert!(changes >= 8, "fixture must be a live oscillator");
            compiler.reset(&mut compiled, bounds);
            assert_eq!(
                compiled.scheduler().iter_entries().collect::<Vec<_>>(),
                native.scheduler().iter_entries().collect::<Vec<_>>()
            );
            for _ in 0..16 {
                native.tick_interpreted();
                compiled.tick_interpreted();
                for offset in [
                    BlockPos::new(1, 0, 0),
                    BlockPos::new(0, 1, 0),
                    BlockPos::new(1, 1, 0),
                ] {
                    assert_eq!(
                        compiled.get_block(BASE + offset),
                        native.get_block(BASE + offset)
                    );
                }
            }
        }
    }
}

#[test]
fn native_controls_observers_and_command_outputs_match_interpreter() {
    use mchprs_blocks::block_entities::CommandBlockEntity;
    use mchprs_blocks::blocks::{ButtonFace, RedstoneObserver, StoneButton};
    use mchprs_blocks::BlockFacing;
    let button = BASE;
    let plate = BASE + BlockPos::new(0, 0, 3);
    let command = BASE + BlockPos::new(3, 0, 0);
    let mut blocks = vec![
        (
            button,
            Block::StoneButton {
                button: StoneButton {
                    face: ButtonFace::Floor,
                    facing: BlockDirection::North,
                    powered: false,
                },
            },
        ),
        (plate, Block::StonePressurePlate { powered: false }),
        (command, Block::from_name("command_block").unwrap()),
        (
            BASE + BlockPos::new(3, 0, 1),
            Block::RedstoneLamp { lit: false },
        ),
        (
            BASE + BlockPos::new(3, 0, 3),
            Block::RedstoneLamp { lit: false },
        ),
        (
            BASE + BlockPos::new(1, 0, -1),
            Block::Observer {
                observer: RedstoneObserver {
                    facing: BlockFacing::South,
                    powered: false,
                },
            },
        ),
        (
            BASE + BlockPos::new(1, 0, -2),
            Block::RedstoneLamp { lit: false },
        ),
    ];
    for z in [0, 3] {
        blocks.push((
            BASE + BlockPos::new(1, 0, z),
            Block::RedstoneWire {
                wire: RedstoneWire::default(),
            },
        ));
        blocks.push((
            BASE + BlockPos::new(2, 0, z),
            Block::RedstoneComparator {
                comparator: RedstoneComparator::new(
                    BlockDirection::West,
                    ComparatorMode::Compare,
                    false,
                ),
            },
        ));
    }
    for optimize in [false, true] {
        let mut native = load(&blocks, 6);
        let mut compiled = load(&blocks, 6);
        for world in [&mut native, &mut compiled] {
            world.disable_command_output_limits_for_replay();
            world.set_block_entity(
                command,
                BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
                    command: "say native redstone".into(),
                    ..Default::default()
                })),
            );
        }
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
            .unwrap();
        compiled.clear_scheduled_ticks();
        for tick in 0..=48 {
            if [0, 5, 25].contains(&tick) {
                compiler.on_use_block(button);
                let Block::StoneButton { button: mut state } = native.get_block(button) else {
                    panic!()
                };
                if !state.powered {
                    state.powered = true;
                    native.set_block(button, Block::StoneButton { button: state });
                    native.schedule_tick(button, 10, TickPriority::Normal);
                    crate::redstone::update_surrounding_blocks(&mut native, button);
                    crate::redstone::update_surrounding_blocks(
                        &mut native,
                        button.offset(BlockFace::Bottom),
                    );
                }
            }
            if [3, 8, 25, 29].contains(&tick) {
                let powered = [3, 25].contains(&tick);
                compiler.set_pressure_plate(plate, powered);
                native.set_block(plate, Block::StonePressurePlate { powered });
                crate::redstone::update_surrounding_blocks(&mut native, plate);
                crate::redstone::update_surrounding_blocks(
                    &mut native,
                    plate.offset(BlockFace::Bottom),
                );
            }
            if tick > 0 {
                native.tick_interpreted();
                compiler.tick_with_world(&mut compiled);
            }
            compiler.flush(&mut compiled);
            for &(pos, _) in &blocks {
                assert_eq!(
                    compiled.get_block(pos),
                    native.get_block(pos),
                    "tick={tick} pos={pos:?}"
                );
            }
            assert_eq!(
                compiled.command_output().collect::<Vec<_>>(),
                native.command_output().collect::<Vec<_>>()
            );
        }
        assert_eq!(
            compiled.command_output().count(),
            2,
            "both button pulses must execute the command"
        );
        let before = compiled.command_output().count();
        compiler.reset(&mut compiled, bounds);
        assert_eq!(compiled.command_output().count(), before);
        assert_eq!(
            format!("{:?}", compiled.get_block_entity(command)),
            format!("{:?}", native.get_block_entity(command))
        );
    }
}

#[test]
fn native_compilation_errors_preserve_world_and_scheduler() {
    use crate::redpiler::backend::BackendError;
    use crate::redpiler::compile_graph::GraphError;
    let block = Block::RedstoneComparator {
        comparator: RedstoneComparator::new(BlockDirection::West, ComparatorMode::Subtract, false),
    };
    for (strength, export, export_dot_graph) in
        [(16, false, false), (0, true, false), (0, false, true)]
    {
        let mut world = load(&[(BASE, block)], 0);
        world.set_block_entity(
            BASE,
            BlockEntity::Comparator {
                output_strength: strength,
            },
        );
        world.schedule_tick(BASE, 1, TickPriority::High);
        let ticks = world.scheduler().iter_entries().collect::<Vec<_>>();
        let mut compiler = Compiler::default();
        let error = compiler
            .compile(
                &world,
                world.get_corners(),
                CompilerOptions {
                    optimize: true,
                    export,
                    export_dot_graph,
                    ..Default::default()
                },
                ticks.clone(),
                Default::default(),
            )
            .unwrap_err();
        assert!(matches!(
            error,
            CompileError::Backend(BackendError::InvalidStrength { .. })
                | CompileError::Graph(GraphError::UnsupportedNativeExport)
        ));
        assert!(!compiler.is_active());
        assert!(compiler.stats().is_none());
        assert_eq!(world.get_block(BASE), block);
        assert_eq!(
            crate::redstone::source_strength(block, &world, BASE),
            strength
        );
        assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), ticks);
    }
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
