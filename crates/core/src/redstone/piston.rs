use crate::interaction::place_in_world;
use crate::world::{BlockAction, PistonAction, World};
use mchprs_blocks::block_entities::{BlockEntity, MovingPistonEntity};
use mchprs_blocks::blocks::{Block, RedstoneMovingPiston, RedstonePiston, RedstonePistonHead};
use mchprs_blocks::{BlockFace, BlockFacing, BlockPos};
use mchprs_world::TickPriority;
#[allow(unused)]
use tracing::*;

use super::update;

// Some source code of pistons:
//https://github.com/Marcelektro/MCP-919/blob/main/src/minecraft/net/minecraft/tileentity/TileEntityPiston.java
//https://github.com/Marcelektro/MCP-919/blob/main/src/minecraft/net/minecraft/block/BlockPistonBase.java

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
    if should_extend != piston.extended && !world.pending_tick_at(piston_pos) {
        world.schedule_tick(piston_pos, 0, TickPriority::Higher);
    }
}

pub fn piston_tick(world: &mut impl World, piston: RedstonePiston, piston_pos: BlockPos) {
    //info!("piston tick {piston:?}");
    let should_extend = should_piston_extend(world, piston, piston_pos);
    if should_extend != piston.extended {
        if should_extend {
            schedule_extend(world, piston, piston_pos);
        } else {
            schedule_retract(world, piston, piston_pos);
        }
    }
}

pub fn moving_piston_tick(
    world: &mut impl World,
    moving: RedstoneMovingPiston,
    head_pos: BlockPos,
) {
    let entity = match world.get_block_entity(head_pos) {
        Some(BlockEntity::MovingPiston(entity)) => *entity,
        _ => return,
    };

    world.delete_block_entity(head_pos); //delete moving block entity, block at this place will always be set later in this function

    //set piston state anyway
    let direction = BlockFace::from(moving.facing);
    let piston_pos = head_pos.offset(direction.opposite());
    let piston = RedstonePiston {
        extended: entity.extending,
        facing: moving.facing,
        sticky: moving.sticky,
    };
    world.set_block(piston_pos, Block::Piston { piston });
    if entity.extending {
        let pushed_pos = head_pos.offset(entity.facing);
        let head = RedstonePistonHead {
            facing: moving.facing,
            sticky: moving.sticky,
            short: false,
        };
        world.set_block(head_pos, Block::PistonHead { head });
        let pushed_block = Block::from_id(entity.block_state);
        //push block only if its a cube (also half-slab) and without block entity
        let move_block = !pushed_block.has_block_entity() && pushed_block.is_cube();
        if move_block {
            world.set_block(pushed_pos, pushed_block);
        }
        on_piston_state_change(world, piston_pos, direction, move_block);
    } else {
        if moving.sticky {
            world.set_block(head_pos, Block::from_id(entity.block_state));
        } else {
            world.set_block(head_pos, Block::Air);
        }
        on_piston_state_change(world, piston_pos, direction, false);
    }
    //schedule update in next game tick to check if piston state should change again
    world.schedule_half_tick(piston_pos, 1, TickPriority::Normal);
}

fn schedule_extend(world: &mut impl World, piston: RedstonePiston, piston_pos: BlockPos) {
    let direction = piston.facing.into();
    let head_pos = piston_pos.offset(direction);
    let head_block = world.get_block(head_pos);
    // very important condition preventing infinite loops
    match head_block {
        Block::MovingPiston { .. } => {
            return;
        }
        Block::PistonHead { .. } => {
            if piston.extended == false {
                world.set_block(
                    piston_pos,
                    Block::Piston {
                        piston: piston.extend(true),
                    },
                );
            }
            return;
        }
        _ => {}
    }

    let has_entity = head_block.has_block_entity();
    let is_cube = head_block.is_cube();

    //if normal block without entity destroy because it will be moved, when block is not a cube destroy it anyways (and dont move)
    let extend_piston = !has_entity || !is_cube;

    if extend_piston {
        // let piston_block = Block::Piston {
        //     piston: piston.extend(true),
        // };
        // world.set_block(piston_pos, piston_block); //todo this might cause animation flickering but is needed for update logic (actually it's not needed at all)

        //todo check for existing moving piston entity here (maybe not needed)
        destroy_moved_block(world, head_pos);
        world.set_block(
            head_pos,
            Block::MovingPiston {
                moving: piston.into(),
            },
        );

        let entity = MovingPistonEntity {
            extending: true,
            facing: direction,
            progress: 0,
            block_state: head_block.get_id(),
            source: true,
        };

        world.set_block_entity(head_pos, BlockEntity::MovingPiston(entity));
        world.schedule_tick(head_pos, 1, TickPriority::Normal);
        let action = BlockAction::Piston {
            action: PistonAction::Extend,
            piston,
        };
        world.block_action(piston_pos, action);
        on_piston_state_change(world, piston_pos, direction, true);
    }
}

