use crate::chat::ChatComponent;
use crate::config::CONFIG;
use crate::messages;
use crate::permissions;
use crate::player::{Gamemode, PacketSender, Player, SkinParts};
use crate::plot::{self, Plot, database};
use crate::utils::HyphenatedUUID;
use crate::velocity;
use backtrace::Backtrace;
use bus::Bus;
#[cfg(test)]
use mchprs_network::packets::PacketEncoderExt;
use mchprs_network::packets::clientbound::{CPlayerInfoAddPlayerProperty, CPong, CResponse, ClientBoundPacket};
use mchprs_network::packets::serverbound::{SHandshake, SLoginPluginResponse, SLoginStart, SPing, SRequest, ServerBoundPacketHandler};
use mchprs_network::{NetworkServer, PlayerPacketSender};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs::{self, File};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

mod login;

pub const MC_VERSION: &str = "1.21.5";
pub const MC_DATA_VERSION: i32 = 4325;
pub const PROTOCOL_VERSION: i32 = 770;

#[cfg(test)]
mod chat_tests;
#[cfg(test)]
mod skin_tests;

pub fn version_string() -> String {
    format!(
        "MROWW {} (Minecraft {}, protocol {})",
        env!("MCHPRS_VERSION"),
        MC_VERSION,
        PROTOCOL_VERSION
    )
}

/// `Message` gets send from a plot thread to the server thread.
#[derive(Debug)]
pub enum Message {
    /// This message is sent to the server thread when a player sends a chat message,
    /// It contains the uuid and name of the player and the raw message the player sent.
    ChatInfo(u128, String, String),
    /// This message is sent to the server thread when a player joins the server.
    PlayerJoined(Player),
    /// This message is sent to the server thread when a player leaves the server.
    PlayerLeft(u128),
    /// This message is sent to the server thread when a player goes outside of their plot.
    PlayerLeavePlot(Player),
    /// Routes a standalone `/tp <player>` request.
    PlayerTeleportOther(Player, String),
    CompletePlayerNames(PlayerPacketSender, i32, String),
    /// The proxy asked RedBox to teleport a player to a position.
    RedBoxTeleport {
        uuid: u128,
        position: crate::redbox::Position,
    },
    /// The proxy asked RedBox to change a player's gamemode.
    RedBoxGamemode(u128, Gamemode),
    /// The proxy resolved a player's effective LuckPerms data.
    RedBoxPermissions {
        uuid: u128,
        group: String,
        prefix: String,
        permissions: Vec<(String, bool)>,
    },
    ServerInfo {
        sender: PlayerPacketSender,
        page: usize,
        plot: Option<(i32, i32)>,
        span: tracing::Span,
    },
    /// This message is sent to the server thread when a player changes their gamemode.
    PlayerUpdateGamemode(u128, Gamemode),
    /// This message is sent to the server thread when a plot unloads itself.
    PlotUnload(i32, i32),
    /// Retains queued players until the server removes the failed plot's sender.
    PlotLoadFailed(i32, i32, String, Option<Player>, Receiver<PrivMessage>),
    /// This message is sent to the server thread when a player runs /whitelist add.
    WhitelistAdd(u128, String, PlayerPacketSender),
    /// This message is sent to the server thread when a player runs /whitelist remove.
    WhitelistRemove(u128, PlayerPacketSender),
    /// This message is sent to the server thread when a player runs /stop.
    Shutdown,
}

/// `BroadcastMessage` gets broadcasted from the server thread to all the plot threads.
/// This happens when there is a chat message, a player joins or leaves, or the server
/// shuts down.
#[derive(Debug, Clone)]
pub enum BroadcastMessage {
    /// This message is broadcasted for chat messages. It contains the uuid of the player and
    /// the raw json data to send to the clients.
    Chat(u128, Vec<ChatComponent>),
    /// This message is broadcasted when a player joins the server. It is used to update
    /// the tab-list on all connected clients.
    PlayerJoinedInfo(PlayerJoinInfo),
    /// This message is broadcasted when a player leaves the server. It is used to update
    /// the tab-list on all connected clients.
    PlayerLeft(u128),
    /// This message is broadcasted when a player changes their gamemode,
    PlayerUpdateGamemode(u128, Gamemode),
    /// This message is broadcasted when the server is stopping, either through the stop
    /// command or through the ctrl+c handler.
    Shutdown,
}

