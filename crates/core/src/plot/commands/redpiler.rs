use super::super::Plot;
use crate::messages;
use crate::player::PacketSender;
use crate::redpiler::CompilerOptions;
use mchprs_network::packets::clientbound::{CTabComplete, CTabCompleteMatch};
pub(in crate::plot) fn complete_redpiler(id: i32, text: &str) -> Option<CTabComplete> {
    let boundary = text.rfind(char::is_whitespace)?;
    let start = boundary + text[boundary..].chars().next()?.len_utf8();
    let words: Vec<_> = text[..start].split_whitespace().collect();
    if !matches!(words.first(), Some(&"/rp" | &"/redpiler")) {
        return None;
    }
    match words.get(1) {
        Some(&"compile" | &"c") => {}
        _ => return None,
    }
    let prefix = &text[start..];
    let matches = ["--io-only", "--update", "-i", "-u"]
        .into_iter()
        .filter(|flag| flag.starts_with(prefix) && !words[2..].contains(flag))
        .map(|flag| CTabCompleteMatch {
            match_: flag.to_owned(),
            tooltip: None,
        })
        .collect();
    Some(CTabComplete {
        id,
        start: text[..start].encode_utf16().count() as i32,
        length: prefix.encode_utf16().count() as i32,
        matches,
    })
}

impl Plot {
    /// Handles a command that starts with `/redpiler` or `/rp`
    pub(super) fn handle_redpiler_command(&mut self, player: usize, command: &str, args: &[&str]) {
        match command {
            "compile" | "c" => {
                if self.compilation.is_some() {
                    self.players[player].send_error_message(messages::REDPILER_COMPILATION_BUSY);
                    return;
                }
                let args = args.join(" ");
                let options = match CompilerOptions::parse(&args) {
                    Ok(options) => options,
                    Err(error) => {
                        self.players[player].send_error_message(&error);
                        return;
                    }
                };
                if let Err(error) = self.reset_redpiler() {
                    self.players[player].send_error_message(&error.to_string());
                    return;
                }
                self.start_redpiler(options);
            }
            "reset" | "r" => {
                if let Err(error) = self.reset_redpiler() {
                    self.players[player].send_error_message(&error.to_string());
                }
            }
            _ => self.players[player].send_error_message(messages::REDPILER_INVALID_ARGUMENT),
        }
    }
}
