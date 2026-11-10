use super::{Plot, PlotWorld, compass};
use crate::player::{Gamemode, PacketSender, PlayerPos, allocate_entity_id};
use crate::world::World;
use mchprs_blocks::items::Item;
use mchprs_network::generated::SNOWBALL_ENTITY;
use mchprs_network::packets::PacketEncoder;
use mchprs_network::packets::clientbound::{CDestroyEntities, CEntityPositionSync, CEntityStatus, CSpawnEntity, ClientBoundPacket};
use std::time::{Duration, Instant};

const STEP: Duration = Duration::from_millis(50);
const LIFETIME: Duration = Duration::from_secs(5);
const THROW_COOLDOWN: Duration = Duration::from_millis(200);
const MAX_ACTIVE: usize = 64;

pub(super) struct Snowball {
    pub(super) entity_id: i32,
    uuid: u128,
    owner_id: i32,
    pos: PlayerPos,
    velocity: [f64; 3],
    next_step: Instant,
    expires: Instant,
}

impl Snowball {
    pub(super) fn spawn_packet(&self) -> PacketEncoder {
        let [velocity_x, velocity_y, velocity_z] = self.velocity.map(|v| (v * 8000.0) as i16);
        CSpawnEntity {
            entity_id: self.entity_id,
            object_uuid: self.uuid,
            entity_type: SNOWBALL_ENTITY,
            x: self.pos.x,
            y: self.pos.y,
            z: self.pos.z,
            pitch: 0.0,
            yaw: 0.0,
            data: self.owner_id,
            velocity_x,
            velocity_y,
            velocity_z,
        }
        .encode()
    }

    fn step(&mut self, world: &PlotWorld) -> bool {
        let [x, y, z] = self.velocity;
        let distance = x.hypot(y).hypot(z);
        let direction = self.velocity.map(|component| component / distance);
        // shortcut: reuse the compass's approximate block shapes until exact collision shapes are available.
        if let Some(hit) = compass::trace_direction(
            self.pos,
            direction,
            distance,
            &|pos| Plot::in_plot_bounds(world.x, world.z, pos.x, pos.z).then(|| world.get_block(pos)),
            &|block| compass::collision_bounds(block).filter(|_| !matches!(block.get_name(), "water" | "lava")),
        ) {
            self.pos = hit.position();
            self.velocity = [0.0; 3];
            return true;
        }

        self.pos = PlayerPos::new(self.pos.x + x, self.pos.y + y, self.pos.z + z);
        self.velocity = [x * 0.99, y * 0.99 - 0.03, z * 0.99];
        false
    }
}

impl Plot {
    pub(super) fn throw_snowball(&mut self, player_idx: usize, hand: i32, yaw: f32, pitch: f32) {
        let player = &mut self.players[player_idx];
        let slot = match hand {
            0 => 36 + player.selected_slot as usize,
            1 => 45,
            _ => return,
        };
        let Some(mut stack) = player
            .inventory
            .get(slot)
            .and_then(Option::as_ref)
            .filter(|item| item.item_type == Item::Snowball && item.count > 0)
            .cloned()
        else {
            return;
        };

        let now = Instant::now();
        let mut pos = player.eye_position();
        pos.y -= 0.1 * player.scale();
        if player.awaiting_teleport()
            || player.gamemode == Gamemode::Spectator
            || !yaw.is_finite()
            || !pitch.is_finite()
            || !pos.is_valid()
            || pos.y < 0.0
            || pos.plot_pos() != (self.world.x, self.world.z)
            || player.last_snowball_throw.is_some_and(|last| now - last < THROW_COOLDOWN)
            || self.snowballs.len() >= MAX_ACTIVE
        {
            if player.gamemode != Gamemode::Creative {
                player.set_inventory_slot(slot as u32, Some(stack));
            }
            return;
        }

        let equipment = if player.gamemode != Gamemode::Creative {
            stack.count -= 1;
            player.set_inventory_slot(slot as u32, (stack.count != 0).then_some(stack));
            Some(player.entity_equipment_packet())
        } else {
            None
        };

        player.last_snowball_throw = Some(now);
        let snowball = Snowball {
            entity_id: allocate_entity_id() as i32,
            uuid: rand::random(),
            owner_id: player.entity_id as i32,
            pos,
            velocity: compass::look_direction(yaw, pitch.clamp(-90.0, 90.0)).map(|component| component * 1.5),
            next_step: now + STEP,
            expires: now + LIFETIME,
        };
        let packet = snowball.spawn_packet();
        for viewer in &self.players {
            viewer.send_packet(&packet);
        }
        if let Some(equipment) = equipment {
            self.broadcast_player_packets(player_idx, &[&equipment]);
        }
        self.snowballs.push(snowball);
        crate::sound::play(&mut self.world, pos.block_pos(), "entity.snowball.throw", 0.5, 1.0, None);
    }

