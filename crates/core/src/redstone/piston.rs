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
    if powered != piston.extended {
        //todo without this there is stack overflow!!, probably cause update of placing/destroying blocks triggers this again.
        world.set_block(
            pos,
            Block::Piston {
                piston: piston.extend(powered),
            },
        );
        if powered {
            // extend
            let headpos = pos.offset(piston.facing.into());

            if move_block(world, headpos, piston.facing, Move::Push) {
                world.set_block(
                    pos,
                    Block::Piston {
                        piston: piston.extend(true),
                    },
                );
                world.set_block(
                    headpos,
                    Block::PistonHead {
                        head: piston.into(),
                    },
                );
            } else {
                //temporary fix for stack overflow
                world.set_block(
                    pos,
                    Block::Piston {
                        piston: piston.extend(false),
                    },
                );
            }
        } else {
            // retract
            let headpos = pos.offset(piston.facing.into());
            move_block(world, headpos, piston.facing, Move::Pull(piston.sticky));
            world.set_block(
                pos,
                Block::Piston {
                    piston: piston.extend(false),
                },
            );
            // pull cannot fail, but the block might just not move
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Move {
    Push,
    Pull(bool), // sticky: bool
}

fn move_block(
    world: &mut impl World,
    head: BlockPos,
    direction: BlockFacing,
    move_type: Move,
) -> bool {
    match move_type {
        Move::Push => {
            let block = world.get_block(head);
            let pushed_pos = head.offset(direction.into());

            let has_entity = block.has_block_entity();
            let is_cube = block.is_cube();
            //if normal block without entity destroy because it will be moved, when block is not a cube destroy it anyways (and dont move)
            let extend_piston = !has_entity || !is_cube;
            if extend_piston {
                destroy(block, world, head);
            }
            //push block only if its a cube (also half-slab) and without block entity
            if !has_entity && is_cube {
                push_block_column(world, pushed_pos, direction, block)
            } else {
                return extend_piston;
            }
        }
        Move::Pull(sticky) => {
            let pushed_pos = head.offset(direction.into());
            let block = world.get_block(pushed_pos);

            world.delete_block_entity(head); //head can have block entity
            world.set_block(head, Block::Air {}); // raw set without update (todo send block updates for BUD switches)

            let has_entity = block.has_block_entity();
            let is_cube = block.is_cube();
            //pull block only if its a cube (also half-slab) and without block entity
            if !has_entity && is_cube && sticky {
                destroy(block, world, pushed_pos);
                place_in_world(block, world, head, &None);
            }
            true
        }
    }
}

const MAX_PUSHED_BLOCKS: usize = 12;

fn push_block_column(
    world: &mut impl World,
    pos: BlockPos,
    _direction: BlockFacing,
    first_block: Block,
) -> bool {
    //todo move column of blocks, instead of one block
    place_in_world(first_block, world, pos, &None);
    true
}
