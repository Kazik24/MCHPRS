use super::Plot;
use crate::container;
use crate::player::PacketSender;
use crate::world::World;
use mchprs_blocks::block_entities::{BlockEntity, ContainerType};
use mchprs_network::packets::serverbound::SContainerClick;

impl Plot {
    pub(super) fn close_open_container(&mut self, player: usize) {
        if let Some((pos, ContainerType::Barrel)) = self.players[player].close_container() {
            let still_open = self.players.iter().any(|p| {
                p.open_container
                    .as_ref()
                    .is_some_and(|m| m.pos == pos && m.ty == ContainerType::Barrel)
            });
            if !still_open {
                container::set_barrel_open(&mut self.world, pos, false);
                self.world.flush_block_changes();
            }
        }
    }

    pub(super) fn close_all_containers(&mut self) {
        for player in 0..self.players.len() {
            self.close_open_container(player);
        }
    }

    pub(super) fn container_in_reach(&self, player: usize, pos: mchprs_blocks::BlockPos) -> bool {
        let p = self.players[player].pos;
        let squared = (p.x - pos.x as f64 - 0.5).powi(2)
            + (p.y - pos.y as f64 - 0.5).powi(2)
            + (p.z - pos.z as f64 - 0.5).powi(2);
        squared.is_finite() && squared <= 64.0
    }

    fn container_valid(&self, player: usize) -> bool {
        let Some(menu) = &self.players[player].open_container else {
            return false;
        };
        self.container_in_reach(player, menu.pos)
            && self.world.get_block(menu.pos).get_name()
                == menu.ty.to_string().trim_start_matches("minecraft:")
            && matches!(self.world.get_block_entity(menu.pos), Some(BlockEntity::Container { ty, .. }) if *ty == menu.ty)
    }

    pub(super) fn update_open_containers(&mut self) {
        for player in 0..self.players.len() {
            if self.players[player].open_container.is_none() {
                continue;
            }
            if !self.container_valid(player) {
                self.close_open_container(player);
                continue;
            }
            let menu = self.players[player].open_container.as_ref().unwrap();
            let pos = menu.pos;
            if menu.ty == ContainerType::Barrel {
                container::set_barrel_open(&mut self.world, pos, true);
            }
            if let Some(BlockEntity::Container { inventory, .. }) = self.world.get_block_entity(pos)
            {
                self.players[player].send_container_contents(inventory, false);
            }
        }
    }

    pub(super) fn click_open_container(&mut self, packet: SContainerClick, player_idx: usize) {
        let Some(menu) = &self.players[player_idx].open_container else {
            return;
        };
        // A delayed click from a previous screen must never affect a new container.
        if menu.window_id as i32 != packet.window_id {
            return;
        }
        if !self.container_valid(player_idx) {
            self.close_open_container(player_idx);
            return;
        }
        let pos = menu.pos;
        if !self.players[player_idx].can_build_action("container", self.owner) {
            self.players[player_idx].send_no_permission_message();
            self.close_open_container(player_idx);
            return;
        }
        if self.redpiler.is_active() {
            if self
                .redpiler
                .current_flags()
                .is_some_and(|flags| flags.io_only)
            {
                self.players[player_idx].send_error_message(super::packet_handlers::ERROR_IO_ONLY);
                self.close_open_container(player_idx);
                return;
            }
            self.reset_redpiler();
        }
        let Some(BlockEntity::Container { inventory, ty, .. }) = self.world.get_block_entity(pos)
        else {
            return;
        };
        let size = ty.num_slots() as usize;
        let mut slots = container::inventory_slots(inventory, size);
        let before = container::signature(&slots, &None);
        let player = &mut self.players[player_idx];
        slots.extend_from_slice(&player.inventory[9..45]);
        let creative = matches!(player.gamemode, crate::player::Gamemode::Creative);
        let menu = player.open_container.as_mut().unwrap();
        // Stale state IDs still carry gestures. Compute from server state and resync.
        let _predicted_state = packet.state_id;
        container::click(
            menu,
            &mut slots,
            &mut player.inventory[45],
            packet.slot,
            packet.button,
            packet.mode,
            creative,
        );
        player.inventory[9..45].clone_from_slice(&slots[size..]);
        player.set_inventory_slot(45, player.inventory[45].clone());
        let strength = container::comparator_strength(&slots[..size]);
        let entries = container::inventory_entries(&slots[..size]);
        if let Some(BlockEntity::Container {
            inventory,
            comparator_override,
            ..
        }) = self.world.get_block_entity_mut(pos)
        {
            *inventory = entries.clone().into();
            *comparator_override = strength;
        }
        if container::signature(&slots[..size], &None) != before {
            // Comparator input can also be read through an adjacent solid block.
            crate::redstone::update_surrounding_blocks(&mut self.world, pos);
            for face in mchprs_blocks::BlockFace::values() {
                let adjacent = pos.offset(face);
                if self.world.get_block(adjacent).is_solid() {
                    crate::redstone::update_surrounding_blocks(&mut self.world, adjacent);
                }
            }
        }
        player.send_container_contents(&entries, true);
        self.update_open_containers();
    }
}
