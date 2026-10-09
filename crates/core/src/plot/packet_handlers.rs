use super::Plot;
use crate::config::CONFIG;
use crate::interaction::{self, UseOnBlockContext};
use crate::messages;
use crate::player::{PacketSender, PlayerPos, SkinParts};
use crate::server::Message;
use crate::utils::HyphenatedUUID;
use crate::world::World;
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::Block;
use mchprs_blocks::items::{Item, ItemStack};
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_network::packets::clientbound::*;
use mchprs_network::packets::serverbound::*;
use mchprs_network::packets::PacketEncoder;
use serde_json::json;
use std::path::PathBuf;
use std::time::Instant;
use tracing::{error, warn};

pub(super) const ERROR_IO_ONLY: &str = messages::PLOT_CANNOT_INTERACTED_WHILE_REDPILER_ACTIVE;

fn relative_movement(old: PlayerPos, new: PlayerPos) -> Option<[i16; 3]> {
    // Quantize endpoints rather than each step, so discarded fractions do not accumulate.
    let encode = |coordinate: f64| (coordinate * 4096.0 + 0.5).floor();
    let deltas = [
        encode(new.x) - encode(old.x),
        encode(new.y) - encode(old.y),
        encode(new.z) - encode(old.z),
    ];
    // The positive eight-block boundary exceeds i16::MAX; use a teleport there.
    deltas
        .iter()
        .all(|delta| (-32768.0..32768.0).contains(delta))
        .then(|| deltas.map(|delta| delta as i16))
}

impl Plot {
    pub(super) fn broadcast_player_packets(&self, player: usize, packets: &[&PacketEncoder]) {
        for (index, viewer) in self.players.iter().enumerate() {
            if index != player {
                for packet in packets {
                    viewer.send_packet(packet);
                }
            }
        }
    }

    fn update_player_position(
        &mut self,
        player: usize,
        new: PlayerPos,
        on_ground: bool,
        rotation: Option<(f32, f32)>,
    ) {
        if self.players[player].awaiting_teleport() {
            return;
        }
        if !new.is_valid()
            || rotation.is_some_and(|(yaw, pitch)| !yaw.is_finite() || !pitch.is_finite())
        {
            warn!(
                player = %self.players[player].username,
                position = ?new,
                ?rotation,
                "Closing client: invalid movement"
            );
            self.players[player].client.close_connection();
            return;
        }
        let moving = &mut self.players[player];
        let Some(old) = moving.accept_position(new, on_ground, rotation) else {
            return;
        };
        let packet = match relative_movement(old, new) {
            None => moving.entity_teleport_packet(),
            Some([delta_x, delta_y, delta_z]) if rotation.is_some() => CEntityPositionAndRotation {
                delta_x,
                delta_y,
                delta_z,
                yaw: moving.yaw,
                pitch: moving.pitch,
                entity_id: moving.entity_id as i32,
                on_ground,
            }
            .encode(),
            Some([delta_x, delta_y, delta_z]) => CEntityPosition {
                delta_x,
                delta_y,
                delta_z,
                entity_id: moving.entity_id as i32,
                on_ground,
            }
            .encode(),
        };
        if rotation.is_some() {
            let head = CEntityHeadLook {
                entity_id: moving.entity_id as i32,
                yaw: moving.yaw,
            }
            .encode();
            self.broadcast_player_packets(player, &[&packet, &head]);
        } else {
            self.broadcast_player_packets(player, &[&packet]);
        }
        self.on_player_move(player, old, new);
    }

    /// Send the authoritative state before the prediction acknowledgement,
    /// including no-op, rejected, abort and finish actions.
    fn correct_predicted_block(&mut self, player: usize, pos: BlockPos) {
        if !(0..super::PLOT_BLOCK_HEIGHT).contains(&pos.y) {
            return;
        }
        if !Plot::in_plot_bounds(self.world.x, self.world.z, pos.x, pos.z) {
            self.restore_neighbor_chunk(player, pos);
            return;
        }
        let state = if self.world.fast_rendering || self.world.screen_only() {
            self.world.screen_state(pos)
        } else {
            self.world.get_block_raw(pos)
        };
        self.players[player].client.send_packet(
            &CBlockChange {
                pos: pos.packed(),
                block_id: state as i32,
            }
            .encode(),
        );
    }

