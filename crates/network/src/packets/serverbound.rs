use super::{DecodeResult, PackedPos, PacketDecoderExt, SlotData};

pub trait ServerBoundPacketHandler {
    fn handle_teleport_confirm(&mut self, _packet: STeleportConfirm, _player_idx: usize) {}
    fn handle_handshake(&mut self, _packet: SHandshake, _player_idx: usize) {}
    fn handle_request(&mut self, _packet: SRequest, _player_idx: usize) {}
    fn handle_ping(&mut self, _packet: SPing, _player_idx: usize) {}
    fn handle_login_acknowledged(&mut self, _packet: SLoginAcknowledged, _idx: usize) {}
    fn handle_known_packs(&mut self, _packet: SKnownPacks, _idx: usize) {}
    fn handle_configuration_finished(&mut self, _packet: SConfigurationFinished, _idx: usize) {}
    fn handle_login_start(&mut self, _packet: SLoginStart, _player_idx: usize) {}
    fn handle_login_plugin_response(&mut self, _packet: SLoginPluginResponse, _idx: usize) {}
    fn handle_chat_message(&mut self, _packet: SChatMessage, _player_idx: usize) {}
    fn handle_client_settings(&mut self, _packet: SClientSettings, _player_idx: usize) {}
    fn handle_tab_complete(&mut self, _packet: STabComplete, _player_idx: usize) {}
    fn handle_plugin_message(&mut self, _packet: SPluginMessage, _player_idx: usize) {}
    fn handle_keep_alive(&mut self, _packet: SKeepAlive, _player_idx: usize) {}
    fn handle_player_position(&mut self, _packet: SPlayerPosition, _player_idx: usize) {}
    fn handle_player_position_and_rotation(
        &mut self,
        _packet: SPlayerPositionAndRotation,
        _player_idx: usize,
    ) {
    }
    fn handle_player_rotation(&mut self, _packet: SPlayerRotation, _player_idx: usize) {}
    fn handle_player_movement(&mut self, _packet: SPlayerMovement, _player_idx: usize) {}
    fn handle_player_abilities(&mut self, _packet: SPlayerAbilities, _player_idx: usize) {}
    fn handle_player_digging(&mut self, _packet: SPlayerDigging, _player_idx: usize) {}
    fn handle_entity_action(&mut self, _packet: SEntityAction, _player_idx: usize) {}
    fn handle_animation(&mut self, _packet: SAnimation, _player_idx: usize) {}
    fn handle_use_item(&mut self, _packet: SUseItem, _player_idx: usize) {}
    fn handle_player_block_placement(&mut self, _packet: SPlayerBlockPlacemnt, _player_idx: usize) {
    }
    fn handle_held_item_change(&mut self, _packet: SHeldItemChange, _player_idx: usize) {}
    fn handle_pick_item_from_block(&mut self, _packet: SPickItemFromBlock, _player_idx: usize) {}
    fn handle_pick_item_from_entity(&mut self, _packet: SPickItemFromEntity, _player_idx: usize) {}
    fn handle_update_command_block(&mut self, _packet: SUpdateCommandBlock, _player_idx: usize) {}
    fn handle_container_click(&mut self, _packet: SContainerClick, _player_idx: usize) {}
    fn handle_container_close(&mut self, _packet: SContainerClose, _player_idx: usize) {}
    fn handle_creative_inventory_action(
        &mut self,
        _packet: SCreativeInventoryAction,
        _player_idx: usize,
    ) {
    }
    fn handle_update_sign(&mut self, _packet: SUpdateSign, _player_idx: usize) {}
    fn handle_unknown(&mut self, _packet: SUnknown, _player_idx: usize) {}
}

pub trait ServerBoundPacket: Send {
    fn decode<T: PacketDecoderExt>(reader: &mut T) -> DecodeResult<Self>
    where
        Self: Sized;

    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, player_idx: usize);
}

pub struct SLoginPluginResponse {
    pub message_id: i32,
    pub successful: bool,
    pub data: Vec<u8>,
}

