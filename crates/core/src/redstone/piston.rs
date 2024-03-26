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

    tracing::info!(
        "Extend piston: {:?}, movable: {:?}",
        head_block,
        head_block.is_movable()
    );

    if piston.extended {
        tracing::info!("Piston already extended");
        return;
    }

    if !head_block.is_movable() {
        return;
    }

    tracing::info!("Piston can be extended");

    // replace head block with piston head
    world.set_block(
        head_pos,
        Block::PistonHead {
            head: piston.into(),
        },
    );

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

    tracing::info!(
        "Replaced block: {:?} {:?}",
        old_block,
        old_block.is_movable()
    );

    if !old_block.is_movable() {
        return;
    }
    // destroy block (check what happens if we just override this block or ignore if not air)
    // ignoring can be nice, bsc it will mean that pistion just can push one block.
    destroy(old_block, world, pushed_pos);

    // place block
    place_in_world(head_block, world, pushed_pos, &None);
}
fn retract(
    world: &mut impl World,
    piston: RedstonePiston,
    piston_pos: BlockPos,
    direction: BlockFacing,
) {
    let head_pos = piston_pos.offset(direction.into());
    let head_block = world.get_block(head_pos);

    if piston.extended {
        tracing::info!("Piston already retracted");
        return;
    }

    world.delete_block_entity(head_pos); //head can have block entity. why it can have block entity?
    world.set_block(head_pos, Block::Air {}); // raw set without update (todo send block updates for BUD switches)

    tracing::info!(
        "Pull block: {:?}, sticky: {:?}, movable: {:?}, has_entity: {:?}",
        head_block,
        piston,
        head_block.is_movable(),
        head_block.has_block_entity()
    );

    let pull_pos = head_pos.offset(direction.into());
    let pull_block = world.get_block(pull_pos);

    //pull block only if its a cube (also half-slab) and without block entity
    if pull_block.is_movable() && piston.sticky {
        tracing::info!("Pull block: {:?}", pull_block);
        destroy(pull_block, world, pull_pos);
        place_in_world(pull_block, world, head_pos, &None);
    }

    // update piston to be retracted
    world.set_block(
        piston_pos,
        Block::Piston {
            piston: piston.extend(false),
        },
    );
}

// Idk, but this functiuon is very convoluted for me - it splits work into 3-4 places (two branches
// instide function and two branches in caller)
// I think that two functions that retract and extend piston would be lot cleaner
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Move {
    Push,
    Pull(PistonType),
}

fn move_block(
    world: &mut impl World,
    head: BlockPos,
    direction: BlockFacing,
    move_type: Move,
) -> bool {
    // when pistion is powered direcly it has double update.
    // chaning
    match move_type {
        Move::Push => {
            let head_block = world.get_block(head);

            if let Block::PistonHead { .. } = head_block {
                return false;
            }

            let pushed_pos = head.offset(direction.into());

            let has_entity = head_block.has_block_entity();
            let is_cube = head_block.is_cube();
            //if normal block without entity destroy because it will be moved, when block is not a cube destroy it anyways (and dont move)
            let extend_piston = !has_entity || !is_cube;

            tracing::info!(
                "Push block: {:?}, has_entity: {:?}, is_cube: {:?}, extend_piston: {:?}, movable: {:?}",
                head_block,
                has_entity,
                is_cube,
                extend_piston,
                head_block.is_movable()
            );

            if extend_piston {
                destroy(head_block, world, head);
            }
            //push block only if its a cube (also half-slab) and without block entity
            if head_block.is_movable() {
                push_block_column(world, pushed_pos, direction, head_block)
            } else {
                extend_piston
            }
        }
        Move::Pull(sticky) => {
            let pushed_pos = head.offset(direction.into());
            let head_block = world.get_block(pushed_pos);

            world.delete_block_entity(head); //head can have block entity
            world.set_block(head, Block::Air {}); // raw set without update (todo send block updates for BUD switches)

            tracing::info!(
                "Pull block: {:?}, sticky: {:?}, movable: {:?}, has_entity: {:?}",
                head_block,
                sticky,
                head_block.is_movable(),
                head_block.has_block_entity()
            );

            //pull block only if its a cube (also half-slab) and without block entity
            if head_block.is_movable() && sticky == PistonType::Sticky {
                tracing::info!("Pull block: {:?}", head_block);
                destroy(head_block, world, pushed_pos);
                place_in_world(head_block, world, head, &None);
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
