use super::super::Plot;
use crate::messages;
use crate::player::PacketSender;
use crate::redpilator::{CompilerOptions, PublicationMode};
use mchprs_network::packets::clientbound::{CTabComplete, CTabCompleteMatch};

/// Build tab-completion suggestions for Redpilator commands.
///
/// Inputs: The completion request ID and full command text.
///
/// Find the current argument, choose compile flags or publication values, filter already-used/nonmatching
/// candidates and return UTF-16 replacement offsets required by the protocol.
pub(in crate::plot) fn complete_redpilator(id: i32, text: &str) -> Option<CTabComplete> {
    let boundary = text.rfind(char::is_whitespace)?;
    let start = boundary + text[boundary..].chars().next()?.len_utf8();
    let words: Vec<_> = text[..start].split_whitespace().collect();
    if !matches!(words.first(), Some(&"/rp" | &"/redpilator")) {
        return None;
    }
    let candidates: &[&str] = match words.get(1) {
        Some(&"compile" | &"c") => &[
            "--publish-all",
            "--publish-redstone",
            "--publish-io",
            "--io-only",
            "--update",
            "--llvm",
            "-i",
            "-u",
        ],
        Some(&"publish") if words.len() == 2 => &["all", "redstone", "io"],
        _ => return None,
    };
    let prefix = &text[start..];
    // Offer only flags that match the current word and have not already been used.
    let matches = candidates
        .iter()
        .copied()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::World;
    use mchprs_blocks::{
        BlockDirection, BlockFace, BlockPos,
        blocks::{Block, Lever, LeverFace},
    };

    #[test]
    fn publication_command_requires_permission_and_catches_up_immediately() {
        let (mut plot, _peer) = crate::plot::client_sync_tests::fixture(false);
        plot.owner = Some(plot.players[0].uuid);
        let lever = BlockPos::new(34, 21, 32);
        plot.world.set_block(lever.offset(BlockFace::Bottom), Block::Stone {});
        plot.world.set_block(
            lever,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
            },
        );
        plot.redpilator
            .compile(
                &plot.world,
                (BlockPos::new(32, 19, 30), BlockPos::new(36, 23, 34)),
                CompilerOptions::default(),
                vec![],
                Default::default(),
            )
            .unwrap();
        plot.redpilator.try_on_use_block(lever).unwrap();
        plot.players[0].set_test_permissions(&[]);
        plot.handle_command(0, "/rp", vec!["publish", "io"]);
        assert!(matches!(plot.world.get_block(lever), Block::Lever { lever } if !lever.powered));

        plot.players[0].set_test_permissions(&["commands.redpilator.compile"]);
        for mode in ["io", "redstone", "all"] {
            plot.handle_command(0, "/rp", vec!["publish", mode]);
            assert_eq!(plot.redpilator.current_flags().unwrap().publication, PublicationMode::parse(mode));
            assert_eq!(plot.redpilator.current_flags().unwrap().io_only, mode == "io");
            assert!(matches!(plot.world.get_block(lever), Block::Lever { lever } if lever.powered));
        }
        plot.handle_command(0, "/rp", vec!["publish", "invalid"]);
        assert_eq!(plot.redpilator.current_flags().unwrap().publication, Some(PublicationMode::All));
        plot.redpilator.try_reset(&mut plot.world, (lever, lever)).unwrap();
        plot.handle_command(0, "/rp", vec!["publish", "io"]);
        assert!(!plot.redpilator.is_active());
    }

    #[test]
    fn reset_and_off_validate_arguments_permissions_and_cache_lifecycle() {
        for command in ["reset", "off", "r"] {
            let (mut plot, _peer) = crate::plot::client_sync_tests::fixture(false);
            plot.owner = Some(plot.players[0].uuid);
            let pos = BlockPos::new(34, 21, 32);
            plot.world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
            plot.world.set_block(pos, Block::from_name("stone_button").unwrap());
            let bounds = (pos - BlockPos::new(2, 2, 2), pos + BlockPos::new(2, 2, 2));
            plot.redpilator
                .compile(&plot.world, bounds, CompilerOptions::default(), vec![], Default::default())
                .unwrap();

            plot.players[0].set_test_permissions(&[]);
            plot.handle_command(0, "/rp", vec![command]);
            assert!(plot.redpilator.is_active(), "{command} bypassed reset permission");
            plot.players[0].set_test_permissions(&["commands.redpilator.reset"]);
            for args in [
                vec![command, "--bad"],
                vec![command, "--clear"],
                vec![command, "--clear", "--clear"],
                vec![command, "--clear", "extra"],
            ] {
                plot.handle_command(0, "/rp", args);
                assert!(plot.redpilator.is_active(), "invalid {command} arguments stopped execution");
            }
            plot.handle_command(0, "/rp", vec![command]);
            assert!(!plot.redpilator.is_active());
            assert!(plot.redpilator_cache.is_none(), "{command} retained the warm checkpoint");
            plot.handle_command(0, "/rp", vec![command, "--bad"]);
            assert!(plot.redpilator_cache.is_none());

            plot.redpilator
                .compile(&plot.world, bounds, CompilerOptions::default(), vec![], Default::default())
                .unwrap();
            plot.players[0].set_test_permissions(&[]);
            plot.handle_command(0, "/rp", vec!["clear"]);
            assert!(plot.redpilator.is_active(), "clear bypassed reset permission");
            plot.players[0].set_test_permissions(&["commands.redpilator.reset"]);
            for args in [vec!["clear", "--clear"], vec!["clear", "extra"]] {
                plot.handle_command(0, "/rp", args);
                assert!(plot.redpilator.is_active(), "invalid clear arguments stopped execution");
            }
            plot.handle_command(0, "/rp", vec!["clear"]);
            assert!(!plot.redpilator.is_active());
            assert!(plot.redpilator_cache.is_none(), "clear retained the warm checkpoint");
        }
    }

    #[test]
    fn reset_commands_cancel_pending_compilation_without_repopulating_cache() {
        for command in ["reset", "r", "off", "clear"] {
            let (mut plot, _peer) = crate::plot::client_sync_tests::fixture(false);
            plot.owner = Some(plot.players[0].uuid);
            plot.players[0].set_test_permissions(&["commands.redpilator.reset"]);
            plot.start_redpilator_with_budget(CompilerOptions::default(), None);
            assert!(plot.compilation.is_some());
            plot.handle_command(0, "/rp", vec![command]);
            assert!(!plot.compiling_redpilator());
            plot.wait_for_redpilator();
            assert!(!plot.redpilator.is_active());
            assert!(plot.redpilator_cache.is_none());
        }
    }
}

