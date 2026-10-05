use super::{Plot, PLOT_BLOCK_HEIGHT};
use crate::player::{PacketSender, PlayerPos};
use crate::world::World;
use mchprs_blocks::blocks::{Block, SlabType};
use mchprs_blocks::items::Item;
use mchprs_blocks::BlockPos;
use mchprs_network::packets::clientbound::{CEntityTeleport, ClientBoundPacket};
use std::time::{Duration, Instant};

const RANGE: f64 = 1024.0;
const HALF_WIDTH: f64 = 0.3;
const SEARCH_DISTANCE: f64 = 8.0;

impl Plot {
    /// Returns true when the held item consumes this interaction, even on a miss.
    pub(super) fn use_compass(&mut self, player: usize, hand: i32, yaw: f32, pitch: f32) -> bool {
        let data = &self.players[player];
        let slot = match hand {
            0 => 36 + data.selected_slot as usize,
            1 => 45,
            _ => return false,
        };
        if !data
            .inventory
            .get(slot)
            .and_then(Option::as_ref)
            .is_some_and(|item| item.item_type == Item::Compass)
        {
            return false;
        }
        if !yaw.is_finite() || !pitch.is_finite() {
            return true;
        }
        // Java can send both use-on-block and use-item for a single click.
        if data
            .last_compass_use
            .is_some_and(|last| last.elapsed() < Duration::from_millis(200))
        {
            return true;
        }
        let crouching = data.crouching;
        let eye = PlayerPos::new(
            data.pos.x,
            data.pos.y + if crouching { 1.27 } else { 1.62 },
            data.pos.z,
        );
        self.players[player].last_compass_use = Some(Instant::now());
        let read = |pos: BlockPos| {
            if !Plot::in_plot_bounds(self.world.x, self.world.z, pos.x, pos.z) {
                None
            } else if !(0..PLOT_BLOCK_HEIGHT).contains(&pos.y) {
                Some(Block::Air)
            } else {
                Some(self.world.get_block(pos))
            }
        };
        let destination = compass_destination(eye, yaw, pitch, &read);
        let Some(destination) = destination else {
            self.players[player].send_error_message("No safe compass destination in sight.");
            return true;
        };
        self.close_open_container(player);
        let old = self.players[player].pos;
        self.players[player].teleport(destination);
        self.players[player].on_ground = false;
        let data = &self.players[player];
        let packet = CEntityTeleport {
            entity_id: data.entity_id as i32,
            x: destination.x,
            y: destination.y,
            z: destination.z,
            yaw: data.yaw,
            pitch: data.pitch,
            on_ground: false,
        }
        .encode();
        for (index, other) in self.players.iter().enumerate() {
            if index != player {
                other.client.send_packet(&packet);
            }
        }
        self.on_player_move(player, old, destination);
        self.update_view_pos_for_player(player, false);
        true
    }
}

fn is_air(block: Block) -> bool {
    matches!(block.get_name(), "air" | "cave_air" | "void_air")
}

/// Walk voxel boundaries instead of sampling: thin diagonal hits cannot be skipped.
fn pointed_block(
    eye: PlayerPos,
    yaw: f32,
    pitch: f32,
    read: &impl Fn(BlockPos) -> Option<Block>,
) -> Option<RayHit> {
    if !eye.is_valid() || !yaw.is_finite() || !pitch.is_finite() {
        return None;
    }
    let yaw = f64::from(yaw).to_radians();
    let pitch = f64::from(pitch).to_radians();
    let direction = [
        -yaw.sin() * pitch.cos(),
        -pitch.sin(),
        yaw.cos() * pitch.cos(),
    ];
    let origin = [eye.x, eye.y, eye.z];
    let mut cell = [
        eye.x.floor() as i32,
        eye.y.floor() as i32,
        eye.z.floor() as i32,
    ];
    let step = direction.map(|d| if d < 0.0 { -1 } else { 1 });
    let delta = direction.map(|d| {
        if d.abs() < 1e-12 {
            f64::INFINITY
        } else {
            1.0 / d.abs()
        }
    });
    let mut next = [0.0; 3];
    for axis in 0..3 {
        next[axis] = if delta[axis].is_infinite() {
            f64::INFINITY
        } else {
            let boundary = cell[axis] + i32::from(step[axis] > 0);
            (f64::from(boundary) - origin[axis]) / direction[axis]
        };
    }
    let mut entry = 0.0;
    loop {
        let pos = BlockPos::new(cell[0], cell[1], cell[2]);
        let block = read(pos)?;
        let distance = next.into_iter().fold(f64::INFINITY, f64::min);
        let below = BlockPos::new(pos.x, pos.y - 1, pos.z);
        let extended = read(below)
            .and_then(|block| collision_bounds(block).filter(|bounds| bounds.max[1] > 1.0));
        let current =
            collision_bounds(block).filter(|_| !matches!(block.get_name(), "water" | "lava"));
        let mut closest: Option<RayHit> = None;
        // Fence and wall collision bounds extend into the voxel above them.
        for (target, bounds) in [(pos, current), (below, extended)] {
            if let Some(hit_distance) =
                bounds.and_then(|bounds| bounds.ray_intersection(target, origin, direction))
            {
                if hit_distance >= entry - 1e-9 && hit_distance <= distance.min(RANGE) + 1e-9 {
                    if closest
                        .as_ref()
                        .is_none_or(|previous| hit_distance < previous.distance)
                    {
                        closest = Some(RayHit {
                            block: target,
                            distance: hit_distance,
                            origin,
                            direction,
                        });
                    }
                }
            }
        }
        if closest.is_some() {
            return closest;
        }
        if distance > RANGE {
            return None;
        }
        // Advance tied axes together so touching a voxel corner is not a hit.
        for axis in 0..3 {
            if next[axis] <= distance + 1e-10 {
                cell[axis] += step[axis];
                next[axis] += delta[axis];
            }
        }
        entry = distance;
    }
}

