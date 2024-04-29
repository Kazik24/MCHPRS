//! A very basic redstone implementation with focus on accuracy over speed.
//! This is the implementation that is used by default in low-performance
//! scenerio (i.e. regular buiding)

pub mod comparator;
pub mod noteblock;
mod piston;
pub mod repeater;
pub mod wire;

use crate::world::World;
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::{Block, ButtonFace, LeverFace};
use mchprs_blocks::{BlockDirection, BlockFace, BlockFacing, BlockPos};
use mchprs_world::TickPriority;

pub fn bool_to_ss(b: bool) -> u8 {
    match b {
        true => 15,
        false => 0,
    }
}

fn get_weak_power(
    block: Block,
    world: &impl World,
    pos: BlockPos,
    side: BlockFace,
    dust_power: bool,
) -> u8 {
    match block {
        Block::RedstoneTorch { lit: true } => 15,
        Block::RedstoneWallTorch { lit: true, facing } if facing.block_face() != side => 15,
        Block::RedstoneBlock {} => 15,
        Block::StonePressurePlate { powered: true } => 15,
        Block::Lever { lever } if lever.powered => 15,
        Block::StoneButton { button } if button.powered => 15,
        Block::RedstoneRepeater { repeater }
            if repeater.facing.block_face() == side && repeater.powered =>
        {
            15
        }
        Block::RedstoneComparator { comparator } if comparator.facing.block_face() == side => {
            if let Some(BlockEntity::Comparator { output_strength }) = world.get_block_entity(pos) {
                *output_strength
            } else {
                0
            }
        }
        Block::RedstoneWire { wire } if dust_power => match side {
            BlockFace::Top => wire.power,
            BlockFace::Bottom => 0,
            _ => {
                let direction = side.unwrap_direction();
                if wire::get_current_side(
                    wire::get_regulated_sides(wire, world, pos),
                    direction.opposite(),
                )
                .is_none()
                {
                    0
                } else {
                    wire.power
                }
            }
        },
        Block::Observer { observer } if observer.facing == side.into() && observer.powered => 15,
        _ => 0,
    }
}

fn get_strong_power(
    block: Block,
    world: &impl World,
    pos: BlockPos,
    side: BlockFace,
    dust_power: bool,
) -> u8 {
    match block {
        Block::RedstoneTorch { lit: true } if side == BlockFace::Bottom => 15,
        Block::RedstoneWallTorch { lit: true, .. } if side == BlockFace::Bottom => 15,
        Block::Lever { lever } => bool_to_ss(
            match side {
                BlockFace::Top => lever.face == LeverFace::Floor,
                BlockFace::Bottom => lever.face == LeverFace::Ceiling,
                _ => lever.face == LeverFace::Wall && lever.facing == side.unwrap_direction(),
            } && lever.powered,
        ),
        Block::StoneButton { button } => bool_to_ss(
            match side {
                BlockFace::Top => button.face == ButtonFace::Floor,
                BlockFace::Bottom => button.face == ButtonFace::Ceiling,
                _ => button.face == ButtonFace::Wall && button.facing == side.unwrap_direction(),
            } && button.powered,
        ),
        Block::StonePressurePlate { powered: true } if side == BlockFace::Top => 15,
        Block::RedstoneWire { .. } => get_weak_power(block, world, pos, side, dust_power),
        Block::RedstoneRepeater { .. } => get_weak_power(block, world, pos, side, dust_power),
        Block::RedstoneComparator { .. } => get_weak_power(block, world, pos, side, dust_power),
        Block::Observer { observer } if observer.powered => 15,
        _ => 0,
    }
}

fn get_max_strong_power(world: &impl World, pos: BlockPos, dust_power: bool) -> u8 {
    let mut max_power = 0;
    for side in &BlockFace::values() {
        let block = world.get_block(pos.offset(*side));
        max_power = max_power.max(get_strong_power(
            block,
            world,
            pos.offset(*side),
            *side,
            dust_power,
        ));
    }
    max_power
}

pub fn get_redstone_power(
    block: Block,
    world: &impl World,
    pos: BlockPos,
    facing: BlockFace,
) -> u8 {
    if block.is_solid() {
        get_max_strong_power(world, pos, true)
    } else {
        get_weak_power(block, world, pos, facing, true)
    }
}

fn get_redstone_power_no_dust(
    block: Block,
    world: &impl World,
    pos: BlockPos,
    facing: BlockFace,
) -> u8 {
    if block.is_solid() {
        get_max_strong_power(world, pos, false)
    } else {
        get_weak_power(block, world, pos, facing, false)
    }
}

pub fn torch_should_be_off(world: &impl World, pos: BlockPos) -> bool {
    let bottom_pos = pos.offset(BlockFace::Bottom);
    let bottom_block = world.get_block(bottom_pos);
    get_redstone_power(bottom_block, world, bottom_pos, BlockFace::Top) > 0
}

