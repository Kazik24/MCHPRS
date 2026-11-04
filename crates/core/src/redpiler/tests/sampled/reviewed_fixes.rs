use super::*;
use crate::plot::{PLOT_WIDTH, PlotWorld};
use crate::world::{World, storage::Chunk};
use mchprs_blocks::blocks::{Block, Lever, LeverFace, RedstoneObserver};
use mchprs_blocks::{BlockDirection, BlockFace, BlockFacing, BlockPos};

fn host() -> PlotWorld {
    PlotWorld::from_chunks(
        0,
        0,
        (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
        Default::default(),
    )
}

#[test]
#[ignore = "known source-sampling FIFO bug with a separately powered glass wire branch"]
fn peer_power_changes_wire_callback_fifo() {
    use mchprs_blocks::blocks::{RedstoneRepeater, RedstoneWire};

    let mut initial = host();
    let source = BlockPos::new(22, 15, 19);
    let peer = BlockPos::new(20, 15, 23);
    let targets = [BlockPos::new(19, 15, 20), BlockPos::new(25, 15, 20)];
    let mut wires = vec![BlockPos::new(20, 15, 22)];
    for x in 20..=24 {
        wires.extend([BlockPos::new(x, 15, 20), BlockPos::new(x, 16, 21)]);
    }
    for &at in &wires {
        // Glass makes the upper lane receive power without feeding it back into the lower lane.
        initial.set_block(at.offset(BlockFace::Bottom), if at.y == 16 { Block::Glass {} } else { Block::Stone {} });
        initial.set_block(
            at,
            Block::RedstoneWire {
                wire: RedstoneWire::default(),
            },
        );
    }
    for at in [source, peer] {
        initial.set_block(at.offset(BlockFace::Bottom), Block::Stone {});
        initial.set_block(
            at,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
            },
        );
    }
    for (at, facing) in targets.into_iter().zip([BlockDirection::East, BlockDirection::West]) {
        initial.set_block(at.offset(BlockFace::Bottom), Block::Stone {});
        initial.set_block(
            at,
            Block::RedstoneRepeater {
                repeater: RedstoneRepeater {
                    facing,
                    delay: 1,
                    locked: false,
                    powered: false,
                },
            },
        );
    }
    for &at in &wires {
        let wire = crate::redstone::wire::get_state_for_placement(&initial, at);
        initial.set_block(at, Block::RedstoneWire { wire });
    }
    let program = compiler::compile(
        &initial,
        (BlockPos::new(17, 13, 17), BlockPos::new(27, 18, 27)),
        ScratchEntry {
            scheduled: vec![],
            piston: initial.piston_state().clone(),
        },
    )
    .unwrap();
    for peer_on in [false, true] {
        let mut world = initial.activation_snapshot();
        let mut runtime = program.start();
        for at in [peer, source].into_iter().filter(|&at| at != peer || peer_on) {
            crate::interaction::use_redstone(world.get_block(at), &mut world, at, None);
            runtime.use_control(at).unwrap();
            assert_eq!(
                runtime.pending_entries(),
                world.scheduler().iter_entries().collect::<Vec<_>>(),
                "setup control={at:?}, peer_on={peer_on}"
            );
            for _ in 0..4 {
                world.tick_interpreted();
                runtime.tick().unwrap();
                assert_eq!(
                    runtime.pending_entries(),
                    world.scheduler().iter_entries().collect::<Vec<_>>(),
                    "settling control={at:?}, peer_on={peer_on}"
                );
            }
            if at == peer {
                for target in targets {
                    assert!(matches!(world.get_block(target), Block::RedstoneRepeater { repeater } if !repeater.powered));
                }
            }
        }
        assert!(world.scheduler().iter_entries().next().is_none());
        assert!(runtime.pending_entries().is_empty());
        for &at in &program.positions {
            assert_eq!(runtime.project(at), Some(world.get_block(at)), "before falling edge: {at:?}");
        }
        world.observe_redstone_updates(true);
        crate::interaction::use_redstone(world.get_block(source), &mut world, source, None);
        runtime.use_control(source).unwrap();
        let expected: Vec<_> = world.scheduler().iter_entries().collect();
        let actual = runtime.pending_entries();
        let callbacks: Vec<_> = world
            .take_redstone_update_observations()
            .into_iter()
            .filter(|event| targets.contains(&event.pos))
            .map(|event| (event.pos, event.input_on))
            .collect();
        eprintln!("peer_on={peer_on}: expected={expected:?}, actual={actual:?}, callbacks={callbacks:?}");
        assert_eq!(actual, expected, "falling edge with peer_on={peer_on}");
    }
}