struct RayHit {
    block: BlockPos,
    distance: f64,
    origin: [f64; 3],
    direction: [f64; 3],
}

impl RayHit {
    fn point(&self, distance: f64) -> PlayerPos {
        PlayerPos::new(
            self.origin[0] + self.direction[0] * distance,
            self.origin[1] + self.direction[1] * distance,
            self.origin[2] + self.direction[2] * distance,
        )
    }
}

fn compass_destination(
    eye: PlayerPos,
    yaw: f32,
    pitch: f32,
    read: &impl Fn(BlockPos) -> Option<Block>,
) -> Option<PlayerPos> {
    let hit = pointed_block(eye, yaw, pitch, read)?;
    if let Some(pos) = landing_position(hit.block, read) {
        return Some(pos);
    }
    let contact = hit.point(hit.distance);
    let mut best: Option<(f64, PlayerPos)> = None;
    let mut checked = std::collections::HashSet::new();
    // Search surfaces around the last eight blocks of the visible line, including
    // a few blocks above the hit so walls and steps have usable landing spots.
    for sample in 0..=(SEARCH_DISTANCE * 2.0) as usize {
        let distance = hit.distance - sample as f64 * 0.5;
        if distance < 0.0 {
            break;
        }
        let cell = hit.point(distance).block_pos();
        for dx in -1..=1 {
            for dz in -1..=1 {
                for dy in -3..=3 {
                    let target = BlockPos::new(cell.x + dx, cell.y + dy, cell.z + dz);
                    if !checked.insert(target) || !(0..PLOT_BLOCK_HEIGHT).contains(&target.y) {
                        continue;
                    }
                    let Some(pos) = landing_position(target, read) else {
                        continue;
                    };
                    let offset = [
                        pos.x - hit.origin[0],
                        pos.y + 0.9 - hit.origin[1],
                        pos.z - hit.origin[2],
                    ];
                    let projection: f64 =
                        offset.iter().zip(hit.direction).map(|(a, b)| a * b).sum();
                    // Keep alternatives on the visible side, rather than finding
                    // an empty room behind the block that stopped the ray.
                    if projection > hit.distance + 1.0 {
                        continue;
                    }
                    let from_hit = (pos.x - contact.x).powi(2)
                        + (pos.y - contact.y).powi(2)
                        + (pos.z - contact.z).powi(2);
                    let from_line: f64 = offset
                        .iter()
                        .zip(hit.direction)
                        .map(|(a, b)| (a - projection * b).powi(2))
                        .sum();
                    let score = from_hit + from_line;
                    if best.as_ref().is_none_or(|(previous, _)| score < *previous) {
                        best = Some((score, pos));
                    }
                }
            }
        }
    }
    best.map(|(_, pos)| pos)
}

#[derive(Clone, Copy)]
struct Bounds {
    min: [f64; 3],
    max: [f64; 3],
}