fn schedule_retract(world: &mut impl World, piston: RedstonePiston, piston_pos: BlockPos) {
    //info!("retracting {piston:?}");
    let direction = piston.facing.into();
    let head_pos = piston_pos.offset(direction);
    let head_block = world.get_block(head_pos);

    // very important condition preventing infinite loops
    match head_block {
        Block::PistonHead { .. } => {}
        Block::Air => {
            if piston.extended == true {
                let head = RedstonePistonHead {
                    facing: piston.facing,
                    sticky: piston.sticky,
                    short: false,
                };
                place_in_world(Block::PistonHead { head }, world, head_pos, &None);
            }
            return;
        }
        _ => {
            return;
        }
    }

    let pull_pos = head_pos.offset(direction);
    let pull_block = world.get_block(pull_pos);

    let action = BlockAction::Piston {
        action: PistonAction::Retract,
        piston,
    };
    world.block_action(piston_pos, action);

    //pull block only if its a cube (also half-slab) and without block entity, else use air as placeholder
    let block_state = if !pull_block.has_block_entity() && pull_block.is_cube() && piston.sticky {
        destroy_moved_block(world, pull_pos);
        pull_block
    } else {
        Block::Air
    };

    //temporary moving block at head position
    world.set_block(
        head_pos,
        Block::MovingPiston {
            moving: piston.into(),
        },
    );
    let entity = MovingPistonEntity {
        extending: false,
        facing: direction,
        progress: 0,
        source: true,
        block_state: block_state.get_id(),
    };
    world.set_block_entity(head_pos, BlockEntity::MovingPiston(entity));
    world.schedule_tick(head_pos, 1, TickPriority::Normal);

    let full_update = block_state != Block::Air;
    on_piston_state_change(world, piston_pos, direction, full_update);
}

//version of destroy that doesn't update blocks
fn destroy_moved_block(world: &mut impl World, pos: BlockPos) {
    world.delete_block_entity(pos);
    world.set_block(pos, Block::Air {});
}

/// Update piston but be smart to not send too many updates
/// full update set to true will update all 3 blocks of piston (base, head, pushed)
/// full update set to false will only update base and head
fn on_piston_state_change(
    world: &mut impl World,
    piston_pos: BlockPos,
    facing: BlockFace,
    full_update: bool,
) {
    // update base
    for direction in BlockFace::values() {
        if direction == facing {
            continue;
        }
        let neighbor_pos = piston_pos.offset(direction);
        let block = world.get_block(neighbor_pos);
        //change(block, world, neighbor_pos, direction);
        update(block, world, neighbor_pos, Some(direction.opposite()));
    }

    // update head
    let head_pos = piston_pos.offset(facing.into());
    let block = world.get_block(head_pos);
    update(block, world, head_pos, None); //update block itself, e.g in case of lamps
    let opposite = facing.opposite();
    for direction in BlockFace::values() {
        if direction == opposite {
            continue;
        }
        if direction == facing && !full_update {
            //if pushed block is not updated, try to update also place where pushed pos is
            continue;
        }
        let neighbor_pos = head_pos.offset(direction);
        let block = world.get_block(neighbor_pos);
        update(block, world, neighbor_pos, Some(direction.opposite()));
    }

    //update pushed block
    if full_update {
        let pushed_pos = head_pos.offset(facing.into());
        let block = world.get_block(head_pos);
        update(block, world, head_pos, None); //update block itself, e.g in case of lamps
        let opposite = facing.opposite();
        for direction in BlockFace::values() {
            if direction == opposite {
                continue;
            }
            let neighbor_pos = pushed_pos.offset(direction);
            let block = world.get_block(neighbor_pos);
            update(block, world, neighbor_pos, Some(direction.opposite()));
        }
    }
}