    fn place_block(&mut self, player_block_placement: SPlayerBlockPlacemnt, player: usize) {
        if self.players[player].awaiting_teleport() {
            return;
        }
        let (yaw, pitch) = (self.players[player].yaw, self.players[player].pitch);
        if self.use_wire_tool(player, player_block_placement.hand, yaw, pitch) {
            return;
        }
        if self.sword_git(player, player_block_placement.hand, yaw, pitch) {
            return;
        }
        if self.use_compass(player, player_block_placement.hand, yaw, pitch) {
            return;
        }
        let block_pos = BlockPos::from_packed(player_block_placement.pos);
        if !Plot::in_plot_bounds(self.world.x, self.world.z, block_pos.x, block_pos.z) {
            self.restore_neighbor_chunk(player, block_pos);
            if let Some(face) = BlockFace::try_from_id(player_block_placement.face as u32) {
                self.restore_neighbor_chunk(player, block_pos.offset(face));
            }
            return;
        }
        if !(0..super::PLOT_BLOCK_HEIGHT).contains(&block_pos.y)
            || !self.container_in_reach(player, block_pos)
            || !(0..=1).contains(&player_block_placement.hand)
            || [
                player_block_placement.cursor_x,
                player_block_placement.cursor_y,
                player_block_placement.cursor_z,
            ]
            .iter()
            // Outline shapes can extend past the voxel; also allow f32 face rounding.
            .any(|n| !n.is_finite() || !(-0.500001..=1.500001).contains(n))
        {
            return;
        }
        let Some(block_face) = BlockFace::try_from_id(player_block_placement.face as u32) else {
            warn!("Invalid block face: {}", player_block_placement.face);
            return;
        };

        let offset_pos = block_pos.offset(block_face);
        if !Plot::in_plot_bounds(self.world.x, self.world.z, offset_pos.x, offset_pos.z) {
            self.restore_neighbor_chunk(player, offset_pos);
        }

        let cancel = |plot: &mut Plot| {
            plot.send_block_change(block_pos, plot.world.get_block_raw(block_pos));

            let offset_pos = block_pos.offset(block_face);
            plot.send_block_change(offset_pos, plot.world.get_block_raw(offset_pos));
        };

        if self.git_checkout_locked() {
            cancel(self);
            return;
        }

        let selected_slot = self.players[player].selected_slot as usize;
        let item_in_hand = if player_block_placement.hand == 0 {
            // Slot in hotbar
            self.players[player].inventory[selected_slot + 36].clone()
        } else {
            // Slot for left hand
            self.players[player].inventory[45].clone()
        };

        if let Some(item) = &item_in_hand {
            let has_permission = self.players[player].has_permission("worldedit.selection.pos");
            if item.item_type == (Item::WEWand {}) && has_permission {
                let same = self.players[player].second_position == Some(block_pos);
                if !same {
                    self.players[player].worldedit_set_second_position(block_pos);
                }
                cancel(self);
                // FIXME: Because the client sends another packet after this for the left hand for most blocks,
                // redpiler will get reset anyways.
                return;
            }
        }

        if !self.players[player].can_edit_plot(self.owner, (self.world.x, self.world.z)) {
            self.players[player].send_no_permission_message();
            cancel(self);
            return;
        }

        if self.redpiler.is_active() {
            let block = self.world.get_block(block_pos);
            let lever_or_button = matches!(block, Block::Lever { .. } | Block::StoneButton { .. });
            if lever_or_button && !self.players[player].crouching {
                if !self.players[player].can_build_action(
                    "interact",
                    self.owner,
                    (self.world.x, self.world.z),
                ) {
                    self.players[player].send_no_permission_message();
                    cancel(self);
                    return;
                }
                self.redpiler.on_use_block(block_pos);
                self.redpiler.flush(&mut self.world);
                crate::sound::control_used(
                    &mut self.world,
                    block_pos,
                    block,
                    self.players[player].uuid,
                );
                self.world.flush_block_changes();
                return;
            } else {
                match self.redpiler.current_flags() {
                    Some(flags) if flags.io_only => {
                        self.players[player].send_error_message(ERROR_IO_ONLY);
                        cancel(self);
                        return;
                    }
                    _ => {}
                }
                self.reset_redpiler();
            }
        }

        self.close_open_container(player);

        if let Some(item) = item_in_hand {
            let result = interaction::use_item_on_block(
                &item,
                &mut self.world,
                UseOnBlockContext {
                    block_face,
                    block_pos,
                    player: &mut self.players[player],
                    cursor_y: player_block_placement.cursor_y,
                },
            );
            match result {
                interaction::ItemUseResult::Cancelled => cancel(self),
                interaction::ItemUseResult::Placed(pos) => self.mirror_auto_stack(player, pos),
                interaction::ItemUseResult::Used => {}
            }
            self.world.flush_block_changes();
            return;
        }

        let block = self.world.get_block(block_pos);
        if !self.players[player].crouching {
            interaction::on_use(
                block,
                &mut self.world,
                &mut self.players[player],
                block_pos,
                None,
            );
            self.world.flush_block_changes();
        }
    }