impl Bounds {
    fn ray_intersection(
        self,
        block: BlockPos,
        origin: [f64; 3],
        direction: [f64; 3],
    ) -> Option<f64> {
        let location = [f64::from(block.x), f64::from(block.y), f64::from(block.z)];
        let mut near: f64 = 0.0;
        let mut far = RANGE;
        for axis in 0..3 {
            let min = location[axis] + self.min[axis];
            let max = location[axis] + self.max[axis];
            if direction[axis].abs() < 1e-12 {
                if origin[axis] < min || origin[axis] > max {
                    return None;
                }
                continue;
            }
            let a = (min - origin[axis]) / direction[axis];
            let b = (max - origin[axis]) / direction[axis];
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if near > far {
                return None;
            }
        }
        Some(near)
    }
}

fn collision_bounds(block: Block) -> Option<Bounds> {
    let (bottom, top) = vertical_bounds(block)?;
    let mut bounds = Bounds {
        min: [0.0, bottom, 0.0],
        max: [1.0, top, 1.0],
    };
    let name = block.get_name();
    if name.ends_with("_trapdoor") {
        if block.property("open") == Some("true") {
            bounds.min[1] = 0.0;
            bounds.max[1] = 1.0;
            match block.property("facing") {
                Some("north") => bounds.min[2] = 0.8125,
                Some("south") => bounds.max[2] = 0.1875,
                Some("west") => bounds.min[0] = 0.8125,
                Some("east") => bounds.max[0] = 0.1875,
                _ => {}
            }
        } else if block.property("half") == Some("top") {
            bounds.min[1] = 0.8125;
        } else {
            bounds.max[1] = 0.1875;
        }
    }
    Some(bounds)
}

/// Conservative vertical collision bounds; unmodeled blocks occupy a full cube.
/// Redstone solidity/transparency describe conduction, not player collisions.
fn vertical_bounds(block: Block) -> Option<(f64, f64)> {
    if is_air(block) {
        return None;
    }
    if let Some(slab) = block.slab_type() {
        return Some(match slab {
            SlabType::Bottom => (0.0, 0.5),
            SlabType::Top => (0.5, 1.0),
            SlabType::Double => (0.0, 1.0),
        });
    }
    let name = block.get_name();
    // These decorations do not obstruct the player or provide a landing floor.
    // Fluids/hazards retain bounds for clearance, but are skipped as ray targets.
    if name.ends_with("_sign")
        || name.ends_with("_banner")
        || name.ends_with("_button")
        || name.ends_with("_torch")
        || name.ends_with("_rail")
        || matches!(
            name,
            "torch"
                | "soul_torch"
                | "redstone_torch"
                | "redstone_wire"
                | "lever"
                | "rail"
                | "short_grass"
                | "tall_grass"
                | "fern"
                | "large_fern"
                | "dead_bush"
                | "tripwire"
                | "tripwire_hook"
        )
    {
        return None;
    }
    if name.ends_with("_fence_gate") && block.property("open") == Some("true") {
        return None;
    }
    let height = match name {
        "soul_sand" => 0.875,
        "farmland" | "dirt_path" => 0.9375,
        "snow" => block.property("layers")?.parse::<f64>().ok()? / 8.0,
        "repeater" | "comparator" => 0.125,
        name if name.ends_with("_carpet") => 0.0625,
        name if name.ends_with("_pressure_plate") => 0.0625,
        name if name.ends_with("_fence")
            || name.ends_with("_wall")
            || name.ends_with("_fence_gate") =>
        {
            1.5
        }
        _ => 1.0,
    };
    Some((0.0, height))
}

