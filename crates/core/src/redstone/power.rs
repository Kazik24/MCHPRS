//! Signal emission geometry shared by interpretation and dependency analysis.
//!
//! These predicates describe a possible connection, including an unpowered
//! source. The interpreter separately reads its current output strength.
use crate::world::World;
use mchprs_blocks::blocks::{Block, ButtonFace, LeverFace};
use mchprs_blocks::{BlockFace, BlockPos};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConsumerInput {
    Main,
    ComparatorSide,
}

/// Receiving cells and faces used by wire routing.
pub(crate) fn consumer_roots(
    block: Block,
    pos: BlockPos,
) -> Vec<(BlockPos, BlockFace, ConsumerInput)> {
    let main = |face: BlockFace| (pos.offset(face), face, ConsumerInput::Main);
    match block {
        Block::RedstoneRepeater { repeater } => vec![main(repeater.facing.block_face())],
        Block::RedstoneComparator { comparator } => vec![
            main(comparator.facing.block_face()),
            (
                pos.offset(comparator.facing.rotate().block_face()),
                comparator.facing.rotate().block_face(),
                ConsumerInput::ComparatorSide,
            ),
            (
                pos.offset(comparator.facing.rotate_ccw().block_face()),
                comparator.facing.rotate_ccw().block_face(),
                ConsumerInput::ComparatorSide,
            ),
        ],
        Block::RedstoneTorch { .. } => vec![(
            pos.offset(BlockFace::Bottom),
            BlockFace::Top,
            ConsumerInput::Main,
        )],
        Block::RedstoneWallTorch { facing, .. } => vec![main(facing.opposite().block_face())],
        block if is_wire_consumer(block) => BlockFace::values().into_iter().map(main).collect(),
        _ => Vec::new(),
    }
}

fn is_wire_consumer(block: Block) -> bool {
    block.is_command_block()
        || block.is_copper_bulb()
        || matches!(
            block,
            Block::RedstoneRepeater { .. }
                | Block::RedstoneComparator { .. }
                | Block::RedstoneTorch { .. }
                | Block::RedstoneWallTorch { .. }
                | Block::RedstoneLamp { .. }
                | Block::IronTrapdoor { .. }
                | Block::NoteBlock { .. }
        )
}

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
