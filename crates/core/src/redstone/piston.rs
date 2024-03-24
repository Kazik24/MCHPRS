use crate::interaction::{destroy, place_in_world};
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

pub fn is_piston_powered(world: &impl World, piston: RedstonePiston, pos: BlockPos) -> bool {
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

pub fn update_piston_state(world: &mut impl World, piston: RedstonePiston, pos: BlockPos) {
    let powered = is_piston_powered(world, piston, pos);
    let is_extended = piston.extended;
    if powered != is_extended {
        world.set_block(
            pos,
            Block::Piston {
                piston: RedstonePiston {
                    extended: powered,
                    ..piston
                },
            },
        );

        if powered {
            // extend
            let headpos = pos.offset(piston.facing.into());

            if move_block(world, headpos, piston.facing, Move::Push) {
                world.set_block(
                    headpos,
                    Block::PistonHead {
                        head: piston.into(),
                    },
                );
            }
        } else {
            // retract
            let headpos = pos.offset(piston.facing.into());
            move_block(world, headpos, piston.facing, Move::Pull); // pull cannot fail, but the block might just not move
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Move {
    Push,
    Pull,
}

fn move_block(
    world: &mut impl World,
    head: BlockPos,
    direction: BlockFacing,
    move_type: Move,
) -> bool {
    match move_type {
        Move::Push => {
            //todo move column of blocks, instead of one block
            let block = world.get_block(head);
            let pushed_pos = head.offset(direction.into());
            destroy(block, world, head);
            place_in_world(block, world, pushed_pos, &None);
            true
        }
        Move::Pull => {
            let pushed_pos = head.offset(direction.into());
            let block = world.get_block(pushed_pos);
            destroy(block, world, pushed_pos);
            place_in_world(block, world, head, &None);
            true
        }
    }
}
