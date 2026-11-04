//! Retained geometry candidate used by the wire-neighborhood regression test.
use mchprs_blocks::{
    BlockDirection,
    blocks::{Block, RedstoneWire, RedstoneWireSide},
};

pub(crate) fn get_raw_sides_indexed(wire: RedstoneWire, read: impl Fn(usize) -> Block) -> RedstoneWire {
    let mut above_solid = None;
    let mut sides = [RedstoneWireSide::None; 4];
    for (i, direction) in [BlockDirection::North, BlockDirection::South, BlockDirection::East, BlockDirection::West]
        .into_iter()
        .enumerate()
    {
        let neighbor = read(1 + i * 3);
        if crate::redstone::wire::can_connect_to_uncached(neighbor, direction) {
            sides[i] = RedstoneWireSide::Side;
            continue;
        }
        let up_solid = *above_solid.get_or_insert_with(|| read(0).is_solid());
        if !up_solid && matches!(read(2 + i * 3), Block::RedstoneWire { .. }) {
            sides[i] = RedstoneWireSide::Up;
        } else if !neighbor.is_solid() && matches!(read(3 + i * 3), Block::RedstoneWire { .. }) {
            sides[i] = RedstoneWireSide::Side;
        }
    }
    RedstoneWire {
        north: sides[0],
        south: sides[1],
        east: sides[2],
        west: sides[3],
        power: wire.power,
    }
}
