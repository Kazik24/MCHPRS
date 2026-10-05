use super::Plot;
use crate::config::CONFIG;
use crate::interaction::{self, UseOnBlockContext};
use crate::messages;
use crate::player::{PacketSender, PlayerPos, SkinParts};
use crate::server::Message;
use crate::utils::HyphenatedUUID;
use crate::world::World;
use mchprs_blocks::block_entities::{BlockEntity, SignBlockEntity};
use mchprs_blocks::blocks::Block;
use mchprs_blocks::items::{Item, ItemStack};
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_network::packets::clientbound::*;
use mchprs_network::packets::serverbound::*;
use mchprs_network::packets::PacketEncoderExt;
use mchprs_network::packets::SlotData;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;
use tracing::{error, warn};

pub(super) const ERROR_IO_ONLY: &str = messages::PLOT_CANNOT_INTERACTED_WHILE_REDPILER_ACTIVE;

impl Plot {
    pub(super) fn handle_packets_for_player(&mut self, player: usize) {
        self.players[player].refresh_permissions();
        let packets = self.players[player].client.receive_packets();
        for packet in packets {
            packet.handle(self, player);
        }
    }
}

fn traverse_dir(
    path: &std::path::Path,
    to_complete: &str,
    matches: &mut Vec<CTabCompleteMatch>,
    base: &std::path::Path,
    max_depth: usize,
) -> anyhow::Result<()> {
    if max_depth == 0 {
        return Ok(());
    }
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            if matches.len() >= 256 { break; }
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_symlink() { continue; }
            let path = entry.path();
            if let Some(file_name) = path.file_name() {
                let file_name = file_name.to_string_lossy();

                if path
                    .to_string_lossy()
                    .starts_with(base.join(to_complete).to_string_lossy().as_ref())
                {
                    let relative = path.strip_prefix(base)?;
                    let relative = relative.to_string_lossy();
                    matches.push(CTabCompleteMatch {
                        match_: relative.to_string(),
                        tooltip: None,
                    });
                }
                if kind.is_dir() {
                    matches.push(CTabCompleteMatch {
                        match_: format!("{}/", file_name),
                        tooltip: None,
                    });
                    traverse_dir(&path, to_complete, matches, base, max_depth - 1)?;
                }
            }
        }
    }
    Ok(())
}

