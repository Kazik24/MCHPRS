use super::*;
use crate::{
    plot::{PLOT_WIDTH, PlotWorld},
    redpiler::{Compiler, CompilerOptions, TaskMonitor},
    world::{World, storage::Chunk},
};
use mchprs_blocks::{
    BlockDirection, BlockFace, BlockPos,
    blocks::{Lever, LeverFace, TrapdoorHalf},
};
use std::sync::Arc;

fn world() -> PlotWorld {
    let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
    PlotWorld::from_chunks(0, 0, chunks, Default::default())
}

#[test]
fn every_inert_registry_state_is_admitted() {
    let mut world = world();
    let mut count = 0;
    for id in 0..mchprs_blocks::generated::STATE_TO_BLOCK_INDEX.len() as u32 {
        let block = Block::from_id(id);
        if crate::redstone::has_neighbor_update(block)
            || matches!(
                block,
                Block::Lever { .. } | Block::StoneButton { .. } | Block::RedstoneBlock | Block::MovingPiston { .. }
            )
            || block.pressure_plate_powered().is_some()
        {
            continue;
        }
        let pos = BlockPos::new(40 + count % 32, 20 + count / 1024, 40 + (count / 32) % 32);
        world.set_block(pos, block);
        count += 1;
    }
    let bounds = (BlockPos::new(39, 19, 39), BlockPos::new(72, 21 + count / 1024, 72));
    let program = compile(
        &world,
        bounds,
        ScratchEntry {
            scheduled: vec![],
            piston: Default::default(),
        },
    )
    .unwrap();
    assert!(program.initial.is_empty());
    let runtime = program.start();
    for (pos, block) in &runtime.program.geometry {
        assert_eq!(runtime.project(*pos), Some(*block));
    }
}

#[test]
fn captured_pressure_plate_power_and_trapdoor_updates_match_interpreter() {
    for name in ["stone_pressure_plate", "oak_pressure_plate"] {
        for powered in [false, true] {
            let mut native = world();
            let plate = BlockPos::new(40, 30, 40);
            let trapdoor = plate + BlockPos::new(1, 0, 0);
            let lever = plate + BlockPos::new(2, 0, 0);
            for pos in [plate, lever] {
                native.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
            }
            native.set_block(plate, Block::from_name(name).unwrap().with_pressure_plate_power(powered).unwrap());
            native.set_block(
                trapdoor,
                Block::IronTrapdoor {
                    facing: BlockDirection::North,
                    half: TrapdoorHalf::Bottom,
                    powered: false,
                },
            );
            native.set_block(
                lever,
                Block::Lever {
                    lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
                },
            );
            let mut published = native.activation_snapshot();
            let bounds = (plate - BlockPos::new(2, 2, 2), lever + BlockPos::new(2, 2, 2));
            let mut compiler = Compiler::default();
            compiler
                .compile(&native, bounds, CompilerOptions::default(), vec![], Arc::new(TaskMonitor::default()))
                .unwrap();
            for _ in 0..4 {
                crate::interaction::use_redstone(native.get_block(lever), &mut native, lever, None);
                compiler.try_on_use_block(lever).unwrap();
                for _ in 0..4 {
                    native.tick_interpreted();
                    compiler.tick().unwrap();
                    compiler.try_flush_complete(&mut published).unwrap();
                    assert_eq!(published.get_block(trapdoor), native.get_block(trapdoor), "{name}, powered={powered}");
                    assert_eq!(published.get_block(plate), native.get_block(plate));
                }
            }
        }
    }
}

