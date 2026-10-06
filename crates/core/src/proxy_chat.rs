//! Public chat transport over the player's authenticated Velocity connection.
use crate::config::CONFIG;
use crate::messages;
use crate::player::{PacketSender, Player};
use mchprs_network::packets::clientbound::{CPluginMessage, ClientBoundPacket};
use mchprs_network::PlayerConn;

pub const CHANNEL: &str = "redstonefun:chat";
const VERSION: u8 = 1;
const HELLO: u8 = 0;
const READY: u8 = 1;
const CHAT: u8 = 2;

#[derive(Default)]
pub struct Session {
    token: Option<[u8; 16]>,
    sequence: u64,
}

pub fn enabled() -> bool {
    CONFIG.proxy_chat
}

impl Session {
    pub fn receive(&mut self, data: &[u8], client: &PlayerConn) {
        if !enabled() || data.len() != 18 || data[0] != VERSION || data[1] != HELLO {
            return;
        }
        let token: [u8; 16] = data[2..].try_into().unwrap();
        // Retransmitted handshakes must not reset the sequence counter.
        if self.token != Some(token) {
            self.token = Some(token);
            self.sequence = 0;
        }
        let mut response = data.to_vec();
        response[1] = READY;
        client.send_packet(
            &CPluginMessage {
                channel: CHANNEL.into(),
                data: response,
            }
            .encode(),
        );
    }

    fn message(&mut self, text: &str) -> Option<CPluginMessage> {
        let token = self.token?;
        if text.trim().is_empty()
            || text.encode_utf16().count() > 256
            || text.chars().any(|c| c.is_control() || c == '\u{00a7}')
        {
            return None;
        }
        if self.sequence >= i64::MAX as u64 {
            return None;
        }
        self.sequence += 1;
        let mut data = Vec::with_capacity(28 + text.len());
        data.extend_from_slice(&[VERSION, CHAT]);
        data.extend_from_slice(&token);
        data.extend_from_slice(&self.sequence.to_be_bytes());
        data.extend_from_slice(&(text.len() as u16).to_be_bytes());
        data.extend_from_slice(text.as_bytes());
        Some(CPluginMessage {
            channel: CHANNEL.into(),
            data,
        })
    }
}

pub fn send(player: &mut Player, text: &str) {
    if let Some(packet) = player.proxy_chat.message(text) {
        player.client.send_packet(&packet.encode());
    } else {
        // Never fall back to local broadcast: that silently splits the network chat.
        player.send_system_message(messages::SHARED_CHAT_UNAVAILABLE);
    }
}
