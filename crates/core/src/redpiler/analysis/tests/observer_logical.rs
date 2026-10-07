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
fn logical_side_observer_return_is_derived_from_rotated_geometry_and_held_data() {
    for rotation in [
        None,
        Some(RotateAmt::Rotate90),
        Some(RotateAmt::Rotate180),
        Some(RotateAmt::Rotate270),
    ] {
        for left in [false, true] {
            for right in [false, true] {
                let mut restored = None;
                for optimize in [false, true] {
                    let (mut world, inputs, output, observer) = side_return(rotation);
                    let Block::RedstoneRepeater { repeater } = world.get_block(output) else {
                        unreachable!()
                    };
                    assert_eq!(
                        world.get_block(output.offset(repeater.facing.block_face())),
                        Block::RedstoneBlock
                    );
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
                    for (input, powered) in inputs.into_iter().zip([left, right]) {
                        if !powered {
                            compiler.on_use_block(input);
                        }
                    }
                    for _ in 0..16 {
                        compiler.tick_with_world(&mut world);
                    }
                    compiler.flush(&mut world);
                    let Block::RedstoneRepeater { repeater } = world.get_block(output) else {
                        unreachable!()
                    };
                    assert_eq!(
                        repeater.powered,
                        left || right,
                        "the first data response is OR; its observer return is internal reset work"
                    );
                    let backend = compiler.backend.as_ref().unwrap();

                    let settled = backend.logical_stats();
                    assert!(!settled.is_empty());
                    assert!(settled
                        .iter()
                        .all(|(_, samples, memory)| *samples == 0 && memory.is_empty()));
                    for _ in 0..32 {
                        compiler.tick_with_world(&mut world);
                    }
                    assert_eq!(
                        compiler.backend.as_ref().unwrap().logical_stats(),
                        settled,
                        "held data must not replay the observer reset or reevaluate its domain"
                    );
                    assert!(world.piston_state().motions.is_empty());
                    assert!(world.piston_state().events.is_empty());

                    let bounds = world.get_corners();
                    compiler.reset(&mut world, bounds);
                    assert!(!compiler.is_active());
                    let Block::Piston { piston } = world.get_block(BASE) else {
                        unreachable!()
                    };
                    assert_eq!(piston.extended, left || right);
                    assert!(
                        matches!(world.get_block(observer), Block::Observer { observer } if !observer.powered)
                    );
                    assert!(world.piston_state().motions.is_empty());
                    assert!(world.piston_state().events.is_empty());
                    assert!(!world.scheduler().iter_entries().any(|tick| matches!(
                        world.get_block(tick.pos),
                        Block::Piston { .. } | Block::Observer { .. }
                    )));
                    let snapshot = snapshot(
                        &world,
                        (BASE - BlockPos::new(5, 2, 5), BASE + BlockPos::new(5, 4, 5)),
                    );
                    if let Some(previous) = &restored {
                        assert_eq!(&snapshot, previous);
                    }
                    restored = Some(snapshot);
                }
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
                (error.contains("independently") || error.contains("independent BUD"))
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
fn ordinary_observer_sees_only_committed_constant_logical_outputs() {
    use mchprs_blocks::blocks::TrapdoorHalf;
    for optimize in [false, true] {
        for io_only in [false, true] {
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
            // Direct input x and the owner's near payload !x provide a constant
            // settled level. Neither transaction may expose an old/new mixture.
            let output = BASE + BlockPos::new(1, 0, -1);
            world.set_block(
                output,
                Block::IronTrapdoor {
                    facing: BlockDirection::North,
                    half: TrapdoorHalf::Bottom,
                    powered: true,
                },
            );
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
            let bounds = world.get_corners();
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &world,
                    bounds,
                    CompilerOptions {
                        assume_instant: true,
                        optimize,
                        io_only,
                        ..Default::default()
                    },
                    vec![],
                    Default::default(),
                )
                .unwrap();
            for _ in 0..12 {
                compiler.tick();
                compiler.flush(&mut world);
            }
            for _ in 0..2 {
                compiler.on_use_block(input);
                compiler.flush(&mut world);
                for _ in 0..16 {
                    assert!(matches!(
                        world.get_block(output),
                        Block::IronTrapdoor { powered: true, .. }
                    ));
                    assert_eq!(
                        world.get_block(lamp),
                        Block::RedstoneLamp { lit: false },
                        "an unchanged committed level must not deliver an observer pulse"
                    );
                    if !io_only {
                        assert!(
                            matches!(world.get_block(observer), Block::Observer { observer } if !observer.powered)
                        );
                    }
                    compiler.tick();
                    compiler.flush(&mut world);
                }
            }
            compiler.reset(&mut world, bounds);
            assert!(matches!(
                world.get_block(output),
                Block::IronTrapdoor { powered: true, .. }
            ));
            assert_eq!(world.get_block(lamp), Block::RedstoneLamp { lit: false });
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
        assert!(error.contains("owned reset observer"), "{error}");
        assert!(error.contains("omitted reset signal"), "{error}");
        assert!(error.contains("state boundary"), "{error}");
        assert!(error.contains(&format!("{reset:?}")), "{error}");
        assert!(error.contains(&format!("{watcher:?}")), "{error}");
        assert!(!compiler.is_active());
        assert_eq!(snapshot(&world, bounds), before);
    }
}
