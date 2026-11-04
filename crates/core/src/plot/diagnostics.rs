use super::{Plot, PLOT_BLOCK_HEIGHT, PLOT_BLOCK_WIDTH, history};
use crate::config::CONFIG;
use crate::messages;
use crate::player::PacketSender;
use crate::server::Message;
use mchprs_save_data::plot_data::Tps;

const USAGE: &str = "Usage: /serverinfo [plots [page]|plot [x z]|settings]";

impl Plot {
    pub(super) fn plot_info(&self, player: usize, owner: &str) -> String {
        let on_off = |enabled| if enabled { messages::SCOREBOARD_ON } else { messages::SCOREBOARD_OFF };
        let (first, last) = self.world.get_corners();
        let position = self.players[player].pos.block_pos();
        let mut names: Vec<_> = self.players.iter().take(20).map(|player| player.username.clone()).collect();
        if self.players.len() > names.len() {
            names.push(messages::plot_info_more_players(self.players.len() - names.len()));
        }
        let measured = self.timings.generate_report().map_or_else(
            || messages::PLOT_INFO_NO_SAMPLES.into(),
            |report| format!("{:.1} / {:.1} / {:.1}", report.two_s, report.ten_s, report.one_m),
        );
        let mspt = self.last_nspt.map_or_else(
            || messages::PLOT_INFO_NO_SAMPLES.into(),
            |duration| format!("{:.3} ms", duration.as_secs_f64() * 1000.0),
        );
        let engine = match self.redpiler.current_flags() {
            Some(options) if options.llvm => "Redpiler-LLVM",
            Some(_) => "Redpiler",
            None => "Interpreter",
        };
        let mut lines = vec![
            messages::plot_info_heading(self.world.x, self.world.z, owner),
            messages::plot_info_position(position, position - first),
            messages::plot_info_bounds(
                first, last, PLOT_BLOCK_WIDTH, PLOT_BLOCK_HEIGHT, self.world.chunks.len(),
                self.world.chunks.iter().map(|chunk| chunk.block_entities.len()).sum(),
            ),
            messages::plot_info_players(self.players.len(), names.join(", ")),
            messages::plot_info_simulation(
                if self.tps == Tps::Limited(0) { messages::PLOT_INFO_PAUSED } else { messages::PLOT_INFO_RUNNING },
                self.tps, measured, mspt,
            ),
            messages::plot_info_engine(engine, on_off(self.compiling_redpiler()), on_off(self.auto_redpiler)),
            messages::plot_info_history(
                on_off(self.world.history.enabled()), self.world.history.len(), self.world.history.capacity(),
                history::format_memory(self.world.history.memory_bytes()),
            ),
            messages::plot_info_rendering(
                self.effective_send_rate(), self.world_send_rate.0, on_off(self.world.screen_only()), on_off(self.world.fast_rendering),
                self.piston_animation, on_off(!self.world.fast_rendering && !self.world.screen_only()),
            ),
            messages::plot_info_waveform(on_off(self.world.waveform.enabled()), self.world.waveform.probes.len()),
        ];
        // Compiled queues live in the runtime; host queues would report stale work.
        if !self.redpiler.is_active() {
            lines.push(messages::plot_info_pending(
                self.world.to_be_ticked.iter().count(), self.world.piston_state.events.len(), self.world.piston_state.motions.len(),
                self.world.piston_state.phase,
            ));
        } else if let Some(stats) = self.redpiler.stats() {
            lines.extend(stats.chat_summary_lines());
        }
        if self.git_access(player, "read") {
            let head = match self.git.head.as_deref() {
                Some("") => messages::PLOT_INFO_NO_COMMITS,
                Some(head) => head,
                None => messages::PLOT_INFO_GIT_LOADING,
            };
            lines.push(messages::plot_info_git(head, on_off(self.git.locked), on_off(self.git.fatal)));
        }
        lines.join("\n")
    }

