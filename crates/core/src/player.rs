use crate::chat::{ChatComponent, ColorCode};
use crate::config::CONFIG;
use crate::messages;
use crate::permissions::{self, PlayerPermissionsCache, Rank};
use crate::plot::worldedit::{WorldEditClipboard, WorldEditUndo};
use crate::plot::PLOT_SCALE;
use crate::utils::HyphenatedUUID;
use byteorder::{BigEndian, ReadBytesExt};
use mchprs_blocks::block_entities::{ContainerType, InventoryEntry};
use mchprs_blocks::items::{Item, ItemStack};
use mchprs_blocks::{BlockDirection, BlockFacing, BlockPos};
use mchprs_network::packets::clientbound::*;
use mchprs_network::packets::{PacketEncoder, SlotData};
use mchprs_network::{PlayerConn, PlayerPacketSender};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::convert::TryInto;
use std::fmt::{self, Display};
use std::fs;
use std::io::Cursor;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Instant, SystemTime};
use tracing::error;

pub type EntityId = u32;
static ENTITY_ID_COUNTER: AtomicU32 = AtomicU32::new(0);

pub(crate) fn allocate_entity_id() -> EntityId {
    ENTITY_ID_COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub enum Gamemode {
    Creative,
    Spectator,
}

impl Gamemode {
    pub fn get_id(self) -> i32 {
        match self {
            Gamemode::Creative => 1,
            Gamemode::Spectator => 3,
        }
    }
}

/// This structure represents how the player will be
/// serialized when saved to it's file.
#[derive(Debug, Serialize, Deserialize)]
pub struct PlayerData {
    on_ground: bool,
    flying: bool,
    motion: [f64; 3],
    position: [f64; 3],
    rotation: [f32; 2],
    inventory: Vec<InventoryEntry>,
    selected_item_slot: i32,
    fly_speed: f32,
    walk_speed: f32,
    gamemode: Gamemode,
}

impl Default for PlayerData {
    fn default() -> PlayerData {
        PlayerData {
            on_ground: true,
            flying: false,
            motion: [0.0, 0.0, 0.0],
            position: [128.0, 128.0, 128.0],
            rotation: [0.0, 0.0],
            inventory: Vec::new(),
            selected_item_slot: 0,
            fly_speed: 1.0,
            walk_speed: 1.0,
            gamemode: Gamemode::Creative,
        }
    }
}

bitflags! {
    #[derive(Default)]
    pub struct SkinParts: u32 {
        const CAPE = 0x01;
        const JACKET = 0x02;
        const LEFT_SLEEVE = 0x04;
        const RIGHT_SLEEVE = 0x08;
        const LEFT_PANTS_LEG = 0x10;
        const RIGHT_PANTS_LEG = 0x20;
        const HAT = 0x40;
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PlayerPos {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl PlayerPos {
    /// Keep coordinates inside Minecraft's border and away from integer overflow.
    pub fn is_valid(self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.z.is_finite()
            && self.x.abs() <= 30_000_000.0
            && self.z.abs() <= 30_000_000.0
            && (-2048.0..=2048.0).contains(&self.y)
    }
    pub fn new(x: f64, y: f64, z: f64) -> PlayerPos {
        PlayerPos { x, y, z }
    }

    pub fn block_pos(self) -> BlockPos {
        BlockPos {
            x: self.x.floor() as i32,
            y: self.y.floor() as i32,
            z: self.z.floor() as i32,
        }
    }

    pub fn chunk_pos(self) -> (i32, i32) {
        (self.x.floor() as i32 >> 4, self.z.floor() as i32 >> 4)
    }

    pub fn plot_pos(self) -> (i32, i32) {
        let (chunk_x, chunk_z) = self.chunk_pos();
        (chunk_x >> PLOT_SCALE, chunk_z >> PLOT_SCALE)
    }
}

impl std::fmt::Display for PlayerPos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {}, {})", self.x, self.y, self.z)
    }
}

pub struct Player {
    pub uuid: u128,
    pub username: String,
    pub profile_properties: Vec<mchprs_network::packets::clientbound::CPlayerInfoAddPlayerProperty>,
    pub skin_parts: SkinParts,
    pub inventory: Vec<Option<ItemStack>>,
    /// The selected slot of the player's hotbar (1-9)
    pub selected_slot: u32,
    permissions_refresh: Option<std::sync::mpsc::Receiver<anyhow::Result<PlayerPermissionsCache>>>,
    next_permissions_refresh: Instant,
    chat_window: Instant,
    chat_count: u32,
    pub(crate) proxy_chat: crate::proxy_chat::Session,
    pub pos: PlayerPos,
    /// The last X chunk the player was in. This is used for updated view position.
    pub last_chunk_x: i32,
    /// The last Z chunk the player was in. This is used for updated view position.
    pub last_chunk_z: i32,
    /// The player's head yaw rotation.
    pub yaw: f32,
    /// The player's head pitch rotation.
    pub pitch: f32,
    pub flying: bool,
    pub sprinting: bool,
    pub crouching: bool,
    pub(crate) last_compass_use: Option<Instant>,
    pub on_ground: bool,
    pub fly_speed: f32,
    pub walk_speed: f32,
    pub gamemode: Gamemode,
    pub entity_id: EntityId,
    pub client: PlayerConn,
    /// The last time the keep alive packet was received.
    pub last_keep_alive_received: Instant,
    /// The last time the keep alive packet was sent.
    last_keep_alive_sent: Instant,
    /// The worldedit first position.
    pub first_position: Option<BlockPos>,
    /// The worldedit second position.
    pub second_position: Option<BlockPos>,
    /// The worldedit current clipboard.
    pub worldedit_clipboard: Option<WorldEditClipboard>,
    /// The saved sections used for worldedit //undo
    /// Each entry stores the plot coords and the clipboard
    pub worldedit_undo: Vec<WorldEditUndo>,
    pub worldedit_redo: Vec<WorldEditUndo>,
    /// Commands are stored so they can be handled after packets
    pub command_queue: Vec<String>,
    pub(crate) open_container: Option<crate::container::OpenContainer>,
    pub(crate) redstone_tools: crate::plot::redstone_tools::PlayerTools,
    next_window_id: u8,
    permissions_cache: Option<PlayerPermissionsCache>,
}

impl fmt::Debug for Player {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Player")
            .field("username", &self.username)
            .field("uuid", &HyphenatedUUID(self.uuid).to_string())
            .finish()
    }
}

