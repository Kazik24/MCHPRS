use crate::chat::{ChatComponentBuilder, ColorCode};
use crate::messages;
use crate::player::{PacketSender, Player};
use crate::redpiler::CompilerOptions;
use mchprs_network::packets::clientbound::{
    CDisplayScoreboard, CScoreboardObjective, CUpdateScore, ClientBoundPacket,
};
use mchprs_save_data::plot_data::Tps;

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

#[derive(Default)]
struct PlotMetrics {
    tps: String,
    history: String,
    history_memory: String,
    visual_updates: String,
}

#[derive(Default)]
pub struct Scoreboard {
    current_state: Vec<String>,
    redpiler_state: RedpilerState,
    compiler_flags: Vec<String>,
    metrics: PlotMetrics,
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
        lines
    }

    fn set_lines(&mut self, players: &[Player], lines: Vec<String>) {
        debug_assert!(lines.iter().all(|line| line.is_ascii() && line.len() <= 20));
        debug_assert!(lines
            .first()
            .is_some_and(|first| lines.iter().all(|line| first.len() >= line.len())));
        if lines == self.current_state {
            return;
        }

        let old_lines = std::mem::replace(&mut self.current_state, lines);
        for old_line in &old_lines {
            if !self.current_state.iter().any(|line| line == old_line) {
                let packet = Self::make_removal_packet(old_line).encode();
                players
                    .iter()
                    .for_each(|player| player.send_packet(&packet));
            }
        }

        for (index, line) in self.current_state.iter().enumerate() {
            let value = (self.current_state.len() - index) as u32;
            let old_value = old_lines
                .iter()
                .position(|old_line| old_line == line)
                .map(|old_index| (old_lines.len() - old_index) as u32);
            if old_value != Some(value) {
                let packet = Self::make_update_packet(line, value).encode();
                players
                    .iter()
                    .for_each(|player| player.send_packet(&packet));
            }
        }
    }

    fn refresh_lines(&mut self, players: &[Player]) {
        self.set_lines(players, self.lines());
    }

    pub fn add_player(&self, player: &Player) {
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
        for (index, line) in self.current_state.iter().enumerate() {
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

    pub fn update_plot_metrics(
        &mut self,
        players: &[Player],
        target_tps: Tps,
        actual_tps: Option<f32>,
        history_enabled: bool,
        history_ticks: usize,
        history_capacity: usize,
        history_memory_bytes: usize,
        visual_update_rate: Option<u32>,
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
        self.refresh_lines(players);
    }
}