    fn dig_block(&mut self, player_digging: SPlayerDigging, player: usize) {
        if self.players[player].awaiting_teleport() {
            return;
        }
        if self.wire_held(player) && matches!(player_digging.status, 0..=4 | 6) {
            if player_digging.status == 0 {
                self.start_wire_route(player, BlockPos::from_packed(player_digging.pos));
            } else if matches!(player_digging.status, 3 | 4 | 6) {
                self.flip_wire_route(player, player_digging.status != 6);
                if player_digging.status != 6 {
                    // Restore the pen after the client's predicted item drop.
                    let slot = self.players[player].selected_slot + 36;
                    let item = self.players[player].inventory[slot as usize].clone();
                    self.players[player].set_inventory_slot(slot, item);
                }
            }
            return;
        }
        if self.git_checkout_locked() {
            let pos = BlockPos::from_packed(player_digging.pos);
            if Plot::in_plot_bounds(self.world.x, self.world.z, pos.x, pos.z)
                && (0..super::PLOT_BLOCK_HEIGHT).contains(&pos.y)
            {
                self.send_block_change(pos, self.world.get_block_raw(pos));
            }
            return;
        }
        if player_digging.status == 0 {
            let block_pos = BlockPos::from_packed(player_digging.pos);
            if !Plot::in_plot_bounds(self.world.x, self.world.z, block_pos.x, block_pos.z) {
                self.restore_neighbor_chunk(player, block_pos);
                return;
            }
            if !(0..super::PLOT_BLOCK_HEIGHT).contains(&block_pos.y)
                || !self.container_in_reach(player, block_pos)
            {
                return;
            }
            let block = self.world.get_block(block_pos);

            // This worldedit wand stuff should probably be done in another file. It's good enough for now.
            let item_in_hand = self.players[player].inventory
                [self.players[player].selected_slot as usize + 36]
                .clone();
            if let Some(item) = item_in_hand {
                if item.item_type.get_name().ends_with("_sword") {
                    return;
                }
                let has_permission = self.players[player].has_permission("worldedit.selection.pos");
                if item.item_type == (Item::WEWand {}) && has_permission {
                    self.send_block_change(block_pos, block.get_id());
                    if let Some(pos) = self.players[player].first_position {
                        if pos == block_pos {
                            return;
                        }
                    }
                    self.players[player].worldedit_set_first_position(block_pos);
                    return;
                }
            }

            if !self.players[player].can_build_action(
                "break",
                self.owner,
                (self.world.x, self.world.z),
            ) {
                self.players[player].send_no_permission_message();
                self.send_block_change(block_pos, block.get_id());
                return;
            }

            match self.redpiler.current_flags() {
                Some(flags) if flags.io_only => {
                    self.players[player].send_error_message(ERROR_IO_ONLY);
                    self.send_block_change(block_pos, block.get_id());
                    return;
                }
                _ => {}
            }

            self.reset_redpiler();

            interaction::destroy(block, &mut self.world, block_pos);
            if !matches!(block, Block::Air) {
                self.mirror_auto_stack(player, block_pos);
            }
            self.world.flush_block_changes();

            let effect = CEffect {
                effect_id: 2001,
                pos: player_digging.pos,
                data: block.get_id() as i32,
                disable_relative_volume: false,
            }
            .encode();
            for other_player in 0..self.players.len() {
                if player == other_player {
                    continue;
                };
                self.players[other_player].client.send_packet(&effect);
            }
        } else {
            let selected_slot = self.players[player].selected_slot as usize + 36;
            if player_digging.status == 3 {
                self.players[player].inventory[selected_slot] = None;
            } else if player_digging.status == 4 {
                let mut stack_empty = false;
                if let Some(item_stack) = &mut self.players[player].inventory[selected_slot] {
                    item_stack.count = item_stack.count.saturating_sub(1);
                    stack_empty = item_stack.count == 0;
                }
                if stack_empty {
                    self.players[player].inventory[selected_slot] = None;
                }
            }
        }
    }