impl Player {
    pub fn generate_offline_uuid(username: &str) -> u128 {
        Cursor::new(md5::compute(format!("OfflinePlayer:{}", username)).0)
            .read_u128::<BigEndian>()
            .unwrap()
            // Encode version and varient into uuid
            & (!(0xC << 60) & !(0xF << 76))
            | ((0x8 << 60) | (0x3 << 76))
    }

    fn from_data(
        player_data: PlayerData,
        uuid: u128,
        username: String,
        client: PlayerConn,
    ) -> Player {
        // Load inventory
        let mut inventory: Vec<Option<ItemStack>> = vec![None; 46];
        for entry in player_data.inventory {
            let nbt = entry
                .nbt
                .map(|data| nbt::Blob::from_reader(&mut Cursor::new(data)).unwrap());
            inventory[entry.slot as usize] = Some(ItemStack {
                item_type: Item::from_id(entry.id),
                count: entry.count as u8,
                nbt,
            });
        }
        let permissions_cache = CONFIG
            .luckperms
            .is_some()
            .then(|| {
                permissions::load_player_cache(uuid).unwrap_or_else(|error| {
                    tracing::error!("Could not load LuckPerms permissions: {error}; denying permissions for this session");
                    PlayerPermissionsCache::default()
                })
            });
        Player {
            uuid,
            username,
            profile_properties: Vec::new(),
            skin_parts: Default::default(),
            inventory,
            selected_slot: player_data.selected_item_slot as u32,
            permissions_refresh: None,
            next_permissions_refresh: Instant::now() + std::time::Duration::from_secs(25),
            chat_window: Instant::now(),
            chat_count: 0,
            proxy_chat: Default::default(),
            pos: PlayerPos {
                x: player_data.position[0],
                y: player_data.position[1],
                z: player_data.position[2],
            },
            pitch: player_data.rotation[0],
            yaw: player_data.rotation[1],
            last_chunk_x: 0,
            last_chunk_z: 0,
            entity_id: allocate_entity_id(),
            client,
            flying: player_data.flying,
            sprinting: false,
            crouching: false,
            last_compass_use: None,
            gamemode: if permissions::dedicated_permissions()
                && !permissions_cache
                    .as_ref()
                    .and_then(|cache| cache.get_node_val("mchprs.build"))
                    .is_some_and(|value| value > 0)
            {
                Gamemode::Spectator
            } else {
                player_data.gamemode
            },
            on_ground: player_data.on_ground,
            walk_speed: player_data.walk_speed,
            fly_speed: player_data.fly_speed,
            last_keep_alive_received: Instant::now(),
            last_keep_alive_sent: Instant::now(),
            first_position: None,
            second_position: None,
            worldedit_clipboard: None,
            worldedit_undo: Vec::new(),
            worldedit_redo: Vec::new(),
            command_queue: Vec::new(),
            open_container: None,
            redstone_tools: Default::default(),
            next_window_id: 0,
            permissions_cache,
        }
    }