/// `PrivMessage` gets send from the server thread directly to a plot thread.
/// This only happens when a player is getting transfered to a plot.
#[derive(Debug)]
pub enum PrivMessage {
    PlayerEnterPlot(Player),
    PlayerTeleportOther(Player, String),
    /// The proxy asked RedBox to teleport a player already in this plot.
    RedBoxTeleport {
        uuid: u128,
        position: crate::redbox::Position,
    },
    /// The proxy asked RedBox to change a player's gamemode.
    RedBoxGamemode(u128, Gamemode),
    /// Effective LuckPerms data resolved by the proxy for a player.
    RedBoxPermissions {
        uuid: u128,
        group: String,
        prefix: String,
        permissions: Vec<(String, bool)>,
    },
    Diagnostics(PlayerPacketSender, tracing::Span),
}

/// This is the data that gets sent in the `PlayerJoinedInfo` broadcast message.
/// It contains imformation such as the player's username, uuid, and skin.
#[derive(Debug, Clone)]
pub struct PlayerJoinInfo {
    pub username: String,
    pub uuid: u128,
    pub properties: Vec<CPlayerInfoAddPlayerProperty>,
    pub gamemode: Gamemode,
}

#[derive(Debug, Clone)]
struct PlayerListEntry {
    plot_x: i32,
    plot_z: i32,
    username: String,
    gamemode: Gamemode,
    properties: Vec<CPlayerInfoAddPlayerProperty>,
    chat_prefix: Option<String>,
}

struct PlotListEntry {
    plot_x: i32,
    plot_z: i32,
    priv_message_sender: mpsc::Sender<PrivMessage>,
}

#[derive(Serialize, Deserialize)]
struct WhitelistEntry {
    uuid: HyphenatedUUID,
    name: String,
}

/// This represents a minecraft server
pub struct MinecraftServer {
    waveform_api: Option<crate::waveform::api::ApiServer>,
    network: NetworkServer,
    broadcaster: Bus<BroadcastMessage>,
    receiver: Receiver<Message>,
    plot_sender: Sender<Message>,
    online_players: FxHashMap<u128, PlayerListEntry>,
    running_plots: Vec<PlotListEntry>,
    whitelist: Option<Vec<WhitelistEntry>>,
    /// RedBox messages that arrived before the player finished logging in.
    pending_redbox: FxHashMap<u128, Vec<crate::redbox::Incoming>>,
}