impl ServerBoundPacket for SLoginPluginResponse {
    fn decode<T: PacketDecoderExt>(reader: &mut T) -> DecodeResult<Self> {
        let message_id = reader.read_varint()?;
        let successful = reader.read_bool()?;
        let data = PacketDecoderExt::read_to_end(reader)?;
        if data.len() > 32768 || (!successful && !data.is_empty()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid login forwarding response",
            )
            .into());
        }
        Ok(Self {
            message_id,
            successful,
            data,
        })
    }

    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, idx: usize) {
        handler.handle_login_plugin_response(*self, idx);
    }
}

pub struct SUnknown;

pub struct STeleportConfirm {
    pub id: i32,
}

impl ServerBoundPacket for STeleportConfirm {
    fn decode<T: PacketDecoderExt>(decoder: &mut T) -> DecodeResult<Self> {
        let id = decoder.read_varint()?;
        if id < 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "negative teleport confirmation id",
            )
            .into());
        }
        Ok(Self { id })
    }

    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, player_idx: usize) {
        handler.handle_teleport_confirm(*self, player_idx);
    }
}

pub struct SPickItemFromBlock {
    pub pos: PackedPos,
    pub include_data: bool,
}

pub struct SUpdateCommandBlock {
    pub pos: PackedPos,
    pub command: String,
    pub mode: i32,
    pub flags: u8,
}

impl ServerBoundPacket for SUpdateCommandBlock {
    fn decode<T: PacketDecoderExt>(reader: &mut T) -> DecodeResult<Self> {
        let packet = Self {
            pos: reader.read_position()?,
            command: reader.read_string()?,
            mode: reader.read_varint()?,
            flags: reader.read_unsigned_byte()?,
        };
        if !(0..=2).contains(&packet.mode)
            || packet.flags & !7 != 0
            || packet.command.chars().count() > 32767
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid command block update",
            )
            .into());
        }
        Ok(packet)
    }
    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, player_idx: usize) {
        handler.handle_update_command_block(*self, player_idx);
    }
}

impl ServerBoundPacket for SPickItemFromBlock {
    fn decode<T: PacketDecoderExt>(reader: &mut T) -> DecodeResult<Self> {
        Ok(Self {
            pos: reader.read_position()?,
            include_data: reader.read_bool()?,
        })
    }
    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, player_idx: usize) {
        handler.handle_pick_item_from_block(*self, player_idx);
    }
}

pub struct SPickItemFromEntity {
    pub entity_id: i32,
    pub include_data: bool,
}

impl ServerBoundPacket for SPickItemFromEntity {
    fn decode<T: PacketDecoderExt>(reader: &mut T) -> DecodeResult<Self> {
        Ok(Self {
            entity_id: reader.read_varint()?,
            include_data: reader.read_bool()?,
        })
    }
    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, player_idx: usize) {
        handler.handle_pick_item_from_entity(*self, player_idx);
    }
}

impl ServerBoundPacket for SUnknown {
    fn decode<T: PacketDecoderExt>(_: &mut T) -> DecodeResult<Self> {
        Ok(SUnknown)
    }

    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, player_idx: usize) {
        handler.handle_unknown(*self, player_idx);
    }
}

pub struct SHandshake {
    pub protocol_version: i32,
    pub server_address: String,
    pub server_port: u16,
    pub next_state: i32,
}

impl ServerBoundPacket for SHandshake {
    fn decode<T: PacketDecoderExt>(decoder: &mut T) -> DecodeResult<Self> {
        Ok(SHandshake {
            protocol_version: decoder.read_varint()?,
            server_address: decoder.read_string()?,
            server_port: decoder.read_unsigned_short()?,
            next_state: decoder.read_varint()?,
        })
    }

    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, player_idx: usize) {
        handler.handle_handshake(*self, player_idx);
    }
}

pub struct SRequest;

impl ServerBoundPacket for SRequest {
    fn decode<T: PacketDecoderExt>(_decoder: &mut T) -> DecodeResult<Self> {
        Ok(SRequest)
    }

    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, player_idx: usize) {
        handler.handle_request(*self, player_idx);
    }
}