    pub(super) fn update_snowballs(&mut self) {
        let Self {
            snowballs, players, world, ..
        } = self;
        if snowballs.is_empty() {
            return;
        }
        if players.is_empty() {
            snowballs.clear();
            return;
        }

        let now = Instant::now();
        snowballs.retain_mut(|snowball| {
            let mut remove = now >= snowball.expires;
            let mut hit = false;
            let mut moved = false;
            while !remove && now >= snowball.next_step {
                hit = snowball.step(world);
                snowball.next_step += STEP;
                moved = true;
                let pos = snowball.pos;
                remove = hit || !pos.is_valid() || pos.y < 0.0 || pos.plot_pos() != (world.x, world.z);
            }
            if moved && (!remove || hit) {
                let pos = snowball.pos;
                let packet = CEntityPositionSync {
                    entity_id: snowball.entity_id,
                    x: pos.x,
                    y: pos.y,
                    z: pos.z,
                    yaw: 0.0,
                    pitch: 0.0,
                    on_ground: false,
                }
                .encode_with_velocity(snowball.velocity);
                for viewer in players.iter() {
                    viewer.send_packet(&packet);
                }
            }
            if hit {
                let packet = CEntityStatus {
                    entity_id: snowball.entity_id,
                    entity_status: 3,
                }
                .encode();
                for viewer in players.iter() {
                    viewer.send_packet(&packet);
                }
            }
            if remove {
                let packet = CDestroyEntities {
                    entity_ids: vec![snowball.entity_id],
                }
                .encode();
                for viewer in players.iter() {
                    viewer.send_packet(&packet);
                }
            }
            !remove
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plot::{benchmark, client_sync_tests, client_test_utils::read_ack};
    use mchprs_blocks::blocks::Block;
    use mchprs_blocks::items::ItemStack;
    use mchprs_blocks::{BlockFace, BlockPos};
    use mchprs_network::PlayerPacketSender;
    use mchprs_network::packets::PacketDecoderExt;
    use mchprs_network::packets::serverbound::{SPlayerBlockPlacemnt, SUseItem, ServerBoundPacketHandler};
    use mchprs_network::test_support::{connection, read_frame};

    #[test]
    fn snowball_cooldown_preserves_the_stack_on_rejected_throws() {
        let (mut plot, _peer) = client_sync_tests::fixture(false);
        let player = &mut plot.players[0];
        player.gamemode = Gamemode::Survival;
        player.inventory[36] = Some(ItemStack {
            item_type: Item::Snowball,
            count: 2,
            nbt: None,
        });

        plot.throw_snowball(0, 0, 0.0, 0.0);
        plot.throw_snowball(0, 0, 0.0, 0.0);
        assert_eq!(plot.snowballs.len(), 1);
        assert_eq!(plot.players[0].inventory[36].as_ref().unwrap().count, 1);

        plot.players[0].last_snowball_throw = Some(Instant::now() - THROW_COOLDOWN);
        plot.throw_snowball(0, 0, 0.0, 0.0);
        assert_eq!(plot.snowballs.len(), 2);
        assert!(plot.players[0].inventory[36].is_none());
    }

    #[test]
    fn snowball_throws_once_and_hits_a_wall_with_redstone_paused() {
        let (mut plot, _) = client_sync_tests::fixture(false);
        let conn = connection(false).unwrap();
        let _incoming = conn.incoming;
        let mut peer = conn.peer;
        let mut player = benchmark::player(conn.player);
        player.gamemode = Gamemode::Survival;
        player.set_test_permissions(&["*"]);
        player.inventory[45] = Some(ItemStack {
            item_type: Item::Snowball,
            count: 1,
            nbt: None,
        });
        plot.players[0] = player;
        plot.world.packet_senders[0] = PlayerPacketSender::new(&plot.players[0].client);

        let wall = BlockPos::new(32, 22, 38);
        plot.world.set_block(wall, Block::Stone {});
        plot.handle_player_block_placement(
            SPlayerBlockPlacemnt {
                hand: 1,
                pos: wall.packed(),
                face: BlockFace::North.get_id() as i32,
                cursor_x: 0.5,
                cursor_y: 0.52,
                cursor_z: 0.0,
                inside_block: false,
                sequence: 10,
            },
            0,
        );
        assert!(plot.snowballs.is_empty());
        loop {
            let (id, mut frame) = read_frame(&mut peer, false).unwrap();
            if id == 0x04 {
                assert_eq!(frame.read_varint().unwrap(), 10);
                break;
            }
        }

        plot.handle_use_item(
            SUseItem {
                hand: 1,
                sequence: 11,
                yaw: 0.0,
                pitch: 0.0,
            },
            0,
        );
        assert!(plot.players[0].inventory[45].is_none());
        let (id, mut frame) = read_frame(&mut peer, false).unwrap();
        assert_eq!(id, 0x14);
        assert_eq!(frame.read_varint().unwrap(), 0);
        assert_eq!(frame.read_varint().unwrap(), 0);
        assert_eq!(frame.read_short().unwrap(), 45);
        assert_eq!(frame.read_varint().unwrap(), 0);

        let [snowball] = &mut plot.snowballs[..] else {
            panic!("expected one snowball")
        };
        let entity_id = snowball.entity_id;
        let (id, mut frame) = read_frame(&mut peer, false).unwrap();
        assert_eq!(id, 0x01);
        assert_eq!(frame.read_varint().unwrap(), entity_id);
        assert_eq!(frame.read_uuid().unwrap(), snowball.uuid);
        assert_eq!(frame.read_varint().unwrap(), 114);
        read_ack(&mut peer, false, 11);

        snowball.next_step = Instant::now();
        plot.update();
        assert_eq!(plot.snowballs[0].pos.z, 37.0);
        plot.snowballs[0].next_step = Instant::now();
        plot.update();
        assert!(plot.snowballs.is_empty());
        assert_eq!(plot.world.get_block(wall), Block::Stone {});
        let mut impact = false;
        loop {
            let (id, mut frame) = read_frame(&mut peer, false).unwrap();
            match id {
                0x1e => {
                    assert_eq!(frame.read_int().unwrap(), entity_id);
                    assert_eq!(frame.read_byte().unwrap(), 3);
                    impact = true;
                }
                0x46 => {
                    assert!(impact);
                    assert_eq!(frame.read_varint().unwrap(), 1);
                    assert_eq!(frame.read_varint().unwrap(), entity_id);
                    break;
                }
                _ => {}
            }
        }
    }
}