pub fn on_state_change(facing: BlockFacing, world: &mut impl World, pos: BlockPos) {
    let front_pos = pos.offset(facing.opposite().into());
    let front_block = world.get_block(front_pos);
    update(front_block, world, front_pos, Some(facing.into()));
    for direction in BlockFace::values() {
        let neighbor_pos = front_pos.offset(direction);
        let block = world.get_block(neighbor_pos);
        update(block, world, neighbor_pos, Some(direction));
    }
}

pub fn wall_torch_should_be_off(
    world: &impl World,
    pos: BlockPos,
    direction: BlockDirection,
) -> bool {
    let wall_pos = pos.offset(direction.opposite().block_face());
    let wall_block = world.get_block(wall_pos);
    get_redstone_power(
        wall_block,
        world,
        wall_pos,
        direction.opposite().block_face(),
    ) > 0
}

pub fn redstone_lamp_should_be_lit(world: &impl World, pos: BlockPos) -> bool {
    for face in &BlockFace::values() {
        let neighbor_pos = pos.offset(*face);
        if get_redstone_power(world.get_block(neighbor_pos), world, neighbor_pos, *face) > 0 {
            return true;
        }
    }
    false
}

fn diode_get_input_strength(world: &impl World, pos: BlockPos, facing: BlockDirection) -> u8 {
    let input_pos = pos.offset(facing.block_face());
    let input_block = world.get_block(input_pos);
    let mut power = get_redstone_power(input_block, world, input_pos, facing.block_face());
    if power == 0 {
        if let Block::RedstoneWire { wire } = input_block {
            power = wire.power;
        }
    }
    power
}

pub fn update(block: Block, world: &mut impl World, pos: BlockPos, dir: Option<BlockFace>) {
    match block {
        Block::RedstoneWire { wire } => {
            wire::on_neighbor_updated(wire, world, pos);
        }
        Block::RedstoneTorch { lit } => {
            if lit == torch_should_be_off(world, pos) && !world.pending_tick_at(pos) {
                world.schedule_tick(pos, 1, TickPriority::Normal);
            }
        }
        Block::RedstoneWallTorch { lit, facing } => {
            if lit == wall_torch_should_be_off(world, pos, facing) && !world.pending_tick_at(pos) {
                world.schedule_tick(pos, 1, TickPriority::Normal);
            }
        }
        Block::RedstoneRepeater { repeater } => {
            repeater::on_neighbor_updated(repeater, world, pos);
        }
        Block::RedstoneComparator { comparator } => {
            comparator::update(comparator, world, pos);
        }
        Block::RedstoneLamp { lit } => {
            let should_be_lit = redstone_lamp_should_be_lit(world, pos);
            if lit && !should_be_lit {
                world.schedule_tick(pos, 2, TickPriority::Normal);
            } else if !lit && should_be_lit {
                world.set_block(pos, Block::RedstoneLamp { lit: true });
            }
        }
        Block::IronTrapdoor {
            powered,
            facing,
            half,
        } => {
            let should_be_powered = redstone_lamp_should_be_lit(world, pos);
            if powered != should_be_powered {
                let new_block = Block::IronTrapdoor {
                    facing,
                    half,
                    powered: should_be_powered,
                };
                world.set_block(pos, new_block);
            }
        }
        Block::Piston { piston } => {
            piston::update_piston_state(world, piston, pos);
        }
        Block::PistonHead { head } => {
            let piston_pos = pos.offset(head.facing.opposite().into());
            let piston = world.get_block(piston_pos);
            if let Block::Piston { piston } = piston {
                piston::update_piston_state(world, piston, piston_pos);
            }
        }
        Block::Observer { observer } => {
            if let Some(dir) = dir {
                tracing::info!(
                    "Observer update: {:?} {:?} {:?} {:?} ",
                    dir,
                    pos,
                    observer,
                    !world.pending_tick_at(pos)
                );
                if observer.facing == dir.into() && !observer.powered && !world.pending_tick_at(pos)
                {
                    tracing::info!("Scheduling observer tick at {:?}", pos);
                    world.schedule_tick(pos, 1, TickPriority::Normal);
                }
            }
        }
        Block::NoteBlock {
            instrument: _instrument,
            note,
            ..
        } => {
            let should_be_powered = redstone_lamp_should_be_lit(world, pos);
            // We need to recheck if the live version of the block is powered,
            // because the supplied block is cached and could be outdated
            let Block::NoteBlock { powered, .. } = world.get_block(pos) else {
                unreachable!("Underlying block changed, this should never happen")
            };
            if powered != should_be_powered {
                // Hack: Update the instrument only just before the noteblock is updated
                let instrument = noteblock::get_noteblock_instrument(world, pos);
                let new_block = Block::NoteBlock {
                    instrument,
                    note,
                    powered: should_be_powered,
                };

                if should_be_powered && noteblock::is_noteblock_unblocked(world, pos) {
                    noteblock::play_note(world, pos, instrument, note);
                }
                world.set_block(pos, new_block);
            }
        }
        _ => {}
    }
}

