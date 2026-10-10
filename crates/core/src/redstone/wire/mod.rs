mod turbo;

pub(crate) const TURBO_ORDER: [[usize; 24]; 4] = [
    [
        2, 3, 16, 19, 0, 4, 1, 5, 7, 8, 17, 20, 12, 13, 18, 21, 6, 9, 22, 14, 11, 10, 23, 15,
    ],
    [
        2, 3, 16, 19, 4, 1, 5, 0, 17, 20, 12, 13, 18, 21, 7, 8, 22, 14, 11, 15, 23, 9, 6, 10,
    ],
    [
        2, 3, 16, 19, 1, 5, 0, 4, 12, 13, 18, 21, 7, 8, 17, 20, 11, 15, 23, 10, 6, 14, 22, 9,
    ],
    [
        2, 3, 16, 19, 5, 0, 4, 1, 18, 21, 7, 8, 17, 20, 12, 13, 23, 10, 6, 9, 22, 15, 11, 14,
    ],
];

use crate::interaction::ActionResult;
use crate::world::World;
use mchprs_blocks::blocks::{Block, RedstoneWire, RedstoneWireSide};
use mchprs_blocks::{BlockDirection, BlockFace, BlockPos};
use turbo::RedstoneWireTurbo;

pub(crate) fn invalidate_turbo_cache() {
    turbo::invalidate_scratch();
}

fn make_cross(power: u8) -> RedstoneWire {
    RedstoneWire {
        north: RedstoneWireSide::Side,
        south: RedstoneWireSide::Side,
        east: RedstoneWireSide::Side,
        west: RedstoneWireSide::Side,
        power,
    }
}

pub fn get_state_for_placement(world: &impl World, pos: BlockPos) -> RedstoneWire {
    let mut wire = RedstoneWire {
        power: calculate_power(world, pos),
        ..Default::default()
    };
    wire = get_regulated_sides(wire, world, pos);
    if is_dot(wire) {
        wire = make_cross(wire.power);
    }
    wire
}

pub fn on_neighbor_changed(
    wire: RedstoneWire,
    world: &impl World,
    pos: BlockPos,
    side: BlockFace,
) -> RedstoneWire {
    if side == BlockFace::Top {
        return wire;
    }
    on_neighbor_changed_from(wire, get_all_sides(wire, world, pos), side)
}

pub(crate) fn on_neighbor_changed_from(
    mut wire: RedstoneWire,
    raw: RedstoneWire,
    side: BlockFace,
) -> RedstoneWire {
    let old_state = wire;
    if side == BlockFace::Top {
        return wire;
    }
    // Read the geometry once. The cross guard needs the raw side below, while
    // regulation needs the wire with that one stored side already updated.
    let new_side = match side {
        BlockFace::Top => unreachable!(),
        BlockFace::Bottom => {
            return regulate_sides(wire, raw);
        }
        BlockFace::North => {
            wire.south = raw.south;
            wire.south
        }
        BlockFace::South => {
            wire.north = raw.north;
            wire.north
        }

        BlockFace::East => {
            wire.west = raw.west;
            wire.west
        }
        BlockFace::West => {
            wire.east = raw.east;
            wire.east
        }
    };
    wire = regulate_sides(wire, raw);
    if is_cross(old_state) && new_side.is_none() {
        // Don't mess up the cross
        return old_state;
    }
    if !is_dot(old_state) && is_dot(wire) {
        // Save the power until the transformation into cross is complete
        let power = wire.power;
        // Become the cross it always wanted to be
        wire = make_cross(power);
    }
    wire
}

// todo can piston instant repeater be implemented as a redstone wire????
pub fn on_neighbor_updated(mut wire: RedstoneWire, world: &mut impl World, pos: BlockPos) {
    let new_power = calculate_power(world, pos);

    if wire.power != new_power {
        wire.power = new_power;
        world.set_block(pos, Block::RedstoneWire { wire });
        RedstoneWireTurbo::update_surrounding_neighbors(world, pos);
    }
}

pub fn on_use(wire: RedstoneWire, world: &mut impl World, pos: BlockPos) -> ActionResult {
    if is_dot(wire) || is_cross(wire) {
        let mut new_wire = if is_cross(wire) {
            RedstoneWire::default()
        } else {
            make_cross(0)
        };
        new_wire.power = wire.power;
        new_wire = get_regulated_sides(new_wire, world, pos);
        if wire != new_wire {
            world.set_block(pos, Block::RedstoneWire { wire: new_wire });
            super::update_wire_neighbors(world, pos);
            return ActionResult::Success;
        }
    }
    ActionResult::Pass
}

pub(crate) fn can_connect_to_uncached(block: Block, side: BlockDirection) -> bool {
    if block.pressure_plate_powered().is_some() {
        return true;
    }
    match block {
        Block::RedstoneWire { .. }
        | Block::RedstoneComparator { .. }
        | Block::RedstoneTorch { .. }
        | Block::RedstoneBlock
        | Block::RedstoneWallTorch { .. }
        | Block::StonePressurePlate { .. }
        | Block::TripwireHook { .. }
        | Block::StoneButton { .. }
        | Block::Target
        | Block::Lever { .. } => true,
        Block::RedstoneRepeater { repeater } => {
            repeater.facing == side || repeater.facing == side.opposite()
        }
        Block::Observer { observer } => observer.facing == side.block_facing(),
        _ => false,
    }
}

fn can_connect_diagonal_to(block: Block) -> bool {
    matches!(block, Block::RedstoneWire { .. })
}