    fn complete_warps(&self, player: usize, id: i32, text: &str) -> Option<CTabComplete> {
        let (command, prefix) = text.split_once(' ')?;
        let permission = match command {
            "/warp" => "commands.warp",
            "/setwarp" => "commands.setwarp",
            _ => return None,
        };
        let mut response = CTabComplete {
            id,
            start: (command.encode_utf16().count() + 1) as i32,
            length: prefix.encode_utf16().count() as i32,
            matches: Vec::new(),
        };
        if (crate::permissions::dedicated_permissions()
            && !self.players[player].has_permission(permission))
            || prefix.chars().any(char::is_whitespace)
        {
            return Some(response);
        }
        match super::database::warp_names() {
            Ok(names) => {
                let prefix = prefix.to_ascii_lowercase();
                response.matches = names
                    .into_iter()
                    .filter(|name| name.to_ascii_lowercase().starts_with(&prefix))
                    .take(100)
                    .map(|name| CTabCompleteMatch {
                        match_: name,
                        tooltip: None,
                    })
                    .collect();
            }
            Err(error) => error!("Could not complete warp names: {error}"),
        }
        Some(response)
    }

    fn complete_plot_members(&self, player: usize, id: i32, text: &str) -> Option<CTabComplete> {
        let (command, tail) = text.split_once(' ')?;
        if !matches!(command, "/p" | "/plot") {
            return None;
        }
        let (action, prefix) = tail.split_once(' ')?;
        if !matches!(action, "add" | "remove") {
            return None;
        }
        let mut response = CTabComplete {
            id,
            start: (text.len() - prefix.len()) as i32,
            length: prefix.encode_utf16().count() as i32,
            matches: Vec::new(),
        };
        let actor = &self.players[player];
        if !actor.has_permission("plots.claim")
            || (self.owner != Some(actor.uuid)
                && !actor.has_permission("plots.admin.interact.other"))
            || prefix.chars().any(char::is_whitespace)
        {
            return Some(response);
        }
        let names = if action == "add" {
            super::database::known_usernames()
        } else {
            super::database::plot_member_names(self.world.x, self.world.z)
        };
        match names {
            Ok(names) => {
                let prefix = prefix.to_lowercase();
                response.matches = names
                    .into_iter()
                    .filter(|name| {
                        name.to_lowercase().starts_with(&prefix) && name != &actor.username
                    })
                    .take(100)
                    .map(|name| CTabCompleteMatch {
                        match_: name,
                        tooltip: None,
                    })
                    .collect();
            }
            Err(error) => error!("Could not complete plot member names: {error}"),
        }
        Some(response)
    }

    pub(super) fn handle_packets_for_player(&mut self, player: usize) {
        self.players[player].refresh_permissions();
        let packets = self.players[player].client.receive_packets();
        for packet in packets {
            packet.handle(self, player);
        }
    }
}

impl ServerBoundPacketHandler for Plot {
    fn handle_teleport_confirm(&mut self, packet: STeleportConfirm, player: usize) {
        if self.players[player].confirm_teleport(packet.id) {
            let position = self.players[player].entity_teleport_packet();
            self.broadcast_player_packets(player, &[&position]);
            // Establish the destination view before later packets in this batch
            // can place/break blocks there. Confirmation does not force reloads.
            self.update_view_pos_for_player(player, false);
        }
    }

    fn handle_update_command_block(&mut self, packet: SUpdateCommandBlock, player: usize) {
        if self.git_checkout_locked() {
            return;
        }
        let pos = BlockPos::from_packed(packet.pos);
        let data = &mut self.players[player];
        if !matches!(data.gamemode, crate::player::Gamemode::Creative)
            || !data.has_permission("commands.commandblock.edit")
            || !Plot::in_plot_bounds(self.world.x, self.world.z, pos.x, pos.z)
            || !(0..super::PLOT_BLOCK_HEIGHT).contains(&pos.y)
            || !super::picking::within_reach(data.eye_position(), pos)
        {
            return;
        }
        if !data.can_build_action("commandblock", self.owner, (self.world.x, self.world.z)) {
            data.send_no_permission_message();
            return;
        }
        let old = self.world.get_block(pos);
        if !old.is_command_block() {
            return;
        }
        self.reset_redpiler();
        let name = match packet.mode {
            0 => "chain_command_block",
            1 => "repeating_command_block",
            _ => "command_block",
        };
        let mut block = Block::from_name(name).unwrap();
        block.set_properties(std::collections::HashMap::from([
            ("facing", old.property("facing").unwrap_or("north")),
            (
                "conditional",
                if packet.flags & 2 != 0 {
                    "true"
                } else {
                    "false"
                },
            ),
        ]));
        let mut entity = match self.world.get_block_entity(pos) {
            Some(BlockEntity::CommandBlock(entity)) => (**entity).clone(),
            _ => Default::default(),
        };
        entity.command = packet.command;
        entity.track_output = packet.flags & 1 != 0;
        entity.automatic = packet.flags & 4 != 0;
        entity.success_count = 0;
        entity.last_output = None;
        entity.last_execution = -1;
        self.world.set_block(pos, block);
        self.world.flush_block_changes();
        self.world
            .set_block_entity(pos, BlockEntity::CommandBlock(Box::new(entity)));
        crate::redstone::command_block::update(&mut self.world, pos);
        self.players[player].send_system_message(messages::COMMAND_BLOCK_UPDATED);
    }

