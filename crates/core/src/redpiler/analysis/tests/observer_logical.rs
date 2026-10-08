use super::*;
use mchprs_blocks::blocks::{
    Lever, LeverFace, RedstoneRepeater, RedstoneWire, RedstoneWireSide, RotateAmt,
};
use mchprs_blocks::BlockDirection;

fn side_return(rotation: Option<RotateAmt>) -> (PlotWorld, [BlockPos; 2], BlockPos, BlockPos) {
    let position = |pos: BlockPos| {
        let (x, z) = match rotation {
            None => (pos.x, pos.z),
            Some(RotateAmt::Rotate90) => (-pos.z, pos.x),
            Some(RotateAmt::Rotate180) => (-pos.x, -pos.z),
            Some(RotateAmt::Rotate270) => (pos.z, -pos.x),
        };
        BASE + BlockPos::new(x, pos.y, z)
    };
    let mut world = empty();
    let mut place = |pos, mut block: Block| {
        if let Some(rotation) = rotation {
            block.rotate(rotation);
        }
        world.set_block(position(pos), block);
    };
    let piston = RedstonePiston {
        facing: BlockFacing::East,
        sticky: true,
        extended: true,
    };
    place(BlockPos::new(0, 0, 0), Block::Piston { piston });
    place(
        BlockPos::new(1, 0, 0),
        Block::PistonHead {
            head: piston.into(),
        },
    );
    place(BlockPos::new(2, 0, 0), Block::RedstoneBlock);
    place(
        BlockPos::new(0, 0, 1),
        Block::Observer {
            observer: RedstoneObserver {
                facing: BlockFacing::North,
                powered: false,
            },
        },
    );
    // The side observer's fixed output cap returns through a dust stair to its owner.
    for pos in [
        BlockPos::new(0, 0, 2),
        BlockPos::new(0, 1, 1),
        BlockPos::new(0, 1, 0),
    ] {
        place(pos, Block::Stone {});
    }
    for (pos, north) in [
        (BlockPos::new(0, 1, 2), RedstoneWireSide::Up),
        (BlockPos::new(0, 2, 1), RedstoneWireSide::Side),
        (BlockPos::new(0, 2, 0), RedstoneWireSide::Side),
    ] {
        place(
            pos,
            Block::RedstoneWire {
                wire: RedstoneWire::new(
                    north,
                    RedstoneWireSide::Side,
                    RedstoneWireSide::None,
                    RedstoneWireSide::None,
                    0,
                ),
            },
        );
    }
    let inputs = [BlockPos::new(0, 0, -1), BlockPos::new(-1, 0, 0)];
    for pos in inputs {
        place(pos.offset(BlockFace::Bottom), Block::Stone {});
        place(
            pos,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, true),
            },
        );
    }
    let output = BlockPos::new(3, 0, 0);
    place(output.offset(BlockFace::Bottom), Block::Stone {});
    place(
        output,
        Block::RedstoneRepeater {
            repeater: RedstoneRepeater {
                facing: BlockDirection::West,
                powered: true,
                ..Default::default()
            },
        },
    );
    (
        world,
        inputs.map(position),
        position(output),
        position(BlockPos::new(0, 0, 1)),
    )
}

#[test]
fn logical_side_observer_rejects_a_return_that_drops_its_payload() {
    for rotation in [
        None,
        Some(RotateAmt::Rotate90),
        Some(RotateAmt::Rotate180),
        Some(RotateAmt::Rotate270),
    ] {
        let (mut native, inputs, _, _) = side_return(rotation);
        for input in inputs {
            lever_action(&mut native, input, false);
        }
        let trace = crate::redstone::piston::trace::capture(|| {
            for _ in 0..8 {
                native.tick_interpreted();
            }
        });
        assert!(
            trace.iter().any(|entry| matches!(
                entry.operation,
                crate::redstone::piston::trace::Operation::Applied(PistonEvent {
                    action: PistonAction::RetractWithoutPull,
                    ..
                })
            )),
            "the short return pulse interrupts extension and drops the payload"
        );
        for assume_instant in [false, true] {
            for optimize in [false, true] {
                let (world, _, _, observer) = side_return(rotation);
                let bounds = (BASE - BlockPos::new(5, 2, 5), BASE + BlockPos::new(5, 4, 5));
                let before = snapshot(&world, bounds);
                let mut compiler = Compiler::default();
                let error = compiler
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
                    .unwrap_err()
                    .to_string();
                assert!(error.contains("logical piston admission failed"), "{error}");
                assert!(
                    error.contains(
                        "falling pulse may retract during extension and drop the retained payload"
                    ),
                    "{error}"
                );
                assert!(error.contains(&format!("{observer:?}")), "{error}");
                assert!(!compiler.is_active());
                assert_eq!(snapshot(&world, bounds), before);
            }
        }
    }
}