    /// This will load the player from the file. If the file does not exist,
    /// It will be created.
    pub fn load_player(uuid: u128, username: String, mut client: PlayerConn) -> Option<Player> {
        let filename = format!("./world/players/{:032x}", uuid);
        let path = std::path::Path::new(&filename);
        let result = (|| -> anyhow::Result<PlayerData> {
            let data = match fs::read(path) {
                Ok(d) => d,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Default::default()),
                Err(e) => return Err(e.into()),
            };
            let legacy = !data.starts_with(b"MCHPLY\0");
            let mut player: PlayerData = if legacy {
                bincode::deserialize(&data)?
            } else {
                if data.len() < 15 || u32::from_le_bytes(data[7..11].try_into()?) != 3 {
                    anyhow::bail!("unsupported player save version");
                }
                if u32::from_le_bytes(data[11..15].try_into()?)
                    != crate::server::MC_DATA_VERSION as u32
                {
                    anyhow::bail!("player Minecraft data version mismatch");
                }
                bincode::deserialize(&data[15..])?
            };
            if !(0..9).contains(&player.selected_item_slot) {
                anyhow::bail!("invalid hotbar slot");
            }
            if !PlayerPos::new(player.position[0], player.position[1], player.position[2])
                .is_valid()
                || player.rotation.iter().any(|angle| !angle.is_finite())
                || !player.fly_speed.is_finite()
                || !(0.0..=10.0).contains(&player.fly_speed)
                || !player.walk_speed.is_finite()
                || !(0.0..=10.0).contains(&player.walk_speed)
            {
                anyhow::bail!("invalid player coordinates, rotation or speed");
            }
            let mut inventory_slots = std::collections::HashSet::new();
            for entry in &mut player.inventory {
                if !(0..46).contains(&entry.slot)
                    || entry.count <= 0
                    || !inventory_slots.insert(entry.slot)
                {
                    anyhow::bail!("invalid inventory entry");
                }
                if legacy {
                    entry.id = *mchprs_blocks::generated::LEGACY_ITEMS
                        .get(entry.id as usize)
                        .ok_or_else(|| anyhow::anyhow!("unmappable legacy item {}", entry.id))?;
                }
                if entry.id as usize >= mchprs_blocks::generated::ITEMS.len() {
                    anyhow::bail!("invalid item ID");
                }
                if let Some(data) = &entry.nbt {
                    nbt::Blob::from_reader(&mut Cursor::new(data))?;
                }
            }
            if legacy {
                let mut bytes = b"MCHPLY\0".to_vec();
                bytes.extend_from_slice(&3u32.to_le_bytes());
                bytes.extend_from_slice(&(crate::server::MC_DATA_VERSION as u32).to_le_bytes());
                bytes.extend_from_slice(&bincode::serialize(&player)?);
                mchprs_save_data::atomic::backup(path)?;
                mchprs_save_data::atomic::write(path, &bytes)?;
            }
            Ok(player)
        })();
        match result {
            Ok(data) => Some(Self::from_data(data, uuid, username, client)),
            Err(err) => {
                error!(
                    "Refusing player login; original save preserved for {}: {}",
                    username, err
                );
                client.send_packet(
                    &CDisconnect {
                        reason: json!({"text":messages::PLAYER_SAVE_COULD_NOT_LOADED_ASK})
                            .to_string(),
                    }
                    .encode(),
                );
                client.close_connection();
                None
            }
        }
    }

    /// Saves the player to `./world/players/{uuid}`. This will create
    /// the file if it does not already exist.
    pub fn save(&self) {
        let mut inventory: Vec<InventoryEntry> = Vec::new();
        for (slot, item_option) in self.inventory.iter().enumerate() {
            if let Some(item) = item_option {
                let nbt = item.nbt.clone().map(|blob| {
                    let mut data = Vec::new();
                    blob.to_writer(&mut data).unwrap();
                    data
                });
                inventory.push(InventoryEntry {
                    count: item.count as i8,
                    id: item.item_type.get_id(),
                    slot: slot as i8,
                    nbt,
                });
            }
        }
        let data = bincode::serialize(&PlayerData {
            fly_speed: self.fly_speed,
            flying: self.flying,
            gamemode: self.gamemode,
            inventory,
            motion: [0f64, 0f64, 0f64],
            on_ground: self.on_ground,
            position: [self.pos.x, self.pos.y, self.pos.z],
            rotation: [self.pitch, self.yaw],
            selected_item_slot: self.selected_slot as i32,
            walk_speed: self.walk_speed,
        })
        .unwrap();
        let mut bytes = b"MCHPLY\0".to_vec();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&(crate::server::MC_DATA_VERSION as u32).to_le_bytes());
        bytes.extend_from_slice(&data);
        let filename = format!("./world/players/{:032x}", self.uuid);
        if let Err(err) = mchprs_save_data::atomic::write(std::path::Path::new(&filename), &bytes) {
            error!("Failed to save player {}: {}", self.username, err);
        }
    }

    /// Manages keep alives and packet reading. Return true if the view position should be updated.
    pub fn update(&mut self) -> bool {
        if self.last_keep_alive_received.elapsed().as_secs() > 30 {
            self.kick(json!({ "text": messages::CONNECTION_TIMEOUT }).to_string());
        }
        if self.last_keep_alive_sent.elapsed().as_secs() > 10 {
            self.send_keep_alive();
        }

        // Prevent from locking player position at Infinity or NaN
        if !self.pos.x.is_finite() || !self.pos.y.is_finite() || !self.pos.z.is_finite() {
            self.pos.x = 128.0;
            self.pos.y = 128.0;
            self.pos.z = 128.0;
        }

        let (chunk_x, chunk_z) = self.pos.chunk_pos();
        chunk_x != self.last_chunk_x || chunk_z != self.last_chunk_z
    }

    /// Sends the keep alive packet to the client and updates `last_keep_alive_sent`
    pub fn send_keep_alive(&mut self) {
        let keep_alive = CKeepAlive {
            id: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
        }
        .encode();
        self.client.send_packet(&keep_alive);
        self.last_keep_alive_sent = Instant::now();
    }

    pub fn get_direction(&self) -> BlockDirection {
        match ((self.yaw / 90.0 + 0.5).floor() as i32 & 3).unsigned_abs() {
            0 => BlockDirection::South,
            1 => BlockDirection::West,
            2 => BlockDirection::North,
            3 => BlockDirection::East,
            _ => BlockDirection::South,
        }
    }

    pub fn get_facing(&self) -> BlockFacing {
        self.get_facing_pitch_cutoff(70.0)
    }

    pub fn get_block_facing(&self) -> BlockFacing {
        self.get_facing_pitch_cutoff(45.0)
    }

    fn get_facing_pitch_cutoff(&self, pitch_cutoff: f32) -> BlockFacing {
        let yaw = self.yaw.rem_euclid(360.0);
        if self.pitch <= -pitch_cutoff {
            BlockFacing::Up
        } else if self.pitch >= pitch_cutoff {
            BlockFacing::Down
        } else if (45.0..=135.0).contains(&yaw) {
            BlockFacing::West
        } else if (135.0..=225.0).contains(&yaw) {
            BlockFacing::North
        } else if (225.0..=315.0).contains(&yaw) {
            BlockFacing::East
        } else {
            BlockFacing::South
        }
    }

    pub fn teleport(&mut self, pos: PlayerPos) {
        // Prevent from teleporting to Infinity or NaN
        if !pos.is_valid() {
            self.send_error_message(messages::INVALID_TELEPORT_COORDINATES);
            return;
        }

        let player_position_and_look = CPlayerPositionAndLook {
            x: pos.x,
            y: pos.y,
            z: pos.z,
            yaw: 0f32,
            pitch: 0f32,
            flags: 0x08 | 0x10, // pitch and yaw are relative
            teleport_id: 0,
            dismount_vehicle: false,
        }
        .encode();
        self.pos = pos;
        self.client.send_packet(&player_position_and_look);
    }

    /// Sends the `ChatMessage` packet containing the raw json data.
    /// Position 0: chat (chat box)
    pub fn send_raw_chat(&self, sender: u128, message: String) {
        let chat_message = CChatMessage {
            message,
            sender,
            position: 0,
        }
        .encode();
        self.client.send_packet(&chat_message);
    }

    /// Sends a raw chat message to the player
    pub fn send_chat_message(&self, sender: u128, message: &[ChatComponent]) {
        let json = json!({ "text": "", "extra": message }).to_string();
        self.send_raw_chat(sender, json);
    }

    pub fn send_no_permission_message(&self) {
        self.send_error_message(messages::PERMISSION_DENIED);
    }

    /// Sends the player a light purple system message (`message` is not in json format)
    pub fn send_worldedit_message(&self, message: &str) {
        self.send_color_message(ColorCode::LightPurple, message)
    }

    pub fn worldedit_set_first_position(&mut self, pos: BlockPos) {
        self.send_worldedit_message(&messages::selection_first(pos.x, pos.y, pos.z));
        self.first_position = Some(pos);
        self.worldedit_send_cui(&format!("p|0|{}|{}|{}|0", pos.x, pos.y, pos.z));
    }

    pub fn worldedit_set_second_position(&mut self, pos: BlockPos) {
        self.send_worldedit_message(&messages::selection_second(pos.x, pos.y, pos.z));
        self.second_position = Some(pos);
        self.worldedit_send_cui(&format!("p|1|{}|{}|{}|0", pos.x, pos.y, pos.z));
    }

    pub fn worldedit_send_cui(&self, message: &str) {
        let cui_plugin_message = CPluginMessage {
            channel: String::from("worldedit:cui"),
            data: Vec::from(message.as_bytes()),
        }
        .encode();
        self.client.send_packet(&cui_plugin_message);
    }

    pub(crate) fn entity_teleport_packet(&self) -> PacketEncoder {
        CEntityTeleport {
            entity_id: self.entity_id as i32,
            x: self.pos.x,
            y: self.pos.y,
            z: self.pos.z,
            yaw: self.yaw,
            pitch: self.pitch,
            on_ground: self.on_ground,
        }
        .encode()
    }

    /// Sends the player the disconnect packet, it is still up to the player to end the network stream.
    pub fn kick(&self, reason: String) {
        let disconnect = CDisconnect { reason }.encode();
        self.client.send_packet(&disconnect);
    }

    pub fn update_player_abilities(&self) {
        let player_abilities = CPlayerAbilities {
            flags: 0x0D | ((self.flying as u8) << 1),
            fly_speed: 0.05 * self.fly_speed,
            fov_modifier: 0.1,
        }
        .encode();
        self.client.send_packet(&player_abilities);
    }

    pub fn set_gamemode(&mut self, gamemode: Gamemode) {
        self.gamemode = gamemode;
        let change_game_state = CChangeGameState {
            reason: CChangeGameStateReason::ChangeGamemode,
            value: self.gamemode.get_id() as f32,
        }
        .encode();
        self.client.send_packet(&change_game_state);
    }

    pub fn has_permission(&self, node: &str) -> bool {
        if let Some(cache) = &self.permissions_cache {
            if let Some(val) = cache.get_node_val(node) {
                val > 0
            } else {
                // Node is not in database
                false
            }
        } else {
            // An authenticated proxy deployment must not grant all permissions
            // when its LuckPerms configuration is accidentally omitted.
            CONFIG.velocity.is_none()
        }
    }
    pub(super) fn accept_chat_message(&mut self) -> bool {
        if self.chat_window.elapsed() >= std::time::Duration::from_secs(1) {
            self.chat_window = Instant::now();
            self.chat_count = 0;
        }
        if self.chat_count >= 5 {
            return false;
        }
        self.chat_count += 1;
        true
    }

    /// Refresh off the plot thread. A stale cache stops granting permissions
    /// after 30 seconds even if the database is slow or unavailable.
    pub(super) fn refresh_permissions(&mut self) {
        if self.permissions_cache.is_none() {
            return;
        }
        if let Some(receiver) = &self.permissions_refresh {
            match receiver.try_recv() {
                Ok(result) => {
                    self.permissions_cache = Some(result.unwrap_or_else(|error| {
                        tracing::error!("LuckPerms refresh failed: {error}; denying permissions");
                        PlayerPermissionsCache::default()
                    }));
                    self.permissions_refresh = None;
                    self.next_permissions_refresh =
                        Instant::now() + std::time::Duration::from_secs(25);
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.permissions_cache = Some(PlayerPermissionsCache::default());
                    self.permissions_refresh = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
            }
        }
        if Instant::now() >= self.next_permissions_refresh {
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            let uuid = self.uuid;
            self.permissions_refresh = Some(receiver);
            std::thread::spawn(move || {
                let _ = sender.send(permissions::load_player_cache(uuid));
            });
        }
    }

    pub fn can_use_commands(&self) -> bool {
        if permissions::dedicated_permissions() {
            self.has_permission("mchprs.access.commands")
        } else {
            self.permissions_cache.is_some() || CONFIG.velocity.is_none()
        }
    }

    pub fn numeric_permission_limit(&self, prefix: &str) -> Option<usize> {
        self.permissions_cache
            .as_ref()
            .and_then(|cache| cache.numeric_limit(prefix))
    }

    pub fn can_edit_plot(&self, owner: Option<u128>, plot: (i32, i32)) -> bool {
        if permissions::dedicated_permissions() && !self.has_permission("mchprs.build") {
            return false;
        }
        match owner {
            Some(owner) => {
                let (x, z) = plot;
                owner == self.uuid
                    || crate::plot::database::is_plot_member(x, z, self.uuid)
                    || self.has_permission("plots.admin.interact.other")
            }
            None => self.has_permission("plots.admin.interact.unowned"),
        }
    }

    pub fn can_build_action(&self, action: &str, owner: Option<u128>, plot: (i32, i32)) -> bool {
        self.can_edit_plot(owner, plot)
            && (!permissions::dedicated_permissions()
                || self.has_permission(&format!("mchprs.build.{action}")))
    }

    pub fn chat_prefix(&self) -> Option<&str> {
        if !permissions::ranked_chat() {
            return None;
        }
        Some(
            self.permissions_cache
                .as_ref()
                .and_then(|cache| cache.rank_profile.as_ref())
                .map_or(Rank::Player.default_prefix(), |profile| {
                    profile
                        .prefix
                        .as_deref()
                        .unwrap_or_else(|| profile.rank.default_prefix())
                }),
        )
    }

    /// Require a granted permission without the permissive no-LuckPerms fallback.
    pub fn has_explicit_permission(&self, node: &str) -> bool {
        self.permissions_cache
            .as_ref()
            .and_then(|cache| cache.get_node_val(node))
            .is_some_and(|value| value > 0)
    }

    pub fn open_container(
        &mut self,
        pos: BlockPos,
        inventory: &[InventoryEntry],
        container_type: ContainerType,
    ) {
        self.next_window_id = self.next_window_id % 100 + 1;
        let title = match container_type {
            ContainerType::Barrel => "container.barrel",
            ContainerType::Furnace => "container.furnace",
            ContainerType::Hopper => "container.hopper",
            ContainerType::Chest => "container.chest",
        };
        self.open_container = Some(crate::container::OpenContainer {
            pos,
            ty: container_type,
            window_id: self.next_window_id,
            state_id: 0,
            cursor: None,
            drag: None,
            last_contents: Vec::new(),
        });
        let open_window = COpenWindow {
            window_id: self.next_window_id as i32,
            window_type: container_type.window_type() as i32,
            window_title: serde_json::json!({"translate":title}).to_string(),
        }
        .encode();
        self.client.send_packet(&open_window);
        self.send_container_contents(inventory, true);
    }

    pub fn send_container_contents(&mut self, inventory: &[InventoryEntry], force: bool) {
        let Some(menu) = &mut self.open_container else {
            return;
        };
        let mut slots = crate::container::inventory_slots(inventory, menu.ty.num_slots() as usize);
        slots.extend_from_slice(&self.inventory[9..45]);
        let signature = crate::container::signature(&slots, &menu.cursor);
        if !force && signature == menu.last_contents {
            return;
        }
        menu.last_contents = signature;
        menu.state_id = (menu.state_id + 1) & 32767;
        let window_items = CWindowItems {
            window_id: menu.window_id,
            state_id: menu.state_id,
            slot_data: slots
                .iter()
                .map(|s| s.as_ref().map(crate::container::slot_data))
                .collect(),
            carried_item: menu.cursor.as_ref().map(crate::container::slot_data),
        }
        .encode();
        self.client.send_packet(&window_items);
    }

    /// Return the carried stack before leaving a menu or saving the player.
    pub fn close_container(&mut self) -> Option<(BlockPos, ContainerType)> {
        let mut menu = self.open_container.take()?;
        let indices: Vec<_> = (9..45).collect();
        crate::container::insert(&mut self.inventory, &mut menu.cursor, &indices);
        self.client.send_packet(
            &CCloseWindow {
                window_id: menu.window_id,
            }
            .encode(),
        );
        self.client.send_packet(
            &CWindowItems {
                window_id: 0,
                state_id: 0,
                slot_data: self
                    .inventory
                    .iter()
                    .map(|s| s.as_ref().map(crate::container::slot_data))
                    .collect(),
                carried_item: None,
            }
            .encode(),
        );
        Some((menu.pos, menu.ty))
    }

    pub fn set_inventory_slot(&mut self, slot: u32, item: Option<ItemStack>) {
        let set_slot = CSetSlot {
            window_id: 0,
            state_id: 0,
            slot: slot as i16,
            slot_data: item.as_ref().map(|item| SlotData {
                item_id: item.item_type.get_id() as i32,
                item_count: item.count as i8,
                nbt: item.nbt.clone(),
            }),
        }
        .encode();
        self.client.send_packet(&set_slot);

        self.inventory[slot as usize] = item;
    }
}

