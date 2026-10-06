//! Signal emission geometry shared by interpretation and dependency analysis.
//!
//! These predicates describe a possible connection, including an unpowered
//! source. The interpreter separately reads its current output strength.
use crate::world::World;
use mchprs_blocks::blocks::{Block, ButtonFace, LeverFace};
use mchprs_blocks::BlockFace;
use mchprs_blocks::BlockPos;

pub(crate) fn emits_weak_power(
    block: Block,
    world: &impl World,
    pos: BlockPos,
    side: BlockFace,
    dust_power: bool,
) -> bool {
    match block {
        Block::RedstoneTorch { .. } => side != BlockFace::Top,
        Block::RedstoneWallTorch { facing, .. } => facing.block_face() != side,
        Block::RedstoneBlock
        | Block::Lever { .. }
        | Block::StoneButton { .. }
        | Block::StonePressurePlate { .. } => true,
        block if block.pressure_plate_powered().is_some() => true,
        Block::RedstoneRepeater { repeater } => repeater.facing.block_face() == side,
        Block::RedstoneComparator { comparator } => comparator.facing.block_face() == side,
        Block::Observer { observer } => observer.facing == side.into(),
        Block::RedstoneWire { wire } if dust_power => match side {
            BlockFace::Top => true,
            BlockFace::Bottom => false,
            _ => !super::wire::get_current_side(
                super::wire::get_regulated_sides(wire, world, pos),
                side.unwrap_direction().opposite(),
            )
            .is_none(),
        },
        _ => false,
    }
}

pub(crate) fn emits_strong_power(
    block: Block,
    world: &impl World,
    pos: BlockPos,
    side: BlockFace,
    dust_power: bool,
) -> bool {
    match block {
        Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. } => side == BlockFace::Bottom,
        Block::Lever { lever } => match side {
            BlockFace::Top => lever.face == LeverFace::Floor,
            BlockFace::Bottom => lever.face == LeverFace::Ceiling,
            _ => lever.face == LeverFace::Wall && lever.facing == side.unwrap_direction(),
        },
        Block::StoneButton { button } => match side {
            BlockFace::Top => button.face == ButtonFace::Floor,
            BlockFace::Bottom => button.face == ButtonFace::Ceiling,
            _ => button.face == ButtonFace::Wall && button.facing == side.unwrap_direction(),
        },
        Block::StonePressurePlate { .. } => side == BlockFace::Top,
        block if block.pressure_plate_powered().is_some() => side == BlockFace::Top,
        Block::RedstoneWire { .. }
        | Block::RedstoneRepeater { .. }
        | Block::RedstoneComparator { .. }
        | Block::Observer { .. } => emits_weak_power(block, world, pos, side, dust_power),
        _ => false,
    }
}