#[test]
fn logical_side_observer_rejects_an_exposed_pulse_and_an_independent_sampler_transactionally() {
    for independent_sampler in [false, true] {
        let (mut world, _, _, _) = side_return(None);
        if independent_sampler {
            let pos = BASE + BlockPos::new(1, 2, 1);
            let piston = RedstonePiston {
                facing: BlockFacing::East,
                sticky: true,
                extended: true,
            };
            world.set_block(pos, Block::Piston { piston });
            world.set_block(
                pos.offset(BlockFace::East),
                Block::PistonHead {
                    head: piston.into(),
                },
            );
            world.set_block(pos + BlockPos::new(2, 0, 0), Block::RedstoneBlock);
        } else {
            let pos = BASE + BlockPos::new(0, 0, 3);
            world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
            world.set_block(
                pos,
                Block::RedstoneRepeater {
                    repeater: RedstoneRepeater {
                        facing: BlockDirection::North,
                        ..Default::default()
                    },
                },
            );
        }
        let bounds = (BASE - BlockPos::new(5, 2, 5), BASE + BlockPos::new(5, 4, 5));
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
        if independent_sampler {
            assert!(
                (error.contains("independently") || error.contains("independent sampling"))
                    && (error.contains("samples") || error.contains("sampling")),
                "{error}"
            );
            assert!(
                error.contains(&format!("{:?}", BASE + BlockPos::new(1, 2, 1))),
                "{error}"
            );
        } else {
            assert!(
                error.contains("reset pulse reaches ordinary data consumer"),
                "{error}"
            );
            assert!(
                error.contains(&format!("{:?}", BASE + BlockPos::new(0, 0, 3))),
                "{error}"
            );
        }
        assert!(!compiler.is_active());
        assert_eq!(snapshot(&world, bounds), before);
    }
}

#[test]
fn ordinary_observer_rejects_a_piston_output_without_state_notifications() {
    use mchprs_blocks::blocks::{Instrument, TrapdoorHalf};
    fn make_world(output_block: Block) -> (PlotWorld, BlockPos, BlockPos, BlockPos, BlockPos) {
        let mut world = empty();
        world.set_block(
            BASE,
            Block::Piston {
                piston: RedstonePiston {
                    facing: BlockFacing::East,
                    sticky: true,
                    extended: true,
                },
            },
        );
        world.set_block(
            BASE.offset(BlockFace::East),
            Block::PistonHead {
                head: RedstonePistonHead {
                    facing: BlockFacing::East,
                    sticky: true,
                    short: false,
                },
            },
        );
        world.set_block(BASE + BlockPos::new(2, 0, 0), Block::RedstoneBlock);
        let input = BASE.offset(BlockFace::North);
        world.set_block(input.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(
            input,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, true),
            },
        );
        // x OR !x is constant after settling, but moving payloads briefly
        // remove !x. Its native observer notifications depend on neighboring
        // piston updates rather than the trapdoor's state transition.
        let output = BASE + BlockPos::new(1, 0, -1);
        world.set_block(output, output_block);
        let observer = output.offset(BlockFace::North);
        world.set_block(
            observer,
            Block::Observer {
                observer: RedstoneObserver {
                    facing: BlockFacing::South,
                    powered: false,
                },
            },
        );
        let lamp = observer.offset(BlockFace::North);
        world.set_block(lamp, Block::RedstoneLamp { lit: false });
        (world, input, output, observer, lamp)
    }
    let trapdoor = Block::IronTrapdoor {
        facing: BlockDirection::North,
        half: TrapdoorHalf::Bottom,
        powered: true,
    };
    let (mut native, input, output, _, _) = make_world(trapdoor);
    lever_action(&mut native, input, false);
    assert!(matches!(
        native.get_block(output),
        Block::IronTrapdoor { powered: false, .. }
    ));
    assert!(
        native.scheduler().iter_entries().next().is_none(),
        "the trapdoor state change itself sends no observer notification"
    );
    for output_block in [
        trapdoor,
        Block::RedstoneLamp { lit: true },
        Block::NoteBlock {
            instrument: Instrument::Harp,
            note: 0,
            powered: true,
        },
    ] {
        for assume_instant in [false, true] {
            for optimize in [false, true] {
                let (world, _, _, observer, _) = make_world(output_block);
                let bounds = (BASE - BlockPos::new(3, 2, 4), BASE + BlockPos::new(3, 2, 3));
                let before = snapshot(&world, bounds);
                let mut compiler = Compiler::default();
                let error = compiler
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
                    .unwrap_err()
                    .to_string();
                assert!(error.contains("logical piston admission failed"), "{error}");
                assert!(
                error.contains(
                    "without state notifications; callback-dependent observations are unsupported"
                ),
                "{error}"
            );
                assert!(error.contains(&format!("{observer:?}")), "{error}");
                assert!(!compiler.is_active());
                assert_eq!(snapshot(&world, bounds), before);
            }
        }
    }
}

