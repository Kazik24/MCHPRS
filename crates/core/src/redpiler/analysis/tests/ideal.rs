use super::*;

fn counter_bank(compiler: &Compiler, manifest: &Value) -> (u64, u64, u16) {
    let domains = compiler.backend.as_ref().unwrap().logical_stats();
    let banks: Vec<_> = domains
        .iter()
        .filter(|(_, _, memory)| !memory.is_empty())
        .collect();
    assert_eq!(
        banks.len(),
        1,
        "Counter must have one explicit sampled bank"
    );
    let (_, samples, memory) = banks[0];
    assert_eq!(memory.len(), 16);
    let word = manifest["ports"]["observations"]["memory"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .fold(0u16, |word, (bit, pos)| {
            let pos = local_pos(pos);
            let stored = memory.iter().find(|(base, _)| *base == pos).unwrap().1;
            word | (u16::from(stored) << bit)
        });
    (
        domains.iter().map(|(evaluations, _, _)| evaluations).sum(),
        *samples,
        word,
    )
}

#[test]
fn ideal_counter_commits_old_bank_atomically_every_six_steps_and_holds_when_stopped() {
    for (optimize, assume_instant) in [(false, false), (true, false), (false, true), (true, true)] {
        let (mut world, _, manifest) = fixture("counter_basic");
        let mut compiler = Compiler::default();
        compiler
            .compile(
                &world,
                world.get_corners(),
                CompilerOptions {
                    assume_instant,
                    optimize,
                    ..Default::default()
                },
                Vec::new(),
                Default::default(),
            )
            .unwrap();
        assert_eq!(counter_bank(&compiler, &manifest).1, 0);
        assert_eq!(counter_bank(&compiler, &manifest).2, 0);
        for _ in 0..24 {
            compiler.tick_with_world(&mut world);
        }
        assert_eq!(
            counter_bank(&compiler, &manifest).1,
            0,
            "a held inactive level is not a sample"
        );
        let trigger = local_pos(&manifest["ports"]["inputs"]["trigger"]);
        compiler.on_use_block(trigger);
        let mut previous = counter_bank(&compiler, &manifest);
        let mut previous_commit = None;
        for tick in 1..=130 {
            compiler.tick_with_world(&mut world);
            compiler.flush(&mut world);
            let current = counter_bank(&compiler, &manifest);
            if current.1 != previous.1 {
                assert_eq!(current.1, previous.1 + 1, "one explicit sample per commit");
                assert_eq!(
                    current.2,
                    previous.2.wrapping_add(1),
                    "read all cells from the old bank before writing the new bank"
                );
                if let Some(previous_tick) = previous_commit {
                    assert_eq!(tick - previous_tick, 6);
                }
                previous_commit = Some(tick);
            } else {
                assert_eq!(
                    current.2, previous.2,
                    "memory changes only at an explicit commit"
                );
            }
            assert!(world.piston_state().motions.is_empty());
            assert!(world.piston_state().events.is_empty());
            previous = current;
        }
        assert!(
            previous.1 >= 20,
            "exercise the 15-to-16 carry in the runtime"
        );
        compiler.on_use_block(trigger);
        for _ in 0..24 {
            compiler.tick_with_world(&mut world);
        }
        let held = counter_bank(&compiler, &manifest);
        for _ in 0..48 {
            compiler.tick_with_world(&mut world);
        }
        assert_eq!(
            counter_bank(&compiler, &manifest),
            held,
            "held inputs must reuse decisions and preserve stored memory"
        );
        compiler.on_use_block(trigger);
        for _ in 0..12 {
            compiler.tick_with_world(&mut world);
        }
        let restarted = counter_bank(&compiler, &manifest);
        assert_eq!(restarted.1, held.1 + 2);
        assert_eq!(
            restarted.2,
            held.2.wrapping_add(2),
            "restart samples retained memory"
        );
    }
}

#[test]
fn ideal_counter_recompiles_a_settled_nonzero_bank_without_resampling() {
    for optimize in [false, true] {
        let mut previous_handoff = None;
        for _ in 0..2 {
            let (mut world, fixture_bounds, manifest) = fixture("counter_basic");
            let options = || CompilerOptions {
                assume_instant: true,
                optimize,
                ..Default::default()
            };
            let trigger = local_pos(&manifest["ports"]["inputs"]["trigger"]);
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &world,
                    world.get_corners(),
                    options(),
                    Vec::new(),
                    Default::default(),
                )
                .unwrap();
            compiler.on_use_block(trigger);
            for _ in 0..17 {
                compiler.tick_with_world(&mut world);
            }
            compiler.on_use_block(trigger);
            for _ in 0..24 {
                compiler.tick_with_world(&mut world);
            }
            let stored = counter_bank(&compiler, &manifest).2;
            assert_ne!(stored, 0, "the imported bank must contain retained data");
            let bounds = world.get_corners();
            compiler.reset(&mut world, bounds);
            assert!(matches!(world.get_block(trigger), Block::Lever { lever } if !lever.powered));
            assert!(world.piston_state().motions.is_empty());
            assert!(world.piston_state().events.is_empty());

            compiler
                .compile(&world, bounds, options(), Vec::new(), Default::default())
                .unwrap();
            let initial = counter_bank(&compiler, &manifest);
            assert_eq!(
                (initial.1, initial.2),
                (0, stored),
                "binding must load the saved bank without sampling it"
            );
            for _ in 0..64 {
                compiler.tick_with_world(&mut world);
                let held = counter_bank(&compiler, &manifest);
                assert_eq!(
                    (held.1, held.2),
                    (0, stored),
                    "a held inactive clock preserves the imported bank"
                );
            }

            compiler.on_use_block(trigger);
            let mut previous = counter_bank(&compiler, &manifest);
            let mut previous_commit = None;
            for tick in 1..=24 {
                compiler.tick_with_world(&mut world);
                let current = counter_bank(&compiler, &manifest);
                if current.1 != previous.1 {
                    assert_eq!(current.1, previous.1 + 1);
                    assert_eq!(
                        current.2,
                        previous.2.wrapping_add(1),
                        "the explicit sample must read the retained old bank atomically"
                    );
                    if let Some(previous_tick) = previous_commit {
                        assert_eq!(tick - previous_tick, 6);
                    }
                    previous_commit = Some(tick);
                } else {
                    assert_eq!(current.2, previous.2);
                }
                previous = current;
            }
            assert!(
                previous.1 >= 3,
                "the restarted clock must commit repeatedly"
            );
            compiler.on_use_block(trigger);
            for _ in 0..24 {
                compiler.tick_with_world(&mut world);
            }
            let final_word = counter_bank(&compiler, &manifest).2;
            compiler.reset(&mut world, bounds);
            let restored_word = manifest["ports"]["observations"]["memory"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .fold(0u16, |word, (bit, pos)| {
                    let Block::Piston { piston } = world.get_block(local_pos(pos)) else {
                        panic!("stored bits must restore as stationary bases");
                    };
                    word | (u16::from(!piston.extended) << bit)
                });
            assert_eq!(restored_word, final_word);
            assert!(world.piston_state().motions.is_empty());
            assert!(world.piston_state().events.is_empty());
            assert!(!world.scheduler().iter_entries().any(|tick| matches!(
                world.get_block(tick.pos),
                Block::Piston { .. } | Block::Observer { .. }
            )));
            let handoff = snapshot(&world, fixture_bounds);
            if let Some(previous) = &previous_handoff {
                assert_eq!(&handoff, previous);
            }
            previous_handoff = Some(handoff);
        }
    }
}

#[test]
fn stateless_domain_reuses_inactive_decisions_and_advances_output_phases() {
    for optimize in [false, true] {
        let (mut world, _, manifest) = fixture("instant_observer");
        let mut compiler = Compiler::default();
        compiler
            .compile(
                &world,
                world.get_corners(),
                CompilerOptions {
                    assume_instant: true,
                    optimize,
                    ..Default::default()
                },
                Vec::new(),
                Default::default(),
            )
            .unwrap();
        let evaluations = |compiler: &Compiler| {
            let domains = compiler.backend.as_ref().unwrap().logical_stats();
            assert!(
                !domains.is_empty(),
                "the dedicated logical executor must be present"
            );
            assert!(domains
                .iter()
                .all(|(_, samples, memory)| *samples == 0 && memory.is_empty()));
            domains
                .iter()
                .map(|(evaluations, _, _)| evaluations)
                .sum::<u64>()
        };
        for _ in 0..16 {
            compiler.tick_with_world(&mut world);
        }
        compiler.flush(&mut world);
        let initial_evaluations = evaluations(&compiler);
        assert!(initial_evaluations > 0);
        let output = local_pos(&manifest["ports"]["observations"]["repeater"]);
        let initial_output = world.get_block(output);
        for _ in 0..64 {
            compiler.tick_with_world(&mut world);
        }
        assert_eq!(evaluations(&compiler), initial_evaluations);
        compiler.on_use_block(local_pos(&manifest["ports"]["inputs"]["trigger"]));
        for _ in 0..16 {
            compiler.tick_with_world(&mut world);
        }
        compiler.flush(&mut world);
        assert!(
            evaluations(&compiler) > initial_evaluations,
            "changed source must invalidate its dependent domain"
        );
        assert_ne!(world.get_block(output), initial_output);
        let changed_evaluations = evaluations(&compiler);
        for _ in 0..64 {
            compiler.tick_with_world(&mut world);
        }
        assert!(
            evaluations(&compiler) > changed_evaluations,
            "the observable reset cycle remains active with held inputs"
        );
        compiler.on_use_block(local_pos(&manifest["ports"]["inputs"]["trigger"]));
        for _ in 0..16 {
            compiler.tick_with_world(&mut world);
        }
        let stopped_evaluations = evaluations(&compiler);
        for _ in 0..64 {
            compiler.tick_with_world(&mut world);
        }
        assert_eq!(evaluations(&compiler), stopped_evaluations);
    }
}

#[test]
fn ideal_reset_restores_stored_bits_and_stationary_geometry_deterministically() {
    for optimize in [false, true] {
        let mut previous = None;
        for _ in 0..2 {
            let (mut world, bounds, manifest) = fixture("counter_basic");
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &world,
                    world.get_corners(),
                    CompilerOptions {
                        assume_instant: true,
                        optimize,
                        ..Default::default()
                    },
                    Vec::new(),
                    Default::default(),
                )
                .unwrap();
            compiler.on_use_block(local_pos(&manifest["ports"]["inputs"]["trigger"]));
            for _ in 0..17 {
                compiler.tick_with_world(&mut world);
            }
            let expected = counter_bank(&compiler, &manifest).2;
            let full_bounds = world.get_corners();
            compiler.reset(&mut world, full_bounds);
            assert!(!compiler.is_active());
            let mut actual = 0u16;
            for (bit, pos) in manifest["ports"]["observations"]["memory"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
            {
                let Block::Piston { piston } = world.get_block(local_pos(pos)) else {
                    panic!("logical memory must restore as a stationary base");
                };
                actual |= u16::from(!piston.extended) << bit;
            }
            assert_eq!(actual, expected);
            assert!(world.piston_state().motions.is_empty());
            assert!(world.piston_state().events.is_empty());
            assert!(
                !world.scheduler().iter_entries().any(|tick| matches!(
                    world.get_block(tick.pos),
                    Block::Piston { .. } | Block::Observer { .. }
                )),
                "logical reset must not return physical reset or piston deadlines"
            );
            crate::world::for_each_block_optimized(&world, bounds.0, bounds.1, |pos| {
                assert!(
                    !matches!(world.get_block(pos), Block::MovingPiston { .. }),
                    "logical restore cannot leave moving geometry at {pos:?}"
                );
                assert!(
                    !matches!(world.get_block(pos), Block::Observer { observer } if observer.powered),
                    "owned reset observers must be dormant at {pos:?}"
                );
            });
            let restored = snapshot(&world, bounds);
            if let Some(previous) = &previous {
                assert_eq!(&restored, previous);
            }
            previous = Some(restored);
        }
    }
}

#[test]
fn ideal_rejects_uncertified_sampling_without_mutating_the_world() {
    let mut world = empty();
    world.set_block(
        BASE,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::East,
                sticky: true,
                extended: false,
            },
        },
    );
    world.set_block(BASE.offset(BlockFace::East), Block::RedstoneBlock);
    let bounds = (BASE - BlockPos::new(1, 1, 1), BASE + BlockPos::new(3, 2, 2));
    let before = snapshot(&world, bounds);
    let mut compiler = Compiler::default();
    let error = compiler
        .compile(
            &world,
            world.get_corners(),
            CompilerOptions {
                assume_instant: true,
                ..Default::default()
            },
            Vec::new(),
            Default::default(),
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("logical piston admission failed"), "{error}");
    assert!(
        error.contains("sampling") || error.contains("clock"),
        "diagnostic must identify the missing state boundary: {error}"
    );
    assert!(
        error.contains("no independent sampling source; data changes alone cannot update memory"),
        "diagnostic must explain the missing update or sampling source: {error}"
    );
    assert!(error.contains(&format!("{BASE:?}")), "{error}");
    assert!(!error.contains("--assume-instant"), "{error}");
    assert!(!compiler.is_active());
    assert_eq!(snapshot(&world, bounds), before);
}

