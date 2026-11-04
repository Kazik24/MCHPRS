use super::*;
use crate::redpiler::{Compiler, CompilerOptions, TaskMonitor};
use crate::world::World;
use mchprs_blocks::blocks::{Block, Instrument, Lever, LeverFace};
use mchprs_blocks::{BlockDirection, BlockFace, BlockPos};
use std::sync::Arc;

#[test]
fn compiled_notes_play_once_per_flush_and_respect_blocked_top() {
    let lever = BlockPos::new(15, 15, 15);
    let note = lever.offset(BlockFace::East);
    for blocked in [false, true] {
        let mut world = PlotWorld::from_chunks(
            0,
            0,
            (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
            Default::default(),
        );
        world.set_block(lever.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(
            lever,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
            },
        );
        world.set_block(
            note,
            Block::NoteBlock {
                instrument: Instrument::Harp,
                note: 1,
                powered: false,
            },
        );

        if blocked {
            world.set_block(note.offset(BlockFace::Top), Block::Stone {});
        }
        let mut compiler = Compiler::default();
        compiler
            .compile(
                &world,
                (BlockPos::new(10, 10, 10), BlockPos::new(20, 20, 20)),
                CompilerOptions::default(),
                vec![],
                Arc::new(TaskMonitor::default()),
            )
            .unwrap();

        for _ in 0..3 {
            compiler.try_on_use_block(lever).unwrap();
        }
        assert!(world.sounds.is_empty());
        compiler.try_flush(&mut world).unwrap();

        assert_eq!(world.sounds.len(), usize::from(!blocked));
        if !blocked {
            assert_eq!(world.sounds[0].pos, note);
        }
        compiler.try_flush(&mut world).unwrap();
        assert_eq!(world.sounds.len(), usize::from(!blocked));
        compiler.try_on_use_block(lever).unwrap();
        compiler.try_on_use_block(lever).unwrap();
        compiler
            .try_reset(&mut world, (BlockPos::new(10, 10, 10), BlockPos::new(20, 20, 20)))
            .unwrap();
        assert_eq!(world.sounds.len(), 2 * usize::from(!blocked));
    }
}
