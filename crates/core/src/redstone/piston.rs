use crate::interaction::{destroy, place_in_world};
use crate::world::World;
use mchprs_blocks::block_entities::{BlockEntity, MovingPistonEntity};
use mchprs_blocks::blocks::{Block, RedstonePiston};
use mchprs_blocks::{BlockFace, BlockFacing, BlockPos};
use mchprs_world::TickPriority;

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

    if piston.facing != BlockFacing::Up
        && is_powered_in_direction(world, piston_pos, BlockFacing::Up)
    {
        return true;
    }

    if piston.facing != BlockFacing::Down
        && is_powered_in_direction(world, piston_pos, BlockFacing::Down)
    {
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
        // tracing::info!("piston update: {} {}", should_extend, piston.extended);
        if should_extend == false {
            // should retraxt
            retract_block(world, piston, piston_pos, piston.facing);
        }
        if !world.pending_tick_at(piston_pos) {
            world.schedule_half_tick(piston_pos, 3, TickPriority::Normal);
        }
    }
}

pub fn piston_tick(world: &mut impl World, piston: RedstonePiston, piston_pos: BlockPos) {
    let should_extend = should_piston_extend(world, piston, piston_pos);
    if should_extend != piston.extended {
        // tracing::info!("piston tick: {} {}", should_extend, piston.extended);
        if should_extend {
            extend(world, piston, piston_pos, piston.facing);
        } else {
            retract_place_block(world, piston, piston_pos, piston.facing);
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

    let has_entity = head_block.has_block_entity();
    let is_cube = head_block.is_cube();

    //if normal block without entity destroy because it will be moved, when block is not a cube destroy it anyways (and dont move)
    let extend_piston = !has_entity || !is_cube;
    if extend_piston {
        destroy(head_block, world, head_pos);
        world.set_block(
            head_pos,
            Block::PistonHead {
                head: piston.into(),
            },
        );
    }

    //push block only if its a cube (also half-slab) and without block entity
    if !has_entity && is_cube {
        let pushed_pos = head_pos.offset(direction.into());
        place_in_world(head_block, world, pushed_pos, &None)
    } else {
        return;
    }
}

fn retract_block(
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
    world.set_block(head_pos, Block::Air {}); // raw set without update (todo send block updates for BUD switches)

    let pull_pos = head_pos.offset(direction.into());
    let pull_block = world.get_block(pull_pos);

    //pull block only if its a cube (also half-slab) and without block entity
    if !pull_block.has_block_entity() && pull_block.is_cube() && piston.sticky {
        destroy(pull_block, world, pull_pos);
        place_in_world(pull_block, world, head_pos, &None);
    }

    // tracing::info!(
    //     "retracting piston - setting entity {} {}",
    //     piston_pos,
    //     pull_block.get_id()
    // );

    world.set_block_entity(
        piston_pos,
        BlockEntity::MovingPiston(MovingPistonEntity {
            extending: false,
            facing: piston.facing.into(),
            progress: 0,
            source: false,
            block_state: pull_block.get_id(),
        }),
    )
}

fn retract_place_block(
    world: &mut impl World,
    piston: RedstonePiston,
    piston_pos: BlockPos,
    direction: BlockFacing,
) {
    let head_pos = piston_pos.offset(direction.into());

    // get block entity
    let block_entity = world.get_block_entity(piston_pos);
    // tracing::info!(
    //     "retracting piston - block entity {:?} {}",
    //     block_entity,
    //     head_pos
    // );
    if let Some(BlockEntity::MovingPiston(moving_piston)) = block_entity {
        // set block
        let pull_block = Block::from_id(moving_piston.block_state);
        place_in_world(pull_block, world, head_pos, &None);
        world.delete_block_entity(piston_pos);
    }
    world.set_block(
        piston_pos,
        Block::Piston {
            piston: piston.extend(false),
        },
    );
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
    world.set_block(head_pos, Block::Air {}); // raw set without update (todo send block updates for BUD switches)

    let pull_pos = head_pos.offset(direction.into());
    let pull_block = world.get_block(pull_pos);

    //pull block only if its a cube (also half-slab) and without block entity
    if !pull_block.has_block_entity() && pull_block.is_cube() && piston.sticky {
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
