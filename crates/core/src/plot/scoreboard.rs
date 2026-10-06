use crate::chat::{ChatComponentBuilder, ColorCode};
use crate::messages;
use crate::player::{PacketSender, Player};
use crate::redpiler::CompilerOptions;
use mchprs_network::packets::clientbound::{
    CDisplayScoreboard, CScoreboardObjective, CUpdateScore, ClientBoundPacket,
};
use mchprs_save_data::plot_data::{PistonAnimation, Tps};
use std::collections::HashSet;

const OBJECTIVE_NAME: &str = "redpiler_status";

#[derive(PartialEq, Eq, Default, Clone, Copy)]
pub enum RedpilerState {
    #[default]
    Stopped,
    Compiling,
    Running,
}

impl RedpilerState {
    fn to_str(self) -> &'static str {
        match self {
            RedpilerState::Stopped => messages::SCOREBOARD_ENGINE_INTERPRETER,
            RedpilerState::Compiling => messages::SCOREBOARD_ENGINE_COMPILING,
            RedpilerState::Running => messages::SCOREBOARD_ENGINE_REDPILER,
        }
    }
}

fn compact_number(value: f64) -> String {
    if !value.is_finite() {
        return "-".to_owned();
    }
    let value = value.max(0.0);
    if value < 10_000.0 {
        let rounded = value.round();
        if rounded < 10_000.0 {
            return format!("{rounded:.0}");
        }
    }
    if value < 1_000_000.0 {
        let thousands = (value / 1_000.0).round();
        if thousands < 1_000.0 {
            return format!("{thousands:.0}k");
        }
    }
    if value < 1_000_000_000.0 {
        let millions = value / 1_000_000.0;
        if millions < 10.0 {
            let tenths = (millions * 10.0).round() / 10.0;
            if tenths < 10.0 {
                return format!("{tenths:.1}m");
            }
        }
        let rounded = millions.round();
        if rounded < 1_000.0 {
            return format!("{rounded:.0}m");
        }
    }
    format!("{:.1}b", value / 1_000_000_000.0)
}

fn compact_memory(bytes: usize) -> String {
    const UNITS: [&str; 7] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if value >= 999.95 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes}B")
    } else {
        format!("{value:.1}{}", UNITS[unit])
    }
}

struct PlotMetrics {
    tps: String,
    history: String,
    history_memory: String,
    visual_updates: String,
    pistons: String,
    screen_only: String,
    git: String,
}

impl Default for PlotMetrics {
    fn default() -> Self {
        Self {
            tps: messages::scoreboard_tps("-", "-"),
            history: messages::SCOREBOARD_HISTORY_OFF.into(),
            history_memory: messages::scoreboard_history_memory("0B"),
            visual_updates: messages::SCOREBOARD_VISUAL_OFF.into(),
            pistons: messages::scoreboard_pistons(PistonAnimation::Auto, messages::SCOREBOARD_ON),
            screen_only: messages::scoreboard_screen_only(messages::SCOREBOARD_OFF),
            git: messages::SCOREBOARD_GIT_LOADING.into(),
        }
    }
}

pub(super) struct PlotStatus<'a> {
    pub piston_mode: PistonAnimation,
    pub pistons_animated: bool,
    pub screen_only: bool,
    pub git_head: Option<&'a str>,
    pub git_restoring: bool,
    pub git_recovery: bool,
    pub git_readers: HashSet<u128>,
}

fn git_line(head: Option<&str>, restoring: bool, recovery: bool) -> String {
    if recovery {
        return messages::SCOREBOARD_GIT_RECOVERY.into();
    }
    if restoring {
        return messages::SCOREBOARD_GIT_RESTORING.into();
    }
    match head {
        None => messages::SCOREBOARD_GIT_LOADING.into(),
        Some("") => messages::SCOREBOARD_GIT_NONE.into(),
        Some(head) if head.starts_with('@') => {
            messages::scoreboard_git_detached(head[1..].chars().take(8).collect::<String>())
        }
        Some(branch) => {
            // The engine heading is at least 19 columns. Keep long branch names
            // within that width so the status footer never widens the sidebar.
            let mut branch: String = branch.chars().filter(char::is_ascii).collect();
            if branch.len() > 14 {
                branch.truncate(11);
                branch.push_str("...");
            }
            messages::scoreboard_git_branch(branch)
        }
    }
}

