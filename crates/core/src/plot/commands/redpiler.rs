use super::super::Plot;
use crate::messages;
use crate::player::PacketSender;
use crate::redpiler::{CompilerOptions, PublicationMode};
use mchprs_network::packets::clientbound::{CTabComplete, CTabCompleteMatch};

pub(in crate::plot) fn complete_redpiler(id: i32, text: &str) -> Option<CTabComplete> {
    let boundary = text.rfind(char::is_whitespace)?;
    let start = boundary + text[boundary..].chars().next()?.len_utf8();
    let words: Vec<_> = text[..start].split_whitespace().collect();
    if !matches!(words.first(), Some(&"/rp" | &"/redpiler")) {
        return None;
    }
    let candidates: &[&str] = match words.get(1) {
        Some(&"compile" | &"c") => &["--publish-all", "--publish-redstone", "--publish-io", "--io-only", "--update", "--llvm", "-i", "-u"],
        Some(&"publish") if words.len() == 2 => &["all", "redstone", "io"],
        _ => return None,
    };
    let prefix = &text[start..];
    // Offer only flags that match the current word and have not already been used.
    let matches = candidates.iter().copied()
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
            "publish" => {
                let mode = match args {
                    [value] => PublicationMode::parse(value),
                    _ => None,
                };
                let Some(mode) = mode else {
                    self.players[player].send_error_message(messages::REDPILER_PUBLICATION_USAGE);
                    return;
                };
                if !self.redpiler.set_publication_mode(mode) {
                    self.players[player].send_error_message(messages::REDPILER_NOT_ACTIVE);
                    return;
                }
                if let Err(error) = self.redpiler.try_flush(&mut self.world) {
                    self.players[player].send_error_message(&error.to_string());
                    return;
                }
                self.scoreboard.set_redpiler_options(&self.players, self.redpiler.current_flags().unwrap());
                self.players[player].send_system_message(&messages::redpiler_publication(mode.name()));
            }
            _ => self.players[player].send_error_message(messages::REDPILER_INVALID_ARGUMENT),
        }
    }
}