    fn handle_pick_item_from_block(&mut self, packet: SPickItemFromBlock, player: usize) {
        if crate::permissions::dedicated_permissions()
            && !self.players[player].has_permission("mchprs.inventory.creative")
        {
            return;
        }
        if !matches!(
            self.players[player].gamemode,
            crate::player::Gamemode::Creative
        ) {
            return;
        }
        let pos = BlockPos::from_packed(packet.pos);
        if !Plot::in_plot_bounds(self.world.x, self.world.z, pos.x, pos.z)
            || !(0..super::PLOT_BLOCK_HEIGHT).contains(&pos.y)
            || !super::picking::within_reach(self.players[player].eye_position(), pos)
        {
            return;
        }
        let Some(picked) = super::picking::picked_stack(&self.world, pos, packet.include_data)
        else {
            return;
        };
        let player_data = &mut self.players[player];
        let changed = super::picking::pick_into_inventory(
            &mut player_data.inventory,
            &mut player_data.selected_slot,
            picked,
        );
        for slot in changed {
            let item = player_data.inventory[slot].clone();
            player_data.set_inventory_slot(slot as u32, item);
        }
        let slot = player_data.selected_slot as i16;
        player_data.send_packet(&CHeldItemChange { slot: slot as i8 }.encode());
        self.handle_held_item_change(SHeldItemChange { slot }, player);
    }

    fn handle_tab_complete(&mut self, packet: STabComplete, player_idx: usize) {
        if !self.players[player_idx].can_use_commands() {
            return;
        }
        if packet.text.starts_with("/tp ") || packet.text.starts_with("/teleport ") {
            let _ = self.message_sender.send(Message::CompletePlayerNames(
                mchprs_network::PlayerPacketSender::new(&self.players[player_idx].client),
                packet.transaction_id,
                packet.text,
            ));
            return;
        }
        if let Some(completion) =
            self.complete_warps(player_idx, packet.transaction_id, &packet.text)
        {
            self.players[player_idx].send_packet(&completion.encode());
            return;
        }
        if let Some(completion) =
            super::commands::complete_redpiler(packet.transaction_id, &packet.text)
        {
            self.players[player_idx].send_packet(&completion.encode());
            return;
        }
        if let Some(completion) = self.complete_git(player_idx, packet.transaction_id, &packet.text)
        {
            self.players[player_idx].send_packet(&completion.encode());
            return;
        }
        if let Some(completion) =
            self.complete_plot_members(player_idx, packet.transaction_id, &packet.text)
        {
            self.players[player_idx].send_packet(&completion.encode());
            return;
        }
        if let Some(completion) =
            self.complete_redstone_tools(player_idx, packet.transaction_id, &packet.text)
        {
            self.players[player_idx].send_packet(&completion.encode());
            return;
        }
        if !packet.text.starts_with("//load ")
            || !self.players[player_idx].has_permission("worldedit.clipboard.load")
            || !self.players[player_idx].can_edit_plot(self.owner, (self.world.x, self.world.z))
        {
            return;
        }

        let mut path = PathBuf::from("./schems");
        if CONFIG.schemati {
            let uuid = self.players[player_idx].uuid;
            path.push(HyphenatedUUID(uuid).to_string());
        }

        let current = &packet.text[7..];
        let mut res = CTabComplete {
            id: packet.transaction_id,
            start: 7,
            length: current.encode_utf16().count() as i32,
            matches: Vec::new(),
        };

        match super::worldedit::complete_schematic_names(&path, current) {
            Ok(names) => {
                res.matches = names
                    .into_iter()
                    .map(|name| CTabCompleteMatch {
                        match_: name,
                        tooltip: None,
                    })
                    .collect()
            }
            Err(err) => error!("Error while tab completing: {err:?}"),
        }

        self.players[player_idx].send_packet(&res.encode());
    }