pub fn get_current_side(wire: RedstoneWire, side: BlockDirection) -> RedstoneWireSide {
    use BlockDirection::*;
    match side {
        North => wire.north,
        South => wire.south,
        East => wire.east,
        West => wire.west,
    }
}

pub fn get_side(world: &impl World, pos: BlockPos, side: BlockDirection) -> RedstoneWireSide {
    get_side_with_above(world, pos, side, &mut None)
}

fn get_side_with_above(
    world: &impl World,
    pos: BlockPos,
    side: BlockDirection,
    above_solid: &mut Option<bool>,
) -> RedstoneWireSide {
    get_side_from(pos, side, above_solid, &|pos| world.get_block(pos))
}

fn get_side_from(
    pos: BlockPos,
    side: BlockDirection,
    above_solid: &mut Option<bool>,
    read: &impl Fn(BlockPos) -> Block,
) -> RedstoneWireSide {
    let neighbor_pos = pos.offset(side.block_face());
    let neighbor = read(neighbor_pos);

    if crate::world::wire_cache::connects(neighbor, side) {
        return RedstoneWireSide::Side;
    }

    let up_solid = *above_solid.get_or_insert_with(|| read(pos.offset(BlockFace::Top)).is_solid());

    if !up_solid && can_connect_diagonal_to(read(neighbor_pos.offset(BlockFace::Top))) {
        RedstoneWireSide::Up
    } else if !neighbor.is_solid()
        && can_connect_diagonal_to(read(neighbor_pos.offset(BlockFace::Bottom)))
    {
        RedstoneWireSide::Side
    } else {
        RedstoneWireSide::None
    }
}

fn get_all_sides(mut wire: RedstoneWire, world: &impl World, pos: BlockPos) -> RedstoneWire {
    let mut above_solid = None;
    wire.north = get_side_with_above(world, pos, BlockDirection::North, &mut above_solid);
    wire.south = get_side_with_above(world, pos, BlockDirection::South, &mut above_solid);
    wire.east = get_side_with_above(world, pos, BlockDirection::East, &mut above_solid);
    wire.west = get_side_with_above(world, pos, BlockDirection::West, &mut above_solid);
    wire
}

pub fn get_regulated_sides(wire: RedstoneWire, world: &impl World, pos: BlockPos) -> RedstoneWire {
    regulate_sides(wire, get_all_sides(wire, world, pos))
}

pub(crate) fn get_raw_sides_from(
    wire: RedstoneWire,
    pos: BlockPos,
    read: impl Fn(BlockPos) -> Block,
) -> RedstoneWire {
    let mut state = wire;
    let mut above = None;
    state.north = get_side_from(pos, BlockDirection::North, &mut above, &read);
    state.south = get_side_from(pos, BlockDirection::South, &mut above, &read);
    state.east = get_side_from(pos, BlockDirection::East, &mut above, &read);
    state.west = get_side_from(pos, BlockDirection::West, &mut above, &read);
    state
}

pub(crate) fn regulate_sides(wire: RedstoneWire, mut state: RedstoneWire) -> RedstoneWire {
    if is_dot(wire) && is_dot(state) {
        return state;
    }
    let north_none = state.north.is_none();
    let south_none = state.south.is_none();
    let east_none = state.east.is_none();
    let west_none = state.west.is_none();
    let north_south_none = north_none && south_none;
    let east_west_none = east_none && west_none;
    if north_none && east_west_none {
        state.north = RedstoneWireSide::Side;
    }
    if south_none && east_west_none {
        state.south = RedstoneWireSide::Side;
    }
    if east_none && north_south_none {
        state.east = RedstoneWireSide::Side;
    }
    if west_none && north_south_none {
        state.west = RedstoneWireSide::Side;
    }
    state
}

pub(crate) fn is_dot(wire: RedstoneWire) -> bool {
    wire.north == RedstoneWireSide::None
        && wire.south == RedstoneWireSide::None
        && wire.east == RedstoneWireSide::None
        && wire.west == RedstoneWireSide::None
}

pub(crate) fn is_cross(wire: RedstoneWire) -> bool {
    wire.north == RedstoneWireSide::Side
        && wire.south == RedstoneWireSide::Side
        && wire.east == RedstoneWireSide::Side
        && wire.west == RedstoneWireSide::Side
}

fn max_wire_power(wire_power: u8, block: Block) -> u8 {
    if let Block::RedstoneWire { wire } = block {
        wire_power.max(wire.power)
    } else {
        wire_power
    }
}

fn calculate_power(world: &impl World, pos: BlockPos) -> u8 {
    let mut block_power = 0;
    let mut wire_power = 0;

    let up_pos = pos.offset(BlockFace::Top);
    let up_solid = world.get_block(up_pos).is_solid();

    for side in &BlockFace::values() {
        let neighbor_pos = pos.offset(*side);
        let neighbor = world.get_block(neighbor_pos);
        wire_power = max_wire_power(wire_power, neighbor);
        block_power = block_power.max(super::get_redstone_power_no_dust(
            neighbor,
            world,
            neighbor_pos,
            *side,
        ));
        if side.is_horizontal() {
            if !up_solid && !neighbor.is_transparent() {
                wire_power = max_wire_power(
                    wire_power,
                    world.get_block(neighbor_pos.offset(BlockFace::Top)),
                );
            }

            if !neighbor.is_solid() {
                wire_power = max_wire_power(
                    wire_power,
                    world.get_block(neighbor_pos.offset(BlockFace::Bottom)),
                );
            }
        }
    }

    block_power.max(wire_power.saturating_sub(1))
}
