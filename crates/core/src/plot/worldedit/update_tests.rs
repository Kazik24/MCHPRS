use super::*;
use crate::plot::{PLOT_BLOCK_HEIGHT, PLOT_WIDTH};
use crate::world::storage::Chunk;

fn world() -> PlotWorld {
    let chunks = (0..PLOT_WIDTH)
        .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
        .collect();
    PlotWorld::from_chunks(0, 0, chunks, Default::default())
}

#[test]
fn update_crosses_unaligned_section_boundaries_and_leaves_outside_blocks_alone() {
    let mut world = world();
    let selected = BlockPos::new(16, 16, 16);
    let outside = BlockPos::new(19, 16, 16);
    for lamp in [selected, outside] {
        world.set_block(lamp, Block::RedstoneLamp { lit: false });
        world.set_block(
            lamp.offset(mchprs_blocks::BlockFace::East),
            Block::RedstoneBlock {},
        );
    }
    // The first section is empty. Reversed selection order is also supported.
    update_selection(&mut world, selected, BlockPos::new(15, 15, 15)).unwrap();
    assert_eq!(world.get_block(selected), Block::RedstoneLamp { lit: true });
    assert_eq!(world.get_block(outside), Block::RedstoneLamp { lit: false });
}

#[test]
fn invalid_update_bounds_are_rejected_before_any_callbacks() {
    let mut world = world();
    let lamp = BlockPos::new(16, 16, 16);
    world.set_block(lamp, Block::RedstoneLamp { lit: false });
    world.set_block(
        lamp.offset(mchprs_blocks::BlockFace::East),
        Block::RedstoneBlock {},
    );
    for invalid in [
        BlockPos::new(-1, 16, 16),
        BlockPos::new(256, 16, 16),
        BlockPos::new(16, -1, 16),
        BlockPos::new(16, PLOT_BLOCK_HEIGHT, 16),
    ] {
        assert!(update_selection(&mut world, lamp, invalid).is_err());
        assert!(update_selection(&mut world, invalid, lamp).is_err());
        assert_eq!(world.get_block(lamp), Block::RedstoneLamp { lit: false });
        assert_eq!(world.scheduler().iter_entries().count(), 0);
    }
}