#[test]
fn ideal_rejects_combinational_feedback_with_positions_and_a_state_boundary_hint() {
    let mut world = empty();
    let second = BASE + BlockPos::new(2, 1, 0);
    for (pos, facing) in [(BASE, BlockFacing::East), (second, BlockFacing::West)] {
        let piston = RedstonePiston {
            facing,
            sticky: true,
            extended: true,
        };
        let head = pos.offset(facing.into());
        world.set_block(pos, Block::Piston { piston });
        world.set_block(
            head,
            Block::PistonHead {
                head: piston.into(),
            },
        );
        world.set_block(head.offset(facing.into()), Block::RedstoneBlock);
    }
    // Each far payload powers the other actor; neither edge is sampled memory.
    let bounds = (
        BASE - BlockPos::new(1, 1, 1),
        second + BlockPos::new(1, 2, 1),
    );
    let before = snapshot(&world, bounds);
    let mut compiler = Compiler::default();
    let error = compiler
        .compile(
            &world,
            world.get_corners(),
            CompilerOptions {
                assume_instant: true,
                ..Default::default()
            },
            Vec::new(),
            Default::default(),
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("cycle"), "{error}");
    assert!(
        error.contains(&format!("{BASE:?}")) && error.contains(&format!("{second:?}")),
        "feedback diagnostic must identify both actors: {error}"
    );
    assert!(
        error.contains("sampling") || error.contains("clock"),
        "feedback needs an explicit state boundary: {error}"
    );
    assert!(!compiler.is_active());
    assert_eq!(snapshot(&world, bounds), before);
}
