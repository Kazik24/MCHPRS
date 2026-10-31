use super::{Message, MinecraftServer, Player, SkinParts};
use crate::config::CONFIG;
use crate::messages;
use crate::permissions;
use crate::plot::commands::DECLARE_COMMANDS;
use crate::velocity;
use mchprs_network::NetworkState;
use mchprs_network::packets::PacketEncoderExt;
use mchprs_network::packets::clientbound::{
    CDisconnectLogin, CHeldItemChange, CJoinGame, CLoginPluginRequest, CLoginSuccess, CPlayerInfo, CPlayerInfoAddPlayer, CPluginMessage,
    CSetCompression, CTimeUpdate, CWindowItems, ClientBoundPacket,
};
use mchprs_network::packets::serverbound::{SHandshake, SLoginPluginResponse, SLoginStart};
use serde_json::json;
use std::convert::TryInto;
use tracing::warn;
impl MinecraftServer {
    pub(super) fn handle_player_login(&mut self, client_idx: usize, login_start: SLoginStart) {
        if !velocity::valid_username(&login_start.name) {
            self.network.handshaking_clients[client_idx].close_connection();
            return;
        }
        let clients = &mut self.network.handshaking_clients;
        let username = login_start.name;
        clients[client_idx].username = Some(username.clone());
        let set_compression = CSetCompression { threshold: 256 }.encode();
        clients[client_idx].send_packet(&set_compression);
        clients[client_idx].set_compressed(true);

        if CONFIG.velocity.is_some() {
            clients[client_idx].forwarding_pending = true;
            clients[client_idx].send_packet(&CLoginPluginRequest { message_id: 0 }.encode());
            return;
        }
        self.accept_player_login(client_idx);
    }

    fn accept_player_login(&mut self, client_idx: usize) {
        let clients = &mut self.network.handshaking_clients;
        let username = clients[client_idx].username.clone().unwrap();
        // Modern forwarding must never fall back to a username or offline UUID.
        if CONFIG.velocity.is_some() && clients[client_idx].uuid.is_none() {
            clients[client_idx].close_connection();
            return;
        }

        if let Some(whitelist) = &self.whitelist {
            // Authenticated proxy identities are whitelisted by UUID.
            let whitelisted = if let Some(uuid) = clients[client_idx].uuid {
                whitelist.iter().any(|entry| entry.uuid.0 == uuid)
            } else {
                whitelist.iter().any(|entry| entry.name == username)
            };
            if !whitelisted {
                let disconnect = CDisconnectLogin {
                    reason: json!({
                        "text": messages::YOU_NOT_WHITELISTED_ON_SERVER
                    })
                    .to_string(),
                }
                .encode();
                clients[client_idx].send_packet(&disconnect);
                clients[client_idx].close_connection();
                return;
            }
        }

        let uuid = clients[client_idx].uuid.unwrap_or_else(|| Player::generate_offline_uuid(&username));

        let login_success = CLoginSuccess {
            uuid,
            username: username.clone(),
            properties: clients[client_idx].profile_properties.clone(),
        }
        .encode();
        clients[client_idx].send_packet(&login_success);

        clients[client_idx].uuid = Some(uuid);
        clients[client_idx].protocol_phase = 1;
    }