fn visible_lines(lines: &[String], git_read: bool) -> Vec<&str> {
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            if !git_read && index + 1 == lines.len() {
                messages::SCOREBOARD_GIT_HIDDEN
            } else {
                line.as_str()
            }
        })
        .collect()
}

#[derive(Default)]
pub struct Scoreboard {
    current_state: Vec<String>,
    redpiler_state: RedpilerState,
    compiler_flags: Vec<String>,
    metrics: PlotMetrics,
    git_readers: HashSet<u128>,
}

impl Scoreboard {
    fn make_update_packet(entity_name: &str, value: u32) -> CUpdateScore {
        CUpdateScore {
            entity_name: entity_name.to_owned(),
            action: 0,
            objective_name: OBJECTIVE_NAME.to_owned(),
            value,
        }
    }

    fn make_removal_packet(entity_name: &str) -> CUpdateScore {
        CUpdateScore {
            entity_name: entity_name.to_owned(),
            action: 1,
            objective_name: OBJECTIVE_NAME.to_owned(),
            value: 0,
        }
    }

    fn lines(&self) -> Vec<String> {
        let mut lines = vec![
            self.redpiler_state.to_str().to_owned(),
            self.metrics.tps.clone(),
            self.metrics.history.clone(),
            self.metrics.history_memory.clone(),
            self.metrics.visual_updates.clone(),
        ];
        if self.redpiler_state == RedpilerState::Running && !self.compiler_flags.is_empty() {
            lines.extend(self.compiler_flags.iter().map(messages::scoreboard_flag));
        }
        lines.extend([
            self.metrics.pistons.clone(),
            self.metrics.screen_only.clone(),
            self.metrics.git.clone(),
        ]);
        lines
    }

    fn set_lines(&mut self, players: &[Player], lines: Vec<String>, git_readers: HashSet<u128>) {
        debug_assert!(lines.iter().all(|line| line.is_ascii() && line.len() <= 20));
        debug_assert!(lines
            .first()
            .is_some_and(|first| lines.iter().all(|line| first.len() >= line.len())));
        if lines == self.current_state && git_readers == self.git_readers {
            return;
        }

        let old_lines = std::mem::replace(&mut self.current_state, lines);
        let old_readers = std::mem::replace(&mut self.git_readers, git_readers);
        for player in players {
            let previous = visible_lines(&old_lines, old_readers.contains(&player.uuid));
            let current =
                visible_lines(&self.current_state, self.git_readers.contains(&player.uuid));
            for old_line in &previous {
                if !current.contains(old_line) {
                    player.send_packet(&Self::make_removal_packet(old_line).encode());
                }
            }
            for (index, line) in current.iter().enumerate() {
                let value = (current.len() - index) as u32;
                let old_value = previous
                    .iter()
                    .position(|old_line| old_line == line)
                    .map(|old_index| (previous.len() - old_index) as u32);
                if old_value != Some(value) {
                    player.send_packet(&Self::make_update_packet(line, value).encode());
                }
            }
        }
    }

    fn refresh_lines(&mut self, players: &[Player]) {
        self.set_lines(players, self.lines(), self.git_readers.clone());
    }

    pub fn add_player(&mut self, player: &Player, git_read: bool) {
        // Rejoining players may have changed ranks since their last visit.
        // Record the permission used for these initial rows so later updates
        // remove exactly the entries that this connection actually received.
        if git_read {
            self.git_readers.insert(player.uuid);
        } else {
            self.git_readers.remove(&player.uuid);
        }
        player.send_packet(
            &CScoreboardObjective {
                objective_name: OBJECTIVE_NAME.into(),
                mode: 0,
                objective_value: ChatComponentBuilder::new(messages::PLOT_SIDEBAR_TITLE.into())
                    .color_code(ColorCode::Red)
                    .finish()
                    .encode_json(),
                ty: 0,
            }
            .encode(),
        );
        player.send_packet(
            &CDisplayScoreboard {
                position: 1,
                score_name: OBJECTIVE_NAME.into(),
            }
            .encode(),
        );
        for (index, line) in
            visible_lines(&self.current_state, self.git_readers.contains(&player.uuid))
                .iter()
                .enumerate()
        {
            player.send_packet(
                &Self::make_update_packet(line, (self.current_state.len() - index) as u32).encode(),
            );
        }
    }

