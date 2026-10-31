//! Native RedBox backend bridge.
//!
//! RedBox runs on the Velocity proxy and owns the network-wide commands such as
//! `/home`, `/warp`, `/tp`, `/spawn`, and `/back`. This module is the backend
//! half of that bridge and mirrors the RedBox Paper plugin: it reports player
//! positions and chat to the proxy, and applies the teleports and gamemode
//! changes the proxy requests.
//!
//! Every payload uses the Java `DataOutputStream` primitives the proxy reads,
//! on the `redbox:main` plugin channel.

use crate::config::CONFIG;
use crate::player::{Gamemode, PlayerPos};
use crate::utils::HyphenatedUUID;

pub(crate) const CHANNEL: &str = "redbox:main";

const STATE: &str = "STATE";
/// MROWW has no death mechanic, so the DEATH message is defined for protocol
/// completeness but not emitted yet.
#[allow(dead_code)]
const DEATH: &str = "DEATH";
const TELEPORT: &str = "TELEPORT";
const CHAT: &str = "CHAT";
const GAMEMODE: &str = "GAMEMODE";
const PERMISSIONS_REQUEST: &str = "PERMISSIONS_REQUEST";
const PERMISSIONS: &str = "PERMISSIONS";
const HELD_ITEM: &str = "HELD_ITEM";
const PT_CLICK: &str = "PT_CLICK";
const PT_RUN: &str = "PT_RUN";

/// Whether the RedBox bridge is active.
pub(crate) fn enabled() -> bool {
    CONFIG.redbox
}

/// A position exactly as the proxy stores it.
#[derive(Clone, Debug, PartialEq)]
pub struct Position {
    pub world: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
}

impl Position {
    pub(crate) fn new(world: impl Into<String>, pos: PlayerPos, yaw: f32, pitch: f32) -> Self {
        Self {
            world: world.into(),
            x: pos.x,
            y: pos.y,
            z: pos.z,
            yaw,
            pitch,
        }
    }

    pub(crate) fn player_pos(&self) -> PlayerPos {
        PlayerPos::new(self.x, self.y, self.z)
    }

    /// Rejects coordinates or rotations the server cannot represent.
    pub(crate) fn is_valid(&self) -> bool {
        self.player_pos().is_valid() && self.yaw.is_finite() && self.pitch.is_finite()
    }
}

/// Backend -> proxy: a player's current world position.
pub(crate) fn encode_player_state(uuid: u128, position: &Position) -> Vec<u8> {
    encode(STATE, |data| {
        write_utf(data, &HyphenatedUUID(uuid).to_string());
        write_position(data, position);
    })
}

/// Backend -> proxy: a player died, used by `/back`.
///
/// MROWW never kills players, so this stays ready for a future death event.
#[allow(dead_code)]
pub(crate) fn encode_death(uuid: u128, position: &Position) -> Vec<u8> {
    encode(DEATH, |data| {
        write_utf(data, &HyphenatedUUID(uuid).to_string());
        write_position(data, position);
    })
}

/// Backend -> proxy: a chat message the proxy must broadcast network-wide.
pub(crate) fn encode_chat(uuid: u128, message: &str) -> Vec<u8> {
    encode(CHAT, |data| {
        write_utf(data, &HyphenatedUUID(uuid).to_string());
        write_utf(data, message);
    })
}

/// Backend -> proxy: ask for the player's effective LuckPerms data.
pub(crate) fn encode_permissions_request(uuid: u128) -> Vec<u8> {
    encode(PERMISSIONS_REQUEST, |data| {
        write_utf(data, &HyphenatedUUID(uuid).to_string());
    })
}

/// Backend -> proxy: the item a player currently holds.
pub(crate) fn encode_held_item(uuid: u128, item: &str) -> Vec<u8> {
    encode(HELD_ITEM, |data| {
        write_utf(data, &HyphenatedUUID(uuid).to_string());
        write_utf(data, item);
    })
}

