use crate::interaction::{destroy, place_in_world};
use crate::world::World;
use mchprs_blocks::block_entities::{BlockEntity, MovingPistonEntity};
use mchprs_blocks::blocks::{Block, RedstonePiston, RedstonePistonHead};
use mchprs_blocks::{BlockFace, BlockFacing, BlockPos};
use mchprs_world::TickPriority;
use tracing::info;

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
        world.schedule_tick(piston_pos, 0, TickPriority::Higher);
    }
}

pub fn piston_tick(world: &mut impl World, piston: RedstonePiston, piston_pos: BlockPos) {
    info!("piston tick {piston:?}");
    let should_extend = should_piston_extend(world, piston, piston_pos);
    if should_extend {
        schedule_extend(world, piston, piston_pos);
    } else {
        schedule_retract(world, piston, piston_pos);
    }
}

pub fn head_tick(world: &mut impl World, head: RedstonePistonHead, head_pos: BlockPos) {
    let entity = match world.get_block_entity(head_pos) {
        Some(BlockEntity::MovingPiston(entity)) => *entity,
        _ => return,
    };
    info!("head tick {head:?} entity: {entity:?}");
    world.delete_block_entity(head_pos);
    if entity.extending {
        let pushed_pos = head_pos.offset(entity.facing);
        place_in_world(Block::from_id(entity.block_state), world, pushed_pos, &None);
    } else {
        if head.sticky {
            place_in_world(Block::from_id(entity.block_state), world, head_pos, &None);
        } else {
            place_in_world(Block::Air {}, world, head_pos, &None);
        }
        let piston_pos = head_pos.offset(head.facing.opposite().into());
        let piston = RedstonePiston {
            extended: false,
            facing: head.facing,
            sticky: head.sticky,
        };
        world.set_block(piston_pos, Block::Piston { piston });
    }
}

fn schedule_extend(world: &mut impl World, piston: RedstonePiston, piston_pos: BlockPos) {
    info!("extending {piston:?}");
    let head_pos = piston_pos.offset(piston.facing.into());
    let head_block = world.get_block(head_pos);

    let has_entity = head_block.has_block_entity();
    let is_cube = head_block.is_cube();

    //if normal block without entity destroy because it will be moved, when block is not a cube destroy it anyways (and dont move)
    let extend_piston = !has_entity || !is_cube;
    //push block only if its a cube (also half-slab) and without block entity
    let move_block = !has_entity && is_cube;
    if extend_piston {
        world.set_block(
            piston_pos,
            Block::Piston {
                piston: piston.extend(true),
            },
        );
        destroy(head_block, world, head_pos);
        world.set_block(
            head_pos,
            Block::PistonHead {
                head: piston.into(),
            },
        );

        if move_block {
            let entity = MovingPistonEntity {
                extending: true,
                facing: piston.facing.into(),
                progress: 0,
                block_state: head_block.get_id(),
                source: true,
            };
            world.set_block_entity(head_pos, BlockEntity::MovingPiston(entity));
            world.schedule_half_tick(head_pos, 3, TickPriority::Normal);
        }
    }

    // if move_block {

    //     let pushed_pos = head_pos.offset(direction.into());
    //     place_in_world(head_block, world, pushed_pos, &None)
    // } else {
    //     return;
    // }
}

fn schedule_retract(world: &mut impl World, piston: RedstonePiston, piston_pos: BlockPos) {
    info!("retracting {piston:?}");
    let direction = piston.facing.into();
    let head_pos = piston_pos.offset(direction);
    let head_block = world.get_block(head_pos);

    match head_block {
        Block::PistonHead { .. } => {}
        _ => {
            return;
        }
    }

    let pull_pos = head_pos.offset(direction);
    let pull_block = world.get_block(pull_pos);

    //pull block only if its a cube (also half-slab) and without block entity
    if !pull_block.has_block_entity() && pull_block.is_cube() {
        if piston.sticky {
            destroy(pull_block, world, pull_pos);

            let entity = MovingPistonEntity {
                extending: false,
                facing: piston.facing.into(),
                progress: 255,
                source: true,
                block_state: pull_block.get_id(),
            };
            world.set_block_entity(head_pos, BlockEntity::MovingPiston(entity));
            world.schedule_half_tick(head_pos, 3, TickPriority::Normal);
        } else {
        }
    }
}