    fn handle_keep_alive(&mut self, _keep_alive: SKeepAlive, player_idx: usize) {
        self.players[player_idx].last_keep_alive_received = Instant::now();
    }

    fn handle_creative_inventory_action(
        &mut self,
        creative_inventory_action: SCreativeInventoryAction,
        player: usize,
    ) {
        if !matches!(
            self.players[player].gamemode,
            crate::player::Gamemode::Creative
        ) {
            return;
        }
        if !(0..46).contains(&creative_inventory_action.slot) {
            return;
        }
        if crate::permissions::dedicated_permissions()
            && !self.players[player].has_permission("mchprs.inventory.creative")
        {
            return;
        }
        if let Some(slot_data) = creative_inventory_action.clicked_item {
            if creative_inventory_action.slot < 0 || creative_inventory_action.slot >= 46 {
                return;
            }
            let item = ItemStack {
                count: slot_data.item_count as u8,
                item_type: Item::from_id(slot_data.item_id as u32),
                nbt: slot_data.nbt,
            };
            self.players[player].inventory[creative_inventory_action.slot as usize] = Some(item);
            if creative_inventory_action.slot as u32 == self.players[player].selected_slot + 36 {
                let entity_equipment = self.players[player].entity_equipment_packet();
                for other_player in 0..self.players.len() {
                    if player == other_player {
                        continue;
                    };
                    self.players[other_player]
                        .client
                        .send_packet(&entity_equipment);
                }
            }
        } else {
            self.players[player].inventory[creative_inventory_action.slot as usize] = None;
        }

        // We wanna retrieve the item slot from the inventory
        // rather than re-using the value from the client,
        // to avoid getting out of sync if the server validates
        // or sanitizes data or something
        let item = self.players[player].inventory[creative_inventory_action.slot as usize].clone();
        self.players[player].set_inventory_slot(creative_inventory_action.slot as u32, item);
    }

    fn handle_container_click(&mut self, packet: SContainerClick, player: usize) {
        if self.git_checkout_locked() {
            return;
        }
        self.click_open_container(packet, player);
    }

    fn handle_container_close(&mut self, packet: SContainerClose, player: usize) {
        if self.players[player]
            .open_container
            .as_ref()
            .is_some_and(|menu| menu.window_id as i32 == packet.window_id)
        {
            self.close_open_container(player);
        }
    }

    fn handle_player_abilities(&mut self, player_abilities: SPlayerAbilities, player: usize) {
        self.players[player].flying = player_abilities.is_flying;
    }

    fn handle_animation(&mut self, animation: SAnimation, player: usize) {
        let animation_id = match animation.hand {
            0 => 0,
            1 => 3,
            _ => 0,
        };
        let entity_animation = CEntityAnimation {
            entity_id: self.players[player].entity_id as i32,
            animation: animation_id,
        }
        .encode();
        for other_player in 0..self.players.len() {
            if player == other_player {
                continue;
            };
            self.players[other_player]
                .client
                .send_packet(&entity_animation);
        }
    }

    fn handle_use_item(&mut self, packet: SUseItem, player: usize) {
        let _acknowledgement = mchprs_network::BlockActionAcknowledgement::new(
            &self.players[player].client,
            packet.sequence,
        );
        if self.use_wire_tool(player, packet.hand, packet.yaw, packet.pitch) {
            return;
        }
        if self.sword_git(player, packet.hand, packet.yaw, packet.pitch) {
            return;
        }
        self.use_compass(player, packet.hand, packet.yaw, packet.pitch);
    }

    fn handle_player_block_placement(
        &mut self,
        player_block_placement: SPlayerBlockPlacemnt,
        player: usize,
    ) {
        let _acknowledgement = mchprs_network::BlockActionAcknowledgement::new(
            &self.players[player].client,
            player_block_placement.sequence,
        );
        let previous = self.world.set_authoritative_updates(true);
        let pos = BlockPos::from_packed(player_block_placement.pos);
        let face = BlockFace::try_from_id(player_block_placement.face as u32);
        self.place_block(player_block_placement, player);
        self.world.flush_block_changes();
        self.world.set_authoritative_updates(previous);
        self.correct_predicted_block(player, pos);
        if let Some(face) = face {
            self.correct_predicted_block(player, pos.offset(face));
        }
    }

