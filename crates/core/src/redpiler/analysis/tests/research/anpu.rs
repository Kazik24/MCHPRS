//! ANPU admission and visible-output checks against the unchanged Pong fixture.
use super::*;

#[test]
#[ignore = "ANPU isolated sampled preparation; region ownership is bypassed, no execution"]
fn anpu_isolated_sampled_preparation() {
    let world = cpus::load_cpu(cpus::CPUS[1]);
    let before = cpus::checkpoint(&world, 0, &[]);
    let ticks = world.scheduler().iter_entries().collect::<Vec<_>>();
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let started = Instant::now();
    let report = analyze(
        &world,
        world.get_corners(),
        &ticks,
        &monitor,
        AnalysisLimits::for_budget(8),
    )
    .unwrap();
    println!(
        "ANPU analysis: {:?}, {} dependency steps, {} pistons, {} observers, {} groups",
        started.elapsed(),
        report.dependency_steps,
        report.pistons.len(),
        report.observers.len(),
        report.payload_groups.len()
    );
    let options = CompilerOptions {
        budget_multiplier: 8,
        ..Default::default()
    };
    let started = Instant::now();
    match crate::redpiler::instant::sequential::prepare(&world, &report, &ticks, &options, &monitor)
    {
        Ok(program) => {
            let sequential = program.sequential.as_ref().unwrap();
            println!("ANPU isolated sampled preparation: {:?}, {} decisions, {} sources, {} sensors, {} outputs, {} observers", started.elapsed(), program.logic.arena.nodes.len(), program.logic.sources.len(), sequential.sensors.len(), program.logic.outputs.len(), sequential.observers.len());
        }
        Err(error) => println!(
            "ANPU isolated sampled preparation: {:?}, {error}",
            started.elapsed()
        ),
    }
    println!("Scope: region ownership is bypassed only in this diagnostic; no backend is activated and no compiled output is compared.");
    assert_eq!(cpus::checkpoint(&world, 0, &[]), before);
}

#[test]
#[ignore = "ANPU admission matrix; opt-in larger fixture analysis"]
fn anpu_current_compile_admission() {
    let cpu = cpus::CPUS[1];
    let world = cpus::load_cpu(cpu);
    let before = cpus::checkpoint(&world, 0, &[]);
    let ticks = world.scheduler().iter_entries().collect::<Vec<_>>();
    for budget_multiplier in [1, 8] {
        for assume_instant in [false, true] {
            let mut compiler = Compiler::default();
            let result = compiler.compile(
                &world,
                world.get_corners(),
                CompilerOptions {
                    assume_instant,
                    budget_multiplier,
                    ..Default::default()
                },
                ticks.clone(),
                Default::default(),
            );
            let error = result.unwrap_err().to_string();
            println!("ANPU budget={budget_multiplier}, assume_instant={assume_instant}: {error}");
            if budget_multiplier == 1 {
                assert!(error.contains("dependency budget"), "{error}");
            } else if assume_instant {
                assert!(
                    error.contains("--assume-instant requires a certified logical domain"),
                    "{error}"
                );
            } else {
                assert!(error.contains("ordinary non-instant movement"), "{error}");
            }
            assert!(!compiler.is_active());
            assert!(compiler.current_flags().is_none());
            assert_eq!(
                cpus::checkpoint(&world, 0, &[]),
                before,
                "admission must preserve physical state and scheduled work"
            );
        }
    }
    println!("ANPU admission: all four configurations rejected transactionally");
}

#[test]
fn anpu_rejects_mechanisms_without_certified_instant_boundaries() {
    let cpu = cpus::CPUS[1];
    let world = cpus::load_cpu(cpu);
    let before = cpus::checkpoint(&world, 0, &[]);
    let ticks = world.scheduler().iter_entries().collect::<Vec<_>>();
    // A ready mechanism in the unchanged fixture has no recognized instant
    // response/reset contract. A larger analysis budget cannot certify it.
    for assume_instant in [false, true] {
        let mut compiler = Compiler::default();
        let error = compiler
            .compile(
                &world,
                world.get_corners(),
                CompilerOptions {
                    assume_instant,
                    budget_multiplier: 8,
                    ..Default::default()
                },
                ticks.clone(),
                Default::default(),
            )
            .unwrap_err()
            .to_string();
        if assume_instant {
            assert!(
                error.contains("--assume-instant requires a certified logical domain"),
                "{error}"
            );
            assert!(
                error.contains("boundary") || error.contains("sampling"),
                "{error}"
            );
        } else {
            assert!(
                error.contains("BlockPos { x: 106, y: 33, z: 80 }"),
                "{error}"
            );
            assert!(
                error.contains(
                    "neither a certified observer reset nor a proven payload-following response"
                ),
                "{error}"
            );
            assert!(error.contains("ordinary non-instant movement"), "{error}");
        }
        assert!(!compiler.is_active());
        assert!(compiler.current_flags().is_none());
        assert_eq!(cpus::checkpoint(&world, 0, &[]), before);
        assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), ticks);
    }
}

