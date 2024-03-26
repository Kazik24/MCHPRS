use crate::interaction::{destroy, place_in_world};
use crate::world::World;
use mchprs_blocks::blocks::{Block, ComparatorMode, RedstonePiston};
use mchprs_blocks::{BlockDirection, BlockFace, BlockFacing, BlockPos};

fn is_powered_in_direction(world: &impl World, pos: BlockPos, direction: BlockFacing) -> bool {
    let offset = pos.offset(direction.into());
    let block = world.get_block(offset);
    let power = super::get_redstone_power(block, world, offset, direction.into());

    power > 0
}

pub fn should_piston_extend(
    world: &impl World,
    piston: RedstonePiston,
    piston_pos: BlockPos,
) -> bool {
    // check for direct power
    if is_powered_in_direction(world, piston_pos, piston.facing.opposite()) {
        return true;
    }

    if is_powered_in_direction(world, piston_pos, piston.facing.rotate_ccw()) {
        return true;
    }

    if is_powered_in_direction(world, piston_pos, piston.facing.rotate()) {
        return true;
    }

    if is_powered_in_direction(world, piston_pos, BlockFacing::Up) {
        return true;
    }

    if is_powered_in_direction(world, piston_pos, BlockFacing::Down) {
        return true;
    }
    if is_powered_in_direction(world, piston_pos.offset(BlockFace::Top), BlockFacing::Down) {
        return true;
    }
    for direction in BlockFacing::horizontal_values() {
        if is_powered_in_direction(world, piston_pos.offset(BlockFace::Top), direction) {
            return true;
        }
    }

    return false;
}

pub fn update_piston_state(world: &mut impl World, piston: RedstonePiston, piston_pos: BlockPos) {
    let should_extend = should_piston_extend(world, piston, piston_pos);
    if should_extend != piston.extended {
        tracing::info!(
            "Piston state changes {:?}, pos {:?}",
            should_extend,
            piston_pos
        );
        //todo without this there is stack overflow!!, probably cause update of placing/destroying blocks triggers this again.
        if should_extend {
            extend(world, piston, piston_pos, piston.facing);
        } else {
            retract(world, piston, piston_pos, piston.facing);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PistonType {
    Sticky,
    Normal,
}

impl From<bool> for PistonType {
    fn from(sticky: bool) -> Self {
        if sticky {
            PistonType::Sticky
        } else {
            PistonType::Normal
        }
    }
}

fn extend(
    world: &mut impl World,
    piston: RedstonePiston,
    piston_pos: BlockPos,
    direction: BlockFacing,
) {
    let head_pos = piston_pos.offset(direction.into());
    let head_block = world.get_block(head_pos);

    if piston.extended {
        return;
    }

    if !head_block.is_movable() {
        return;
    }

    // replace head block with piston head
    world.set_block(
        head_pos,
        Block::PistonHead {
            head: piston.into(),
        },
    );

    // update piston to be extended
    world.set_block(
        piston_pos,
        Block::Piston {
            piston: piston.extend(true),
        },
    );

    match head_block {
        Block::Air {} => {
            return; // do not push air
        }
        _ => {}
    }

    // block sticed to piston
    let pushed_pos = head_pos.offset(direction.into());
    let old_block = world.get_block(pushed_pos);

    if old_block.is_movable() {
        // destroy block (check what happens if we just override this block or ignore if not air)
        // ignoring can be nice, bsc it will mean that pistion just can push one block.
        destroy(old_block, world, pushed_pos);

        // place block
        place_in_world(head_block, world, pushed_pos, &None);
    }
}

fn retract(
    world: &mut impl World,
    piston: RedstonePiston,
    piston_pos: BlockPos,
    direction: BlockFacing,
) {
    let head_pos = piston_pos.offset(direction.into());

    if piston.extended {
        return;
    }

    world.delete_block_entity(head_pos); //head can have block entity. why it can have block entity?
    world.set_block(head_pos, Block::Air {}); // raw set without update (todo send block updates for BUD switches)

    world.set_block(
        piston_pos,
        Block::Piston {
            piston: piston.extend(false),
        },
    );

    let pull_pos = head_pos.offset(direction.into());
    let pull_block = world.get_block(pull_pos);

    //pull block only if its a cube (also half-slab) and without block entity
    if pull_block.is_movable() && piston.sticky {
        destroy(pull_block, world, pull_pos);
        place_in_world(pull_block, world, head_pos, &None);
    }
}