    fn handle_chat_message(&mut self, chat_message: SChatMessage, player: usize) {
        let message = chat_message.message;
        let max_length = if message.starts_with('/') { 32767 } else { 256 };
        if message.encode_utf16().count() > max_length
            || message.chars().any(|c| c.is_control())
            || !self.players[player].accept_chat_message()
        {
            return;
        }
        if message.starts_with('/') {
            if self.players[player].command_queue.len() >= 16 {
                return;
            }
            self.players[player].command_queue.push(message);
        } else {
            let player = &mut self.players[player];
            if crate::permissions::dedicated_permissions()
                && !player.has_permission("mchprs.access.chat")
            {
                player.send_no_permission_message();
                return;
            }
            if crate::proxy_chat::enabled() {
                crate::proxy_chat::send(player, &message);
                return;
            }
            let broadcast_message =
                Message::ChatInfo(player.uuid, player.username.clone(), message);
            self.message_sender.send(broadcast_message).unwrap();
        }
    }

    fn handle_client_settings(&mut self, client_settings: SClientSettings, player: usize) {
        self.players[player].skin_parts =
            SkinParts::from_bits_truncate(client_settings.displayed_skin_parts as u32);
        for (index, viewer) in self.players.iter().enumerate() {
            viewer.send_packet(&self.players[player].entity_metadata_packet(index == player));
        }
    }

    fn handle_plugin_message(&mut self, plugin_message: SPluginMessage, player: usize) {
        if plugin_message.channel == crate::proxy_chat::CHANNEL {
            let player = &mut self.players[player];
            player
                .proxy_chat
                .receive(&plugin_message.data, &player.client);
            return;
        }
        if plugin_message.channel == "worldedit:cui" {
            self.players[player].worldedit_send_cui("s|cuboid");
        }
    }

    fn handle_player_position(&mut self, player_position: SPlayerPosition, player: usize) {
        let new = PlayerPos::new(player_position.x, player_position.y, player_position.z);
        self.update_player_position(player, new, player_position.on_ground, None);
    }

    fn handle_player_position_and_rotation(
        &mut self,
        player_position_and_rotation: SPlayerPositionAndRotation,
        player: usize,
    ) {
        let new = PlayerPos::new(
            player_position_and_rotation.x,
            player_position_and_rotation.y,
            player_position_and_rotation.z,
        );
        self.update_player_position(
            player,
            new,
            player_position_and_rotation.on_ground,
            Some((
                player_position_and_rotation.yaw,
                player_position_and_rotation.pitch,
            )),
        );
    }

    fn handle_player_rotation(&mut self, player_rotation: SPlayerRotation, player: usize) {
        if self.players[player].awaiting_teleport() {
            return;
        }
        if !player_rotation.yaw.is_finite() || !player_rotation.pitch.is_finite() {
            warn!(
                player = %self.players[player].username,
                position = ?self.players[player].pos,
                yaw = player_rotation.yaw,
                pitch = player_rotation.pitch,
                "Closing client: invalid rotation"
            );
            self.players[player].client.close_connection();
            return;
        }
        self.players[player].yaw = player_rotation.yaw;
        self.players[player].pitch = player_rotation.pitch;
        self.players[player].on_ground = player_rotation.on_ground;
        let rotation_packet = CEntityRotation {
            entity_id: self.players[player].entity_id as i32,
            yaw: player_rotation.yaw,
            pitch: player_rotation.pitch,
            on_ground: player_rotation.on_ground,
        }
        .encode();
        let entity_head_look = CEntityHeadLook {
            entity_id: self.players[player].entity_id as i32,
            yaw: player_rotation.yaw,
        }
        .encode();
        self.broadcast_player_packets(player, &[&rotation_packet, &entity_head_look]);
    }

    fn handle_player_movement(&mut self, player_movement: SPlayerMovement, player: usize) {
        if self.players[player].awaiting_teleport() {
            return;
        }
        self.players[player].on_ground = player_movement.on_ground;
    }

    fn handle_player_digging(&mut self, player_digging: SPlayerDigging, player: usize) {
        let _acknowledgement = mchprs_network::BlockActionAcknowledgement::new(
            &self.players[player].client,
            player_digging.sequence,
        );
        let previous = self.world.set_authoritative_updates(true);
        let pos = BlockPos::from_packed(player_digging.pos);
        let predicts_block = (0..=2).contains(&player_digging.status);
        self.dig_block(player_digging, player);
        self.world.flush_block_changes();
        self.world.set_authoritative_updates(previous);
        if predicts_block {
            self.correct_predicted_block(player, pos);
        }
    }