pub trait PacketSender {
    fn send_packet(&self, data: &PacketEncoder);

    /// Sends the `ChatMessage` packet containing the raw json data.
    /// Position 1: system message (chat box)
    fn send_raw_system_message(&self, message: String) {
        let chat_message = CChatMessage {
            message,
            sender: 0,
            position: 1,
        }
        .encode();
        self.send_packet(&chat_message);
    }

    /// Sends the player a red system message (`message` is not in json format)
    fn send_error_message(&self, message: &str) {
        self.send_color_message(ColorCode::Red, message)
    }

    /// Sends the player a yellow system message (`message` is not in json format)
    fn send_system_message(&self, message: &str) {
        self.send_color_message(ColorCode::Yellow, message);
    }

    /// Sends the player a given color message (`message` is not in json format)
    fn send_color_message(&self, col: ColorCode, message: impl Display) {
        self.send_raw_system_message(
            json!({
                "text": message.to_string(),
                "color": col
            })
            .to_string(),
        )
    }
}

impl PacketSender for PlayerPacketSender {
    fn send_packet(&self, data: &PacketEncoder) {
        self.send_packet(data);
    }
}

impl PacketSender for Player {
    fn send_packet(&self, data: &PacketEncoder) {
        self.client.send_packet(data);
    }
}

#[cfg(test)]
mod coordinate_security_tests {
    use super::*;
    #[test]
    fn player_coordinates_reject_nonfinite_and_extreme_positions() {
        for value in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            30_000_001.0,
            -30_000_001.0,
        ] {
            assert!(!PlayerPos::new(value, 64.0, 0.0).is_valid());
            assert!(!PlayerPos::new(0.0, 64.0, value).is_valid());
        }
        assert!(!PlayerPos::new(0.0, 2049.0, 0.0).is_valid());
        assert!(!PlayerPos::new(0.0, f64::NAN, 0.0).is_valid());
        assert!(PlayerPos::new(-128.0, 128.0, 128.0).is_valid());
    }
}
