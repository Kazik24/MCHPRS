use crate::world::World;
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::{Block, ComparatorMode, RedstonePiston};
use mchprs_blocks::{BlockDirection, BlockFace, BlockFacing, BlockPos};
use mchprs_world::TickPriority;

fn is_powered_in_direction(
    world: &impl World,
    pos: BlockPos,
    piston: &RedstonePiston,
    direction: BlockFacing,
) -> bool {
    let offset = pos.offset(direction.into());
    let block = world.get_block(offset);
    super::get_weak_power(block, world, offset, direction.into(), false) > 0
}

pub fn is_piston_powered(world: &impl World, pos: BlockPos) -> bool {
    // todo piston as argument for perf resons (as it is done everywhere like that)
    let piston_block = world.get_block(pos);
    let Block::Piston { piston } = piston_block else {
        false
    };

    // check for direct power
    if is_powered_in_direction(world, pos, &piston, piston.facing.opposite()) {
        return true;
    }

    if is_powered_in_direction(world, pos, &piston, piston.facing.rotate_ccw()) {
        return true;
    }

    if is_powered_in_direction(world, pos, &piston, piston.facing.rotate()) {
        return true;
    }

    if is_powered_in_direction(world, pos, &piston, BlockFacing::Up) {
        return true;
    }

    if is_powered_in_direction(world, pos, &piston, BlockFacing::Down) {
        return true;
    }
    if is_powered_in_direction(
        world,
        pos.offset(BlockFace::Top),
        &piston,
        BlockFacing::Down,
    ) {
        return true;
    }
    for direction in BlockFacing::horizontal_values() {
        if is_powered_in_direction(world, pos.offset(BlockFace::Top), &piston, direction) {
            return true;
        }
    }

    return false;
}

  
pub fn update_piston_state(world: &impl World, pos: BlockPos) {
    let piston_block = world.get_block(pos);
    let Block::Piston { piston } = piston_block else {
        return;
    };

    let powered = is_piston_powered(world, pos);
    let is_extended = piston.extended;
    if powered != is_extended {
        world.set_block(pos, Block::Piston {
            piston: RedstonePiston {
                extended: powered,
                ..piston
            },
        });
        
        if powered {
            // extend 
            let headpos = pos.offset(piston.facing.into()); 
            
            // get block
            let block = world.get_block(headpos);

            world.set_block(headpos, Block::PistonHead {
                head: piston.into()
            });

        } else {
            // retract

        }
    } 
}