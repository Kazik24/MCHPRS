//! Structural roles must not turn stationary actors into unsampled memory.
use super::*;
use crate::redpiler::instant::{clocked, sampling};

#[test]
fn stationary_qc_piston_is_proved_constant_and_never_allocates_memory() {
    for assume_instant in [false, true] {
        for optimize in [false, true] {
            let mut world = empty();
            observer_seed(&mut world);
            world.set_block(BASE.offset(BlockFace::Top), Block::Air);
            world.set_block(BASE + BlockPos::new(0, 2, 0), Block::RedstoneBlock);
            let report = analyze_world(&world);
            assert!(sampling::fixed_powered(&report, 0));
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
                    vec![],
                    Default::default(),
                )
                .unwrap();
            let stats = compiler.backend.as_ref().unwrap().logical_stats();
            assert!(stats.iter().all(|(_, _, memory)| memory.is_empty()));
            for _ in 0..12 {
                compiler.tick_with_world(&mut world);
            }
            let bounds = world.get_corners();
            compiler.reset(&mut world, bounds);
            assert!(matches!(world.get_block(BASE), Block::Piston { piston } if piston.extended));
            assert!(world.piston_state().events.is_empty());
            assert!(world.piston_state().motions.is_empty());
        }
    }
}

#[test]
fn constant_power_does_not_prove_a_mismatched_saved_pose() {
    let mut world = empty();
    observer_seed(&mut world);
    world.set_block(BASE.offset(BlockFace::Top), Block::Air);
    world.set_block(BASE + BlockPos::new(0, 2, 0), Block::RedstoneBlock);
    let Block::Piston { mut piston } = world.get_block(BASE) else {
        unreachable!()
    };
    piston.extended = false;
    world.set_block(BASE, Block::Piston { piston });
    world.set_block(BASE.offset(BlockFace::South), Block::RedstoneBlock);
    world.set_block(BASE + BlockPos::new(0, 0, 2), Block::Air);
    let report = analyze_world(&world);
    assert!(!sampling::fixed_powered(&report, 0));
    for assume_instant in [false, true] {
        let mut compiler = Compiler::default();
        let error = compiler
            .compile(
                &world,
                world.get_corners(),
                CompilerOptions {
                    assume_instant,
                    ..Default::default()
                },
                vec![],
                Default::default(),
            )
            .unwrap_err();
        assert!(
            error.to_string().contains("no independent sampling source"),
            "{error}"
        );
    }
}

#[test]
fn overhead_observer_without_sampling_output_does_not_claim_a_clock() {
    let mut world = empty();
    world.set_block(
        BASE,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::Down,
                sticky: false,
                extended: false,
            },
        },
    );
    world.set_block(
        BASE.offset(BlockFace::Top),
        Block::Observer {
            observer: RedstoneObserver {
                facing: BlockFacing::Down,
                powered: false,
            },
        },
    );
    world.set_block(BASE + BlockPos::new(0, 2, 0), Block::Stone {});
    let report = analyze_world(&world);
    for assume_instant in [false, true] {
        assert!(
            clocked::recognize(&world, &report, &TaskMonitor::default(), assume_instant)
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn conductor_reset_requires_a_fixed_repeater_supply_without_lock_inputs() {
    use mchprs_blocks::blocks::{
        Lever, LeverFace, RedstoneRepeater, RedstoneWire, RedstoneWireSide,
    };
    use mchprs_blocks::BlockDirection;
    let mut world = empty();
    let piston = RedstonePiston {
        facing: BlockFacing::South,
        sticky: true,
        extended: true,
    };
    let head = BASE.offset(BlockFace::South);
    let dust = head.offset(BlockFace::Bottom);
    let emitter = head.offset(BlockFace::East);
    let supply = emitter.offset(BlockFace::East);
    world.set_block(BASE, Block::Piston { piston });
    world.set_block(
        head,
        Block::PistonHead {
            head: piston.into(),
        },
    );
    world.set_block(head.offset(BlockFace::South), Block::Stone {});
    world.set_block(BASE.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        dust,
        Block::RedstoneWire {
            wire: RedstoneWire::new(
                RedstoneWireSide::Side,
                RedstoneWireSide::Side,
                RedstoneWireSide::Side,
                RedstoneWireSide::Side,
                0,
            ),
        },
    );
    world.set_block(dust.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(emitter.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        emitter,
        Block::RedstoneRepeater {
            repeater: RedstoneRepeater {
                facing: BlockDirection::East,
                powered: true,
                ..Default::default()
            },
        },
    );
    world.set_block(supply, Block::RedstoneBlock);
    let input = BASE.offset(BlockFace::North);
    world.set_block(input.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        input,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, true),
        },
    );
    let report = analyze_world(&world);
    assert!(
        report.recognition[0].is_matched(),
        "{:?}",
        report.recognition[0]
    );
    world.set_block(supply, Block::Air);
    assert!(!analyze_world(&world).recognition[0].is_matched());
    world.set_block(supply, Block::RedstoneBlock);
    let lock = emitter.offset(BlockFace::South);
    world.set_block(
        lock,
        Block::RedstoneRepeater {
            repeater: RedstoneRepeater {
                facing: BlockDirection::South,
                powered: false,
                ..Default::default()
            },
        },
    );
    assert!(!analyze_world(&world).recognition[0].is_matched());
}

#[test]
fn prepared_qc_data_is_allowed_but_unrepresented_internal_feedback_is_rejected() {
    use mchprs_blocks::blocks::RedstoneRepeater;
    use mchprs_blocks::BlockDirection;
    let (mut world, cells, data, sample) = super::memory::wire_bank();
    world.set_block(
        data,
        Block::RedstoneRepeater {
            repeater: RedstoneRepeater {
                facing: BlockDirection::East,
                powered: true,
                ..Default::default()
            },
        },
    );
    let clock = sample + BlockPos::new(-2, 0, 0);
    let piston = RedstonePiston {
        facing: BlockFacing::East,
        sticky: true,
        extended: true,
    };
    world.set_block(clock, Block::Piston { piston });
    world.set_block(
        clock.offset(BlockFace::East),
        Block::PistonHead {
            head: piston.into(),
        },
    );
    world.set_block(sample, Block::RedstoneBlock);
    let report = analyze_world(&world);
    let targets = [cells[0]].into_iter().collect();
    sampling::validate_feedback(
        &world,
        &report,
        &TaskMonitor::default(),
        &targets,
        &Default::default(),
    )
    .unwrap();
    let internal = [data].into_iter().collect();
    let error = sampling::validate_feedback(
        &world,
        &report,
        &TaskMonitor::default(),
        &targets,
        &internal,
    )
    .unwrap_err();
    assert!(
        error.contains("unsupported internally driven QC sampling interface"),
        "{error}"
    );
    assert!(error.contains(&format!("{:?}", cells[0])), "{error}");
    assert!(error.contains(&format!("{data:?}")), "{error}");
    sampling::validate_feedback(
        &world,
        &report,
        &TaskMonitor::default(),
        &Default::default(),
        &internal,
    )
    .unwrap();
}
