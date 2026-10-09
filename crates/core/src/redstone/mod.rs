//! A very basic redstone implementation with focus on accuracy over speed.
//! This is the implementation that is used by default in low-performance
//! scenerio (i.e. regular buiding)

#[cfg(test)]
mod adder_tests;
pub(crate) mod command_block;
pub mod comparator;
pub(crate) mod copper_bulb;
#[cfg(test)]
pub(crate) mod instant_piston_tests;
#[cfg(test)]
mod master_tests;
pub mod noteblock;
#[cfg(test)]
mod observer_tests;
pub(crate) mod piston;
pub(crate) mod power;
pub mod repeater;
pub mod wire;

use crate::world::World;
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::{Block, ButtonFace};
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
    let strength = source_strength(block, world, pos);
    if strength > 0 && power::emits_weak_power(block, world, pos, side, dust_power) {
        strength
    } else {
        0
    }
}

pub(crate) fn source_strength(block: Block, world: &impl World, pos: BlockPos) -> u8 {
    match block {
        Block::RedstoneTorch { lit: true } | Block::RedstoneWallTorch { lit: true, .. } => 15,
        Block::RedstoneBlock => 15,
        Block::StonePressurePlate { powered: true } => 15,
        block if block.pressure_plate_powered() == Some(true) => 15,
        Block::Lever { lever } if lever.powered => 15,
        Block::StoneButton { button } if button.powered => 15,
        Block::RedstoneRepeater { repeater } if repeater.powered => 15,
        Block::RedstoneComparator { .. } => {
            if let Some(BlockEntity::Comparator { output_strength }) = world.get_block_entity(pos) {
                *output_strength
            } else {
                0
            }
        }
        Block::RedstoneWire { wire } => wire.power,
        Block::Observer { observer } if observer.powered => 15,
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
    let strength = source_strength(block, world, pos);
    if strength > 0 && power::emits_strong_power(block, world, pos, side, dust_power) {
        strength
    } else {
        0
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

/// Threshold consumers need existence, whereas analog consumers need the
/// complete maximum. Reads here do not issue callbacks or mutate the world.
fn has_redstone_power(block: Block, world: &impl World, pos: BlockPos, facing: BlockFace) -> bool {
    if block.is_solid() {
        BlockFace::values().into_iter().any(|side| {
            let neighbor = pos.offset(side);
            get_strong_power(world.get_block(neighbor), world, neighbor, side, true) > 0
        })
    } else {
        get_weak_power(block, world, pos, facing, true) > 0
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
    if let Some(input) = world.resolved_redstone_input(pos, false) {
        return input > 0;
    }
    let bottom_pos = pos.offset(BlockFace::Bottom);
    let bottom_block = world.get_block(bottom_pos);
    has_redstone_power(bottom_block, world, bottom_pos, BlockFace::Top)
}

pub fn on_state_change(facing: BlockFacing, world: &mut impl World, pos: BlockPos) {
    let front_pos = pos.offset(facing.opposite().into());
    update_output_neighbors(world, front_pos, facing.into(), pos);
}

fn update_output_neighbors(
    world: &mut impl World,
    front_pos: BlockPos,
    source_face: BlockFace,
    source: BlockPos,
) {
    diode_notifications(front_pos, source_face, |pos, dir| {
        update_from(world.get_block(pos), world, pos, dir, source)
    });
}

pub(crate) fn diode_notifications(
    front_pos: BlockPos,
    source_face: BlockFace,
    mut visit: impl FnMut(BlockPos, Option<BlockFace>),
) {
    visit(front_pos, Some(source_face));
    for direction in BlockFace::values() {
        visit(front_pos.offset(direction), Some(direction));
    }
}

pub fn wall_torch_should_be_off(
    world: &impl World,
    pos: BlockPos,
    direction: BlockDirection,
) -> bool {
    if let Some(input) = world.resolved_redstone_input(pos, false) {
        return input > 0;
    }
    let wall_pos = pos.offset(direction.opposite().block_face());
    let wall_block = world.get_block(wall_pos);
    has_redstone_power(
        wall_block,
        world,
        wall_pos,
        direction.opposite().block_face(),
    )
}

pub fn redstone_lamp_should_be_lit(world: &impl World, pos: BlockPos) -> bool {
    if let Some(input) = world.resolved_redstone_input(pos, false) {
        return input > 0;
    }
    for face in &BlockFace::values() {
        let neighbor_pos = pos.offset(*face);
        if has_redstone_power(world.get_block(neighbor_pos), world, neighbor_pos, *face) {
            return true;
        }
    }
    false
}

fn diode_get_input_strength(world: &impl World, pos: BlockPos, facing: BlockDirection) -> u8 {
    if let Some(input) = world.resolved_redstone_input(pos, false) {
        return input;
    }
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

/// Electrical input only; comparator inventory precedence is applied separately.
pub(crate) fn consumer_input(world: &impl World, pos: BlockPos, side: bool) -> u8 {
    match world.get_block(pos) {
        Block::RedstoneComparator { comparator } if side => {
            comparator::get_power_on_sides(comparator, world, pos)
        }
        Block::RedstoneComparator { comparator } => {
            diode_get_input_strength(world, pos, comparator.facing)
        }
        Block::RedstoneRepeater { repeater } => {
            diode_get_input_strength(world, pos, repeater.facing)
        }
        Block::RedstoneTorch { .. } => bool_to_ss(torch_should_be_off(world, pos)),
        Block::RedstoneWallTorch { facing, .. } => {
            bool_to_ss(wall_torch_should_be_off(world, pos, facing))
        }
        _ => bool_to_ss(redstone_lamp_should_be_lit(world, pos)),
    }
}

/// Whether `update` can do any work for this cached block state. Wire walks use
/// this to omit inert callback queue entries without pruning the power/heading graph.
/// Keep this classification in sync with the dispatcher below (tested over all states).
pub(crate) fn has_neighbor_update(block: Block) -> bool {
    match block {
        Block::RedstoneWire { .. }
        | Block::RedstoneTorch { .. }
        | Block::RedstoneWallTorch { .. }
        | Block::RedstoneRepeater { .. }
        | Block::RedstoneComparator { .. }
        | Block::RedstoneLamp { .. }
        | Block::Hopper { .. }
        | Block::IronTrapdoor { .. }
        | Block::Piston { .. }
        | Block::PistonHead { .. }
        | Block::Observer { .. }
        | Block::NoteBlock { .. } => true,
        // Command blocks are registry states outside the modeled enum. Known
        // inert variants (especially air) do not need a registry/name lookup.
        Block::Unknown { .. } => block.is_command_block() || block.is_copper_bulb(),
        _ => false,
    }
}

pub fn update(block: Block, world: &mut impl World, pos: BlockPos, dir: Option<BlockFace>) {
    update_inner(block, world, pos, dir, None);
}

pub(crate) fn update_from(
    block: Block,
    world: &mut impl World,
    pos: BlockPos,
    dir: Option<BlockFace>,
    source: BlockPos,
) {
    update_inner(block, world, pos, dir, Some(source));
}

fn update_inner(
    block: Block,
    world: &mut impl World,
    pos: BlockPos,
    dir: Option<BlockFace>,
    source: Option<BlockPos>,
) {
    #[cfg(test)]
    let _trace = instant_piston_tests::callback(world, block, pos, dir, source);
    if world.dispatch_redstone_update(pos, dir, source) {
        return;
    }
    if block.is_command_block() {
        command_block::update(world, pos);
        return;
    }
    if block.is_copper_bulb() {
        copper_bulb::update(world, pos);
        return;
    }
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
        Block::Hopper { facing, enabled } => {
            let should_be_enabled = !redstone_lamp_should_be_lit(world, pos);
            if enabled != should_be_enabled {
                world.set_block(
                    pos,
                    Block::Hopper {
                        facing,
                        enabled: should_be_enabled,
                    },
                );
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
                if observer.facing == dir.into() && !world.pending_tick_at(pos) {
                    world.schedule_tick(pos, 1, TickPriority::Normal);
                }
            } else if observer.powered && !world.pending_tick_at(pos) {
                world.schedule_tick(pos, 1, TickPriority::Normal);
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
    if block.is_command_block() {
        command_block::tick(world, pos);
        return;
    }
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
                world.play_sound(
                    pos,
                    crate::sound::event_id("block.stone_button.click_off"),
                    4,
                    1.0,
                    1.0,
                );
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
        Block::MovingPiston { .. } => {}
        _ => {}
    }
}

fn on_observer_state_change(facing: BlockFacing, world: &mut impl World, pos: BlockPos) {
    observer_notifications(facing, pos, |target, dir| {
        update_from(world.get_block(target), world, target, dir, pos)
    });
}

pub(crate) fn observer_notifications(
    facing: BlockFacing,
    pos: BlockPos,
    mut visit: impl FnMut(BlockPos, Option<BlockFace>),
) {
    let front_pos = pos.offset(facing.opposite().into());
    visit(front_pos, Some(facing.into()));
    for direction in BlockFace::values() {
        if direction == facing.into() {
            continue;
        }
        visit(front_pos.offset(direction), None);
    }
}

pub fn update_wire_neighbors(world: &mut impl World, pos: BlockPos) {
    for direction in &BlockFace::values() {
        let neighbor_pos = pos.offset(*direction);
        let block = world.get_block(neighbor_pos);
        update_from(block, world, neighbor_pos, Some(direction.opposite()), pos);
        for n_direction in &BlockFace::values() {
            let n_neighbor_pos = neighbor_pos.offset(*n_direction);
            let block = world.get_block(n_neighbor_pos);
            // These are power notifications through a neighboring block. Its
            // state did not change, so an observer watching it must not pulse.
            // Observers adjacent to the changed wire are notified above.
            if !matches!(block, Block::Observer { .. }) {
                update_from(
                    block,
                    world,
                    n_neighbor_pos,
                    Some(n_direction.opposite()),
                    pos,
                );
            }
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
    surrounding_notifications(pos, |target, dir, diagonal| {
        let block = world.get_block(target);
        if diagonal
            && ((target == pos && block.is_copper_bulb())
                || matches!(block, Block::Observer { .. })
                || (skip_pistons && matches!(block, Block::Piston { .. })))
        {
            return;
        }
        update_from(block, world, target, Some(dir), pos);
    });
}

/// Visit direct neighbors and their vertical diagonals in interpreter order.
/// Keep repeated positions, including the origin: these are ordered callbacks.
/// Callers filter recipients because diagonal rechecks are not watched-block changes.
pub(crate) fn surrounding_notifications(
    pos: BlockPos,
    mut visit: impl FnMut(BlockPos, BlockFace, bool),
) {
    for direction in &BlockFace::values() {
        let neighbor_pos = pos.offset(*direction);
        visit(neighbor_pos, direction.opposite(), false);
        let up_pos = neighbor_pos.offset(BlockFace::Top);
        visit(up_pos, BlockFace::Bottom, true);
        let down_pos = neighbor_pos.offset(BlockFace::Bottom);
        visit(down_pos, BlockFace::Top, true);
    }
}

pub fn is_diode(block: Block) -> bool {
    matches!(
        block,
        Block::RedstoneRepeater { .. } | Block::RedstoneComparator { .. }
    )
}

#[cfg(test)]
mod tests {
    use mchprs_blocks::blocks::Block;
    use mchprs_blocks::BlockPos;
    use std::fs::File;
    use std::ops::{Deref, DerefMut};
    use std::path::PathBuf;

    use crate::interaction::place_in_world;
    use crate::plot::worldedit::{load_schematic, paste_clipboard};
    use crate::plot::{empty_plot, PlotWorld, PLOT_WIDTH};
    use crate::tests::click_floor_button;
    use crate::world::storage::Chunk;
    use crate::world::World;

    const BUTTON_POS: BlockPos = BlockPos::new(100, 30, 100);
    const BASE_WIRE: BlockPos = BlockPos::new(100, 30, 102);
    const HEAD_WIRE: BlockPos = BlockPos::new(102, 30, 102);
    const PUSH_WIRE: BlockPos = BlockPos::new(104, 30, 102);

    pub struct TestWorld(PlotWorld);

    impl Deref for TestWorld {
        type Target = PlotWorld;
        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }
    impl DerefMut for TestWorld {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.0
        }
    }
    impl TestWorld {
        pub fn load_with_schematic(name: &str, at_pos: BlockPos) -> Self {
            let path = ["..", "..", "test_data", name].iter().collect::<PathBuf>();
            let file = File::open(path).unwrap();
            let clipboard = load_schematic(file).unwrap();

            let chunks: Vec<Chunk> = empty_plot()
                .chunk_data
                .into_iter()
                .enumerate()
                .map(|(i, c)| Chunk::load(i as i32 / PLOT_WIDTH, i as i32 % PLOT_WIDTH, c))
                .collect();

            let mut world = PlotWorld::from_chunks(0, 0, chunks, Default::default());
            paste_clipboard(&mut world, &clipboard, at_pos, false);
            Self(world)
        }
        pub fn is_wire_powered(&self, pos: BlockPos) -> bool {
            match self.get_block(pos) {
                Block::RedstoneWire { wire } => wire.power > 0,
                _ => false,
            }
        }
        pub fn click_floor_button(&mut self, pos: BlockPos) {
            click_floor_button(self, pos);
        }
        pub fn place(&mut self, pos: BlockPos, block: Block) {
            place_in_world(block, &mut self.0, pos, &None);
        }
        pub fn get_piston_extended(&self, pos: BlockPos) -> bool {
            match self.get_block(pos) {
                Block::Piston { piston } => piston.extended,
                _ => false,
            }
        }
    }

    fn java_trace(name: &str) -> serde_json::Value {
        let data: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../test_data/piston-repair/java-traces.json"
        ))
        .unwrap();
        assert_eq!(
            data["server_sha1"],
            "e6ec2f64e6080b9b5d9b471b291c33cc7f509733"
        );
        data["fixtures"][name]["trace"].clone()
    }

    fn compare_wire_trace(name: &str) {
        let expected: Vec<[bool; 3]> = serde_json::from_value(java_trace(name)).unwrap();
        let world = &mut TestWorld::load_with_schematic(name, BUTTON_POS);
        world.click_floor_button(BUTTON_POS);
        let got: Vec<_> = expected
            .iter()
            .map(|_| {
                world.tick_interpreted();
                [
                    world.is_wire_powered(BASE_WIRE),
                    world.is_wire_powered(HEAD_WIRE),
                    world.is_wire_powered(PUSH_WIRE),
                ]
            })
            .collect();
        assert_eq!(expected, got, "Java 1.21.5 trace for {name}");
    }

    #[test]
    fn test_edgecase_single_pulse_on_both_outputs() {
        let reference: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../test_data/piston-repair/java-edgecase-traces.json"
        ))
        .unwrap();
        assert_eq!(
            reference["server_sha1"],
            "e6ec2f64e6080b9b5d9b471b291c33cc7f509733"
        );
        let fixture = &reference["fixtures"]["MCHPRS_EDGECASE.schem"];
        use sha2::{Digest, Sha256};
        assert_eq!(
            fixture["sha256"],
            format!(
                "{:x}",
                Sha256::digest(include_bytes!(
                    "../../../../test_data/MCHPRS_EDGECASE.schem"
                ))
            )
        );
        let expected: Vec<[bool; 2]> = serde_json::from_value(fixture["trace"].clone()).unwrap();
        let invalid = BlockPos::new(100, 30, 100);
        let correct = BlockPos::new(102, 30, 100);
        let trigger = BlockPos::new(98, 30, 100);
        // Both outputs have exactly one two-game-tick (one redstone-tick) pulse.
        for output in 0..2 {
            assert_eq!(expected.iter().filter(|sample| sample[output]).count(), 2);
        }
        for stepping in ["game", "nano", "pico"] {
            let world = &mut TestWorld::load_with_schematic("MCHPRS_EDGECASE.schem", BUTTON_POS);
            world.place(trigger, Block::RedstoneBlock {});
            for _ in 0..8 {
                world.tick_interpreted();
            }
            world.place(trigger, Block::Air);
            let mut trace = Vec::new();
            for _ in &expected {
                let target_tick = world.piston_state().logical_tick + 1;
                for _ in 0..256 {
                    match stepping {
                        "game" => world.tick_interpreted(),
                        "nano" => world.nanotick_advance(1),
                        "pico" => world.picotick_advance(1),
                        _ => unreachable!(),
                    }
                    if world.piston_state().logical_tick == target_tick
                        && world.piston_state().phase == mchprs_world::AdvancePhase::BetweenTicks
                    {
                        break;
                    }
                }
                assert_eq!(world.piston_state().logical_tick, target_tick);
                assert_eq!(
                    world.piston_state().phase,
                    mchprs_world::AdvancePhase::BetweenTicks
                );
                trace.push([
                    world.is_wire_powered(invalid),
                    world.is_wire_powered(correct),
                ]);
            }
            assert_eq!(
                trace, expected,
                "Java 1.21.5 edge-case trace with {stepping} stepping"
            );
            for x in [100, 102] {
                assert!(world.get_piston_extended(BlockPos::new(x, 30, 103)));
            }
        }
    }

    #[test]
    fn test_updates_non_instant() {
        compare_wire_trace("UpdateTesterNonInst.schem");
    }
    #[test]
    fn test_updates_instant() {
        compare_wire_trace("UpdateTesterInst.schem");
    }
    #[test]
    fn test_updates_extend_instant() {
        compare_wire_trace("UpdateTesterExtendInst.schem");
    }
    #[test]
    fn test_updates_extend_non_instant() {
        compare_wire_trace("UpdateTesterExtendNonInst.schem");
    }

    #[test]
    fn test_memory_cell_unaligned_nanoticks() {
        let expected: Vec<bool> =
            serde_json::from_value(java_trace("MemCellUnalignedNanoTicks.schem")).unwrap();
        let cell_pos = BlockPos::new(97, 30, 107);
        let world =
            &mut TestWorld::load_with_schematic("MemCellUnalignedNanoTicks.schem", BUTTON_POS);
        assert!(!world.get_piston_extended(cell_pos));

        world.place(BUTTON_POS, Block::Air);
        let got: Vec<_> = expected
            .iter()
            .map(|_| {
                world.tick_interpreted();
                world.get_piston_extended(cell_pos)
            })
            .collect();
        assert_eq!(expected, got, "Java 1.21.5 memory cell trace");
    }
}