/// Backend -> proxy: the player clicked with the held item. 0 = LMB, 1 = RMB.
pub(crate) fn encode_pt_click(uuid: u128, button: i32, item: &str) -> Vec<u8> {
    encode(PT_CLICK, |data| {
        write_utf(data, &HyphenatedUUID(uuid).to_string());
        data.extend_from_slice(&button.to_be_bytes());
        write_utf(data, item);
    })
}

/// A message the proxy can send to the backend.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Incoming {
    Teleport {
        uuid: u128,
        position: Position,
    },
    Gamemode {
        uuid: u128,
        mode: i32,
    },
    Permissions {
        uuid: u128,
        group: String,
        prefix: String,
        suffix: String,
        permissions: Vec<(String, bool)>,
    },
    /// Proxy -> backend: run a command as the player (powertool).
    PtRun {
        uuid: u128,
        command: String,
    },
}

impl Incoming {
    pub(crate) fn uuid(&self) -> u128 {
        match self {
            Self::Teleport { uuid, .. } | Self::Gamemode { uuid, .. } | Self::Permissions { uuid, .. } | Self::PtRun { uuid, .. } => *uuid,
        }
    }
}

/// Decodes one plugin message payload, rejecting anything malformed.
pub(crate) fn decode(data: &[u8]) -> Result<Incoming, String> {
    let mut data = data;
    let kind = read_utf(&mut data)?;
    let uuid = read_uuid(&mut data)?;
    let message = match kind.as_str() {
        TELEPORT => Incoming::Teleport {
            uuid,
            position: read_position(&mut data)?,
        },
        GAMEMODE => Incoming::Gamemode {
            uuid,
            mode: read_i32(&mut data)?,
        },
        PERMISSIONS => {
            let group = read_utf(&mut data)?;
            let prefix = read_utf(&mut data)?;
            let suffix = read_utf(&mut data)?;
            let count = read_i32(&mut data)?;
            if !(0..=65_536).contains(&count) {
                return Err("Invalid RedBox permission count".into());
            }
            let mut permissions = Vec::with_capacity(count as usize);
            for _ in 0..count {
                let permission = read_utf(&mut data)?;
                permissions.push((permission, read_bool(&mut data)?));
            }
            Incoming::Permissions {
                uuid,
                group,
                prefix,
                suffix,
                permissions,
            }
        }
        PT_RUN => {
            let command = read_utf(&mut data)?;
            if command.is_empty() || command.len() > 1024 || command.chars().any(char::is_control) || command.starts_with('/') {
                return Err("Invalid RedBox powertool command".into());
            }
            Incoming::PtRun { uuid, command }
        }
        _ => return Err(format!("Unknown RedBox message type '{kind}'")),
    };
    if !data.is_empty() {
        return Err("Trailing RedBox data".into());
    }
    Ok(message)
}

/// Maps a bridge gamemode id to a server gamemode. Survival (0) is unsupported.
pub(crate) fn gamemode_from_id(mode: i32) -> Option<Gamemode> {
    match mode {
        0 => Some(Gamemode::Survival),
        1 => Some(Gamemode::Creative),
        2 => Some(Gamemode::Adventure),
        3 => Some(Gamemode::Spectator),
        _ => None,
    }
}

fn encode(kind: &str, block: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let mut data = Vec::new();
    write_utf(&mut data, kind);
    block(&mut data);
    data
}

fn write_position(data: &mut Vec<u8>, position: &Position) {
    write_utf(data, &position.world);
    data.extend_from_slice(&position.x.to_be_bytes());
    data.extend_from_slice(&position.y.to_be_bytes());
    data.extend_from_slice(&position.z.to_be_bytes());
    data.extend_from_slice(&position.yaw.to_be_bytes());
    data.extend_from_slice(&position.pitch.to_be_bytes());
}

fn read_position(data: &mut &[u8]) -> Result<Position, String> {
    Ok(Position {
        world: read_utf(data)?,
        x: read_f64(data)?,
        y: read_f64(data)?,
        z: read_f64(data)?,
        yaw: read_f32(data)?,
        pitch: read_f32(data)?,
    })
}

