use crate::world::World;
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::{Block, ComparatorMode, RedstonePiston};
use mchprs_blocks::{BlockDirection, BlockFace, BlockPos};
use mchprs_world::TickPriority;

pub fn is_piston_powered(world: &impl World, pos: BlockPos) -> bool {
    let piston_block = world.get_block(pos);
    if let Block::Piston { piston } = piston_block {
        true
    } else {
        false
    }
}
