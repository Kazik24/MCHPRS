//! SignedVelocity 1.5.0 decisions, received only through authenticated forwarding.
use crate::utils::HyphenatedUUID;
use anyhow::{Result, bail, ensure};
use byteorder::{BigEndian, ReadBytesExt};
use std::collections::VecDeque;

pub(crate) const CHANNEL: &str = "signedvelocity:main";
const QUEUE_LIMIT: usize = 16;

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
        let queue = if command {
            &mut self.commands
        } else {
            &mut self.chat
        };
        if let Some(decision) = queue.decisions.pop_front() {
            return Ok(decision.apply(message));
        }
        ensure!(
            queue.messages.len() < QUEUE_LIMIT,
            "SignedVelocity input queue overflow"
        );
        queue.messages.push_back(message);
        Ok(None)
    }

    pub(crate) fn receive(&mut self, mut data: &[u8], uuid: u128) -> Result<Option<String>> {
        ensure!(
            read_utf(&mut data)? == HyphenatedUUID(uuid).to_string(),
            "SignedVelocity UUID mismatch"
        );
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
                let replacement = if command {
                    format!("/{replacement}")
                } else {
                    replacement
                };
                ensure!(
                    command || !replacement.starts_with('/'),
                    "SignedVelocity chat cannot become a command"
                );
                validate_message(&replacement, command)?;
                Decision::Modify(replacement)
            }
            _ => bail!("Invalid SignedVelocity decision"),
        };
        ensure!(data.is_empty(), "Trailing SignedVelocity data");
        let queue = if command {
            &mut self.commands
        } else {
            &mut self.chat
        };
        if let Some(message) = queue.messages.pop_front() {
            return Ok(decision.apply(message));
        }
        ensure!(
            queue.decisions.len() < QUEUE_LIMIT,
            "SignedVelocity decision queue overflow"
        );
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
                ensure!(
                    second & 0xc0 == 0x80 && third & 0xc0 == 0x80,
                    "Invalid modified UTF-8 continuation"
                );
                let unit = (u16::from(first & 0x0f) << 12)
                    | (u16::from(second & 0x3f) << 6)
                    | u16::from(third & 0x3f);
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
pub(crate) mod tests {
    use super::*;

    pub(crate) fn payload(
        uuid: u128,
        source: &str,
        decision: &str,
        replacement: Option<&str>,
    ) -> Vec<u8> {
        let uuid = HyphenatedUUID(uuid).to_string();
        let mut data = Vec::new();
        for field in [
            Some(uuid.as_str()),
            Some(source),
            Some(decision),
            replacement,
        ]
        .into_iter()
        .flatten()
        {
            assert!(field.is_ascii());
            data.extend_from_slice(&(field.len() as u16).to_be_bytes());
            data.extend_from_slice(field.as_bytes());
        }
        data
    }

    #[test]
    fn chat_and_command_decisions_are_fifo_and_independent_in_both_arrival_orders() {
        for input_first in [false, true] {
            for (decision, replacement, expected) in [
                ("ALLOWED", None, Some("original")),
                ("CANCEL", None, None),
                ("MODIFY", Some("approved"), Some("approved")),
            ] {
                let mut session = Session::default();
                let data = payload(1, "CHAT_RESULT", decision, replacement);
                let result = if input_first {
                    assert_eq!(session.submit("original".into()).unwrap(), None);
                    session.receive(&data, 1).unwrap()
                } else {
                    assert_eq!(session.receive(&data, 1).unwrap(), None);
                    session.submit("original".into()).unwrap()
                };
                assert_eq!(result.as_deref(), expected);

                session
                    .receive(&payload(1, "CHAT_RESULT", "CANCEL", None), 1)
                    .unwrap();
                session
                    .receive(
                        &payload(1, "COMMAND_RESULT", "MODIFY", Some("/load build.schem")),
                        1,
                    )
                    .unwrap();
                assert_eq!(
                    session.submit("/help".into()).unwrap().as_deref(),
                    Some("//load build.schem")
                );
                assert_eq!(session.submit("blocked".into()).unwrap(), None);
                session
                    .receive(&payload(1, "CHAT_RESULT", "ALLOWED", None), 1)
                    .unwrap();
                assert_eq!(
                    session.submit("next".into()).unwrap().as_deref(),
                    Some("next")
                );
            }
        }
    }

    #[test]
    fn java_modified_utf8_handles_polish_text_emoji_and_nul() {
        // DataOutputStream.writeUTF("żółw 🐈\0"), including its byte length.
        let mut bytes = &b"\x00\x10\xc5\xbc\xc3\xb3\xc5\x82w \xed\xa0\xbd\xed\xb0\x88\xc0\x80"[..];
        assert_eq!(read_utf(&mut bytes).unwrap(), "żółw 🐈\0");
        assert!(bytes.is_empty());

        let mut data = payload(1, "CHAT_RESULT", "MODIFY", None);
        data.extend_from_slice(b"\x00\x0e\xc5\xbc\xc3\xb3\xc5\x82w \xed\xa0\xbd\xed\xb0\x88");
        let mut session = Session::default();
        session.receive(&data, 1).unwrap();
        assert_eq!(
            session.submit("original".into()).unwrap().as_deref(),
            Some("żółw 🐈")
        );
    }

    #[test]
    fn invalid_payloads_cannot_change_another_player_or_execute_chat_as_a_command() {
        let valid = payload(1, "CHAT_RESULT", "ALLOWED", None);
        for length in 0..valid.len() {
            assert!(Session::default().receive(&valid[..length], 1).is_err());
        }
        for data in [
            payload(2, "CHAT_RESULT", "ALLOWED", None),
            payload(1, "UNKNOWN", "ALLOWED", None),
            payload(1, "CHAT_RESULT", "UNKNOWN", None),
            payload(1, "CHAT_RESULT", "MODIFY", Some("/op player")),
            payload(1, "CHAT_RESULT", "MODIFY", Some("line\nfeed")),
            payload(1, "CHAT_RESULT", "MODIFY", Some(&"x".repeat(257))),
            [valid.clone(), vec![0]].concat(),
        ] {
            assert!(Session::default().receive(&data, 1).is_err());
        }
        for bytes in [
            &b"\x00\x01\x00"[..],
            &b"\x00\x02\xc0\xaf"[..],
            &b"\x00\x03\xe0\x80\x80"[..],
            &b"\x00\x02\xc3w"[..],
            &b"\x00\x03\xed\xa0\xbd"[..],
            &b"\x00\x04\xf0\x9f\x90\x88"[..],
        ] {
            assert!(read_utf(&mut &bytes[..]).is_err());
        }
    }

    #[test]
    fn missing_decisions_never_allow_input_and_both_queues_are_bounded() {
        for source in ["CHAT_RESULT", "COMMAND_RESULT"] {
            let input = if source == "CHAT_RESULT" {
                "hello"
            } else {
                "/help"
            };
            let mut waiting = Session::default();
            let mut ready = Session::default();
            let data = payload(1, source, "ALLOWED", None);
            for _ in 0..QUEUE_LIMIT {
                assert_eq!(waiting.submit(input.into()).unwrap(), None);
                assert_eq!(ready.receive(&data, 1).unwrap(), None);
            }
            assert!(waiting.submit(input.into()).is_err());
            assert!(ready.receive(&data, 1).is_err());
            for _ in 0..QUEUE_LIMIT {
                assert_eq!(waiting.receive(&data, 1).unwrap().as_deref(), Some(input));
                assert_eq!(ready.submit(input.into()).unwrap().as_deref(), Some(input));
            }
        }
    }
}
