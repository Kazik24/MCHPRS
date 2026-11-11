use super::super::{Plot, visuals::PublicationSetting};
use crate::config::CONFIG;
use crate::messages;
use crate::player::PacketSender;
use crate::redpilator::CompilerOptions;
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
    let command = words.first().copied()?;
    let candidates: &[&str] = match words.get(1) {
        Some(&"compile" | &"c") if matches!(command, "/rp" | "/redpilator") => &[
            "--publish-all",
            "--publish-redstone",
            "--publish-io",
            "--io-only",
            "--update",
            "--llvm",
            "-i",
            "-u",
        ],
        Some(&"publish") if words.len() == 2 && matches!(command, "/visual" | "/rp" | "/redpilator") => &PublicationSetting::CHOICES,
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
    use crate::redpilator::PublicationMode;
    use crate::world::World;
    use mchprs_blocks::{
        BlockDirection, BlockFace, BlockPos,
        blocks::{Block, Lever, LeverFace},
    };

    #[test]
    fn publication_command_requires_permission_and_catches_up_immediately() {
        use crate::plot::client_test_utils::decode_blocks;
        use mchprs_network::test_support::read_frame;
        use mchprs_save_data::plot_data::WorldSendRate;

        let (mut plot, mut peer) = crate::plot::client_sync_tests::fixture(false);
        plot.owner = Some(plot.players[0].uuid);
        plot.world.set_screen_only(false);
        plot.world_send_rate = WorldSendRate(0);
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

        plot.players[0].set_test_permissions(&["commands.visual", "commands.visual.set"]);
        plot.handle_command(0, "/visual", vec!["publish", "io"]);
        assert_eq!(plot.publication_setting, None);
        assert!(matches!(plot.world.get_block(lever), Block::Lever { lever } if !lever.powered));

        plot.players[0].set_test_permissions(&["commands.redpilator.compile", "commands.visual", "commands.visual.set"]);
        for (command, mode) in [("/visual", "io"), ("/rp", "redstone"), ("/redpilator", "all")] {
            plot.handle_command(0, command, vec!["publish", mode]);
            assert_eq!(plot.publication_setting, PublicationSetting::parse(mode));
            assert_eq!(plot.redpilator.current_flags().unwrap().publication, PublicationMode::parse(mode));
            assert_eq!(plot.redpilator.current_flags().unwrap().io_only, mode == "io");
            assert!(matches!(plot.world.get_block(lever), Block::Lever { lever } if lever.powered));
            if command == "/visual" {
                let powered = plot.world.get_block_raw(lever);
                loop {
                    let (id, mut frame) = read_frame(&mut peer, false).unwrap();
                    if matches!(id, 0x08 | 0x4d) && decode_blocks(id, &mut frame).contains(&(lever, powered)) {
                        break;
                    }
                }
            }
        }
        plot.handle_command(0, "/rp", vec!["publish", "invalid"]);
        assert_eq!(plot.redpilator.current_flags().unwrap().publication, Some(PublicationMode::All));
        plot.redpilator.try_reset(&mut plot.world, (lever, lever)).unwrap();
        plot.handle_command(0, "/visual", vec!["publish", "io"]);
        assert_eq!(plot.visual_publication_mode(), PublicationMode::Io);
        assert_eq!(plot.publication_setting, Some(PublicationSetting::Fixed(PublicationMode::Io)));
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
    pub(super) fn handle_publication_command(&mut self, player: usize, args: &[&str]) {
        if args.is_empty() {
            self.players[player].send_system_message(&messages::redpilator_publication(&self.visual_publication_label()));
            return;
        }
        let setting = match args {
            [value] => PublicationSetting::parse(value),
            _ => None,
        };
        let Some(setting) = setting else {
            self.players[player].send_error_message(messages::REDPILATOR_PUBLICATION_USAGE);
            return;
        };
        if !self.players[player].has_permission("commands.redpilator.compile") {
            self.players[player].send_no_permission_message();
            return;
        }
        if self.compilation.is_some() {
            self.players[player].send_error_message(messages::REDPILATOR_COMPILATION_BUSY);
            return;
        }
        #[cfg(any(test, feature = "plot-bench"))]
        let persist = !self.transient_test_fixture;
        #[cfg(not(any(test, feature = "plot-bench")))]
        let persist = true;
        if persist
            && self.publication_setting != Some(setting)
            && let Err(error) = super::database::set_publication_setting(self.world.x, self.world.z, setting)
        {
            self.players[player].send_error_message(&messages::visual_setting_save_failed(error));
            return;
        }
        self.publication_setting = Some(setting);
        self.redpilator_auto_publication = setting == PublicationSetting::Auto;
        let result = self.apply_publication_mode(setting.resolve(self.tps, CONFIG.fast_render_threshold));
        self.refresh_sidebar(true);
        let label = self.visual_publication_label();
        if let Err(error) = result {
            self.players[player].send_error_message(&messages::redpilator_publication_flush_failed(&label, error));
            return;
        }
        self.players[player].send_system_message(&messages::redpilator_publication(&label));
    }

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
            "publish" => self.handle_publication_command(player, args),
            _ => self.players[player].send_error_message(messages::REDPILATOR_INVALID_ARGUMENT),
        }
    }
}