fn landing_position(
    target: BlockPos,
    read: &impl Fn(BlockPos) -> Option<Block>,
) -> Option<PlayerPos> {
    let block = read(target)?;
    if matches!(
        block.get_name(),
        "water" | "lava" | "fire" | "soul_fire" | "cactus" | "magma_block" | "powder_snow"
    ) {
        return None;
    }
    let floor = collision_bounds(block)?;
    if floor.min[0] > 0.5 || floor.max[0] < 0.5 || floor.min[2] > 0.5 || floor.max[2] < 0.5 {
        return None;
    }
    let top = floor.max[1];
    let pos = PlayerPos::new(
        f64::from(target.x) + 0.5,
        f64::from(target.y) + top,
        f64::from(target.z) + 0.5,
    );
    // Reserve standing height even when sneaking so releasing sneak remains safe.
    let height = 1.8;
    if !pos.is_valid() || pos.y < 0.0 || pos.y + height > f64::from(PLOT_BLOCK_HEIGHT) {
        return None;
    }
    let min_x = (pos.x - HALF_WIDTH).floor() as i32;
    let max_x = (pos.x + HALF_WIDTH).ceil() as i32;
    let min_z = (pos.z - HALF_WIDTH).floor() as i32;
    let max_z = (pos.z + HALF_WIDTH).ceil() as i32;
    for x in min_x..max_x {
        for z in min_z..max_z {
            // Include blocks below the feet: fences/walls can extend 1.5 blocks up.
            for y in ((pos.y - 1.5).floor() as i32).max(0)..(pos.y + height).ceil() as i32 {
                if let Some(bounds) = collision_bounds(read(BlockPos::new(x, y, z))?) {
                    if f64::from(y) + bounds.max[1] > pos.y + 1e-9
                        && f64::from(y) + bounds.min[1] < pos.y + height
                        && f64::from(x) + bounds.max[0] > pos.x - HALF_WIDTH
                        && f64::from(x) + bounds.min[0] < pos.x + HALF_WIDTH
                        && f64::from(z) + bounds.max[2] > pos.z - HALF_WIDTH
                        && f64::from(z) + bounds.min[2] < pos.z + HALF_WIDTH
                    {
                        return None;
                    }
                }
            }
        }
    }
    Some(pos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn ray_hits_first_block_in_all_directions_and_stops_at_bounds() {
        for (yaw, pitch, target) in [
            (0.0, 0.0, BlockPos::new(0, 64, 10)),
            (90.0, 0.0, BlockPos::new(-10, 64, 0)),
            (180.0, 0.0, BlockPos::new(0, 64, -10)),
            (270.0, 0.0, BlockPos::new(10, 64, 0)),
            (0.0, 90.0, BlockPos::new(0, 54, 0)),
            (0.0, -90.0, BlockPos::new(0, 74, 0)),
            (-45.0, 0.0, BlockPos::new(10, 64, 10)),
        ] {
            let read = |pos| {
                Some(if pos == target {
                    Block::Stone {}
                } else {
                    Block::Air
                })
            };
            assert_eq!(
                pointed_block(PlayerPos::new(0.5, 64.5, 0.5), yaw, pitch, &read)
                    .map(|hit| hit.block),
                Some(target)
            );
        }
        let read = |pos: BlockPos| if pos.z < 4 { Some(Block::Air) } else { None };
        assert!(pointed_block(PlayerPos::new(0.5, 64.5, 0.5), 0.0, 0.0, &read).is_none());
        let read = |pos: BlockPos| {
            Some(if pos.z == RANGE as i32 + 1 {
                Block::Stone {}
            } else {
                Block::Air
            })
        };
        assert!(pointed_block(PlayerPos::new(0.5, 64.5, 0.5), 0.0, 0.0, &read).is_none());
    }

    #[test]
    fn landing_centers_feet_and_requires_full_headroom() {
        let target = BlockPos::new(-5, 64, -7);
        let mut blocks = HashMap::from([(target, Block::Stone {})]);
        let destination =
            landing_position(target, &|p| Some(*blocks.get(&p).unwrap_or(&Block::Air))).unwrap();
        assert_eq!(
            (destination.x, destination.y, destination.z),
            (-4.5, 65.0, -6.5)
        );
        for y in [65, 66] {
            blocks.insert(BlockPos::new(-5, y, -7), Block::Glass);
            assert!(
                landing_position(target, &|p| Some(*blocks.get(&p).unwrap_or(&Block::Air)))
                    .is_none()
            );
            blocks.remove(&BlockPos::new(-5, y, -7));
        }
    }

    #[test]
    fn landing_handles_partial_and_tall_blocks_and_world_ceiling() {
        for (name, expected) in [
            ("stone_slab", 0.5),
            ("soul_sand", 0.875),
            ("oak_fence", 1.5),
        ] {
            let block = Block::from_name(name).unwrap();
            let target = BlockPos::new(1, 64, 1);
            let read = |p| Some(if p == target { block } else { Block::Air });
            assert_eq!(landing_position(target, &read).unwrap().y, 64.0 + expected);
        }
        let target = BlockPos::new(1, PLOT_BLOCK_HEIGHT - 2, 1);
        let read = |p| {
            Some(if p == target {
                Block::Stone {}
            } else {
                Block::Air
            })
        };
        assert!(landing_position(target, &read).is_none());
    }

    #[test]
    fn misses_and_invalid_rotation_do_not_produce_destinations() {
        let read = |_| Some(Block::Air);
        let eye = PlayerPos::new(0.5, 64.5, 0.5);
        assert!(pointed_block(eye, 0.0, 0.0, &read).is_none());
        assert!(pointed_block(eye, f32::NAN, 0.0, &read).is_none());
        assert!(pointed_block(PlayerPos::new(f64::INFINITY, 64.0, 0.0), 0.0, 0.0, &read).is_none());
    }

    #[test]
    fn range_extends_beyond_old_limit_and_partial_blocks_do_not_hide_targets() {
        let target = BlockPos::new(0, 64, 700);
        let slab = Block::from_name("stone_slab").unwrap();
        let read = |p| {
            Some(if p == BlockPos::new(0, 64, 5) {
                slab
            } else if p == target {
                Block::Stone {}
            } else {
                Block::Air
            })
        };
        let hit = pointed_block(PlayerPos::new(0.5, 64.75, 0.5), 0.0, 0.0, &read).unwrap();
        assert_eq!(hit.block, target);
        let hit = pointed_block(PlayerPos::new(0.5, 64.25, 0.5), 0.0, 0.0, &read).unwrap();
        assert_eq!(hit.block, BlockPos::new(0, 64, 5));
    }

    #[test]
    fn obstructed_target_searches_back_along_the_visible_line() {
        // A wall blocks the aim, with a floor in front and no room on the wall.
        let read = |p: BlockPos| {
            Some(
                if (p.z == 10 && (63..=70).contains(&p.y)) || (p.y == 62 && (2..=9).contains(&p.z))
                {
                    Block::Stone {}
                } else {
                    Block::Air
                },
            )
        };
        let pos = compass_destination(PlayerPos::new(0.5, 64.5, 0.5), 0.0, 0.0, &read).unwrap();
        assert_eq!((pos.x, pos.y, pos.z), (0.5, 63.0, 9.5));
    }

    #[test]
    fn search_can_find_the_top_of_a_wall_but_does_not_tunnel_through_it() {
        let read = |p: BlockPos| {
            Some(if p.x == 0 && p.z == 10 && (63..=66).contains(&p.y) {
                Block::Stone {}
            } else {
                Block::Air
            })
        };
        let pos = compass_destination(PlayerPos::new(0.5, 64.5, 0.5), 0.0, 0.0, &read).unwrap();
        assert_eq!((pos.x, pos.y, pos.z), (0.5, 67.0, 10.5));
        let read = |p: BlockPos| {
            Some(
                if (p.z == 10 && (60..=75).contains(&p.y)) || (p.y == 62 && p.z > 10) {
                    Block::Stone {}
                } else {
                    Block::Air
                },
            )
        };
        assert!(compass_destination(PlayerPos::new(0.5, 64.5, 0.5), 0.0, 0.0, &read).is_none());
    }

    #[test]
    fn decorations_do_not_block_clearance_and_search_rejects_hazards() {
        let target = BlockPos::new(0, 63, 10);
        let read = |p| {
            Some(if p == target {
                Block::Stone {}
            } else if p == BlockPos::new(0, 64, 10) {
                Block::from_name("redstone_wire").unwrap()
            } else {
                Block::Air
            })
        };
        assert_eq!(landing_position(target, &read).unwrap().y, 64.0);
        let read = |p: BlockPos| {
            Some(if p.y == 63 {
                Block::from_name("lava").unwrap()
            } else {
                Block::Air
            })
        };
        assert!(landing_position(target, &read).is_none());
    }

    #[test]
    fn ray_hits_tall_shapes_above_their_voxel_and_passes_through_open_trapdoor_gap() {
        let fence = BlockPos::new(0, 64, 5);
        let read = |p| {
            Some(if p == fence {
                Block::from_name("oak_fence").unwrap()
            } else {
                Block::Air
            })
        };
        assert_eq!(
            pointed_block(PlayerPos::new(0.5, 65.25, 0.5), 0.0, 0.0, &read)
                .unwrap()
                .block,
            fence
        );
        let mut trapdoor = Block::from_name("oak_trapdoor").unwrap();
        trapdoor.set_properties(HashMap::from([("open", "true"), ("facing", "east")]));
        let target = BlockPos::new(0, 64, 10);
        let read = |p| {
            Some(if p == fence {
                trapdoor
            } else if p == target {
                Block::Stone {}
            } else {
                Block::Air
            })
        };
        assert_eq!(
            pointed_block(PlayerPos::new(0.5, 64.5, 0.5), 0.0, 0.0, &read)
                .unwrap()
                .block,
            target
        );
        assert!(landing_position(fence, &read).is_none());
    }
}