pub fn tick(block: Block, world: &mut impl World, pos: BlockPos) {
    match block {
        Block::RedstoneRepeater { repeater } => {
            repeater::tick(repeater, world, pos);
        }
        Block::RedstoneComparator { comparator } => {
            comparator::tick(comparator, world, pos);
        }
        Block::RedstoneTorch { lit } => {
            let should_be_off = torch_should_be_off(world, pos);
            if lit && should_be_off {
                world.set_block(pos, Block::RedstoneTorch { lit: false });
                on_torch_state_change(world, pos);
            } else if !lit && !should_be_off {
                world.set_block(pos, Block::RedstoneTorch { lit: true });
                on_torch_state_change(world, pos);
            }
        }
        Block::RedstoneWallTorch { lit, facing } => {
            let should_be_off = wall_torch_should_be_off(world, pos, facing);
            if lit && should_be_off {
                world.set_block(pos, Block::RedstoneWallTorch { lit: false, facing });
                on_torch_state_change(world, pos);
            } else if !lit && !should_be_off {
                world.set_block(pos, Block::RedstoneWallTorch { lit: true, facing });
                on_torch_state_change(world, pos);
            }
        }
        Block::RedstoneLamp { lit } => {
            let should_be_lit = redstone_lamp_should_be_lit(world, pos);
            if lit && !should_be_lit {
                world.set_block(pos, Block::RedstoneLamp { lit: false });
            }
        }
        Block::StoneButton { mut button } => {
            if button.powered {
                button.powered = false;
                world.set_block(pos, Block::StoneButton { button });
                update_surrounding_blocks(world, pos);
                match button.face {
                    ButtonFace::Ceiling => {
                        update_surrounding_blocks(world, pos.offset(BlockFace::Top));
                    }
                    ButtonFace::Floor => {
                        update_surrounding_blocks(world, pos.offset(BlockFace::Bottom));
                    }
                    ButtonFace::Wall => update_surrounding_blocks(
                        world,
                        pos.offset(button.facing.opposite().block_face()),
                    ),
                }
            }
        }
        Block::Observer { observer } => {
            if observer.powered {
                world.set_block(
                    pos,
                    Block::Observer {
                        observer: observer.power(false),
                    },
                );
            } else {
                world.set_block(
                    pos,
                    Block::Observer {
                        observer: observer.power(true),
                    },
                );
                world.schedule_tick(pos, 1, TickPriority::Normal);
            }
            on_observer_state_change(observer.facing, world, pos);
        }
        Block::Piston { piston } => {
            piston::piston_tick(world, piston, pos);
        }
        Block::MovingPiston { moving } => {
            piston::moving_piston_tick(world, moving, pos);
        }
        _ => {}
    }
}

fn on_observer_state_change(facing: BlockFacing, world: &mut impl World, pos: BlockPos) {
    tracing::info!("Observer state change: {:?} {:?}", facing, pos);
    let front_pos = pos.offset(facing.opposite().into());
    let front_block = world.get_block(front_pos);
    update(front_block, world, front_pos, None);
    tracing::info!("Front block updated, {:?}", front_block);
    for direction in BlockFace::values() {
        if direction == facing.into() {
            continue;
        }
        let neighbor_pos = front_pos.offset(direction);
        let block = world.get_block(neighbor_pos);
        tracing::info!("Neighbor block updated, {:?}", block);
        update(block, world, neighbor_pos, None);
    }
}

pub fn update_wire_neighbors(world: &mut impl World, pos: BlockPos) {
    for direction in &BlockFace::values() {
        let neighbor_pos = pos.offset(*direction);
        let block = world.get_block(neighbor_pos);
        update(block, world, neighbor_pos, Some(direction.opposite()));
        for n_direction in &BlockFace::values() {
            let n_neighbor_pos = neighbor_pos.offset(*n_direction);
            let block = world.get_block(n_neighbor_pos);
            update(block, world, n_neighbor_pos, Some(n_direction.opposite()));
        }
    }
}

// original update_surrounding_blocks
pub fn update_surrounding_blocks(world: &mut impl World, pos: BlockPos) {
    skipping_update_surrounding_blocks(world, pos, true);
}

pub fn on_torch_state_change(world: &mut impl World, pos: BlockPos) {
    skipping_update_surrounding_blocks(world, pos, false);
}

pub fn skipping_update_surrounding_blocks(
    world: &mut impl World,
    pos: BlockPos,
    skip_pistons: bool,
) {
    for direction in &BlockFace::values() {
        let neighbor_pos = pos.offset(*direction);
        let block = world.get_block(neighbor_pos);
        update(block, world, neighbor_pos, Some(direction.opposite()));

        // Also update diagonal blocks

        let up_pos = neighbor_pos.offset(BlockFace::Top);
        let up_block = world.get_block(up_pos);
        if !skip_pistons || !matches!(up_block, Block::Piston { .. }) {
            update(up_block, world, up_pos, Some(BlockFace::Bottom));
        }

        let down_pos = neighbor_pos.offset(BlockFace::Bottom);
        let down_block = world.get_block(down_pos);
        if !skip_pistons || !matches!(down_block, Block::Piston { .. }) {
            update(down_block, world, down_pos, Some(BlockFace::Top));
        }
    }
}

pub fn is_diode(block: Block) -> bool {
    matches!(
        block,
        Block::RedstoneRepeater { .. } | Block::RedstoneComparator { .. }
    )
}