    pub fn remove_player(&self, player: &Player) {
        // Leaving a plot keeps the connection alive. Remove the objective itself
        // so the destination plot can create it again, even on a same-plot /tp.
        player.send_packet(
            &CScoreboardObjective {
                objective_name: OBJECTIVE_NAME.into(),
                mode: 1,
                objective_value: String::new(),
                ty: 0,
            }
            .encode(),
        );
    }

    pub fn set_redpiler_state(&mut self, players: &[Player], state: RedpilerState) {
        self.redpiler_state = state;
        if state != RedpilerState::Running {
            self.compiler_flags.clear();
        }
        self.refresh_lines(players);
    }

    pub fn set_redpiler_options(&mut self, players: &[Player], options: &CompilerOptions) {
        self.compiler_flags.clear();
        if options.piston_events {
            self.compiler_flags.push("piston-events".to_owned());
        }
        if options.optimize {
            self.compiler_flags.push("optimize".to_owned());
        }
        if options.export {
            self.compiler_flags.push("export".to_owned());
        }
        if options.io_only {
            self.compiler_flags.push("io-only".to_owned());
        }
        if options.update {
            self.compiler_flags.push("update".to_owned());
        }
        if options.export_dot_graph {
            self.compiler_flags.push("export-dot".to_owned());
        }
        self.refresh_lines(players);
    }

    pub(super) fn update_plot_metrics(
        &mut self,
        players: &[Player],
        target_tps: Tps,
        actual_tps: Option<f32>,
        history_enabled: bool,
        history_ticks: usize,
        history_capacity: usize,
        history_memory_bytes: usize,
        visual_update_rate: Option<u32>,
        status: PlotStatus<'_>,
    ) {
        let actual = match target_tps {
            Tps::Limited(0) => "0".to_owned(),
            _ => actual_tps
                .map(|tps| compact_number(f64::from(tps)))
                .unwrap_or_else(|| "-".to_owned()),
        };
        let target = match target_tps {
            Tps::Limited(rate) => compact_number(f64::from(rate)),
            Tps::Unlimited => "oo".to_owned(),
        };
        self.metrics.tps = messages::scoreboard_tps(actual, target);
        self.metrics.history = if history_enabled {
            messages::scoreboard_history(
                compact_number(history_ticks as f64),
                compact_number(history_capacity as f64),
            )
        } else {
            messages::SCOREBOARD_HISTORY_OFF.to_owned()
        };
        self.metrics.history_memory =
            messages::scoreboard_history_memory(compact_memory(history_memory_bytes));
        self.metrics.visual_updates = match visual_update_rate {
            Some(rate) => messages::scoreboard_visual_rate(rate),
            None => messages::SCOREBOARD_VISUAL_OFF.to_owned(),
        };
        self.metrics.pistons = messages::scoreboard_pistons(
            status.piston_mode,
            if status.pistons_animated {
                messages::SCOREBOARD_ON
            } else {
                messages::SCOREBOARD_OFF
            },
        );
        self.metrics.screen_only = messages::scoreboard_screen_only(if status.screen_only {
            messages::SCOREBOARD_ON
        } else {
            messages::SCOREBOARD_OFF
        });
        self.metrics.git = git_line(status.git_head, status.git_restoring, status.git_recovery);
        self.set_lines(players, self.lines(), status.git_readers);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(head: Option<&str>) -> PlotStatus<'_> {
        PlotStatus {
            piston_mode: PistonAnimation::Auto,
            pistons_animated: false,
            screen_only: true,
            git_head: head,
            git_restoring: false,
            git_recovery: false,
            git_readers: HashSet::new(),
        }
    }