    fn finish_player_login(&mut self, client_idx: usize) {
        let client = self.network.handshaking_clients.remove(client_idx);
        let username = client.username.clone().unwrap();
        let uuid = client.uuid.unwrap();
        let properties = client.profile_properties.clone();
        let skin_parts = SkinParts::from_bits_truncate(client.displayed_skin_parts as u32);

        let Some(mut player) = Player::load_player(uuid, username, client.into()) else {
            return;
        };
        // Apply any RedBox message that arrived during login, so a cross-server
        // teleport places the player before the world is sent.
        if let Some(messages) = self.pending_redbox.remove(&uuid) {
            for message in messages {
                match message {
                    crate::redbox::Incoming::Teleport { position, .. } if position.is_valid() => {
                        player.pos = position.player_pos();
                        player.yaw = position.yaw;
                        player.pitch = position.pitch;
                    }
                    crate::redbox::Incoming::Gamemode { mode, .. } => {
                        if let Some(gamemode) = crate::redbox::gamemode_from_id(mode) {
                            player.gamemode = gamemode;
                        }
                    }
                    _ => {}
                }
            }
        }
        player.profile_properties = properties;
        player.skin_parts = skin_parts;
        // With the proxy permission source the cache arrives after login; the
        // join gate runs once it is applied instead.
        if permissions::dedicated_permissions() && !player.proxy_permissions_pending() && !player.has_permission("mchprs.access.join") {
            player.kick(json!({"text": messages::PERMISSION_DENIED}).to_string());
            player.client.close_connection();
            return;
        }

        let join_game = CJoinGame {
            entity_id: player.entity_id as i32,
            is_hardcore: false,
            gamemode: player.gamemode.get_id() as u8,
            previous_gamemode: 255,
            world_count: 1,
            world_names: vec!["mchprs:world".to_owned()],
            world_name: "mchprs:world".to_owned(),
            hashed_seed: 0,
            max_players: CONFIG.max_players as i32,
            view_distance: CONFIG.view_distance as i32,
            simulation_distance: CONFIG.view_distance as i32,
            reduced_debug_info: false,
            enable_respawn_screen: false,
            is_debug: false,
            is_flat: true,
        }
        .encode();
        player.client.send_packet(&join_game);
        // Tell 1.21 clients to finish their loading screen once the chunks arrive.
        let mut loading = Vec::new();
        loading.write_unsigned_byte(13);
        loading.write_float(0.0);
        player.client.send_packet(&mchprs_network::packets::PacketEncoder::new(loading, 34));

        // Sends the custom brand name to the player
        // (This can be seen in the f3 debug menu in-game)
        let brand = CPluginMessage {
            channel: String::from("minecraft:brand"),
            data: {
                let mut data = Vec::new();
                data.write_string(32767, "MROWW");
                data
            },
        }
        .encode();
        player.client.send_packet(&brand);

        // Send the player's position and rotation.
        player.teleport(player.pos);

        // Send the player list to the newly connected player.
        // (This is the list you see when you press tab in-game)
        let mut add_player_list = Vec::new();
        for (uuid, player) in &self.online_players {
            add_player_list.push(CPlayerInfoAddPlayer {
                uuid: *uuid,
                name: player.username.clone(),
                display_name: None,
                gamemode: player.gamemode.get_id(),
                ping: 0,
                properties: player.properties.clone(),
            });
        }
        add_player_list.push(CPlayerInfoAddPlayer {
            uuid: player.uuid,
            name: player.username.clone(),
            display_name: None,
            gamemode: player.gamemode.get_id(),
            ping: 0,
            properties: player.profile_properties.clone(),
        });
        let player_info = CPlayerInfo::AddPlayer(add_player_list).encode();
        player.client.send_packet(&player_info);

        // Send the player's inventory
        let slot_data = player
            .inventory
            .iter()
            .map(|item| item.as_ref().map(crate::container::slot_data))
            .collect();
        let window_items = CWindowItems {
            window_id: 0,
            state_id: 0,
            slot_data,
            carried_item: None,
        }
        .encode();
        player.client.send_packet(&window_items);

        // Send the player's selected item slot
        let held_item_change = CHeldItemChange {
            slot: player.selected_slot as i8,
        }
        .encode();
        player.client.send_packet(&held_item_change);

        player.client.send_packet(if player.can_use_commands() {
            &DECLARE_COMMANDS
        } else {
            &crate::plot::commands::NO_COMMANDS
        });

        let time_update = CTimeUpdate {
            world_age: 0,
            // Noon
            time_of_day: -6000,
        }
        .encode();
        player.client.send_packet(&time_update);

        player.update_player_abilities();

        self.plot_sender.send(Message::PlayerJoined(player)).unwrap();
    }

    pub(super) fn admission_client_settings(&mut self, packet: mchprs_network::packets::serverbound::SClientSettings, idx: usize) {
        self.network.handshaking_clients[idx].displayed_skin_parts = packet.displayed_skin_parts;
    }