pub struct SPing {
    pub payload: i64,
}

impl ServerBoundPacket for SPing {
    fn decode<T: PacketDecoderExt>(decoder: &mut T) -> DecodeResult<Self> {
        Ok(SPing {
            payload: decoder.read_long()?,
        })
    }

    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, player_idx: usize) {
        handler.handle_ping(*self, player_idx);
    }
}

pub struct SLoginStart {
    pub name: String,
    pub uuid: u128,
}

impl ServerBoundPacket for SLoginStart {
    fn decode<T: PacketDecoderExt>(decoder: &mut T) -> DecodeResult<Self> {
        Ok(SLoginStart {
            name: decoder.read_string()?,
            uuid: decoder.read_uuid()?,
        })
    }

    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, player_idx: usize) {
        handler.handle_login_start(*self, player_idx);
    }
}

pub struct SChatMessage {
    pub message: String,
}

impl ServerBoundPacket for SChatMessage {
    fn decode<T: PacketDecoderExt>(decoder: &mut T) -> DecodeResult<Self> {
        let message = decoder.read_string()?;
        decoder.read_long()?;
        decoder.read_long()?;
        if decoder.read_bool()? {
            decoder.read_bytes(256)?;
        }
        decoder.read_varint()?;
        decoder.read_bytes(3)?;
        decoder.read_unsigned_byte()?; // last-seen checksum
        Ok(SChatMessage { message })
    }

    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, player_idx: usize) {
        handler.handle_chat_message(*self, player_idx);
    }
}

pub struct SClientSettings {
    pub locale: String,
    pub view_distance: i8,
    pub chat_mode: i32,
    pub chat_colors: bool,
    pub displayed_skin_parts: u8,
    pub main_hand: i32,
    pub enable_text_filtering: bool,
    pub allow_server_listings: bool,
}

impl ServerBoundPacket for SClientSettings {
    fn decode<T: PacketDecoderExt>(decoder: &mut T) -> DecodeResult<Self> {
        Ok(SClientSettings {
            locale: decoder.read_string()?,
            view_distance: decoder.read_byte()?,
            chat_mode: decoder.read_varint()?,
            chat_colors: decoder.read_bool()?,
            displayed_skin_parts: decoder.read_unsigned_byte()?,
            main_hand: decoder.read_varint()?,
            enable_text_filtering: decoder.read_bool()?,
            allow_server_listings: decoder.read_bool()?,
        })
    }

    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, player_idx: usize) {
        handler.handle_client_settings(*self, player_idx);
    }
}

pub struct STabComplete {
    pub transaction_id: i32,
    pub text: String,
}

impl ServerBoundPacket for STabComplete {
    fn decode<T: PacketDecoderExt>(decoder: &mut T) -> DecodeResult<Self> {
        Ok(STabComplete {
            transaction_id: decoder.read_varint()?,
            text: decoder.read_string()?,
        })
    }

    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, player_idx: usize) {
        handler.handle_tab_complete(*self, player_idx);
    }
}

pub struct SPluginMessage {
    pub channel: String,
    pub data: Vec<u8>,
}

impl ServerBoundPacket for SPluginMessage {
    fn decode<T: PacketDecoderExt>(decoder: &mut T) -> DecodeResult<Self> {
        Ok(SPluginMessage {
            channel: decoder.read_string()?,
            data: PacketDecoderExt::read_to_end(decoder)?,
        })
    }

    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, player_idx: usize) {
        handler.handle_plugin_message(*self, player_idx);
    }
}

pub struct SKeepAlive {
    pub id: i64,
}

impl ServerBoundPacket for SKeepAlive {
    fn decode<T: PacketDecoderExt>(decoder: &mut T) -> DecodeResult<Self> {
        Ok(SKeepAlive {
            id: decoder.read_long()?,
        })
    }

    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, player_idx: usize) {
        handler.handle_keep_alive(*self, player_idx);
    }
}