impl ServerBoundPacketHandler for Plot {
    fn handle_update_command_block(&mut self, packet: SUpdateCommandBlock, player: usize) {
        let pos = BlockPos::from_packed(packet.pos);
        let data = &mut self.players[player];
        if !matches!(data.gamemode, crate::player::Gamemode::Creative)
            || !data.has_permission("commands.commandblock.edit")
            || !Plot::in_plot_bounds(self.world.x, self.world.z, pos.x, pos.z)
            || !(0..super::PLOT_BLOCK_HEIGHT).contains(&pos.y)
            || !super::picking::within_reach(
                PlayerPos::new(data.pos.x, data.pos.y + 1.62, data.pos.z),
                pos,
            )
        {
            return;
        }
        if !data.can_build_action("commandblock", self.owner) {
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
        let location = self.players[player].pos;
        if !Plot::in_plot_bounds(self.world.x, self.world.z, pos.x, pos.z)
            || !(0..super::PLOT_BLOCK_HEIGHT).contains(&pos.y)
            || !super::picking::within_reach(
                PlayerPos::new(location.x, location.y + 1.62, location.z),
                pos,
            )
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
        if let Some(completion) =
            self.complete_redstone_tools(player_idx, packet.transaction_id, &packet.text)
        {
            self.players[player_idx].send_packet(&completion.encode());
            return;
        }
        if !packet.text.starts_with("//load ")
            || !self.players[player_idx].has_permission("worldedit.clipboard.load")
            || !self.players[player_idx].can_edit_plot(self.owner) {
            return;
        }

        let mut path = PathBuf::from("./schems");
        if CONFIG.schemati {
            let uuid = self.players[player_idx].uuid;
            path.push(&HyphenatedUUID(uuid).to_string());
        }

        let current = &packet.text[7..];
        let mut res = CTabComplete {
            id: packet.transaction_id,
            start: 7,
            length: current.encode_utf16().count() as i32,
            matches: Vec::new(),
        };

        if let Err(err) = traverse_dir(&path, &current, &mut res.matches, &path, 5) {
            error!("Error while tab completing: {:?}", err);
            return;
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
        if !matches!(self.players[player].gamemode, crate::player::Gamemode::Creative) {
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
                let entity_equipment = CEntityEquipment {
                    entity_id: self.players[player].entity_id as i32,
                    equipment: vec![CEntityEquipmentEquipment {
                        slot: 0, // Main hand
                        item: self.players[player].inventory
                            [creative_inventory_action.slot as usize]
                            .as_ref()
                            .map(|item| SlotData {
                                item_count: item.count as i8,
                                item_id: item.item_type.get_id() as i32,
                                nbt: item.nbt.clone(),
                            }),
                    }],
                }
                .encode();
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

    fn handle_player_block_placement(
        &mut self,
        player_block_placement: SPlayerBlockPlacemnt,
        player: usize,
    ) {
        let mut acknowledgement = Vec::new();
        acknowledgement.write_varint(player_block_placement.sequence);
        self.players[player]
            .client
            .send_packet(&mchprs_network::packets::PacketEncoder::new(
                acknowledgement,
                4,
            ));
        let block_pos = BlockPos::from_packed(player_block_placement.pos);
        if !Plot::in_plot_bounds(self.world.x, self.world.z, block_pos.x, block_pos.z)
            || !(0..super::PLOT_BLOCK_HEIGHT).contains(&block_pos.y)
            || !self.container_in_reach(player, block_pos)
            || !(0..=1).contains(&player_block_placement.hand)
            || [player_block_placement.cursor_x, player_block_placement.cursor_y, player_block_placement.cursor_z]
                .iter().any(|n| !n.is_finite() || !(0.0..=1.0).contains(n)) {
            return;
        }
        let Some(block_face) = BlockFace::try_from_id(player_block_placement.face as u32) else {
            warn!("Invalid block face: {}", player_block_placement.face);
            return;
        };

        let cancel = |plot: &mut Plot| {
            plot.send_block_change(block_pos, plot.world.get_block_raw(block_pos));

            let offset_pos = block_pos.offset(block_face);
            plot.send_block_change(offset_pos, plot.world.get_block_raw(offset_pos));
        };

        if !self.players[player].can_edit_plot(self.owner) {
            self.players[player].send_no_permission_message();
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

        if !Plot::in_plot_bounds(self.world.x, self.world.z, block_pos.x, block_pos.z) {
            self.players[player].send_system_message(messages::CAN_T_INTERACT_BLOCKS_OUTSIDE_PLOT);
            cancel(self);
            return;
        }

        if let Some(item) = &item_in_hand {
            let has_permission = self.players[player].has_permission("worldedit.selection.pos");
            if item.item_type == (Item::WEWand {}) && has_permission {
                let same = self.players[player]
                    .second_position
                    .map_or(false, |p| p == block_pos);
                if !same {
                    self.players[player].worldedit_set_second_position(block_pos);
                }
                cancel(self);
                // FIXME: Because the client sends another packet after this for the left hand for most blocks,
                // redpiler will get reset anyways.
                return;
            }
        }

        if self.redpiler.is_active() {
            let block = self.world.get_block(block_pos);
            let lever_or_button = matches!(block, Block::Lever { .. } | Block::StoneButton { .. });
            if lever_or_button && !self.players[player].crouching {
                if !self.players[player].can_build_action("interact", self.owner) {
                    self.players[player].send_no_permission_message();
                    cancel(self);
                    return;
                }
                self.redpiler.on_use_block(block_pos);
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

        if mchprs_blocks::block_entities::ContainerType::from_block(self.world.get_block(block_pos))
            .is_some()
            && !self.container_in_reach(player, block_pos)
        {
            cancel(self);
            return;
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

    fn handle_chat_message(&mut self, chat_message: SChatMessage, player: usize) {
        let message = chat_message.message;
        let max_length = if message.starts_with('/') { 32767 } else { 256 };
        if message.encode_utf16().count() > max_length
            || message.chars().any(|c| c.is_control())
            || !self.players[player].accept_chat_message() {
            return;
        }
        if message.starts_with('/') {
            if self.players[player].command_queue.len() >= 16 { return; }
            self.players[player].command_queue.push(message);
        } else {
            let player = &self.players[player];
            if crate::permissions::dedicated_permissions()
                && !player.has_permission("mchprs.access.chat")
            {
                player.send_no_permission_message();
                return;
            }
            let broadcast_message =
                Message::ChatInfo(player.uuid, player.username.clone(), message);
            self.message_sender.send(broadcast_message).unwrap();
        }
    }

    fn handle_client_settings(&mut self, client_settings: SClientSettings, player: usize) {
        let player = &mut self.players[player];
        player.skin_parts =
            SkinParts::from_bits_truncate(client_settings.displayed_skin_parts as u32);
        let metadata_entry = CEntityMetadataEntry {
            index: 17,
            metadata_type: 0,
            value: vec![player.skin_parts.bits() as u8],
        };
        let entity_metadata = CEntityMetadata {
            entity_id: player.entity_id as i32,
            metadata: vec![metadata_entry],
        }
        .encode();
        for player in &mut self.players {
            player.client.send_packet(&entity_metadata);
        }
    }

    fn handle_plugin_message(&mut self, plugin_message: SPluginMessage, player: usize) {
        if plugin_message.channel == "worldedit:cui" {
            self.players[player].worldedit_send_cui("s|cuboid");
        }
    }

    fn handle_player_position(&mut self, player_position: SPlayerPosition, player: usize) {
        let old = self.players[player].pos;
        let new = PlayerPos::new(player_position.x, player_position.y, player_position.z);
        if !new.is_valid() {
            self.players[player].client.close_connection();
            return;
        }
        self.players[player].pos = new;
        self.players[player].on_ground = player_position.on_ground;
        let packet = if (new.x - old.x).abs() > 8.0
            || (new.y - old.y).abs() > 8.0
            || (new.z - old.z).abs() > 8.0
        {
            CEntityTeleport {
                entity_id: self.players[player].entity_id as i32,
                x: new.x,
                y: new.y,
                z: new.z,
                yaw: self.players[player].yaw,
                pitch: self.players[player].pitch,
                on_ground: player_position.on_ground,
            }
            .encode()
        } else {
            let delta_x = ((player_position.x * 32.0 - old.x * 32.0) * 128.0) as i16;
            let delta_y = ((player_position.y * 32.0 - old.y * 32.0) * 128.0) as i16;
            let delta_z = ((player_position.z * 32.0 - old.z * 32.0) * 128.0) as i16;
            CEntityPosition {
                delta_x,
                delta_y,
                delta_z,
                entity_id: self.players[player].entity_id as i32,
                on_ground: player_position.on_ground,
            }
            .encode()
        };
        for other_player in 0..self.players.len() {
            if player == other_player {
                continue;
            };
            self.players[other_player].client.send_packet(&packet);
        }
        self.on_player_move(player, old, new);
    }

    fn handle_player_position_and_rotation(
        &mut self,
        player_position_and_rotation: SPlayerPositionAndRotation,
        player: usize,
    ) {
        let old = self.players[player].pos;
        let new = PlayerPos::new(
            player_position_and_rotation.x,
            player_position_and_rotation.y,
            player_position_and_rotation.z,
        );
        if !new.is_valid() || !player_position_and_rotation.yaw.is_finite()
            || !player_position_and_rotation.pitch.is_finite() {
            self.players[player].client.close_connection();
            return;
        }
        self.players[player].pos = new;
        self.players[player].yaw = player_position_and_rotation.yaw;
        self.players[player].pitch = player_position_and_rotation.pitch;
        self.players[player].on_ground = player_position_and_rotation.on_ground;
        let packet = if (new.x - old.x).abs() > 8.0
            || (new.y - old.y).abs() > 8.0
            || (new.z - old.z).abs() > 8.0
        {
            CEntityTeleport {
                entity_id: self.players[player].entity_id as i32,
                x: new.x,
                y: new.y,
                z: new.z,
                yaw: self.players[player].yaw,
                pitch: self.players[player].pitch,
                on_ground: player_position_and_rotation.on_ground,
            }
            .encode()
        } else {
            let delta_x = ((player_position_and_rotation.x * 32.0 - old.x * 32.0) * 128.0) as i16;
            let delta_y = ((player_position_and_rotation.y * 32.0 - old.y * 32.0) * 128.0) as i16;
            let delta_z = ((player_position_and_rotation.z * 32.0 - old.z * 32.0) * 128.0) as i16;
            CEntityPositionAndRotation {
                delta_x,
                delta_y,
                delta_z,
                pitch: player_position_and_rotation.pitch,
                yaw: player_position_and_rotation.yaw,
                entity_id: self.players[player].entity_id as i32,
                on_ground: player_position_and_rotation.on_ground,
            }
            .encode()
        };
        let entity_head_look = CEntityHeadLook {
            entity_id: self.players[player].entity_id as i32,
            yaw: player_position_and_rotation.yaw,
        }
        .encode();
        for other_player in 0..self.players.len() {
            if player == other_player {
                continue;
            };
            self.players[other_player].client.send_packet(&packet);
            self.players[other_player]
                .client
                .send_packet(&entity_head_look);
        }
        self.on_player_move(player, old, new);
    }

    fn handle_player_rotation(&mut self, player_rotation: SPlayerRotation, player: usize) {
        if !player_rotation.yaw.is_finite() || !player_rotation.pitch.is_finite() {
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
        for other_player in 0..self.players.len() {
            if player == other_player {
                continue;
            };
            self.players[other_player]
                .client
                .send_packet(&rotation_packet);
            self.players[other_player]
                .client
                .send_packet(&entity_head_look);
        }
    }

    fn handle_player_movement(&mut self, player_movement: SPlayerMovement, player: usize) {
        self.players[player].on_ground = player_movement.on_ground;
    }

    fn handle_player_digging(&mut self, player_digging: SPlayerDigging, player: usize) {
        let mut acknowledgement = Vec::new();
        acknowledgement.write_varint(player_digging.sequence);
        self.players[player]
            .client
            .send_packet(&mchprs_network::packets::PacketEncoder::new(
                acknowledgement,
                4,
            ));
        if player_digging.status == 0 {
            let block_pos = BlockPos::from_packed(player_digging.pos);
            if !Plot::in_plot_bounds(self.world.x, self.world.z, block_pos.x, block_pos.z)
                || !(0..super::PLOT_BLOCK_HEIGHT).contains(&block_pos.y)
                || !self.container_in_reach(player, block_pos) {
                return;
            }
            let block = self.world.get_block(block_pos);

            if !Plot::in_plot_bounds(self.world.x, self.world.z, block_pos.x, block_pos.z) {
                self.players[player].send_system_message(messages::CAN_T_BREAK_BLOCKS_OUTSIDE_PLOT);
                return;
            }

            // This worldedit wand stuff should probably be done in another file. It's good enough for now.
            let item_in_hand = self.players[player].inventory
                [self.players[player].selected_slot as usize + 36]
                .clone();
            if let Some(item) = item_in_hand {
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

            if !self.players[player].can_build_action("break", self.owner) {
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
            if !matches!(block, Block::Air {}) {
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

    fn handle_entity_action(&mut self, entity_action: SEntityAction, player: usize) {
        match entity_action.action_id {
            0 => self.players[player].crouching = true,
            1 => self.players[player].crouching = false,
            3 => self.players[player].sprinting = true,
            4 => self.players[player].sprinting = false,
            _ => {}
        }
        let mut bitfield = 0;
        if self.players[player].crouching {
            bitfield |= 0x02;
        };
        if self.players[player].sprinting {
            bitfield |= 0x08;
        };
        let metadata_entries = vec![
            CEntityMetadataEntry {
                index: 0,
                metadata_type: 0,
                value: vec![bitfield],
            },
            CEntityMetadataEntry {
                index: 6,
                metadata_type: 21,
                value: vec![if self.players[player].crouching { 5 } else { 0 }],
            },
        ];
        let entity_metadata = CEntityMetadata {
            entity_id: self.players[player].entity_id as i32,
            metadata: metadata_entries,
        }
        .encode();
        for other_player in 0..self.players.len() {
            if player == other_player {
                continue;
            };
            self.players[other_player]
                .client
                .send_packet(&entity_metadata);
        }
    }

    fn handle_held_item_change(&mut self, held_item_change: SHeldItemChange, player: usize) {
        if !(0..9).contains(&held_item_change.slot) {
            return;
        }
        let entity_equipment = CEntityEquipment {
            entity_id: self.players[player].entity_id as i32,
            equipment: vec![CEntityEquipmentEquipment {
                slot: 0, // Main hand
                item: self.players[player].inventory[held_item_change.slot as usize + 36]
                    .as_ref()
                    .map(|item| SlotData {
                        item_count: item.count as i8,
                        item_id: item.item_type.get_id() as i32,
                        nbt: item.nbt.clone(),
                    }),
            }],
        }
        .encode();
        for other_player in 0..self.players.len() {
            if player == other_player {
                continue;
            };
            self.players[other_player]
                .client
                .send_packet(&entity_equipment);
        }
        self.players[player].selected_slot = held_item_change.slot as u32;
    }

    fn handle_update_sign(&mut self, packet: SUpdateSign, player: usize) {
        let pos = BlockPos::from_packed(packet.pos);
        if !self.players[player].can_build_action("sign", self.owner)
            || !Plot::in_plot_bounds(self.world.x, self.world.z, pos.x, pos.z)
            || !(0..super::PLOT_BLOCK_HEIGHT).contains(&pos.y)
            || !self.container_in_reach(player, pos)
            || !matches!(self.world.get_block_entity(pos), Some(BlockEntity::Sign(_)))
        {
            return;
        }
        let mut rows = packet
            .lines
            .iter()
            .map(|line| json!({ "text": line }).to_string());
        let mut sign = match self.world.get_block_entity(pos) {
            Some(BlockEntity::Sign(sign)) => (**sign).clone(),
            _ => SignBlockEntity::default(),
        };
        if sign.waxed {
            return;
        }
        let updated = [
            rows.next().unwrap(),
            rows.next().unwrap(),
            rows.next().unwrap(),
            rows.next().unwrap(),
        ];
        if packet.front {
            sign.rows = updated;
        } else {
            sign.back_rows = updated;
        }
        let block_entity = BlockEntity::Sign(Box::new(sign));
        self.world.set_block_entity(pos, block_entity);
    }
}