    #[test]
    fn footer_stays_below_flags_and_within_sidebar_limits() {
        let mut board = Scoreboard::default();
        board.set_redpiler_state(&[], RedpilerState::Running);
        board.set_redpiler_options(
            &[],
            &CompilerOptions {
                optimize: true,
                export: true,
                io_only: true,
                update: true,
                export_dot_graph: true,
                ..Default::default()
            },
        );
        board.update_plot_metrics(
            &[],
            Tps::Unlimited,
            Some(1_000_000.0),
            true,
            1200,
            2400,
            1024 * 1024 * 1024,
            Some(20),
            status(Some("very-long-branch-name")),
        );
        assert_eq!(board.current_state.len(), 13);
        assert_eq!(
            &board.current_state[10..],
            &[
                "Pistons: auto/off",
                "Screen only: on",
                "Git: very-long-b...",
            ]
        );
        for engine in [
            RedpilerState::Running,
            RedpilerState::Compiling,
            RedpilerState::Stopped,
        ] {
            board.set_redpiler_state(&[], engine);
            let lines = &board.current_state;
            assert!(lines.len() <= 15);
            assert!(lines
                .iter()
                .all(|line| line.is_ascii() && line.len() <= lines[0].len()));
            assert_eq!(lines.iter().collect::<HashSet<_>>().len(), lines.len());
            assert_eq!(lines.last().unwrap(), "Git: very-long-b...");
        }
    }

    #[test]
    fn git_footer_covers_head_and_restore_states() {
        assert_eq!(git_line(Some("main"), false, false), "Git: main");
        assert_eq!(
            git_line(Some(&format!("@{}", "a".repeat(64))), false, false),
            "Git: @aaaaaaaa"
        );
        assert_eq!(
            git_line(Some(""), false, false),
            messages::SCOREBOARD_GIT_NONE
        );
        assert_eq!(
            git_line(None, false, false),
            messages::SCOREBOARD_GIT_LOADING
        );
        assert_eq!(
            git_line(Some("main"), true, false),
            messages::SCOREBOARD_GIT_RESTORING
        );
        assert_eq!(
            git_line(Some("main"), true, true),
            messages::SCOREBOARD_GIT_RECOVERY
        );
        let legacy = git_line(Some(&"long".repeat(12)), false, false);
        assert_eq!(legacy.len(), 19);
        assert!(legacy.ends_with("..."));
    }

    #[test]
    fn viewers_without_git_access_keep_plot_status_without_branch_details() {
        let mut board = Scoreboard::default();
        board.update_plot_metrics(
            &[],
            Tps::Limited(20),
            Some(20.0),
            false,
            0,
            0,
            0,
            Some(20),
            status(Some("secret-experiment")),
        );
        let allowed = visible_lines(&board.current_state, true);
        let denied = visible_lines(&board.current_state, false);
        assert_eq!(allowed.len(), denied.len());
        assert_eq!(&allowed[..allowed.len() - 1], &denied[..denied.len() - 1]);
        assert_eq!(allowed.last(), Some(&"Git: secret-expe..."));
        assert_eq!(denied.last(), Some(&messages::SCOREBOARD_GIT_HIDDEN));
        assert!(visible_lines(&[], false).is_empty());
    }

    #[test]
    fn setting_changes_replace_footer_rows() {
        let mut board = Scoreboard::default();
        let mut current = status(Some("main"));
        current.piston_mode = PistonAnimation::On;
        current.pistons_animated = true;
        current.screen_only = false;
        board.update_plot_metrics(
            &[],
            Tps::Limited(20),
            Some(20.0),
            false,
            0,
            0,
            0,
            Some(20),
            current,
        );
        assert!(board
            .current_state
            .iter()
            .any(|line| line == "Pistons: on/on"));
        assert!(board
            .current_state
            .iter()
            .any(|line| line == "Screen only: off"));
        board.update_plot_metrics(
            &[],
            Tps::Limited(20),
            Some(20.0),
            false,
            0,
            0,
            0,
            Some(20),
            status(Some("experiment")),
        );
        assert!(board
            .current_state
            .iter()
            .any(|line| line == "Pistons: auto/off"));
        assert!(board
            .current_state
            .iter()
            .any(|line| line == "Screen only: on"));
        assert_eq!(board.current_state.last().unwrap(), "Git: experiment");
        assert!(!board.current_state.iter().any(|line| line == "Git: main"));
    }
}