#[test]
fn command_lifecycle_output_and_comparators_match_interpreter() {
    use mchprs_blocks::block_entities::{BlockEntity, CommandBlockEntity};
    use std::collections::HashMap;
    for name in ["command_block", "repeating_command_block"] {
        for valid in [false, true] {
            let mut native = world();
            native.disable_command_output_limits_for_replay();
            let command = BlockPos::new(40, 30, 40);
            let lever = command.offset(BlockFace::West);
            let chain = command.offset(BlockFace::North);
            let comparator = command.offset(BlockFace::East);
            for pos in [lever, comparator] {
                native.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
            }
            native.set_block(
                lever,
                Block::Lever {
                    lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
                },
            );
            native.set_block(command, Block::from_name(name).unwrap());
            let entity = CommandBlockEntity {
                command: if valid { "tellraw @a \"hello\"" } else { "unsupported-command" }.into(),
                ..Default::default()
            };
            native.set_block_entity(command, BlockEntity::CommandBlock(Box::new(entity)));
            let mut block = Block::from_name("chain_command_block").unwrap();
            block.set_properties(HashMap::from([("conditional", "true")]));
            native.set_block(chain, block);
            native.set_block_entity(
                chain,
                BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
                    command: "say chained".into(),
                    automatic: true,
                    ..Default::default()
                })),
            );
            let mut block = Block::from_name("comparator").unwrap();
            block.set_properties(HashMap::from([("facing", "west")]));
            native.set_block(comparator, block);
            native.set_block_entity(comparator, BlockEntity::Comparator { output_strength: 0 });
            let mut published = native.activation_snapshot();
            published.disable_command_output_limits_for_replay();
            let bounds = (command - BlockPos::new(4, 4, 4), command + BlockPos::new(4, 4, 4));
            let mut compiler = Compiler::default();
            compiler
                .compile(
                    &native,
                    bounds,
                    CompilerOptions::default(),
                    native.scheduler().iter_entries().collect(),
                    Arc::new(TaskMonitor::default()),
                )
                .unwrap();
            for tick in 0..16 {
                if [0, 3, 6, 12].contains(&tick) {
                    crate::interaction::use_redstone(native.get_block(lever), &mut native, lever, None);
                    compiler.try_on_use_block(lever).unwrap();
                }
                native.tick_interpreted();
                compiler.tick().unwrap();
                compiler.try_flush_complete(&mut published).unwrap();
                for pos in [command, chain, comparator] {
                    let expected = serde_json::to_value(native.get_block_entity(pos)).unwrap();
                    let actual = serde_json::to_value(published.get_block_entity(pos)).unwrap();
                    assert_eq!(actual, expected, "{name} valid={valid} tick={tick}, pos={pos:?}");
                    assert_eq!(published.get_block(pos), native.get_block(pos));
                }
                assert_eq!(
                    published.command_output().collect::<Vec<_>>(),
                    native.command_output().collect::<Vec<_>>(),
                    "{name} valid={valid} tick={tick}"
                );
                if tick == 7 {
                    compiler.try_reset(&mut published, bounds).unwrap();
                    assert_eq!(
                        published.scheduler().iter_entries().collect::<Vec<_>>(),
                        native.scheduler().iter_entries().collect::<Vec<_>>()
                    );
                    compiler
                        .compile(
                            &published,
                            bounds,
                            CompilerOptions::default(),
                            published.scheduler().iter_entries().collect(),
                            Arc::new(TaskMonitor::default()),
                        )
                        .unwrap();
                }
            }
            compiler.try_reset(&mut published, bounds).unwrap();
            assert_eq!(
                published.scheduler().iter_entries().collect::<Vec<_>>(),
                native.scheduler().iter_entries().collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn compiled_command_output_limits_match_interpreter() {
    use mchprs_blocks::block_entities::{BlockEntity, CommandBlockEntity};
    let mut native = world();
    let pos = BlockPos::new(40, 30, 40);
    native.set_block(pos, Block::from_name("repeating_command_block").unwrap());
    native.set_block_entity(
        pos,
        BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
            command: "say limited".into(),
            automatic: true,
            ..Default::default()
        })),
    );
    crate::redstone::command_block::update(&mut native, pos);
    let mut published = native.activation_snapshot();
    let bounds = (pos - BlockPos::new(3, 3, 3), pos + BlockPos::new(3, 3, 3));
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &native,
            bounds,
            CompilerOptions::default(),
            native.scheduler().iter_entries().collect(),
            Arc::new(TaskMonitor::default()),
        )
        .unwrap();
    for _ in 0..80 {
        native.tick_interpreted();
        compiler.tick().unwrap();
        compiler.try_flush_complete(&mut published).unwrap();
        assert_eq!(
            serde_json::to_value(published.get_block_entity(pos)).unwrap(),
            serde_json::to_value(native.get_block_entity(pos)).unwrap()
        );
        assert_eq!(
            published.command_output().collect::<Vec<_>>(),
            native.command_output().collect::<Vec<_>>()
        );
    }
    assert_eq!(published.command_output().count(), 64);
}