fn read_uuid(data: &mut &[u8]) -> Result<u128, String> {
    read_utf(data)?
        .parse::<HyphenatedUUID>()
        .map(|uuid| uuid.0)
        .map_err(|_| "Invalid RedBox UUID".into())
}

fn read_i32(data: &mut &[u8]) -> Result<i32, String> {
    Ok(i32::from_be_bytes(next(data, 4)?.try_into().unwrap()))
}

fn read_bool(data: &mut &[u8]) -> Result<bool, String> {
    Ok(next(data, 1)?[0] != 0)
}

fn read_f32(data: &mut &[u8]) -> Result<f32, String> {
    Ok(f32::from_be_bytes(next(data, 4)?.try_into().unwrap()))
}

fn read_f64(data: &mut &[u8]) -> Result<f64, String> {
    Ok(f64::from_be_bytes(next(data, 8)?.try_into().unwrap()))
}

fn next(data: &mut &[u8], length: usize) -> Result<Vec<u8>, String> {
    if data.len() < length {
        return Err("Truncated RedBox payload".into());
    }
    let (head, tail) = data.split_at(length);
    *data = tail;
    Ok(head.to_vec())
}

/// Java `DataOutputStream.writeUTF`: 2-byte length plus modified UTF-8.
fn write_utf(data: &mut Vec<u8>, text: &str) {
    let bytes = modified_utf8(text);
    data.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
    data.extend_from_slice(&bytes);
}

fn modified_utf8(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len());
    for unit in text.encode_utf16() {
        match unit {
            0 => bytes.extend_from_slice(&[0xc0, 0x80]),
            1..=0x7f => bytes.push(unit as u8),
            0x80..=0x7ff => {
                bytes.push(0xc0 | (unit >> 6) as u8);
                bytes.push(0x80 | (unit & 0x3f) as u8);
            }
            _ => {
                bytes.push(0xe0 | (unit >> 12) as u8);
                bytes.push(0x80 | ((unit >> 6) & 0x3f) as u8);
                bytes.push(0x80 | (unit & 0x3f) as u8);
            }
        }
    }
    bytes
}

/// Java `DataInputStream.readUTF`, including modified UTF-8.
fn read_utf(data: &mut &[u8]) -> Result<String, String> {
    let length = {
        let bytes = next(data, 2)?;
        u16::from_be_bytes(bytes.try_into().unwrap()) as usize
    };
    let encoded = next(data, length)?;
    let mut units = Vec::with_capacity(encoded.len());
    let mut index = 0;
    while index < encoded.len() {
        let first = encoded[index];
        index += 1;
        let unit = match first {
            0x01..=0x7f => u16::from(first),
            0xc0..=0xdf => {
                if index >= encoded.len() {
                    return Err("Truncated modified UTF-8".into());
                }
                let second = encoded[index];
                index += 1;
                if second & 0xc0 != 0x80 {
                    return Err("Invalid modified UTF-8 continuation".into());
                }
                let unit = (u16::from(first & 0x1f) << 6) | u16::from(second & 0x3f);
                if unit != 0 && unit < 0x80 {
                    return Err("Overlong modified UTF-8".into());
                }
                unit
            }
            0xe0..=0xef => {
                if index + 1 >= encoded.len() {
                    return Err("Truncated modified UTF-8".into());
                }
                let second = encoded[index];
                let third = encoded[index + 1];
                index += 2;
                if second & 0xc0 != 0x80 || third & 0xc0 != 0x80 {
                    return Err("Invalid modified UTF-8 continuation".into());
                }
                let unit = (u16::from(first & 0x0f) << 12) | (u16::from(second & 0x3f) << 6) | u16::from(third & 0x3f);
                if unit < 0x800 {
                    return Err("Overlong modified UTF-8".into());
                }
                unit
            }
            _ => return Err("Invalid modified UTF-8 leading byte".into()),
        };
        units.push(unit);
    }
    String::from_utf16(&units).map_err(|_| "Invalid RedBox UTF-16".into())
}

#[cfg(test)]
mod tests;