#[test]
fn ordinary_observer_tracks_either_owner_of_a_shared_far_payload() {
    let make_world = || {
        let (mut world, _, manifest) = fixture("or_1");
        let report = analyze_world(&world);
        let group = report
            .payload_groups
            .iter()
            .find(|group| group.members.len() > 1)
            .unwrap();
        let first = &report.pistons[group.members[0]];
        let far = first.head.offset(first.piston.facing.into());
        let observer = far.offset(BlockFace::Top);
        let lamp = observer.offset(BlockFace::Top);
        assert_eq!(world.get_block(observer), Block::Air);
        assert_eq!(world.get_block(lamp), Block::Air);
        world.set_block(
            observer,
            Block::Observer {
                observer: RedstoneObserver {
                    facing: BlockFacing::Down,
                    powered: false,
                },
            },
        );
        world.set_block(lamp, Block::RedstoneLamp { lit: false });
        (world, manifest, observer, lamp)
    };
    for assignment in [1, 2] {
        for optimize in [false, true] {
            let (mut world, manifest, observer, lamp) = make_world();
            let (mut native, _, _, _) = make_world();
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &world,
                    world.get_corners(),
                    CompilerOptions {
                        optimize,
                        ..Default::default()
                    },
                    vec![],
                    Default::default(),
                )
                .unwrap();
            for (bit, name) in ["IN1", "IN2"].into_iter().enumerate() {
                let input = local_pos(&manifest["ports"]["inputs"][name]);
                let powered = assignment & (1 << bit) != 0;
                let Block::Lever { lever } = world.get_block(input) else {
                    unreachable!()
                };
                if lever.powered != powered {
                    compiler.on_use_block(input);
                }
                lever_action(&mut native, input, powered);
            }
            let mut pulse = false;
            for tick in 1..=36 {
                native.tick_interpreted();
                compiler.tick_with_world(&mut world);
                compiler.flush(&mut world);
                assert_eq!(
                    world.get_block(observer),
                    native.get_block(observer),
                    "shared observer at tick {tick}, assignment {assignment}, optimize {optimize}"
                );
                assert_eq!(
                    world.get_block(lamp),
                    native.get_block(lamp),
                    "shared lamp at tick {tick}, assignment {assignment}, optimize {optimize}"
                );
                pulse |= matches!(native.get_block(lamp), Block::RedstoneLamp { lit: true });
            }
            assert!(pulse);
        }
    }
}