pub struct SPlayerPosition {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub on_ground: bool,
}

impl ServerBoundPacket for SPlayerPosition {
    fn decode<T: PacketDecoderExt>(decoder: &mut T) -> DecodeResult<Self> {
        Ok(SPlayerPosition {
            x: decoder.read_double()?,
            y: decoder.read_double()?,
            z: decoder.read_double()?,
            on_ground: decoder.read_unsigned_byte()? & 1 != 0,
        })
    }

    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, player_idx: usize) {
        handler.handle_player_position(*self, player_idx);
    }
}

pub struct SPlayerPositionAndRotation {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
}

impl ServerBoundPacket for SPlayerPositionAndRotation {
    fn decode<T: PacketDecoderExt>(decoder: &mut T) -> DecodeResult<Self> {
        Ok(SPlayerPositionAndRotation {
            x: decoder.read_double()?,
            y: decoder.read_double()?,
            z: decoder.read_double()?,
            yaw: decoder.read_float()?,
            pitch: decoder.read_float()?,
            on_ground: decoder.read_unsigned_byte()? & 1 != 0,
        })
    }

    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, player_idx: usize) {
        handler.handle_player_position_and_rotation(*self, player_idx);
    }
}

pub struct SPlayerRotation {
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
}

impl ServerBoundPacket for SPlayerRotation {
    fn decode<T: PacketDecoderExt>(decoder: &mut T) -> DecodeResult<Self> {
        Ok(SPlayerRotation {
            yaw: decoder.read_float()?,
            pitch: decoder.read_float()?,
            on_ground: decoder.read_unsigned_byte()? & 1 != 0,
        })
    }

    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, player_idx: usize) {
        handler.handle_player_rotation(*self, player_idx);
    }
}

pub struct SPlayerMovement {
    pub on_ground: bool,
}

impl ServerBoundPacket for SPlayerMovement {
    fn decode<T: PacketDecoderExt>(decoder: &mut T) -> DecodeResult<Self> {
        Ok(SPlayerMovement {
            on_ground: decoder.read_unsigned_byte()? & 1 != 0,
        })
    }

    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, player_idx: usize) {
        handler.handle_player_movement(*self, player_idx);
    }
}

pub struct SPlayerAbilities {
    pub is_flying: bool,
}

impl ServerBoundPacket for SPlayerAbilities {
    fn decode<T: PacketDecoderExt>(decoder: &mut T) -> DecodeResult<Self> {
        Ok(SPlayerAbilities {
            is_flying: decoder.read_byte()? != 0,
        })
    }

    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, player_idx: usize) {
        handler.handle_player_abilities(*self, player_idx);
    }
}

pub struct SPlayerDigging {
    pub status: i32,
    pub pos: PackedPos,
    pub face: i8,
    pub sequence: i32,
}

impl ServerBoundPacket for SPlayerDigging {
    fn decode<T: PacketDecoderExt>(decoder: &mut T) -> DecodeResult<Self> {
        let status = decoder.read_varint()?;
        let location = decoder.read_position()?;
        let face = decoder.read_byte()?;
        Ok(SPlayerDigging {
            pos: location,
            status,
            face,
            sequence: decoder.read_varint()?,
        })
    }

    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, player_idx: usize) {
        handler.handle_player_digging(*self, player_idx);
    }
}

pub struct SEntityAction {
    pub entity_id: i32,
    pub action_id: i32,
    pub jump_boost: i32,
}

impl ServerBoundPacket for SEntityAction {
    fn decode<T: PacketDecoderExt>(decoder: &mut T) -> DecodeResult<Self> {
        Ok(SEntityAction {
            entity_id: decoder.read_varint()?,
            action_id: decoder.read_varint()?,
            jump_boost: decoder.read_varint()?,
        })
    }

    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, player_idx: usize) {
        handler.handle_entity_action(*self, player_idx);
    }
}

pub struct SAnimation {
    pub hand: i32,
}

