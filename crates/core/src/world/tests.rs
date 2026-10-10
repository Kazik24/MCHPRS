use super::*;
use crate::plot::{PLOT_WIDTH, PlotWorld};
use std::collections::HashSet;

#[test]
fn block_walkers_cross_unaligned_boundaries_and_skip_empty_sections() {
    for plot_coord in [-1, 0] {
        let start_chunk = plot_coord * PLOT_WIDTH;
        let chunks = (start_chunk..start_chunk + PLOT_WIDTH)
            .flat_map(|x| (start_chunk..start_chunk + PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
            .collect();
        let mut world = PlotWorld::from_chunks(plot_coord, plot_coord, chunks, Default::default());
        let edge = if plot_coord < 0 { -16 } else { 16 };
        let first = BlockPos::new(edge - 1, 15, edge - 1);
        let last = BlockPos::new(edge, 16, edge);
        let outside = BlockPos::new(edge + 1, 16, edge);
        world.set_block(last, Block::RedstoneLamp { lit: false });
        world.set_block(outside, Block::RedstoneLamp { lit: false });

        let mut visited = Vec::new();
        for_each_block_optimized(&world, last, first, |pos| visited.push(pos));
        assert_eq!(visited, [last]);

        let mut mutated = Vec::new();
        for_each_block_mut_optimized(&mut world, first, last, |world, pos| {
            mutated.push(pos);
            world.set_block(pos, Block::Air {});
        });
        assert_eq!(mutated, visited);
        assert_eq!(world.get_block(last), Block::Air {});
        assert_eq!(world.get_block(outside), Block::RedstoneLamp { lit: false });
    }
}

#[test]
fn block_walkers_visit_every_selected_position_once_while_removing_blocks() {
    let chunks = (0..PLOT_WIDTH)
        .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
        .collect();
    let mut world = PlotWorld::from_chunks(0, 0, chunks, Default::default());
    let mut expected = HashSet::new();
    for x in 15..=17 {
        for y in 15..=17 {
            for z in 15..=17 {
                let pos = BlockPos::new(x, y, z);
                world.set_block(pos, Block::Glass {});
                expected.insert(pos);
            }
        }
    }
    let first = BlockPos::new(15, 15, 15);
    let last = BlockPos::new(17, 17, 17);
    let mut visited = Vec::new();
    for_each_block_optimized(&world, first, last, |pos| visited.push(pos));
    assert_eq!(visited.len(), expected.len());
    assert_eq!(visited.iter().copied().collect::<HashSet<_>>(), expected);

    let mut mutated = Vec::new();
    for_each_block_mut_optimized(&mut world, last, first, |world, pos| {
        mutated.push(pos);
        world.set_block(pos, Block::Air {});
    });
    assert_eq!(mutated, visited);
    assert!(
        expected
            .iter()
            .all(|&pos| world.get_block(pos) == Block::Air {})
    );
}