    pub(super) fn admission_login_plugin_response(&mut self, packet: SLoginPluginResponse, idx: usize) {
        let client = &mut self.network.handshaking_clients[idx];
        if CONFIG.velocity.is_none() || !client.forwarding_pending || packet.message_id != 0 {
            client.close_connection();
            return;
        }
        client.forwarding_pending = false;
        let profile = if packet.successful {
            velocity::verify(&packet.data, client.username.as_deref().unwrap_or_default())
        } else {
            Err(anyhow::anyhow!("Velocity forwarding is required"))
        };
        match profile {
            Ok(profile) => {
                client.uuid = Some(profile.uuid);
                client.profile_properties = profile.properties;
                // Drop any stale RedBox message from a previous login attempt.
                self.pending_redbox.remove(&profile.uuid);
                self.accept_player_login(idx);
            }
            Err(error) => {
                warn!("Rejected unauthenticated backend login: {error}");
                client.send_packet(
                    &CDisconnectLogin {
                        reason: json!({"text":messages::AUTHENTICATED_PROXY_REQUIRED}).to_string(),
                    }
                    .encode(),
                );
                client.close_connection();
            }
        }
    }

    pub(super) fn admission_login_acknowledged(&mut self, idx: usize) {
        let c = &mut self.network.handshaking_clients[idx];
        if c.protocol_phase != 1 {
            c.close_connection();
            return;
        }
        c.protocol_phase = 2;
        let mut flags = Vec::new();
        flags.write_varint(1);
        flags.write_string(32767, "minecraft:vanilla");
        c.send_packet(&mchprs_network::packets::PacketEncoder::new(flags, 0x0c));
        c.send_packet(&mchprs_network::packets::PacketEncoder::new(vec![0], 0x0e));
    }

    pub(super) fn admission_known_packs(&mut self, idx: usize) {
        let c = &mut self.network.handshaking_clients[idx];
        if c.protocol_phase != 2 {
            c.close_connection();
            return;
        }
        c.protocol_phase = 3;
        let mut data = mchprs_network::generated::REGISTRIES;
        while !data.is_empty() {
            let n = u32::from_be_bytes(data[..4].try_into().unwrap()) as usize;
            c.send_packet(&mchprs_network::packets::PacketEncoder::new(data[5..4 + n].to_vec(), 7));
            data = &data[4 + n..];
        }
        c.send_packet(&mchprs_network::packets::PacketEncoder::new(
            mchprs_network::generated::TAGS.to_vec(),
            0x0d,
        ));
        c.send_packet(&mchprs_network::packets::PacketEncoder::new(vec![], 3));
    }

    pub(super) fn admission_configuration_finished(&mut self, idx: usize) {
        if self.network.handshaking_clients[idx].protocol_phase != 3 {
            self.network.handshaking_clients[idx].close_connection();
            return;
        }
        self.finish_player_login(idx);
    }

    pub(super) fn admission_handshake(&mut self, handshake: SHandshake, client_idx: usize) {
        let clients = &mut self.network.handshaking_clients;
        let client = &mut clients[client_idx];
        let next_state = match handshake.next_state {
            1 => NetworkState::Status,
            2 => NetworkState::Login,
            _ => {
                warn!(next_state = handshake.next_state, "Rejected handshake with invalid next state");
                client.close_connection();
                return;
            }
        };
        if next_state == NetworkState::Login && handshake.protocol_version != super::PROTOCOL_VERSION {
            warn!("A player tried to connect using the wrong version");
            let disconnect = CDisconnectLogin {
                reason: json!({ "text": messages::version_mismatch(super::MC_VERSION) }).to_string(),
            }
            .encode();
            client.send_packet(&disconnect);
            client.close_connection();
        } else if next_state == NetworkState::Login && CONFIG.velocity.is_some() {
            if handshake.server_address.contains('\0') {
                client.close_connection();
            }
        } else if next_state == NetworkState::Login && CONFIG.bungeecord {
            let split: Vec<&str> = handshake.server_address.split('\u{0}').collect();
            if split.len() == 3 || split.len() == 4 {
                client.uuid = u128::from_str_radix(split[2], 16).ok();
            } else {
                let disconnect = CDisconnectLogin {
                    reason: json!({
                        "text": messages::IP_FORWARDING_REQUIRED
                    })
                    .to_string(),
                }
                .encode();
                client.send_packet(&disconnect);
                client.close_connection();
            }
        }
    }
}