impl ServerBoundPacket for SAnimation {
    fn decode<T: PacketDecoderExt>(decoder: &mut T) -> DecodeResult<Self> {
        Ok(SAnimation {
            hand: decoder.read_varint()?,
        })
    }

    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, player_idx: usize) {
        handler.handle_animation(*self, player_idx);
    }
}

pub struct SPlayerBlockPlacemnt {
    pub hand: i32,
    pub pos: PackedPos,
    pub face: i32,
    pub cursor_x: f32,
    pub cursor_y: f32,
    pub cursor_z: f32,
    pub inside_block: bool,
    pub sequence: i32,
}

impl ServerBoundPacket for SPlayerBlockPlacemnt {
    fn decode<T: PacketDecoderExt>(decoder: &mut T) -> DecodeResult<Self> {
        let hand = decoder.read_varint()?;
        let location = decoder.read_position()?;
        let face = decoder.read_varint()?;
        let cursor_x = decoder.read_float()?;
        let cursor_y = decoder.read_float()?;
        let cursor_z = decoder.read_float()?;
        let inside_block = decoder.read_bool()?;
        decoder.read_bool()?; // world border hit
        Ok(SPlayerBlockPlacemnt {
            pos: location,
            hand,
            face,
            cursor_x,
            cursor_y,
            cursor_z,
            inside_block,
            sequence: decoder.read_varint()?,
        })
    }

    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, player_idx: usize) {
        handler.handle_player_block_placement(*self, player_idx);
    }
}

pub struct SUseItem {
    pub hand: i32,
    pub sequence: i32,
    pub yaw: f32,
    pub pitch: f32,
}

impl ServerBoundPacket for SUseItem {
    fn decode<T: PacketDecoderExt>(decoder: &mut T) -> DecodeResult<Self> {
        Ok(Self {
            hand: decoder.read_varint()?,
            sequence: decoder.read_varint()?,
            yaw: decoder.read_float()?,
            pitch: decoder.read_float()?,
        })
    }

    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, player_idx: usize) {
        handler.handle_use_item(*self, player_idx);
    }
}

pub struct SHeldItemChange {
    pub slot: i16,
}

impl ServerBoundPacket for SHeldItemChange {
    fn decode<T: PacketDecoderExt>(decoder: &mut T) -> DecodeResult<Self> {
        Ok(SHeldItemChange {
            slot: decoder.read_short()?,
        })
    }

    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, player_idx: usize) {
        handler.handle_held_item_change(*self, player_idx);
    }
}

pub struct SCreativeInventoryAction {
    pub slot: i16,
    pub clicked_item: Option<SlotData>,
}

/// Protocol 770 carries hashed predictions, not full item stacks, in menu clicks.
pub struct SContainerClick {
    pub window_id: i32,
    pub state_id: i32,
    pub slot: i16,
    pub button: i8,
    pub mode: i32,
}

fn invalid_container(message: &str) -> super::PacketDecodeError {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message).into()
}

fn read_hashed_slot<T: PacketDecoderExt>(reader: &mut T) -> DecodeResult<()> {
    if !reader.read_bool()? {
        return Ok(());
    }
    let id = reader.read_varint()?;
    let count = reader.read_varint()?;
    if !(0..crate::generated::ITEM_COUNT).contains(&id) || !(1..=127).contains(&count) {
        return Err(invalid_container("invalid hashed item"));
    }
    let mut seen = std::collections::HashSet::new();
    for removed in [false, true] {
        let n = reader.read_varint()?;
        if !(0..=crate::generated::COMPONENT_COUNT).contains(&n) {
            return Err(invalid_container("too many hashed components"));
        }
        for _ in 0..n {
            let component = reader.read_varint()?;
            if !(0..crate::generated::COMPONENT_COUNT).contains(&component)
                || !seen.insert(component)
            {
                return Err(invalid_container("invalid hashed component"));
            }
            if !removed {
                reader.read_int()?;
            }
        }
    }
    Ok(())
}