impl MinecraftServer {
    /// Start the server
    pub fn run() {
        std::panic::set_hook(Box::new(|panic_info| {
            let backtrace = Backtrace::new();
            error!("plot {}\n{:?}", panic_info.to_string(), backtrace);
        }));

        info!("Starting MROWW server...");
        let start_time = Instant::now();

        // Create world folders if they don't exist yet
        fs::create_dir_all("./world/players").unwrap();
        fs::create_dir_all("./world/plots").unwrap();
        fs::create_dir_all("./schems").unwrap();

        plot::database::init();

        let bind_addr = CONFIG.bind_address.clone();

        // Create thread messaging structs
        let (plot_tx, server_rx) = mpsc::channel();
        let bus = Bus::new(100);
        let ctrl_handler_sender = plot_tx.clone();

        ctrlc::set_handler(move || {
            ctrl_handler_sender.send(Message::Shutdown).unwrap();
        })
        .expect("There was an error setting the ctrlc handler");

        let whitelist = CONFIG.whitelist.then(|| {
            if !Path::new("whitelist.json").exists() {
                File::create("whitelist.json").expect("Failed to create whitelist.json");
            }
            serde_json::from_reader(File::open("whitelist.json").expect("Failed to open whitelist.json")).unwrap_or_default()
        });

        if let Some(permissions_config) = &CONFIG.luckperms {
            permissions::init(permissions_config.clone()).unwrap();
        }
        if let Some(config) = &CONFIG.velocity {
            assert!(!CONFIG.bungeecord, "Velocity and legacy BungeeCord forwarding are mutually exclusive");
            velocity::init(config).expect("Cannot initialize Velocity forwarding");
        }

        // Create server struct
        let waveform_api = CONFIG
            .waveform_enabled
            .then(|| crate::waveform::api::ApiServer::start(&CONFIG.waveform_bind_address).expect("Cannot start waveform API"));
        if let Some(api) = &waveform_api {
            info!(address = %api.address, "Waveform API ready");
        }
        let mut server = MinecraftServer {
            waveform_api,
            network: NetworkServer::new(bind_addr),
            broadcaster: bus,
            receiver: server_rx,
            plot_sender: plot_tx,
            online_players: FxHashMap::default(),
            running_plots: Vec::new(),
            whitelist,
            pending_redbox: FxHashMap::default(),
        };

        info!("Done! Start took {:?}", start_time.elapsed());

        loop {
            server.update();
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Updates the player's location on the `online_players` list
    fn update_player_entry(&mut self, uuid: u128, plot_x: i32, plot_z: i32) {
        let player = self.online_players.get_mut(&uuid);
        if let Some(player) = player {
            player.plot_x = plot_x;
            player.plot_z = plot_z;
        }
    }

    /// Removes the plot entry from the `running_plots` list
    fn handle_plot_unload(&mut self, plot_x: i32, plot_z: i32) {
        info!(plot_x, plot_z, "Plot unloaded");
        let index = self.running_plots.iter().position(|p| p.plot_x == plot_x && p.plot_z == plot_z);
        if let Some(index) = index {
            self.running_plots.remove(index);
        }
    }

    fn graceful_shutdown(&mut self) {
        drop(self.waveform_api.take());
        info!("Commencing graceful shutdown...");
        self.broadcaster.broadcast(BroadcastMessage::Shutdown);
        // Wait for all plots to save and unload
        while !self.running_plots.is_empty() {
            while let Ok(message) = self.receiver.try_recv() {
                match message {
                    Message::PlotUnload(plot_x, plot_z) => self.handle_plot_unload(plot_x, plot_z),
                    message @ Message::PlotLoadFailed(..) => self.handle_message(message),
                    _ => {}
                }
                std::thread::sleep(Duration::from_millis(2));
            }
        }

        if let Some(whitelist) = &self.whitelist {
            fs::write("whitelist.json", serde_json::to_string(whitelist).unwrap()).unwrap();
        }

        std::process::exit(0);
    }

    fn send_player_to_plot(&mut self, player: Player, new_entry: bool) {
        let (plot_x, plot_z) = player.pos.plot_pos();

        if new_entry {
            let player_list_entry = PlayerListEntry {
                plot_x,
                plot_z,
                username: player.username.clone(),
                gamemode: player.gamemode,
                properties: player.profile_properties.clone(),
                chat_prefix: player.chat_prefix().map(str::to_owned),
            };
            self.online_players.insert(player.uuid, player_list_entry);
        } else {
            self.update_player_entry(player.uuid, plot_x, plot_z);
        }

        let plot = self.running_plots.iter().find(|p| p.plot_x == plot_x && p.plot_z == plot_z);
        if let Some(plot) = plot {
            let _ = plot.priv_message_sender.send(PrivMessage::PlayerEnterPlot(player));
        } else {
            let (priv_tx, priv_rx) = mpsc::channel();
            info!(plot_x, plot_z, "Loading plot");
            Plot::load_and_run(
                plot_x,
                plot_z,
                self.broadcaster.add_rx(),
                self.plot_sender.clone(),
                priv_rx,
                false,
                Some(player),
            );
            self.running_plots.push(PlotListEntry {
                plot_x,
                plot_z,
                priv_message_sender: priv_tx,
            });
        }
    }

    /// Routes a proxy message to the plot thread that currently owns the player.
    fn send_to_player_plot(&self, uuid: u128, message: PrivMessage) {
        let Some(entry) = self.online_players.get(&uuid) else {
            return;
        };
        let (plot_x, plot_z) = (entry.plot_x, entry.plot_z);
        if let Some(plot) = self.running_plots.iter().find(|plot| plot.plot_x == plot_x && plot.plot_z == plot_z) {
            let _ = plot.priv_message_sender.send(message);
        }
    }

    fn handle_message(&mut self, message: Message) {
        match message {
            Message::ServerInfo { sender, page, plot, span } => {
                let _entered = span.enter();
                if let Some((x, z)) = plot {
                    if let Some(plot) = self.running_plots.iter().find(|p| p.plot_x == x && p.plot_z == z) {
                        if let Err(error) = plot.priv_message_sender.send(PrivMessage::Diagnostics(sender, tracing::Span::current())) {
                            if let PrivMessage::Diagnostics(sender, _) = error.0 {
                                sender.send_error_message("Plot is not running");
                            }
                        }
                        return;
                    }
                    sender.send_error_message("Plot is not running");
                    return;
                }
                let pages = self.running_plots.len().div_ceil(10).max(1);
                if page == 0 || page > pages {
                    sender.send_error_message(&format!("Plot page must be between 1 and {pages}"));
                    return;
                }
                let mut text = format!(
                    "{} online players; {} running plots; page {page}/{pages}",
                    self.online_players.len(),
                    self.running_plots.len()
                );
                for plot in self.running_plots.iter().skip((page - 1) * 10).take(10) {
                    let names = self
                        .online_players
                        .values()
                        .filter(|p| p.plot_x == plot.plot_x && p.plot_z == plot.plot_z)
                        .map(|p| p.username.as_str())
                        .collect::<Vec<_>>();
                    text.push_str(&format!(
                        "\n{},{}: {} players [{}]",
                        plot.plot_x,
                        plot.plot_z,
                        names.len(),
                        names.join(", ")
                    ));
                }
                info!(details = %text, "Running plots");
                sender.send_system_message(&text);
            }
            Message::PlayerJoined(mut player) => {
                info!(player = %player.username, uuid = %format_args!("{:032x}", player.uuid), "Player joined");
                if CONFIG.native_chat && permissions::ranked_chat() {
                    player.send_chat_message(0, &ChatComponent::player_joined(&player.username));
                }
                // Send player info to plots
                let player_join_info = PlayerJoinInfo {
                    username: player.username.clone(),
                    uuid: player.uuid,
                    properties: player.profile_properties.clone(),
                    gamemode: player.gamemode,
                };
                if let Err(error) = database::ensure_user(&format!("{:032x}", player.uuid), &player.username) {
                    tracing::error!("Could not register plot user: {error}");
                    player.client.close_connection();
                    return;
                }
                self.broadcaster.broadcast(BroadcastMessage::PlayerJoinedInfo(player_join_info));
                self.send_player_to_plot(player, true);
            }
            Message::PlayerLeft(uuid) => {
                if let Some((_, player)) = self.online_players.remove_entry(&uuid) {
                    info!(player = %player.username, uuid = %format_args!("{uuid:032x}"), "Player left");
                    if CONFIG.native_chat && !crate::redbox::enabled() {
                        self.broadcaster
                            .broadcast(BroadcastMessage::Chat(0, ChatComponent::player_left(&player.username)));
                    }
                }
                self.broadcaster.broadcast(BroadcastMessage::PlayerLeft(uuid));
            }
            Message::PlotUnload(plot_x, plot_z) => self.handle_plot_unload(plot_x, plot_z),
            Message::PlotLoadFailed(x, z, reason, initial_player, queued) => {
                error!("Could not load plot {},{}: {}. Original save preserved.", x, z, reason);
                // Drop the sender before draining: no player can be queued after this point.
                self.handle_plot_unload(x, z);
                let players = initial_player.into_iter().chain(queued.try_iter().filter_map(|message| match message {
                    PrivMessage::PlayerEnterPlot(player) => Some(player),
                    PrivMessage::Diagnostics(sender, _) => {
                        sender.send_error_message("Plot failed to load");
                        None
                    }
                    PrivMessage::PlayerTeleportOther(player, _) => Some(player),
                    PrivMessage::RedBoxTeleport { .. } | PrivMessage::RedBoxGamemode(..) | PrivMessage::RedBoxPermissions { .. } => None,
                }));
                for mut player in players {
                    player.kick(json!({"text": messages::plot_load_failed(x, z)}).to_string());
                    player.client.close_connection();
                    self.handle_message(Message::PlayerLeft(player.uuid));
                }
            }
            Message::ChatInfo(uuid, username, message) => {
                if !CONFIG.native_chat {
                    return;
                }
                info!("<{}> {}", username, message);
                let components = self
                    .online_players
                    .get(&uuid)
                    .and_then(|player| player.chat_prefix.as_deref())
                    .map_or_else(
                        || ChatComponent::from_legacy_text(&CONFIG.chat_format.replace("{username}", &username).replace("{message}", &message)),
                        |prefix| ChatComponent::ranked_message(prefix, &username, &message),
                    );
                self.broadcaster.broadcast(BroadcastMessage::Chat(uuid, components));
            }
            Message::PlayerLeavePlot(player) => {
                self.send_player_to_plot(player, false);
            }
            Message::CompletePlayerNames(sender, id, text) => {
                if let Some(response) =
                    plot::commands::complete_teleport(id, &text, self.online_players.values().map(|player| player.username.as_str()))
                {
                    sender.send_packet(&response.encode());
                }
            }
            Message::PlayerTeleportOther(player, other_username) => {
                let username_lower = other_username.to_lowercase();
                if let Some((_, other_player)) = self
                    .online_players
                    .iter()
                    .find(|(_, p)| p.username.to_lowercase().starts_with(&username_lower))
                {
                    let plot_x = other_player.plot_x;
                    let plot_z = other_player.plot_z;
                    let target_username = other_player.username.clone();
                    let plot_index = self.running_plots.iter().position(|p| p.plot_x == plot_x && p.plot_z == plot_z);
                    if let Some(index) = plot_index {
                        self.update_player_entry(player.uuid, plot_x, plot_z);
                        let _ = self.running_plots[index]
                            .priv_message_sender
                            .send(PrivMessage::PlayerTeleportOther(player, target_username));
                    } else {
                        player.send_system_message(messages::TARGET_PLOT_NOT_LOADED);
                        self.send_player_to_plot(player, false);
                    }
                } else {
                    player.send_system_message(messages::PLAYER_NOT_FOUND);
                    self.send_player_to_plot(player, false);
                }
            }
            Message::Shutdown => {
                self.graceful_shutdown();
            }
            Message::RedBoxTeleport { uuid, position } => {
                self.send_to_player_plot(uuid, PrivMessage::RedBoxTeleport { uuid, position });
            }
            Message::RedBoxGamemode(uuid, gamemode) => {
                self.send_to_player_plot(uuid, PrivMessage::RedBoxGamemode(uuid, gamemode));
            }
            Message::RedBoxPermissions {
                uuid,
                group,
                prefix,
                permissions,
            } => {
                self.send_to_player_plot(
                    uuid,
                    PrivMessage::RedBoxPermissions {
                        uuid,
                        group,
                        prefix,
                        permissions,
                    },
                );
            }
            Message::PlayerUpdateGamemode(uuid, gamemode) => {
                if let Some(player) = self.online_players.get_mut(&uuid) {
                    player.gamemode = gamemode;
                }
                self.broadcaster.broadcast(BroadcastMessage::PlayerUpdateGamemode(uuid, gamemode));
            }
            Message::WhitelistAdd(uuid, username, sender) => {
                if let Some(whitelist) = &mut self.whitelist {
                    let msg = messages::whitelist_added(&username);
                    sender.send_system_message(&msg);
                    let uuid = HyphenatedUUID(uuid);
                    debug!("Added to whitelist: {} ({})", &username, uuid.to_string());

                    whitelist.push(WhitelistEntry { name: username, uuid });
                } else {
                    sender.send_error_message(messages::WHITELIST_NOT_ENABLED);
                }
            }
            Message::WhitelistRemove(uuid, sender) => {
                if let Some(whitelist) = &mut self.whitelist {
                    let mut found = false;
                    whitelist.retain(|entry| {
                        let matches = entry.uuid.0 == uuid;
                        if matches {
                            let msg = messages::whitelist_removed(&entry.name);
                            sender.send_system_message(&msg);
                            debug!("Removed from whitelist: {}", HyphenatedUUID(uuid).to_string());
                            found = true;
                        }
                        !matches
                    });
                    if !found {
                        sender.send_error_message(messages::THAT_PLAYER_NOT_WHITELISTED_ON_SERVER);
                    }
                } else {
                    sender.send_error_message(messages::WHITELIST_NOT_ENABLED);
                }
            }
        }
    }

    fn update(&mut self) {
        while let Ok(message) = self.receiver.try_recv() {
            self.handle_message(message);
        }
        self.network.update();

        let mut client_idx = 0;
        let mut clients_len = self.network.handshaking_clients.len();
        loop {
            if client_idx >= clients_len {
                break;
            }

            let packets = self.network.handshaking_clients[client_idx].receive_packets();
            for packet in packets {
                packet.handle(self, client_idx);
            }

            let new_len = self.network.handshaking_clients.len();

            if clients_len == new_len {
                client_idx += 1;
            }
            clients_len = new_len;
        }
    }
}

impl ServerBoundPacketHandler for MinecraftServer {
    fn handle_client_settings(&mut self, packet: mchprs_network::packets::serverbound::SClientSettings, idx: usize) {
        self.admission_client_settings(packet, idx);
    }
    /// RedBox may answer before the player reaches the play phase (for example a
    /// cross-server `/warp` teleport sent on Velocity's server-connected event).
    /// Buffer those messages and apply them when the player finishes logging in.
    fn handle_plugin_message(&mut self, packet: mchprs_network::packets::serverbound::SPluginMessage, idx: usize) {
        if packet.channel != crate::redbox::CHANNEL || !crate::redbox::enabled() {
            return;
        }
        let Some(uuid) = self.network.handshaking_clients[idx].uuid else {
            return;
        };
        match crate::redbox::decode(&packet.data) {
            Ok(message) if message.uuid() == uuid => self.pending_redbox.entry(uuid).or_default().push(message),
            Ok(_) => warn!("Rejected RedBox message for another player during login"),
            Err(error) => warn!(%error, "Rejected malformed RedBox message during login"),
        }
    }
    fn handle_login_plugin_response(&mut self, packet: SLoginPluginResponse, idx: usize) {
        self.admission_login_plugin_response(packet, idx);
    }
    fn handle_login_acknowledged(&mut self, _: mchprs_network::packets::serverbound::SLoginAcknowledged, idx: usize) {
        self.admission_login_acknowledged(idx);
    }
    fn handle_known_packs(&mut self, _: mchprs_network::packets::serverbound::SKnownPacks, idx: usize) {
        self.admission_known_packs(idx);
    }
    fn handle_configuration_finished(&mut self, _: mchprs_network::packets::serverbound::SConfigurationFinished, idx: usize) {
        self.admission_configuration_finished(idx);
    }

    fn handle_handshake(&mut self, handshake: SHandshake, client_idx: usize) {
        self.admission_handshake(handshake, client_idx);
    }

    fn handle_request(&mut self, _request: SRequest, client_idk: usize) {
        let client = &mut self.network.handshaking_clients[client_idk];
        let response = CResponse {
            json_response: json!({
                "version": {
                    "name": MC_VERSION,
                    "protocol": PROTOCOL_VERSION
                },
                "players": {
                    "max": CONFIG.max_players,
                    "online": self.online_players.len(),
                    "sample": []
                },
                "description": {
                    "text": CONFIG.motd
                }
            })
            .to_string(),
        }
        .encode();
        client.send_packet(&response);
    }

    fn handle_ping(&mut self, ping: SPing, client_idx: usize) {
        let client = &mut self.network.handshaking_clients[client_idx];
        let pong = CPong { payload: ping.payload }.encode();
        client.send_packet(&pong);
    }

    fn handle_login_start(&mut self, login_start: SLoginStart, client_idx: usize) {
        self.handle_player_login(client_idx, login_start);
    }
}
