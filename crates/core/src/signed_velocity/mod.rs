//! SignedVelocity 1.5.0 decisions, received only through authenticated forwarding.
use crate::utils::HyphenatedUUID;
use anyhow::{Result, bail, ensure};
use byteorder::{BigEndian, ReadBytesExt};
use std::collections::VecDeque;

pub(crate) const CHANNEL: &str = "signedvelocity:main";
const QUEUE_LIMIT: usize = 16;

// Used for authenticated forwarding when RedBox is disabled.

enum Decision {
    Allowed,
    Cancel,
    Modify(String),
}

impl Decision {
    fn apply(self, message: String) -> Option<String> {
        match self {
            Self::Allowed => Some(message),
            Self::Cancel => None,
            Self::Modify(replacement) => Some(replacement),
        }
    }
}

#[derive(Default)]
struct Queue {
    decisions: VecDeque<Decision>,
    messages: VecDeque<String>,
}

#[derive(Default)]
pub(crate) struct Session {
    chat: Queue,
    commands: Queue,
}

impl Session {
    pub(crate) fn submit(&mut self, message: String) -> Result<Option<String>> {
        let command = message.starts_with('/');
        validate_message(&message, command)?;
        let queue = if command { &mut self.commands } else { &mut self.chat };
        if let Some(decision) = queue.decisions.pop_front() {
            return Ok(decision.apply(message));
        }
        ensure!(queue.messages.len() < QUEUE_LIMIT, "SignedVelocity input queue overflow");
        queue.messages.push_back(message);
        Ok(None)
    }

    pub(crate) fn receive(&mut self, mut data: &[u8], uuid: u128) -> Result<Option<String>> {
        ensure!(read_utf(&mut data)? == HyphenatedUUID(uuid).to_string(), "SignedVelocity UUID mismatch");
        let command = match read_utf(&mut data)?.as_str() {
            "CHAT_RESULT" => false,
            "COMMAND_RESULT" => true,
            _ => bail!("Invalid SignedVelocity source"),
        };
        let decision = match read_utf(&mut data)?.as_str() {
            "ALLOWED" => Decision::Allowed,
            "CANCEL" => Decision::Cancel,
            "MODIFY" => {
                let replacement = read_utf(&mut data)?;
                // Commands on the plugin channel exclude the leading protocol slash.
                let replacement = if command { format!("/{replacement}") } else { replacement };
                ensure!(command || !replacement.starts_with('/'), "SignedVelocity chat cannot become a command");
                validate_message(&replacement, command)?;
                Decision::Modify(replacement)
            }
            _ => bail!("Invalid SignedVelocity decision"),
        };
        ensure!(data.is_empty(), "Trailing SignedVelocity data");
        let queue = if command { &mut self.commands } else { &mut self.chat };
        if let Some(message) = queue.messages.pop_front() {
            return Ok(decision.apply(message));
        }
        ensure!(queue.decisions.len() < QUEUE_LIMIT, "SignedVelocity decision queue overflow");
        queue.decisions.push_back(decision);
        Ok(None)
    }
}

fn validate_message(message: &str, command: bool) -> Result<()> {
    ensure!(
        message.encode_utf16().count() <= if command { 32767 } else { 256 },
        "SignedVelocity message too long"
    );
    ensure!(
        !message.chars().any(char::is_control),
        "SignedVelocity message contains control characters"
    );
    Ok(())
}

// Java writeUTF uses modified UTF-8: supplementary characters are UTF-16 surrogates.
fn read_utf(data: &mut &[u8]) -> Result<String> {
    let length = data.read_u16::<BigEndian>()? as usize;
    ensure!(data.len() >= length, "Truncated SignedVelocity string");
    let (mut encoded, rest) = data.split_at(length);
    *data = rest;
    let mut units = Vec::with_capacity(length);
    while !encoded.is_empty() {
        let first = encoded.read_u8()?;
        let unit = match first {
            1..=0x7f => u16::from(first),
            0xc0..=0xdf => {
                let second = encoded.read_u8()?;
                ensure!(second & 0xc0 == 0x80, "Invalid modified UTF-8 continuation");
                let unit = (u16::from(first & 0x1f) << 6) | u16::from(second & 0x3f);
                ensure!(unit == 0 || unit >= 0x80, "Overlong modified UTF-8");
                unit
            }
            0xe0..=0xef => {
                let second = encoded.read_u8()?;
                let third = encoded.read_u8()?;
                ensure!(second & 0xc0 == 0x80 && third & 0xc0 == 0x80, "Invalid modified UTF-8 continuation");
                let unit = (u16::from(first & 0x0f) << 12) | (u16::from(second & 0x3f) << 6) | u16::from(third & 0x3f);
                ensure!(unit >= 0x800, "Overlong modified UTF-8");
                unit
            }
            _ => bail!("Invalid modified UTF-8 leading byte"),
        };
        units.push(unit);
    }
    Ok(String::from_utf16(&units)?)
}

#[cfg(test)]
pub(crate) mod tests;