#[test]
fn compiled_observer_watching_lamp_rearms_for_lamp_off_transition() {
    let mut world = host();
    let bounds = (BlockPos::new(10, 10, 10), BlockPos::new(20, 20, 20));
    let lever = BlockPos::new(15, 15, 15);
    let lamp = lever.offset(BlockFace::East);
    let observer = lamp.offset(BlockFace::East);
    world.set_block(
        lever,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    world.set_block(lever.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(lamp, Block::RedstoneLamp { lit: false });
    world.set_block(
        observer,
        Block::Observer {
            observer: RedstoneObserver {
                facing: BlockFacing::West,
                powered: false,
            },
        },
    );

    let entry = ScratchEntry {
        scheduled: world.scheduler().iter_entries().collect(),
        piston: world.piston_state().clone(),
    };
    let program = compiler::compile(&world, bounds, entry).unwrap();
    let source = program.sources.iter().position(|&index| program.positions[index] == lever).unwrap();
    let observer_index = program.positions.iter().position(|&pos| pos == observer).unwrap();
    let mut runtime = program.start();

    runtime.set_input(source, true).unwrap();
    assert_eq!(runtime.project(lamp), Some(Block::RedstoneLamp { lit: true }));
    assert!(runtime.pending_entries().iter().any(|tick| tick.pos == observer));
    for _ in 0..4 {
        if matches!(runtime.blocks[observer_index], Block::Observer { observer } if observer.powered) {
            break;
        }
        runtime.tick().unwrap();
    }
    assert!(matches!(runtime.blocks[observer_index], Block::Observer { observer } if observer.powered));

    for _ in 0..4 {
        if matches!(runtime.blocks[observer_index], Block::Observer { observer } if !observer.powered) {
            break;
        }
        runtime.tick().unwrap();
    }
    assert!(matches!(runtime.blocks[observer_index], Block::Observer { observer } if !observer.powered));

    runtime.set_input(source, false).unwrap();
    for _ in 0..8 {
        if runtime.project(lamp) == Some(Block::RedstoneLamp { lit: false }) {
            break;
        }
        runtime.tick().unwrap();
    }
    assert_eq!(runtime.project(lamp), Some(Block::RedstoneLamp { lit: false }));
    assert!(
        matches!(runtime.blocks[observer_index], Block::Observer { observer } if observer.powered)
            || runtime.pending_entries().iter().any(|tick| tick.pos == observer),
        "compiled observer did not receive/replay the lamp-off shape update",
    );
}

#[test]
fn command_block_reachable_only_by_moving_token_matches_native() {
    use mchprs_blocks::block_entities::{BlockEntity, CommandBlockEntity};
    use mchprs_blocks::blocks::RedstonePiston;
    let mut world = host();
    let base = BlockPos::new(15, 15, 15);
    let home = base.offset(BlockFace::East);
    let far = home.offset(BlockFace::East);
    let command = far.offset(BlockFace::South);
    let lever = base.offset(BlockFace::West);
    world.set_block(
        base,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::East,
                sticky: true,
                extended: false,
            },
        },
    );
    world.set_block(home, Block::RedstoneBlock);
    world.set_block(command, Block::from_name("command_block").unwrap());
    world.set_block_entity(
        command,
        BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
            command: "say moved".into(),
            ..Default::default()
        })),
    );
    world.set_block(
        lever,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    world.set_block(lever.offset(BlockFace::Bottom), Block::Stone {});
    let before = [base, home, far, command, lever].map(|pos| world.get_block(pos));
    let ticks = world.scheduler().iter_entries().collect::<Vec<_>>();
    let entry = ScratchEntry {
        scheduled: ticks.clone(),
        piston: world.piston_state().clone(),
    };
    let program = compiler::compile(&world, (BlockPos::new(10, 10, 10), BlockPos::new(22, 20, 20)), entry).unwrap();
    assert_eq!([base, home, far, command, lever].map(|pos| world.get_block(pos)), before);
    assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), ticks);
    let mut published = world.activation_snapshot();
    let mut runtime = start_runtime(program, ticks, world.piston_state().clone()).unwrap();
    crate::interaction::use_redstone(world.get_block(lever), &mut world, lever, None);
    runtime.set_input(0, true).unwrap();
    for _ in 0..12 {
        world.tick_interpreted();
        runtime.tick().unwrap();
        runtime.publish_commands(&mut published, false);
        assert_eq!(
            serde_json::to_value(published.get_block_entity(command)).unwrap(),
            serde_json::to_value(world.get_block_entity(command)).unwrap()
        );
        assert_eq!(published.command_output().collect::<Vec<_>>(), world.command_output().collect::<Vec<_>>());
    }
    assert_eq!(published.command_output().count(), 1);
}
