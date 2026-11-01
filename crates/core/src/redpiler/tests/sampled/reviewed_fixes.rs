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