fn presentation_lamp(watched: bool) -> (PlotWorld, BlockPos, BlockPos, BlockPos, BlockPos) {
    let (mut world, _, manifest) = fixture("instant_observer");
    let base = local_pos(&manifest["ports"]["observations"]["base"]);
    let cap = base + BlockPos::new(0, 2, 0);
    world.set_block(
        cap,
        Block::Wool {
            color: mchprs_blocks::BlockColorVariant::White,
        },
    );
    let control = cap.offset(BlockFace::Top);
    world.set_block(
        control,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    let lamp = cap.offset(BlockFace::East);
    world.set_block(lamp, Block::RedstoneLamp { lit: false });
    if watched {
        world.set_block(
            lamp.offset(BlockFace::East),
            Block::Observer {
                observer: RedstoneObserver {
                    facing: BlockFacing::West,
                    powered: false,
                },
            },
        );
    }
    (
        world,
        local_pos(&manifest["ports"]["inputs"]["trigger"]),
        control,
        base,
        lamp,
    )
}

#[test]
fn unobserved_lamp_presentation_keeps_live_cap_control_and_omits_reset_flashes() {
    for assume_instant in [false, true] {
        for optimize in [false, true] {
            let (mut world, trigger, control, base, lamp) = presentation_lamp(false);
            let bounds = world.get_corners();
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &world,
                    bounds,
                    CompilerOptions {
                        assume_instant,
                        optimize,
                        ..Default::default()
                    },
                    vec![],
                    Default::default(),
                )
                .unwrap();
            compiler.on_use_block(trigger);
            for _ in 0..24 {
                compiler.tick();
                compiler.flush(&mut world);
                assert_eq!(
                    world.get_block(lamp),
                    Block::RedstoneLamp { lit: false },
                    "logical reset must omit the internal flash"
                );
            }
            for powered in [true, false] {
                compiler.on_use_block(control);
                for _ in 0..16 {
                    compiler.tick();
                    compiler.flush(&mut world);
                }
                assert_eq!(
                    world.get_block(lamp),
                    Block::RedstoneLamp { lit: powered },
                    "the fixed cap's independent ordinary control stays live"
                );
            }
            compiler.reset(&mut world, bounds);
            assert_eq!(world.get_block(lamp), Block::RedstoneLamp { lit: false });
            assert!(
                matches!(world.get_block(base), Block::Piston { piston } if !piston.extended),
                "the logical owner actually responded"
            );
            assert!(world.piston_state().events.is_empty());
            assert!(world.piston_state().motions.is_empty());
        }
    }
}

#[test]
fn watched_lamp_presentation_remains_a_notification_boundary() {
    for assume_instant in [false, true] {
        let (world, _, _, base, lamp) = presentation_lamp(true);
        let bounds = (base - BlockPos::new(3, 2, 6), base + BlockPos::new(4, 4, 6));
        let before = snapshot(&world, bounds);
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
            .unwrap_err()
            .to_string();
        assert!(error.contains("logical piston admission failed"), "{error}");
        assert!(
            error.contains(&format!("{lamp:?}")),
            "the observed presentation boundary must be located: {error}"
        );
        assert!(
            error.contains("consumer") || error.contains("watched"),
            "{error}"
        );
        assert!(!compiler.is_active());
        assert_eq!(snapshot(&world, bounds), before);
    }
}

#[test]
fn ordinary_observer_cannot_watch_an_omitted_owned_reset_signal() {
    for assume_instant in [false, true] {
        let (mut world, _, manifest) = fixture("instant_observer");
        let base = local_pos(&manifest["ports"]["observations"]["base"]);
        let reset = base.offset(BlockFace::Top);
        let watcher = reset.offset(BlockFace::East);
        world.set_block(
            watcher,
            Block::Observer {
                observer: RedstoneObserver {
                    facing: BlockFacing::West,
                    powered: false,
                },
            },
        );
        let bounds = (base - BlockPos::new(3, 2, 6), base + BlockPos::new(4, 4, 6));
        let before = snapshot(&world, bounds);
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
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("reset observer is watched by observer"),
            "{error}"
        );
        assert!(error.contains("reset pulse cannot be omitted"), "{error}");
        assert!(error.contains(&format!("{reset:?}")), "{error}");
        assert!(error.contains(&format!("{watcher:?}")), "{error}");
        assert!(!compiler.is_active());
        assert_eq!(snapshot(&world, bounds), before);
    }
}
