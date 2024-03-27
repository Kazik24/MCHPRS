use crate::interaction::{destroy, place_in_world};
use crate::world::World;
use mchprs_blocks::blocks::{Block, RedstonePiston};
use mchprs_blocks::{BlockFace, BlockFacing, BlockPos};

fn is_powered_in_direction(world: &impl World, pos: BlockPos, direction: BlockFacing) -> bool {
    let offset = pos.offset(direction.into());
    let block = world.get_block(offset);
    super::get_redstone_power(block, world, offset, direction.into()) > 0
}

pub fn should_piston_extend(
    world: &impl World,
    piston: RedstonePiston,
    piston_pos: BlockPos,
) -> bool {
    // normal 

    if piston.facing != BlockFacing::Up && is_powered_in_direction(world, piston_pos, BlockFacing::Up) {
        return true;
    }

    if piston.facing != BlockFacing::Down && is_powered_in_direction(world, piston_pos, BlockFacing::Down) {
        return true;
    }

    for direction in BlockFacing::horizontal_values() {
        if piston.facing != direction && is_powered_in_direction(world, piston_pos, direction) {
            return true;
        }
    }

    // bud
    if is_powered_in_direction(world, piston_pos.offset(BlockFace::Top), BlockFacing::Down) {
        return true;
    }

    if is_powered_in_direction(world, piston_pos.offset(BlockFace::Top), BlockFacing::Up) {
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
        tracing::info!("piston state changed to {:?}", should_extend);
        if should_extend {
            extend(world, piston, piston_pos, piston.facing);
        } else {
            retract(world, piston, piston_pos, piston.facing);
        }
    }
}

fn extend(
    world: &mut impl World,
    piston: RedstonePiston,
    piston_pos: BlockPos,
    direction: BlockFacing,
) {
    // put world.schedule_tick(piston_pos, 1, priority); somewhere
    // but consider the true zero-tick pistons with locking updates in same tick
    // as simple as adding last_update_tick: i32 in RedstonePiston
    // and checking here if tick is the same as last_update_tick

    world.set_block(
        piston_pos,
        Block::Piston {
            piston: piston.extend(true),
        },
    );

    let head_pos = piston_pos.offset(direction.into());
    let head_block = world.get_block(head_pos);

    tracing::info!("head block: {:?} {:?}", head_block, head_pos);

    // with tick check this is not needed
    if let Block::PistonHead { .. } = head_block {
        tracing::info!("head block is piston head");
        return;
    }

    tracing::info!("extending piston head");

    world.set_block(
        head_pos,
        Block::PistonHead {
            head: piston.into(),
        },
    );

    match head_block {
        Block::Air {} => {
            tracing::info!("head block is air");
            return;
        }
        _ => {}
    }

    if head_block.has_block_entity() || !head_block.is_cube() {
        tracing::info!("head block is not cube or has block entity");
        return;
    }

    let pushed_pos = head_pos.offset(direction.into());
    let old_block = world.get_block(pushed_pos);

    tracing::info!("pushed block: {:?} {:?}", old_block, pushed_pos);

    if old_block.is_simple_cube() {
        tracing::info!("pushed block is simple cube");
        destroy(old_block, world, pushed_pos);
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
    let head_block = world.get_block(head_pos);

    match head_block {
        Block::PistonHead { .. } => {}
        _ => {
            return;
        }
    }

    world.delete_block_entity(head_pos); //head can have block entity.
                                         // world.set_block(head_pos, Block::Air {}); // raw set without update (todo send block updates for BUD switches)

    let pull_pos = head_pos.offset(direction.into());
    let pull_block = world.get_block(pull_pos);

    //pull block only if its a cube (also half-slab) and without block entity
    if !(pull_block.has_block_entity() || !pull_block.is_cube()) && piston.sticky {
        destroy(pull_block, world, pull_pos);
        place_in_world(pull_block, world, head_pos, &None);
    }

    world.set_block(
        piston_pos,
        Block::Piston {
            piston: piston.extend(false),
        },
    );
}
