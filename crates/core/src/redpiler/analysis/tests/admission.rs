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

// A repeater feeds one receiving dust through a separately movable conductor.
fn redundant_qc_feedback_fixture() -> (PlotWorld, BlockPos) {
    use mchprs_blocks::blocks::{RedstoneRepeater, RedstoneWire, RedstoneWireSide};
    use mchprs_blocks::BlockDirection;
    let mut world = empty();
    for (pos, facing, payload) in [
        (BASE, BlockFacing::Down, Block::RedstoneBlock),
        (
            BASE + BlockPos::new(0, 1, 4),
            BlockFacing::North,
            Block::Stone {},
        ),
    ] {
        let piston = RedstonePiston {
            facing,
            sticky: true,
            extended: true,
        };
        world.set_block(pos, Block::Piston { piston });
        let head = pos.offset(facing.into());
        world.set_block(
            head,
            Block::PistonHead {
                head: piston.into(),
            },
        );
        world.set_block(head.offset(facing.into()), payload);
    }
    world.set_block(BASE + BlockPos::new(0, 0, 1), Block::Stone {});
    world.set_block(
        BASE + BlockPos::new(0, 1, 1),
        Block::RedstoneWire {
            wire: RedstoneWire::new(
                RedstoneWireSide::Side,
                RedstoneWireSide::Side,
                RedstoneWireSide::Side,
                RedstoneWireSide::Side,
                15,
            ),
        },
    );
    let data = BASE + BlockPos::new(1, 1, 2);
    world.set_block(data.offset(BlockFace::Bottom), Block::Stone {});
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
    (world, data)
}

fn validate_redundant_qc_fixture(world: &PlotWorld, data: BlockPos) -> Result<(), String> {
    sampling::validate_feedback(
        world,
        &analyze_world(world),
        &TaskMonitor::default(),
        &[BASE].into_iter().collect(),
        &[data].into_iter().collect(),
    )
}

#[test]
fn same_dust_direct_power_dominates_redundant_qc_through_fixed_support() {
    use crate::redpiler::analysis::topology::PowerRoute;
    let (world, data) = redundant_qc_feedback_fixture();
    let report = analyze_world(&world);
    let actor = report.pistons.iter().position(|p| p.pos == BASE).unwrap();
    for route in [PowerRoute::Direct, PowerRoute::QuasiConnectivity] {
        assert!(report.recognition[actor]
            .inputs
            .sources
            .iter()
            .any(|s| s.source == data && s.route == route));
    }
    validate_redundant_qc_fixture(&world, data).unwrap();
}

#[test]
fn same_dust_requires_conducting_stationary_support_outside_front_face() {
    for condition in ["glass", "mobile", "front"] {
        let (mut world, data) = redundant_qc_feedback_fixture();
        match condition {
            "glass" => {
                world.set_block(BASE + BlockPos::new(0, 0, 1), Block::Glass {});
            }
            "mobile" => {
                let pos = BASE + BlockPos::new(2, 0, 1);
                let piston = RedstonePiston {
                    facing: BlockFacing::West,
                    sticky: true,
                    extended: true,
                };
                world.set_block(pos, Block::Piston { piston });
                world.set_block(
                    pos.offset(BlockFace::West),
                    Block::PistonHead {
                        head: piston.into(),
                    },
                );
            }
            "front" => {
                world.set_block(BASE.offset(BlockFace::Bottom), Block::Air);
                world.set_block(
                    BASE,
                    Block::Piston {
                        piston: RedstonePiston {
                            facing: BlockFacing::South,
                            sticky: true,
                            extended: false,
                        },
                    },
                );
            }
            _ => unreachable!(),
        }
        let error = validate_redundant_qc_fixture(&world, data).unwrap_err();
        assert!(
            error.contains("QC sampling interface"),
            "{condition}: {error}"
        );
    }
}

#[test]
fn shared_upstream_source_does_not_merge_distinct_receiving_dust_roots() {
    use mchprs_blocks::blocks::{RedstoneWire, RedstoneWireSide};
    let (mut world, data) = redundant_qc_feedback_fixture();
    // The upper QC root is outside the base's notification neighborhood.
    for delta in [
        (-1, 1, 2),
        (-2, 2, 2),
        (-3, 3, 2),
        (-3, 3, 1),
        (-3, 3, 0),
        (-2, 3, 0),
        (-1, 3, 0),
        (0, 3, 0),
    ] {
        let pos = BASE + BlockPos::new(delta.0, delta.1, delta.2);
        world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(
            pos,
            Block::RedstoneWire {
                wire: RedstoneWire::new(
                    RedstoneWireSide::Side,
                    RedstoneWireSide::Side,
                    RedstoneWireSide::Side,
                    RedstoneWireSide::Side,
                    15,
                ),
            },
        );
    }
    let report = analyze_world(&world);
    let actor = report.pistons.iter().position(|p| p.pos == BASE).unwrap();
    assert!(report.recognition[actor]
        .inputs
        .wires
        .contains(&(BASE + BlockPos::new(0, 3, 0))));
    let error = validate_redundant_qc_fixture(&world, data).unwrap_err();
    assert!(error.contains("QC sampling interface"), "{error}");
}

#[test]
fn future_qc_conductor_must_not_hide_an_independent_data_path_at_entry() {
    use mchprs_blocks::blocks::RedstoneRepeater;
    use mchprs_blocks::BlockDirection;
    let (mut world, notified_data) = redundant_qc_feedback_fixture();
    let moving_base = BASE + BlockPos::new(0, 2, -2);
    world.set_block(
        moving_base,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::South,
                sticky: true,
                extended: false,
            },
        },
    );
    world.set_block(moving_base.offset(BlockFace::South), Block::Stone {});
    let hidden_data = BASE + BlockPos::new(1, 2, 0);
    world.set_block(
        hidden_data,
        Block::RedstoneRepeater {
            repeater: RedstoneRepeater {
                facing: BlockDirection::East,
                powered: true,
                ..Default::default()
            },
        },
    );
    let report = analyze_world(&world);
    let actor = report.pistons.iter().position(|p| p.pos == BASE).unwrap();
    assert!(!report.recognition[actor]
        .inputs
        .sources
        .iter()
        .any(|s| s.source == hidden_data));
    let error = sampling::validate_feedback(
        &world,
        &report,
        &TaskMonitor::default(),
        &[BASE].into_iter().collect(),
        &[notified_data, hidden_data].into_iter().collect(),
    )
    .unwrap_err();
    assert!(error.contains("QC sampling interface"), "{error}");
    assert!(error.contains(&format!("{hidden_data:?}")), "{error}");
}

#[test]
fn qc_coupling_proof_requires_complete_context_and_an_active_monitor() {
    let (world, data) = redundant_qc_feedback_fixture();
    let mut report = analyze_world(&world);
    let targets = [BASE].into_iter().collect();
    let internal = [data].into_iter().collect();
    let monitor = TaskMonitor::default();
    monitor.cancel();
    let error =
        sampling::validate_feedback(&world, &report, &monitor, &targets, &internal).unwrap_err();
    assert!(error.contains("cancelled"), "{error}");

    report.bounds.1 = BASE + BlockPos::new(1, 1, 1);
    let error = sampling::validate_feedback(
        &world,
        &report,
        &TaskMonitor::default(),
        &targets,
        &internal,
    )
    .unwrap_err();
    assert!(error.contains("outside the selection"), "{error}");
}