impl ServerBoundPacket for SContainerClick {
    fn decode<T: PacketDecoderExt>(reader: &mut T) -> DecodeResult<Self> {
        let packet = Self {
            window_id: reader.read_varint()?,
            state_id: reader.read_varint()?,
            slot: reader.read_short()?,
            button: reader.read_byte()?,
            mode: reader.read_varint()?,
        };
        if packet.window_id < 0 || packet.state_id < 0 || !(0..=6).contains(&packet.mode) {
            return Err(invalid_container("invalid container click"));
        }
        let count = reader.read_varint()?;
        if !(0..=128).contains(&count) {
            return Err(invalid_container("too many changed slots"));
        }
        let mut seen = std::collections::HashSet::new();
        for _ in 0..count {
            let slot = reader.read_short()?;
            if slot < 0 || !seen.insert(slot) {
                return Err(invalid_container("invalid changed slot"));
            }
            read_hashed_slot(reader)?;
        }
        read_hashed_slot(reader)?;
        Ok(packet)
    }
    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, idx: usize) {
        handler.handle_container_click(*self, idx);
    }
}

pub struct SContainerClose {
    pub window_id: i32,
}
impl ServerBoundPacket for SContainerClose {
    fn decode<T: PacketDecoderExt>(reader: &mut T) -> DecodeResult<Self> {
        Ok(Self {
            window_id: reader.read_varint()?,
        })
    }
    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, idx: usize) {
        handler.handle_container_close(*self, idx);
    }
}

impl ServerBoundPacket for SCreativeInventoryAction {
    fn decode<T: PacketDecoderExt>(decoder: &mut T) -> DecodeResult<Self> {
        let slot = decoder.read_short()?;
        let clicked_item = super::components::read_untrusted_slot(decoder)?;
        Ok(SCreativeInventoryAction { slot, clicked_item })
    }

    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, player_idx: usize) {
        handler.handle_creative_inventory_action(*self, player_idx);
    }
}

pub struct SUpdateSign {
    pub pos: PackedPos,
    pub lines: [String; 4],
    pub front: bool,
}

impl ServerBoundPacket for SUpdateSign {
    fn decode<T: PacketDecoderExt>(decoder: &mut T) -> DecodeResult<Self> {
        let pos = decoder.read_position()?;
        let front = decoder.read_bool()?;
        let lines = [
            decoder.read_string()?,
            decoder.read_string()?,
            decoder.read_string()?,
            decoder.read_string()?,
        ];
        Ok(SUpdateSign { pos, lines, front })
    }

    fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, player_idx: usize) {
        handler.handle_update_sign(*self, player_idx);
    }
}

macro_rules! empty_packet {
    ($name:ident,$handler:ident) => {
        pub struct $name;
        impl ServerBoundPacket for $name {
            fn decode<T: PacketDecoderExt>(_: &mut T) -> DecodeResult<Self> {
                Ok(Self)
            }
            fn handle(self: Box<Self>, handler: &mut dyn ServerBoundPacketHandler, idx: usize) {
                handler.$handler(*self, idx);
            }
        }
    };
}
empty_packet!(SLoginAcknowledged, handle_login_acknowledged);
empty_packet!(SKnownPacks, handle_known_packs);
empty_packet!(SConfigurationFinished, handle_configuration_finished);
pub struct SChatCommand {
    pub message: String,
}
impl ServerBoundPacket for SChatCommand {
    fn decode<T: PacketDecoderExt>(r: &mut T) -> DecodeResult<Self> {
        let message = format!("/{}", r.read_string()?);
        r.read_long()?;
        r.read_long()?;
        let n = r.read_varint()?;
        if !(0..=64).contains(&n) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid signature count",
            )
            .into());
        }
        for _ in 0..n {
            r.read_string()?;
            r.read_bytes(256)?;
        }
        r.read_varint()?;
        r.read_bytes(3)?;
        r.read_unsigned_byte()?;
        Ok(Self { message })
    }
    fn handle(self: Box<Self>, h: &mut dyn ServerBoundPacketHandler, idx: usize) {
        h.handle_chat_message(
            SChatMessage {
                message: self.message,
            },
            idx,
        );
    }
}