#[test]
fn anpu_empty_generator_far_base_notification_probe() {
    use mchprs_blocks::blocks::{Lever, LeverFace, RedstoneObserver, RedstonePiston};
    use mchprs_blocks::{BlockDirection, BlockFace, BlockFacing};

    let second = BASE + BlockPos::new(2, 0, 0);
    let watched = second.offset(BlockFace::Top);
    let input = BASE.offset(BlockFace::North);
    for assume_instant in [false] {
        let build = || {
            let mut world = empty();
            for pos in [BASE, second] {
                world.set_block(
                    pos,
                    Block::Piston {
                        piston: RedstonePiston {
                            sticky: false,
                            extended: false,
                            facing: BlockFacing::East,
                        },
                    },
                );
            }
            world.set_block(
                watched,
                Block::Observer {
                    observer: RedstoneObserver {
                        facing: BlockFacing::Down,
                        powered: false,
                    },
                },
            );
            world.set_block(input.offset(BlockFace::Bottom), Block::Stone {});
            world.set_block(
                input,
                Block::Lever {
                    lever: Lever {
                        face: LeverFace::Floor,
                        facing: BlockDirection::North,
                        powered: false,
                    },
                },
            );
            world
        };
        let compiled = build();
        let mut interpreted = build();
        let report = analyze_world(&compiled);
        let options = CompilerOptions {
            assume_instant,
            ..Default::default()
        };
        let monitor = crate::redpiler::TaskMonitor::default();
        // Isolate payload execution: only region partition is bypassed here.
        let prepared = crate::redpiler::instant::sequential::prepare(
            &compiled,
            &report,
            &[],
            &options,
            &monitor,
        )
        .unwrap();
        let mut boundaries = crate::redpiler::instant::boundary::Boundaries::executable(
            &report,
            &prepared.logic.wires,
            &prepared.logic.sources,
            &prepared.logic.outputs,
        );
        boundaries.retain_sequential_sources(
            prepared
                .logic
                .sources
                .iter()
                .copied()
                .chain(prepared.logic.outputs.iter().map(|output| output.consumer)),
        );
        boundaries.own_sampled_wires(
            prepared
                .sequential
                .as_ref()
                .unwrap()
                .sensors
                .iter()
                .map(|sensor| sensor.pos),
        );
        let graph = crate::redpiler::passes::run_passes(
            &options,
            &crate::redpiler::CompilerInput {
                world: &compiled,
                bounds: compiled.get_corners(),
                ticks: &[],
                boundaries: Some(&boundaries),
            },
            &monitor,
        )
        .unwrap();
        let mut backend = crate::redpiler::backend::direct::DirectBackend::default();
        backend
            .compile(graph, Vec::new(), &options, vec![prepared])
            .unwrap();
        println!("isolated sampled empty-generator probe: raw preparation with normal graph/backend binding; region partition bypassed only in this diagnostic");
        backend.on_use_block(input);
        lever_action(&mut interpreted, input, true);
        for tick in 1..=6 {
            backend.tick();
            interpreted.tick_interpreted();
            let actual = backend
                .sampled_signals()
                .into_iter()
                .find(|&(pos, _)| pos == watched)
                .unwrap()
                .1;
            let Block::Observer { observer } = interpreted.get_block(watched) else {
                unreachable!()
            };
            assert!(
                !observer.powered,
                "native empty extension never changes the far base"
            );
            assert_eq!(actual, 0, "empty generator must not notify its foreign far base, tick {tick}, assume_instant={assume_instant}");
            for (pos, block) in backend.sampled_geometry() {
                let expected = interpreted.get_block(pos);
                if matches!(block, Block::MovingPiston { .. })
                    && matches!(expected, Block::MovingPiston { .. })
                {
                    continue;
                }
                assert_eq!(block, expected, "geometry at {pos:?}, tick {tick}");
            }
        }
    }
}