    pub(super) fn handle_serverinfo(&self, player: usize, args: &[&str]) {
        let actor = &self.players[player];
        // No permissive standalone fallback for server administration.
        if !actor.has_explicit_permission("commands.serverinfo") {
            tracing::warn!("Server diagnostics denied: missing explicit permission");
            actor.send_no_permission_message();
            return;
        }
        let text = match args {
            [] | ["plots"] | ["plots", _] => {
                let page = match args.get(1) {
                    None => 1,
                    Some(value) => match value.parse::<usize>() {
                        Ok(page) if page > 0 => page,
                        _ => {
                            actor.send_error_message(USAGE);
                            return;
                        }
                    },
                };
                let _ = self.message_sender.send(Message::ServerInfo {
                    sender: mchprs_network::PlayerPacketSender::new(&actor.client),
                    page,
                    plot: None,
                    span: tracing::Span::current(),
                });
                return;
            }
            ["plot", x, z] => {
                let (Ok(x), Ok(z)) = (x.parse(), z.parse()) else {
                    actor.send_error_message(USAGE);
                    return;
                };
                let _ = self.message_sender.send(Message::ServerInfo {
                    sender: mchprs_network::PlayerPacketSender::new(&actor.client),
                    page: 1,
                    plot: Some((x, z)),
                    span: tracing::Span::current(),
                });
                return;
            }
            ["plot"] => self.diagnostics(),
            ["settings"] => {
                let (used, limit) = super::git::memory_usage();
                format!(
                    "Players advertised {}; view distance {}; default TPS {}; auto Redpiler {}\nHistory work budget {} MiB (stored usage: /rhistory limit)\nGit RAM reserved {}/{} MiB; snapshot cap {} MiB; plot default/ceiling {}/{} MiB; total disk {} MiB\nCommand steps {}; work time {} ms; WorldEdit operation/history blocks {}/{}",
                    CONFIG.max_players,
                    CONFIG.view_distance,
                    CONFIG.default_tps,
                    CONFIG.auto_redpiler,
                    CONFIG.rhistory_work_memory_limit_mib,
                    used / 1048576,
                    limit / 1048576,
                    CONFIG.git_snapshot_max_mib.min(128),
                    CONFIG.git_default_plot_storage_mib,
                    CONFIG.git_plot_storage_mib,
                    CONFIG.git_total_storage_mib,
                    CONFIG.max_command_ticks,
                    CONFIG.command_work_time_ms,
                    CONFIG.worldedit_max_blocks,
                    CONFIG.worldedit_history_blocks,
                )
            }
            _ => {
                actor.send_error_message(USAGE);
                return;
            }
        };
        tracing::info!(details = %text, "Server diagnostics");
        actor.send_system_message(&text);
    }

    pub(super) fn diagnostics(&self) -> String {
        let timings = self.timings.generate_report().map_or_else(
            || "TPS samples unavailable".to_owned(),
            |t| format!("TPS 2s/10s/60s: {:.1}/{:.1}/{:.1}", t.two_s, t.ten_s, t.one_m),
        );
        let mut text = format!(
            "Plot {},{}; owner {:?}; {} players; {} chunks\nTPS setting {:?}; {timings}; last tick {:?}\nAuto Redpiler {}; compiled {}; WSR {}; screen-only {}; fast rendering {}; piston animation {:?}\nPending half-ticks {}; piston events {}; motions {}\n{}\n{}",
            self.world.x,
            self.world.z,
            self.owner.map(|uuid| format!("{uuid:032x}")),
            self.players.len(),
            self.world.chunks.len(),
            self.tps,
            self.last_nspt,
            self.auto_redpiler,
            self.redpiler.is_active(),
            self.world_send_rate.0,
            self.world.screen_only(),
            self.world.fast_rendering,
            self.piston_animation,
            self.world.to_be_ticked.iter().count(),
            self.world.piston_state.events.len(),
            self.world.piston_state.motions.len(),
            self.world.history.status(),
            self.git.diagnostics(),
        );
        if let Some(stats) = self.redpiler.stats() {
            text.push('\n');
            text.push_str(&stats.chat_summary_lines().join("\n"));
        }

        text
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn diagnostics_require_explicit_permission_even_on_standalone_servers() {
        let (mut plot, _peer) = crate::plot::client_sync_tests::fixture(false);
        let (sender, receiver) = std::sync::mpsc::channel();
        plot.message_sender = sender;
        plot.handle_serverinfo(0, &["plots"]);
        assert!(receiver.try_recv().is_err());
        let text = plot.diagnostics();
        assert!(text.contains("Plot 0,0") && text.contains("1 players"));
        assert!(text.contains("Pending half-ticks") && text.contains("RAM reserved"));
    }
}
