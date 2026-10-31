use crate::world::World;
use mchprs_blocks::blocks::{Block, ButtonFace, StoneButton};
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_world::TickPriority;

pub(crate) fn press(mut button: StoneButton, world: &mut impl World, pos: BlockPos, player: Option<u128>) {
    if button.powered {
        return;
    }
    let block = Block::StoneButton { button };
    button.powered = true;
    world.set_block(pos, Block::StoneButton { button });
    if let Some(uuid) = player {
        crate::sound::control_used(world, pos, block, uuid);
    }
    world.schedule_tick(pos, 10, TickPriority::Normal);
    notify_neighbors(button, world, pos);
}

pub(crate) fn release(mut button: StoneButton, world: &mut impl World, pos: BlockPos) {
    if !button.powered {
        return;
    }
    button.powered = false;
    world.set_block(pos, Block::StoneButton { button });
    world.play_sound(pos, crate::sound::event_id("block.stone_button.click_off"), 4, 1.0, 1.0);
    notify_neighbors(button, world, pos);
}

fn notify_neighbors(button: StoneButton, world: &mut impl World, pos: BlockPos) {
    super::update_surrounding_blocks(world, pos);
    let attached = match button.face {
        ButtonFace::Ceiling => BlockFace::Top,
        ButtonFace::Floor => BlockFace::Bottom,
        ButtonFace::Wall => button.facing.opposite().block_face(),
    };
    super::update_surrounding_blocks(world, pos.offset(attached));
}