    fn handle_entity_action(&mut self, entity_action: SEntityAction, player: usize) {
        match entity_action.action_id {
            0 => self.players[player].crouching = true,
            1 => self.players[player].crouching = false,
            3 => self.players[player].sprinting = true,
            4 => self.players[player].sprinting = false,
            _ => {}
        }
        for (index, viewer) in self.players.iter().enumerate() {
            viewer.send_packet(&self.players[player].entity_metadata_packet(index == player));
        }
    }

    fn handle_held_item_change(&mut self, held_item_change: SHeldItemChange, player: usize) {
        if !(0..9).contains(&held_item_change.slot) {
            return;
        }
        if self.players[player].selected_slot != held_item_change.slot as u32 {
            self.clear_wire_tool(player);
        }
        self.players[player].selected_slot = held_item_change.slot as u32;
        let entity_equipment = self.players[player].entity_equipment_packet();
        for other_player in 0..self.players.len() {
            if player == other_player {
                continue;
            };
            self.players[other_player]
                .client
                .send_packet(&entity_equipment);
        }
    }

    fn handle_update_sign(&mut self, packet: SUpdateSign, player: usize) {
        if self.git_checkout_locked() {
            return;
        }
        let pos = BlockPos::from_packed(packet.pos);
        if !self.players[player].can_build_action("sign", self.owner, (self.world.x, self.world.z))
            || !Plot::in_plot_bounds(self.world.x, self.world.z, pos.x, pos.z)
            || !(0..super::PLOT_BLOCK_HEIGHT).contains(&pos.y)
            || !self.container_in_reach(player, pos)
        {
            return;
        }
        let Some(BlockEntity::Sign(sign)) = self.world.get_block_entity(pos) else {
            return;
        };
        if sign.waxed {
            return;
        }
        let mut sign = (**sign).clone();
        let updated = packet.lines.map(|line| json!({ "text": line }).to_string());
        if packet.front {
            sign.rows = updated;
        } else {
            sign.back_rows = updated;
        }
        let block_entity = BlockEntity::Sign(Box::new(sign));
        self.world.set_block_entity(pos, block_entity);
    }
}

#[cfg(test)]
mod movement_tests {
    use super::*;

    #[test]
    fn relative_movement_accumulates_fractional_steps_without_drift() {
        for initial in [
            PlayerPos::new(32.5, 21.0, 35.5),
            PlayerPos::new(-32.5, -21.0, -35.5),
        ] {
            let mut old = initial;
            let mut received = [0i64; 3];
            for step in 1..=2048 {
                let new = PlayerPos::new(
                    initial.x + f64::from(step) * 0.0001,
                    initial.y + f64::from(step) * 0.04153,
                    initial.z - f64::from(step) * 0.03137,
                );
                let deltas = relative_movement(old, new).unwrap();
                for (axis, distance) in [new.x - initial.x, new.y - initial.y, new.z - initial.z]
                    .into_iter()
                    .enumerate()
                {
                    received[axis] += i64::from(deltas[axis]);
                    assert!(
                        (received[axis] as f64 / 4096.0 - distance).abs() <= 1.0 / 4096.0,
                        "drift at step {step}, axis {axis}"
                    );
                }
                old = new;
            }
        }
    }

    #[test]
    fn relative_movement_uses_the_signed_protocol_range_on_every_axis() {
        let old = PlayerPos::new(16.0, 64.0, -16.0);
        assert_eq!(relative_movement(old, old), Some([0, 0, 0]));
        for axis in 0..3 {
            for (distance, expected) in [
                (8.0, None),
                (-8.0, Some(i16::MIN)),
                (32767.0 / 4096.0, Some(i16::MAX)),
                (-8.0 - 1.0 / 4096.0, None),
            ] {
                let mut coordinates = [old.x, old.y, old.z];
                coordinates[axis] += distance;
                let new = PlayerPos::new(coordinates[0], coordinates[1], coordinates[2]);
                let result = relative_movement(old, new);
                assert_eq!(result.map(|deltas| deltas[axis]), expected);
                if let Some(deltas) = result {
                    assert!(deltas
                        .iter()
                        .enumerate()
                        .all(|(i, &value)| i == axis || value == 0));
                }
            }
        }
    }
}
