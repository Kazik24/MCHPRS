//! Shared electrical rules; callers sample inputs and regulate variable dust geometry separately.
use mchprs_blocks::{
    BlockFace,
    blocks::{Block, ComparatorMode, RedstoneWireSide},
};

pub(crate) fn comparator_output_strength(input: u8, side: u8, mode: ComparatorMode) -> u8 {
    match mode {
        ComparatorMode::Compare if input >= side => input,
        ComparatorMode::Compare => 0,
        ComparatorMode::Subtract => input.saturating_sub(side),
    }
}

pub(crate) fn comparator_is_powered(input: u8, side: u8, mode: ComparatorMode) -> bool {
    input > 0 && (input > side || (input == side && mode == ComparatorMode::Compare))
}

pub(crate) fn emits_weak(block: Block, side: BlockFace) -> bool {
    match block {
        Block::RedstoneTorch { .. } => side != BlockFace::Top,
        Block::RedstoneWallTorch { facing, .. } => facing.block_face() != side,
        Block::RedstoneBlock | Block::Lever { .. } | Block::StoneButton { .. } | Block::StonePressurePlate { .. } => true,
        block if block.pressure_plate_powered().is_some() => true,
        Block::RedstoneRepeater { repeater } => repeater.facing.block_face() == side,
        Block::RedstoneComparator { comparator } => comparator.facing.block_face() == side,
        Block::Observer { observer } => observer.facing == side.into(),
        Block::RedstoneWire { wire } => match side {
            BlockFace::Top => true,
            BlockFace::Bottom => false,
            _ => crate::redstone::wire::get_current_side(wire, side.unwrap_direction().opposite()) != RedstoneWireSide::None,
        },
        _ => false,
    }
}

pub(crate) fn emits_strong(block: Block, side: BlockFace) -> bool {
    use mchprs_blocks::blocks::{ButtonFace, LeverFace};
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
        Block::RedstoneWire { .. } | Block::RedstoneRepeater { .. } | Block::RedstoneComparator { .. } | Block::Observer { .. } => {
            emits_weak(block, side)
        }
        _ => false,
    }
}