impl Plot {
    /// Handles a command that starts with `/redpilator` or `/rp`
    ///
    /// Inputs: The player index, parsed subcommand and remaining argument words; permission checks run in the command
    /// dispatcher.
    ///
    /// Parse compile options/budget before resetting and starting a worker; handle reset/cache clearing or
    /// immediate publication changes through the shared compiler lifecycle APIs.
    pub(super) fn handle_redpilator_command(&mut self, player: usize, command: &str, args: &[&str]) {
        match command {
            "compile" | "c" => {
                if self.compilation.is_some() {
                    self.players[player].send_error_message(messages::REDPILATOR_COMPILATION_BUSY);
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
                let budget = match self.players[player].redpilator_compile_budget() {
                    Ok(budget) => budget,
                    Err(error) => {
                        self.players[player].send_error_message(&error);
                        return;
                    }
                };
                if let Err(error) = self.reset_redpilator() {
                    self.players[player].send_error_message(&error.to_string());
                    return;
                }
                self.start_redpilator_with_budget(options, budget);
            }
            "reset" | "r" | "off" | "clear" => {
                if !args.is_empty() {
                    self.players[player].send_error_message(messages::REDPILATOR_RESET_USAGE);
                    return;
                }
                if let Err(error) = self.reset_redpilator() {
                    self.players[player].send_error_message(&error.to_string());
                    return;
                }
                self.clear_redpilator_cache();
                self.players[player].send_system_message(messages::REDPILATOR_PLOT_CACHE_CLEARED);
                if command == "clear" {
                    match crate::redpilator::clear_llvm_cache() {
                        Ok(summary) => self.players[player].send_system_message(&summary),
                        Err(error) => self.players[player].send_error_message(&error),
                    }
                }
            }
            "publish" => {
                let mode = match args {
                    [value] => PublicationMode::parse(value),
                    _ => None,
                };
                let Some(mode) = mode else {
                    self.players[player].send_error_message(messages::REDPILATOR_PUBLICATION_USAGE);
                    return;
                };
                if !self.redpilator.set_publication_mode(mode) {
                    self.players[player].send_error_message(messages::REDPILATOR_NOT_ACTIVE);
                    return;
                }
                if let Err(error) = self.redpilator.try_flush(&mut self.world) {
                    self.players[player].send_error_message(&error.to_string());
                    return;
                }
                self.scoreboard
                    .set_redpilator_options(&self.players, self.redpilator.current_flags().unwrap());
                self.players[player].send_system_message(&messages::redpilator_publication(mode.name()));
            }
            _ => self.players[player].send_error_message(messages::REDPILATOR_INVALID_ARGUMENT),
        }
    }
}
