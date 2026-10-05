use super::{Plot, PLOT_BLOCK_HEIGHT};
use crate::player::{PacketSender, PlayerPos};
use crate::world::World;
use mchprs_blocks::blocks::{Block, SlabType};
use mchprs_blocks::items::Item;
use mchprs_blocks::BlockPos;
use mchprs_network::packets::clientbound::{CEntityTeleport, ClientBoundPacket};
use std::time::{Duration, Instant};

const RANGE: f64 = 256.0;
const HALF_WIDTH: f64 = 0.3;

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
        let destination = pointed_block(eye, yaw, pitch, &read)
            .and_then(|target| landing_position(target, &read));
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
) -> Option<BlockPos> {
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
    loop {
        let pos = BlockPos::new(cell[0], cell[1], cell[2]);
        let block = read(pos)?;
        if !is_air(block) && !matches!(block.get_name(), "water" | "lava") {
            return Some(pos);
        }
        let distance = next.into_iter().fold(f64::INFINITY, f64::min);
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
    }
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
    let height = match name {
        "soul_sand" => 0.875,
        "farmland" | "dirt_path" => 0.9375,
        "snow" => block.property("layers")?.parse::<f64>().ok()? / 8.0,
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
    let (_, top) = vertical_bounds(read(target)?)?;
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
                if let Some((bottom, top)) = vertical_bounds(read(BlockPos::new(x, y, z))?) {
                    if f64::from(y) + top > pos.y + 1e-9 && f64::from(y) + bottom < pos.y + height {
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
                pointed_block(PlayerPos::new(0.5, 64.5, 0.5), yaw, pitch, &read),
                Some(target)
            );
        }
        let read = |pos: BlockPos| if pos.z < 4 { Some(Block::Air) } else { None };
        assert!(pointed_block(PlayerPos::new(0.5, 64.5, 0.5), 0.0, 0.0, &read).is_none());
        let read = |pos: BlockPos| {
            Some(if pos.z == 257 {
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
}
