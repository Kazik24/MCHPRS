use super::Plot;
use crate::config::CONFIG;
use crate::player::PacketSender;
use crate::server::Message;

const USAGE: &str = "Usage: /serverinfo [plots [page]|plot [x z]|settings]";

impl Plot {
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
                let limits = crate::redpiler::analysis::AnalysisLimits::default();
                let (used, limit) = super::git::memory_usage();
                format!(
                    "Players advertised {}; view distance {}; default TPS {}; auto Redpiler {}\nHistory work budget {} MiB (stored usage: /rhistory limit)\nGit RAM reserved {}/{} MiB; snapshot cap {} MiB; plot default/ceiling {}/{} MiB; total disk {} MiB\nRedpiler base limits: {} cells, {} pistons; rank multiplier 1-8; no shared RAM cap\nCommand steps {}; work time {} ms; WorldEdit operation/history blocks {}/{}",
                    CONFIG.max_players, CONFIG.view_distance, CONFIG.default_tps, CONFIG.auto_redpiler,
                    CONFIG.rhistory_work_memory_limit_mib, used / 1048576, limit / 1048576,
                    CONFIG.git_snapshot_max_mib.min(128), CONFIG.git_default_plot_storage_mib,
                    CONFIG.git_plot_storage_mib, CONFIG.git_total_storage_mib,
                    limits.max_cells, limits.max_pistons,
                    CONFIG.max_command_ticks, CONFIG.command_work_time_ms,
                    CONFIG.worldedit_max_blocks, CONFIG.worldedit_history_blocks,
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
            |t| {
                format!(
                    "TPS 2s/10s/60s: {:.1}/{:.1}/{:.1}",
                    t.two_s, t.ten_s, t.one_m
                )
            },
        );
        let mut text = format!(
            "Plot {},{}; owner {:?}; {} players; {} chunks\nTPS setting {:?}; {timings}; last tick {:?}\nAuto Redpiler {}; compiled {}; WSR {}; screen-only {}; fast rendering {}; piston animation {:?}\nPending half-ticks {}; piston events {}; motions {}\n{}\n{}",
            self.world.x, self.world.z, self.owner.map(|uuid| format!("{uuid:032x}")),
            self.players.len(), self.world.chunks.len(), self.tps, self.last_nspt,
            self.auto_redpiler, self.redpiler.is_active(), self.world_send_rate.0,
            self.world.screen_only(), self.world.fast_rendering, self.piston_animation,
            self.world.to_be_ticked.iter().count(), self.world.piston_state.events.len(),
            self.world.piston_state.motions.len(), self.world.history.status(), self.git.diagnostics(),
        );
        if let Some(stats) = self.redpiler.stats() {
            text.push('\n');
            text.push_str(&stats.summary_lines().join("\n"));
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
