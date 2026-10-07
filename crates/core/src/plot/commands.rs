use super::{database, worldedit, Plot, PlotWorld};
use crate::chat::ChatComponent;
use crate::messages;
use crate::player::{Gamemode, PacketSender, PlayerPos};
use crate::plot::data::sleep_time_for_tps;
use crate::profile::PlayerProfile;
use crate::redpiler::CompilerOptions;
use crate::server::Message;
use mchprs_network::packets::clientbound::{
    CDeclareCommands, CDeclareCommandsNode as Node, CDeclareCommandsNodeParser as Parser,
    CTabComplete, CTabCompleteMatch, ClientBoundPacket,
};
use mchprs_network::packets::PacketEncoder;
use mchprs_network::PlayerPacketSender;
use mchprs_save_data::plot_data::{Tps, WorldSendRate};
use once_cell::sync::Lazy;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, warn};

// Parses a relative or absolute coordinate relative to a reference coordinate
trait RelativeCoordinate: FromStr {
    fn checked_offset(self, offset: Self) -> Option<Self>;
}

/// Stop dispatch as soon as a command transfers the indexed player away.
pub(super) fn run_command_queue(
    commands: Vec<String>,
    mut handle: impl FnMut(&str, Vec<&str>) -> bool,
) -> bool {
    for input in &commands {
        let mut parts = input.split_whitespace();
        let Some(command) = parts.next() else {
            continue;
        };
        if handle(command, parts.collect()) {
            return true;
        }
    }
    false
}
impl RelativeCoordinate for i32 {
    fn checked_offset(self, offset: Self) -> Option<Self> {
        self.checked_add(offset)
    }
}

pub(super) fn complete_redpiler(id: i32, text: &str) -> Option<CTabComplete> {
    let boundary = text.rfind(char::is_whitespace)?;
    let start = boundary + text[boundary..].chars().next()?.len_utf8();
    let words: Vec<_> = text[..start].split_whitespace().collect();
    if !matches!(words.first(), Some(&"/rp" | &"/redpiler")) {
        return None;
    }
    let analyze = match words.get(1) {
        Some(&"compile" | &"c") => false,
        Some(&"analyze") => true,
        _ => return None,
    };
    let prefix = &text[start..];
    let matches = [
        "--assume-instant",
        "--optimize",
        "--io-only",
        "--update",
        "--export",
        "--export-dot",
        "--graph",
        "-o",
        "-i",
        "-u",
        "-e",
    ]
    .into_iter()
    .filter(|flag| flag.starts_with(prefix) && !words[2..].contains(flag))
    .filter(|flag| match *flag {
        "--graph" => analyze,
        "--export" | "--export-dot" | "-e" => !analyze,
        _ => true,
    })
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

impl RelativeCoordinate for f64 {
    fn checked_offset(self, offset: Self) -> Option<Self> {
        let result = self + offset;
        result.is_finite().then_some(result)
    }
}

pub(crate) fn complete_teleport<'a>(
    id: i32,
    text: &str,
    names: impl Iterator<Item = &'a str>,
) -> Option<CTabComplete> {
    let (command, prefix) = text.split_once(' ')?;
    if !matches!(command, "/tp" | "/teleport") {
        return None;
    }
    let normalized_prefix = prefix.to_ascii_lowercase();
    let mut names: Vec<_> = names
        .filter(|name| name.to_ascii_lowercase().starts_with(&normalized_prefix))
        .collect();
    names.sort_unstable();
    Some(CTabComplete {
        id,
        start: (command.encode_utf16().count() + 1) as i32,
        length: prefix.encode_utf16().count() as i32,
        matches: names
            .into_iter()
            .map(|name| CTabCompleteMatch {
                match_: name.to_owned(),
                tooltip: None,
            })
            .collect(),
    })
}
fn parse_relative_coord<F: RelativeCoordinate>(
    coord: &str,
    ref_coord: F,
) -> Result<F, &'static str> {
    if coord == "~" {
        Ok(ref_coord)
    } else if let Some(offset_str) = coord.strip_prefix('~') {
        let offset = offset_str
            .parse::<F>()
            .map_err(|_| messages::INVALID_COORDINATE)?;
        ref_coord
            .checked_offset(offset)
            .ok_or(messages::COORDINATE_OVERFLOW)
    } else {
        coord.parse::<F>().map_err(|_| messages::INVALID_COORDINATE)
    }
}

fn parse_teleport_coord(coord: &str, reference: f64, center: bool) -> Result<f64, &'static str> {
    let value = parse_relative_coord(coord, reference)?;
    if !value.is_finite() {
        return Err(messages::INVALID_TELEPORT_COORDINATES);
    }
    let integer = coord.trim_start_matches(['+', '-']);
    if center && !integer.is_empty() && integer.bytes().all(|byte| byte.is_ascii_digit()) {
        Ok(value + 0.5)
    } else {
        Ok(value)
    }
}

fn valid_warp_name(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn advance_bounded(ticks: u32, mut step: impl FnMut()) -> u32 {
    let started = Instant::now();
    let budget = std::time::Duration::from_millis(crate::config::CONFIG.command_work_time_ms);
    let mut advanced = 0;
    for _ in 0..ticks {
        if started.elapsed() >= budget {
            break;
        }
        step();
        advanced += 1;
    }
    advanced
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AdvanceUnit {
    Game,
    Nano,
    Pico,
}

impl AdvanceUnit {
    fn parse(args: &[&str], limit: u32) -> Result<(Self, u32), String> {
        let (unit, count) = match args {
            [count] => (Self::Game, count),
            [unit, count] if unit.eq_ignore_ascii_case("nano") => (Self::Nano, count),
            [unit, count] if unit.eq_ignore_ascii_case("pico") => (Self::Pico, count),
            _ => return Err(messages::USAGE_ADV.into()),
        };
        let ticks = count
            .parse::<u32>()
            .ok()
            .filter(|&ticks| ticks <= limit)
            .ok_or_else(|| messages::advance_tick_count_limit(limit))?;
        Ok((unit, ticks))
    }

    fn label(self) -> &'static str {
        match self {
            Self::Game => messages::ADV_GAME_TICKS_LABEL,
            Self::Nano => messages::ADV_NANO_TICKS_LABEL,
            Self::Pico => messages::ADV_PICO_TICKS_LABEL,
        }
    }
}

impl Plot {
    /// Transfers immediately so later queued commands cannot affect the old plot.
    pub(super) fn teleport_to_warp(&mut self, player: usize, warp: database::Warp) -> bool {
        if !warp.is_valid() {
            self.players[player].send_error_message(messages::INVALID_TELEPORT_COORDINATES);
            return false;
        }
        self.close_open_container(player);
        self.players[player].yaw = warp.yaw;
        self.players[player].pitch = warp.pitch;
        self.players[player].teleport(warp.pos);
        if warp.pos.plot_pos() != (self.world.x, self.world.z) {
            let uuid = self.players[player].uuid;
            let player = self.leave_plot(uuid);
            let _ = self.message_sender.send(Message::PlayerLeavePlot(player));
            return true;
        }
        false
    }

    /// Handles a command that starts with `/plot` or `/p`
    fn handle_plot_command(&mut self, player: usize, command: &str, args: &[&str]) {
        let (plot_x, plot_z) = self.players[player].pos.plot_pos();

        let permission_node = match command {
            "info" | "i" => "plots.info",
            "claim" | "c" | "add" | "remove" => "plots.claim",
            "auto" | "a" => "plots.auto",
            "middle" => "plots.middle",
            "visit" | "v" | "home" | "h" => "plots.visit",
            "teleport" | "tp" => "plots.visit",
            "lock" | "unlock" => "plots.lock",
            "select" | "sel" => "plots.select",
            _ => {
                self.players[player].send_error_message(messages::PLOT_INVALID_ARGUMENT);
                return;
            }
        };
        if !self.players[player].has_permission(permission_node) {
            self.players[player].send_no_permission_message();
            return;
        }

        match command {
            "home" | "h" => {
                if !args.is_empty() {
                    self.players[player].send_error_message(messages::USAGE_PLOT_HOME);
                    return;
                }
                match database::get_owned_plots_by_uuid(self.players[player].uuid) {
                    Ok(plots) => {
                        if let Some(&(x, z)) = plots.first() {
                            let center = Plot::get_center(x, z);
                            self.players[player].teleport(PlayerPos::new(center.0, 64.0, center.1));
                        } else {
                            self.players[player].send_error_message(messages::PLOT_HOME_UNCLAIMED);
                        }
                    }
                    Err(error) => {
                        self.players[player].send_error_message(&messages::plot_read_failed(error))
                    }
                }
            }
            "add" | "remove" => {
                let [name] = args else {
                    self.players[player].send_error_message(&messages::plot_member_usage(command));
                    return;
                };
                let add = command == "add";
                let actor = &self.players[player];
                let result = database::set_plot_member(
                    self.world.x,
                    self.world.z,
                    actor.uuid,
                    actor.has_permission("plots.admin.interact.other"),
                    name,
                    add,
                );
                use database::MembershipResult;
                match result {
                    Ok(MembershipResult::Changed { name, .. }) => self.players[player]
                        .send_system_message(&if add {
                            messages::plot_member_added(name, self.world.x, self.world.z)
                        } else {
                            messages::plot_member_removed(name, self.world.x, self.world.z)
                        }),
                    Ok(MembershipResult::Unchanged) => {
                        self.players[player].send_system_message(if add {
                            messages::PLOT_MEMBER_ALREADY_ADDED
                        } else {
                            messages::PLOT_MEMBER_NOT_ADDED
                        })
                    }
                    Ok(MembershipResult::UnknownPlayer) => self.players[player]
                        .send_error_message(messages::PLOT_MEMBER_UNKNOWN_PLAYER),
                    Ok(MembershipResult::AmbiguousPlayer) => self.players[player]
                        .send_error_message(messages::PLOT_MEMBER_AMBIGUOUS_PLAYER),
                    Ok(MembershipResult::PlotUnclaimed) => {
                        self.players[player].send_error_message(messages::PLOT_UNCLAIMED)
                    }
                    Ok(MembershipResult::NotOwner) => {
                        self.players[player].send_no_permission_message()
                    }
                    Ok(MembershipResult::IsOwner) => {
                        self.players[player].send_error_message(messages::PLOT_MEMBER_IS_OWNER)
                    }
                    Err(error) => self.players[player]
                        .send_error_message(&messages::plot_members_update_failed(error)),
                }
            }
            "info" | "i" => {
                if let Some(owner) = database::get_plot_owner(plot_x, plot_z) {
                    self.players[player].send_system_message(&messages::plot_owner(
                        database::get_cached_username(owner.clone()).unwrap_or(owner),
                    ));
                } else {
                    self.players[player].send_system_message(messages::PLOT_UNCLAIMED);
                }
            }
            "claim" | "c" => {
                self.claim_plot(plot_x, plot_z, player);
            }
            "auto" | "a" => {
                let mut start = (0, 0);
                for _ in 0..10_000 {
                    match database::is_claimed(start.0, start.1) {
                        Some(true) => start = Plot::get_next_plot(start.0, start.1),
                        Some(false) => {
                            self.claim_plot(start.0, start.1, player);
                            return;
                        }
                        None => {
                            self.players[player]
                                .send_error_message(messages::PLOT_CLAIMS_READ_FAILED);
                            return;
                        }
                    }
                }
                self.players[player].send_error_message(messages::PLOT_AUTO_SEARCH_FULL);
            }
            "middle" => {
                let center = Plot::get_center(plot_x, plot_z);
                self.players[player].teleport(PlayerPos::new(center.0, 64.0, center.1));
            }
            "visit" | "v" => {
                if !(1..=2).contains(&args.len()) {
                    self.players[player].send_error_message(messages::INVALID_ARGUMENT_COUNT);
                    return;
                }

                let idx = if args.len() == 2 {
                    match args[1].parse::<usize>() {
                        Ok(idx) if idx > 0 => idx - 1,
                        Ok(_) => {
                            self.players[player].send_error_message(messages::PLOT_INDEX_ZERO);
                            return;
                        }
                        Err(_) => {
                            self.players[player].send_error_message(messages::PLOT_INVALID_INDEX);
                            return;
                        }
                    }
                } else {
                    0
                };

                let plots = match database::get_owned_plots(args[0]) {
                    Ok(plots) => plots,
                    Err(error) => {
                        self.players[player].send_error_message(&messages::plot_read_failed(error));
                        return;
                    }
                };
                if !plots.is_empty() {
                    if let Some(&(plot_x, plot_z)) = plots.get(idx) {
                        let center = Plot::get_center(plot_x, plot_z);
                        self.players[player].teleport(PlayerPos::new(center.0, 64.0, center.1));
                    } else {
                        self.players[player]
                            .send_system_message(&messages::plot_index_range(plots.len()));
                    }
                } else {
                    self.players[player]
                        .send_system_message(&messages::player_has_no_plots(args[0]));
                }
            }
            "teleport" | "tp" => {
                if args.len() != 2 {
                    self.players[player].send_error_message(messages::INVALID_ARGUMENT_COUNT);
                    return;
                }

                let new_plot_x;
                let new_plot_z;
                if let Ok(x_arg) = parse_relative_coord(args[0], plot_x) {
                    new_plot_x = x_arg;
                } else {
                    self.players[player].send_error_message(messages::INVALID_X_COORDINATE);
                    return;
                }
                if let Ok(z_arg) = parse_relative_coord(args[1], plot_z) {
                    new_plot_z = z_arg;
                } else {
                    self.players[player].send_error_message(messages::INVALID_Z_COORDINATE);
                    return;
                }

                let center = Plot::get_center(new_plot_x, new_plot_z);
                self.players[player].teleport(PlayerPos::new(center.0, 64.0, center.1));
            }
            "lock" => {
                if self.locked_players.insert(self.players[player].entity_id) {
                    let PlotWorld { x, z, .. } = self.world;
                    let res = messages::plot_locked(x, z);
                    self.players[player].send_system_message(&res);
                } else {
                    self.players[player].send_system_message(messages::PLOT_ALREADY_LOCKED);
                }
            }
            "select" | "sel" => {
                let (first, second) = self.world.get_corners();
                self.players[player].worldedit_set_first_position(first);
                self.players[player].worldedit_set_second_position(second);
            }
            "unlock" => {
                if self.locked_players.remove(&self.players[player].entity_id) {
                    self.players[player].send_system_message(messages::PLOT_UNLOCKED);
                } else {
                    self.players[player].send_system_message(messages::PLOT_NOT_LOCKED);
                }
            }
            _ => self.players[player].send_error_message(messages::PLOT_INVALID_ARGUMENT),
        }
    }

    /// Handles a command that starts with `/redpiler` or `/rp`
    fn handle_redpiler_command(&mut self, player: usize, command: &str, args: &[&str]) {
        match command {
            "analyze" => {
                if self.redpiler.is_active() {
                    self.players[player]
                        .send_error_message("Reset Redpiler before analyzing live geometry.");
                    return;
                }
                let ticks: Vec<_> = self.world.scheduler().iter_entries().collect();
                let flags = args
                    .iter()
                    .copied()
                    .filter(|&arg| arg != "--graph")
                    .collect::<Vec<_>>()
                    .join(" ");
                let mut options = match CompilerOptions::parse(&flags) {
                    Ok(options) => options,
                    Err(error) => {
                        self.players[player].send_error_message(&error);
                        return;
                    }
                };
                if options.export || options.export_dot_graph {
                    self.players[player].send_error_message(
                        "Export flags are unavailable during read-only analysis.",
                    );
                    return;
                }
                options.budget_multiplier = self.players[player].compilation_budget_multiplier();
                if args.contains(&"--graph") {
                    let monitor = Arc::new(crate::redpiler::TaskMonitor::default());
                    match crate::redpiler::analysis::graph::prepare_candidate_graph(
                        &self.world,
                        self.world.get_corners(),
                        &ticks,
                        &options,
                        monitor.clone(),
                    ) {
                        Ok(candidate) => {
                            let summary = candidate.summary();
                            self.players[player].send_system_message(&format!(
                                "Candidate graph: {} ordinary nodes, {} instant inputs, {} mobile sources, {} compiled output ports, {} electrical links; execution remains disabled",
                                summary.ordinary_nodes, summary.instant_inputs, summary.mobile_sources, summary.compiled_outputs, summary.electrical_links,
                            ));
                            self.players[player]
                                .send_system_message(&candidate.report.recognition_summary());
                            for line in monitor.graph_statistics().summary_lines() {
                                self.players[player].send_system_message(&line);
                            }
                            debug!(report = %serde_json::to_string(&candidate.report).unwrap(), graph = ?candidate.graph, "Redpiler candidate graph");
                        }
                        Err(error) => self.players[player].send_error_message(&error.to_string()),
                    }
                    return;
                }
                match crate::redpiler::analysis::analyze(
                    &self.world,
                    self.world.get_corners(),
                    &ticks,
                    &Default::default(),
                    crate::redpiler::analysis::AnalysisLimits::for_budget(
                        self.players[player].compilation_budget_multiplier(),
                    ),
                ) {
                    Ok(report) => {
                        self.players[player].send_system_message(&report.summary());
                        if !report.pistons.is_empty() {
                            self.players[player].send_system_message(&report.recognition_summary());
                        }
                        debug!(report = %serde_json::to_string(&report).unwrap(), "Redpiler analysis");
                        // Stage the same backend as compile, then drop it without
                        // activating it or transferring any interpreter work.
                        let mut compiler = crate::redpiler::Compiler::default();
                        match compiler.compile(
                            &self.world,
                            self.world.get_corners(),
                            options,
                            ticks,
                            Default::default(),
                        ) {
                            Ok(()) => {
                                if let Some(stats) = compiler.stats() {
                                    for line in stats.summary_lines() {
                                        self.players[player].send_system_message(&line);
                                    }
                                }
                                self.players[player]
                                    .send_system_message("This plot can compile with these flags.");
                            }
                            Err(error) => self.players[player]
                                .send_error_message(&format!("Redpiler: {error}")),
                        }
                    }
                    Err(error) => self.players[player].send_error_message(&error.to_string()),
                }
            }
            "compile" | "c" => {
                let start_time = Instant::now();
                let args = args.join(" ");
                let mut options = match CompilerOptions::parse(&args) {
                    Ok(options) => options,
                    Err(error) => {
                        self.players[player].send_error_message(&error);
                        return;
                    }
                };
                options.budget_multiplier = self.players[player].compilation_budget_multiplier();

                if options.optimize {
                    let msg = messages::REDPILER_OPTIMIZATION_HIGHLY_UNSTABLE_CAN_BREAK;
                    warn!("{}", msg);
                    self.players[player].send_system_message(msg);
                }

                self.reset_redpiler();
                self.start_redpiler(options);

                debug!("Compile took {:?}", start_time.elapsed());
            }
            "inspect" | "i" => {
                let player = &self.players[player];
                let pos = worldedit::ray_trace_block(
                    &self.world,
                    player.pos,
                    player.pitch as f64,
                    player.yaw as f64,
                    10.0,
                );
                let Some(pos) = pos else {
                    player.send_error_message(messages::BLOCK_TRACE_FAILED);
                    return;
                };
                self.redpiler.inspect(pos);
            }
            "reset" | "r" => {
                self.reset_redpiler();
            }
            _ => self.players[player].send_error_message(messages::REDPILER_INVALID_ARGUMENT),
        }
    }

    // Returns true if packets should stop being handled
    pub(super) fn handle_command(
        &mut self,
        player: usize,
        command: &str,
        mut args: Vec<&str>,
    ) -> bool {
        let span = tracing::info_span!("command", player = %self.players[player].username,
            uuid = %format_args!("{:032x}", self.players[player].uuid),
            plot_x = self.world.x, plot_z = self.world.z, command,
            arguments = ?args.join(" ").chars().take(1024).collect::<String>());
        let _entered = span.enter();
        info!("Command requested");
        if self.git_checkout_locked() && !matches!(command, "/git" | "/help" | "/serverinfo") {
            warn!("Command denied: plot checkout locked");
            let message = self.git_lock_message();
            self.players[player].send_error_message(message);
            return false;
        }
        if !self.players[player].can_use_commands() {
            warn!("Command denied: command access");
            self.players[player].send_no_permission_message();
            return false;
        }
        if crate::permissions::dedicated_permissions()
            && (native_command_permission(command, &args)
                .is_some_and(|node| !self.players[player].has_permission(&node))
                || (changes_plot(command, &args)
                    && !self.players[player]
                        .can_edit_plot(self.owner, (self.world.x, self.world.z))))
        {
            warn!("Command denied: permission or plot ownership");
            self.players[player].send_no_permission_message();
            return false;
        }

        let admin_permission = match command {
            "/stop" => Some("minecraft.command.stop"),
            "/whitelist" => Some("minecraft.command.whitelist"),
            _ => None,
        };
        if admin_permission.is_some_and(|node| !self.players[player].has_permission(node)) {
            self.players[player].send_no_permission_message();
            return false;
        }

        if command == "/git" {
            self.handle_git_command(player, &args);
            return false;
        }
        if command == "/serverinfo" {
            self.handle_serverinfo(player, &args);
            return false;
        }
        if self.handle_redstone_tools_command(player, command, &args) {
            return false;
        }

        // Handle worldedit commands
        if worldedit::execute_command(self, player, &command[1..], &mut args) {
            // If the command was handled, there is no need to continue;
            return false;
        }

        match command {
            "/screenonly" => {
                if !self.players[player].has_permission("commands.screenonly") {
                    self.players[player].send_no_permission_message();
                    return false;
                }
                let enabled = match args.as_slice() {
                    [] => None,
                    ["on"] => Some(true),
                    ["off"] => Some(false),
                    _ => {
                        self.players[player].send_error_message(messages::USAGE_SCREEN_ONLY);
                        return false;
                    }
                };
                if let Some(enabled) = enabled {
                    if self.owner != Some(self.players[player].uuid)
                        && !self.players[player].has_permission("plots.worldedit.bypass")
                    {
                        self.players[player].send_no_permission_message();
                        return false;
                    }
                    if let Err(error) =
                        database::set_screen_only(self.world.x, self.world.z, enabled)
                    {
                        self.players[player]
                            .send_error_message(&messages::visual_setting_save_failed(error));
                        return false;
                    }
                    self.world.set_screen_only(enabled);
                }
                self.players[player].send_system_message(if self.world.screen_only() {
                    messages::SCREEN_ONLY_ON
                } else {
                    messages::SCREEN_ONLY_OFF
                });
            }
            "/help" => {
                if args.len() > 1 {
                    self.players[player].send_error_message(messages::USAGE_HELP_TOPIC);
                } else if let Some(page) = super::help::page(args.first().copied()) {
                    if page == messages::HELP_GIT {
                        for line in page.lines() {
                            self.players[player]
                                .send_color_message(crate::chat::ColorCode::Gray, line);
                        }
                    } else {
                        self.players[player].send_system_message(page);
                    }
                } else {
                    self.players[player]
                        .send_error_message(messages::UNKNOWN_HELP_TOPIC_USE_HELP_TOPICS);
                }
            }
            "/piston_anim" | "/bisdon_anim" => {
                if !self.players[player].has_permission("commands.piston_anim") {
                    self.players[player].send_no_permission_message();
                    return false;
                }
                if !args.is_empty() {
                    self.piston_animation = match args.as_slice() {
                        ["auto"] => mchprs_save_data::plot_data::PistonAnimation::Auto,
                        ["on"] => mchprs_save_data::plot_data::PistonAnimation::On,
                        ["off"] => mchprs_save_data::plot_data::PistonAnimation::Off,
                        _ => {
                            self.players[player]
                                .send_error_message(messages::USAGE_PISTON_ANIM_AUTO_ON_OFF);
                            return false;
                        }
                    };
                    self.update_render_mode();
                }
                self.players[player].send_system_message(&messages::piston_animation(
                    self.piston_animation,
                    if self.world.fast_rendering {
                        "off"
                    } else {
                        "on"
                    },
                ));
            }
            "/tellraw" | "/say" => {
                let permission = if command == "/say" {
                    "commands.say"
                } else {
                    "commands.tellraw"
                };
                if !self.players[player].has_permission(permission) {
                    self.players[player].send_no_permission_message();
                    return false;
                }
                let source = &self.players[player].username;
                match crate::chat_commands::parse(
                    &format!("{command} {}", args.join(" ")),
                    source,
                    Some(source),
                ) {
                    Ok(message) => {
                        self.message_sender
                            .send(Message::CommandChat(message))
                            .unwrap();
                    }
                    Err(error) => self.players[player].send_error_message(&error),
                }
            }
            "/version" => {
                self.players[player].send_system_message(&crate::server::version_string());
            }
            "/whitelist" => match args.as_slice() {
                [operation @ ("add" | "remove"), username] => {
                    let add = *operation == "add";
                    let username = username.to_string();
                    let sender = self.message_sender.clone();
                    let packet_sender = PlayerPacketSender::new(&self.players[player].client);
                    self.async_rt.spawn(async move {
                        match PlayerProfile::lookup_by_username(&username).await {
                            Ok(profile) => {
                                let message = if add {
                                    Message::WhitelistAdd(
                                        profile.uuid.0,
                                        profile.username,
                                        packet_sender,
                                    )
                                } else {
                                    Message::WhitelistRemove(profile.uuid.0, packet_sender)
                                };
                                sender.send(message).unwrap();
                            }
                            Err(_) => {
                                debug!("Failed to look up profile for username {:?}", username)
                            }
                        }
                    });
                }
                _ => {
                    self.players[player]
                        .send_error_message(messages::USAGE_WHITELIST_ADD_REMOVE_USERNAME);
                    return false;
                }
            },
            "/tps" => {
                if args == ["timings"] {
                    self.players[player].send_system_message(&self.update_timing_report());
                    return false;
                }
                if args.is_empty() {
                    let report = self.timings.generate_report();
                    if let Some(report) = report {
                        self.players[player].send_chat_message(
                            0,
                            &ChatComponent::from_legacy_text(&messages::rtps_report(
                                report.two_s,
                                report.ten_s,
                                report.one_m,
                                self.tps,
                            )),
                        );
                    } else {
                        self.players[player].send_chat_message(
                            0,
                            &ChatComponent::from_legacy_text(&messages::rtps_no_data(self.tps)),
                        );
                    }

                    return false;
                }

                let tps = if let Ok(tps) = args[0].parse::<u32>() {
                    Tps::Limited(tps)
                } else if !args[0].is_empty() && "unlimited".starts_with(args[0]) {
                    Tps::Unlimited
                } else {
                    self.players[player].send_error_message(messages::UNABLE_PARSE_RTPS);
                    return false;
                };

                self.sleep_time = sleep_time_for_tps(tps);
                self.timings.set_tps(tps);
                self.tps = tps;
                self.update_render_mode();
                self.reset_timings();
                self.players[player].send_system_message(messages::RTPS_SET);
            }
            "/rhistory" => match self.control_history(player, &args) {
                Ok(message) => {
                    info!(%message, "Tick history command completed");
                    self.players[player].send_system_message(&message);
                }
                Err(error) => {
                    warn!(%error, "Tick history command rejected");
                    self.players[player].send_error_message(&error);
                }
            },
            "/back" => {
                if let Err(error) = self.rewind_plot(player, &args) {
                    warn!(%error, "Rewind rejected");
                    self.players[player].send_error_message(&error);
                } else {
                    info!("Rewind completed");
                }
            }
            "/adv" => {
                let (unit, ticks) =
                    match AdvanceUnit::parse(&args, crate::config::CONFIG.max_command_ticks) {
                        Ok(request) => request,
                        Err(error) => {
                            self.players[player].send_error_message(&error);
                            return false;
                        }
                    };
                let start_time = Instant::now();
                if unit != AdvanceUnit::Game {
                    if self.redpiler.is_active() {
                        self.players[player].send_error_message(if unit == AdvanceUnit::Nano {
                            messages::CANNOT_ADVANCE_NANO_TICKS_WHILE_REDPILER
                        } else {
                            messages::CANNOT_ADVANCE_PICO_TICKS_WHILE_REDPILER
                        });
                        return false;
                    }
                    if self.world.history.enabled() {
                        self.players[player]
                            .send_error_message(messages::DISABLE_TICK_HISTORY_BEFORE_NANO_PICO);
                        return false;
                    }
                }
                let advanced = match unit {
                    AdvanceUnit::Game => advance_bounded(ticks, || self.tick()),
                    AdvanceUnit::Nano => advance_bounded(ticks, || self.world.nanotick_advance(1)),
                    AdvanceUnit::Pico => advance_bounded(ticks, || self.world.picotick_advance(1)),
                };
                let progress = messages::advance_progress(advanced, ticks, unit.label());
                self.players[player]
                    .send_system_message(&messages::plot_advanced(progress, start_time.elapsed()));
            }

            "/toggleautorp" => {
                self.auto_redpiler = !self.auto_redpiler;
                if self.auto_redpiler {
                    self.players[player].send_system_message(messages::REDPILER_AUTO_ENABLED);
                } else {
                    self.players[player].send_system_message(messages::REDPILER_AUTO_DISABLED);
                }
            }
            "/setwarp" => {
                let [name] = args.as_slice() else {
                    self.players[player].send_error_message(messages::USAGE_SETWARP);
                    return false;
                };
                if !valid_warp_name(name) {
                    self.players[player].send_error_message(messages::INVALID_WARP_NAME);
                    return false;
                }
                let actor = &self.players[player];
                let warp = database::Warp {
                    pos: actor.pos,
                    yaw: actor.yaw,
                    pitch: actor.pitch,
                };
                if !warp.is_valid() {
                    actor.send_error_message(messages::INVALID_TELEPORT_COORDINATES);
                    return false;
                }
                match database::set_warp(name, warp) {
                    Ok(()) => actor.send_system_message(&messages::warp_saved(name)),
                    Err(error) => {
                        warn!("Could not save warp {name}: {error}");
                        actor.send_error_message(messages::WARP_STORAGE_FAILED);
                    }
                }
            }
            "/warp" => {
                if args.is_empty() {
                    match database::warp_names() {
                        Ok(names) if names.is_empty() => {
                            self.players[player].send_system_message(messages::NO_WARPS)
                        }
                        Ok(names) => self.players[player]
                            .send_system_message(&messages::warp_list(names.join(", "))),
                        Err(error) => {
                            warn!("Could not list warps: {error}");
                            self.players[player].send_error_message(messages::WARP_STORAGE_FAILED);
                        }
                    }
                    return false;
                }
                let [name] = args.as_slice() else {
                    self.players[player].send_error_message(messages::USAGE_WARP);
                    return false;
                };
                if !valid_warp_name(name) {
                    self.players[player].send_error_message(messages::INVALID_WARP_NAME);
                    return false;
                }
                match database::get_warp(name) {
                    Ok(Some(warp)) => {
                        if warp.is_valid() {
                            self.players[player]
                                .send_system_message(&messages::warp_teleport(name));
                        }
                        return self.teleport_to_warp(player, warp);
                    }
                    Ok(None) => {
                        self.players[player].send_error_message(&messages::warp_not_found(name))
                    }
                    Err(error) => {
                        warn!("Could not read warp {name}: {error}");
                        self.players[player].send_error_message(messages::WARP_STORAGE_FAILED);
                    }
                }
            }
            "/teleport" | "/tp" => {
                if args.len() == 3 {
                    let player_pos = self.players[player].pos;
                    let x;
                    let y;
                    let z;
                    if let Ok(x_arg) = parse_teleport_coord(args[0], player_pos.x, true) {
                        x = x_arg;
                    } else {
                        self.players[player].send_error_message(messages::INVALID_X_COORDINATE);
                        return false;
                    }
                    if let Ok(y_arg) = parse_teleport_coord(args[1], player_pos.y, false) {
                        y = y_arg;
                    } else {
                        self.players[player].send_error_message(messages::INVALID_Y_COORDINATE);
                        return false;
                    }
                    if let Ok(z_arg) = parse_teleport_coord(args[2], player_pos.z, true) {
                        z = z_arg;
                    } else {
                        self.players[player].send_error_message(messages::INVALID_Z_COORDINATE);
                        return false;
                    }
                    self.players[player]
                        .send_system_message(&messages::teleport_coordinates(x, y, z));
                    self.players[player].teleport(PlayerPos::new(x, y, z));
                } else if args.len() == 1 {
                    self.players[player].send_system_message(&messages::teleport_player(args[0]));
                    let uuid = self.players[player].uuid;
                    let player = self.leave_plot(uuid);
                    let _ = self
                        .message_sender
                        .send(Message::PlayerTeleportOther(player, args[0].to_string()));
                    return true;
                } else {
                    self.players[player]
                        .send_error_message(messages::INVALID_NUMBER_ARGUMENTS_TELEPORT_COMMAND);
                }
            }
            "/stop" => {
                let _ = self.message_sender.send(Message::Shutdown);
            }
            "/plot" | "/p" => {
                if args.is_empty() {
                    self.players[player].send_error_message(messages::INVALID_ARGUMENT_COUNT);
                    return false;
                }
                let command = args.remove(0);
                self.handle_plot_command(player, command, &args);
            }
            "/redpiler" | "/rp" => {
                if args.is_empty() {
                    self.players[player].send_error_message(messages::INVALID_ARGUMENT_COUNT);
                    return false;
                }
                let command = args.remove(0);
                self.handle_redpiler_command(player, command, &args);
            }
            "/speed" => {
                if args.len() != 1 {
                    self.players[player].send_error_message(messages::USAGE_SPEED);
                    return false;
                }
                if let Ok(speed_arg) = args[0].parse::<f32>() {
                    if speed_arg < 0.0 {
                        self.players[player].send_error_message(messages::SPEED_NEGATIVE);
                        return false;
                    }
                    if speed_arg > 10.0 {
                        self.players[player].send_error_message(
                            messages::PERFORMANCE_REASONS_PLAYER_SPEED_CANNOT_HIGHER,
                        );
                        return false;
                    }
                    if speed_arg.is_nan() {
                        self.players[player].send_error_message(messages::YOU_CAN_T_SET_SPEED_NAN);
                        return false;
                    }
                    self.players[player].fly_speed = speed_arg;
                    self.players[player].update_player_abilities();
                    let username = self.players[player].username.clone();
                    self.players[player]
                        .send_system_message(&messages::flying_speed(speed_arg, username));
                } else {
                    self.players[player].send_error_message(messages::UNABLE_PARSE_SPEED_VALUE);
                }
            }
            "/gmsp" => self.change_player_gamemode(player, Gamemode::Spectator),
            "/gmc" => self.change_player_gamemode(player, Gamemode::Creative),
            "/gamemode" | "/gm" => {
                if args.len() != 1 {
                    self.players[player].send_error_message(messages::INVALID_ARGUMENT_COUNT);
                    return false;
                }
                let name = args.remove(0);
                let gamemode = match name {
                    "creative" | "1" => Gamemode::Creative,
                    "adventure" | "2" => Gamemode::Adventure,
                    "spectator" | "3" => Gamemode::Spectator,
                    _ => {
                        self.players[player].send_error_message(messages::UNKNOWN_GAMEMODE);
                        return false;
                    }
                };
                self.change_player_gamemode(player, gamemode);
            }
            "/worldsendrate" | "/wsr" => {
                if args.is_empty() {
                    self.players[player].send_system_message(&messages::world_send_rate(
                        self.world_send_rate.0,
                        self.effective_send_rate(),
                    ));
                    return false;
                }
                if args.len() != 1 {
                    self.players[player].send_error_message(messages::USAGE_WORLDSENDRATE_HERTZ);
                    return false;
                }

                let Ok(hertz) = args[0].parse::<u32>() else {
                    self.players[player].send_error_message(messages::UNABLE_PARSE_SEND_RATE);
                    return false;
                };
                if hertz > 1000 {
                    self.players[player]
                        .send_error_message(messages::WORLD_SEND_RATE_CANNOT_GO_HIGHER);
                    return false;
                }

                self.world_send_rate = WorldSendRate(hertz);
                self.reset_timings();
                self.players[player]
                    .send_system_message(messages::WORLD_SEND_RATE_WAS_SUCCESSFULLY_SET);
            }
            "/curse" => {
                if self.world.is_cursed {
                    self.players[player].send_system_message(messages::WORLD_ALREADY_CURSED);
                } else {
                    self.world.is_cursed = true;
                    self.auto_redpiler = false;
                    self.players[player]
                        .send_system_message(messages::WORLD_BEEN_CURSED_REDPILER_DISABLED_BLESS);
                }
                return false;
            }
            "/bless" => {
                if self.world.is_cursed {
                    self.world.is_cursed = false;
                    self.players[player].send_system_message(messages::WORLD_BEEN_BLESSED);
                } else {
                    self.players[player]
                        .send_system_message(messages::WORLD_NOT_CURSED_CURSE_CURSE);
                }
                return false;
            }
            _ => self.players[player].send_error_message(messages::COMMAND_NOT_FOUND),
        }
        false
    }
}

/// Native command permissions are checked before dispatch, including aliases.
fn native_command_permission(command: &str, args: &[&str]) -> Option<String> {
    let action = if args.is_empty() { "view" } else { "set" };
    let name = match command {
        "/help" => "help".to_owned(),
        "/git" => "git".to_owned(),
        "/version" => "version".to_owned(),
        "/teleport" | "/tp" => "teleport".to_owned(),
        "/warp" => "warp".to_owned(),
        "/setwarp" => "setwarp".to_owned(),
        "/speed" => "speed".to_owned(),
        "/gmsp" | "/gmc" | "/gamemode" | "/gm" => "gamemode".to_owned(),
        "/stop" => "stop".to_owned(),
        "/whitelist" => "whitelist".to_owned(),
        "/tellraw" => "tellraw".to_owned(),
        "/say" => "say".to_owned(),
        "/tps" => format!(
            "rtps.{}",
            if args.is_empty() || args == ["timings"] {
                "view"
            } else {
                "set"
            }
        ),
        "/worldsendrate" | "/wsr" => format!("worldsendrate.{action}"),
        "/screenonly" => format!("screenonly.{action}"),
        "/piston_anim" | "/bisdon_anim" => format!("piston_anim.{action}"),
        "/adv" => "radvance".to_owned(),
        "/toggleautorp" => "toggleautorp".to_owned(),
        "/curse" => "curse".to_owned(),
        "/bless" => "bless".to_owned(),
        "/redpiler" | "/rp" => format!(
            "redpiler.{}",
            match args.first().copied() {
                Some("c" | "compile") => "compile",
                Some("r" | "reset") => "reset",
                Some("i" | "inspect") => "inspect",
                Some("analyze") => "analyze",
                _ => "help",
            }
        ),
        // Plot, history, WorldEdit and redstone tools check their own nodes.
        _ => return None,
    };
    Some(format!("commands.{name}"))
}

fn changes_plot(command: &str, args: &[&str]) -> bool {
    match command {
        "/tps" => !args.is_empty() && args != ["timings"],
        "/worldsendrate" | "/wsr" | "/screenonly" | "/piston_anim" | "/bisdon_anim" => {
            !args.is_empty()
        }
        "/adv" | "/toggleautorp" | "/curse" | "/bless" => true,
        "/redpiler" | "/rp" => !matches!(args.first().copied(), Some("inspect" | "i" | "analyze")),
        _ => false,
    }
}

pub static NO_COMMANDS: Lazy<PacketEncoder> = Lazy::new(|| {
    CDeclareCommands {
        nodes: &[Node::root(&[])],
        root_index: 0,
    }
    .encode()
});

/// The `DeclareCommands` packet that is sent when the player joins.
/// This is used for command autocomplete.
fn declared_command_nodes() -> Vec<Node<'static>> {
    vec![
        // 0: Root Node
        Node::root(&[
            1, 4, 5, 6, 11, 12, 14, 16, 18, 19, 20, 21, 22, 23, 24, 26, 29, 31, 33, 35, 46, 48, 52,
            59, 60, 62, 64, 65, 66, 70, 72, 73, 74, 81, 82, 84, 87, 89, 90, 100, 105, 110, 111,
            112, 113, 114, 115, 117, 119, 120, 123, 130, 134, 135, 142, 143, 144, 145, 146, 150,
            152, 153,
        ]),
        // 1: /teleport
        Node::literal("teleport", &[3, 2]),
        // 2: /teleport [x, y, z]
        Node::argument("x, y, z", Parser::Vec3, &[]).executable(),
        // 3: /teleport [player]
        Node::argument("player", Parser::String(0), &[])
            .executable()
            .suggestions("minecraft:ask_server"),
        // 4: /tp
        Node::redirect("tp", 1),
        // 5: /stop
        Node::literal("stop", &[]).executable(),
        // 6: /plot
        Node::literal(
            "plot",
            &[
                7, 8, 9, 10, 37, 38, 39, 40, 42, 43, 45, 57, 58, 79, 80, 124, 125, 126, 128,
            ],
        ),
        // 7: /plot info
        Node::literal("info", &[]).executable(),
        // 8: /plot i
        Node::redirect("i", 7),
        // 9: /plot claim
        Node::literal("claim", &[]).executable(),
        // 10: /plot c
        Node::redirect("c", 9),
        // 11: /p
        Node::redirect("p", 6),
        // 12: /tps
        Node::literal("tps", &[13, 116]).executable(),
        // 13: /tps [tps]
        Node::argument("tps", Parser::Integer(0, i32::MAX), &[]).executable(),
        // 14: //pos1
        Node::literal("/pos1", &[15]).executable(),
        // 15: //pos1 [pos]
        Node::argument("pos", Parser::BlockPos, &[]).executable(),
        // 16: //pos2
        Node::literal("/pos2", &[17]).executable(),
        // 17: //pos2 [pos]
        Node::argument("pos", Parser::BlockPos, &[]).executable(),
        // 18: /1
        Node::redirect("/1", 14),
        // 19: /2
        Node::redirect("/2", 16),
        // 20: //copy
        Node::literal("/copy", &[]).executable(),
        // 21: //c
        Node::redirect("/c", 20),
        // 22: //paste
        Node::literal("/paste", &[]).executable(),
        // 23: //p
        Node::redirect("/p", 22),
        // 24: //set
        Node::literal("/set", &[25]),
        // 25: //set [block]
        Node::argument("block", Parser::String(0), &[]).executable(),
        // 26: //replace
        Node::literal("/replace", &[27]),
        // 27: //replace [oldblock]
        Node::argument("oldblock", Parser::BlockState, &[28]),
        // 28: //replace [oldblock] [newblock]
        Node::argument("newblock", Parser::BlockState, &[]).executable(),
        // 29: /adv
        Node::literal("adv", &[30, 75, 77]),
        // 30: /adv [rticks]
        Node::argument("rticks", Parser::Integer(0, 100000), &[]).executable(),
        // 31: /speed
        Node::literal("speed", &[32]),
        // 32: /speed [speed]
        Node::argument("speed", Parser::Float(0.0, 10.0), &[]).executable(),
        // 33: //stack
        Node::literal("/stack", &[34]).executable(),
        // 34: //stack [amount]
        Node::argument("amount", Parser::Integer(0, 256), &[]).executable(),
        // 35: //undo
        Node::literal("/undo", &[]).executable(),
        // 36: //sel
        Node::literal("/sel", &[]).executable(),
        // 37: /p auto
        Node::literal("auto", &[]).executable(),
        // 38: /p a
        Node::redirect("a", 9),
        // 39: /p middle
        Node::literal("middle", &[]).executable(),
        // 40: /p visit
        Node::literal("visit", &[41]),
        // 41: /p visit [player]
        Node::argument("player", Parser::Entity(3), &[]).executable(),
        // 42: /p v
        Node::redirect("v", 40),
        // 43: /p teleport
        Node::literal("teleport", &[44]),
        // 44: /p teleport [x, z]
        Node::argument("x, z", Parser::Vec2, &[]).executable(),
        // 45: /p tp
        Node::redirect("tp", 43),
        // 46: //shift
        Node::literal("/shift", &[34]).executable(),
        // 47: //shift [amount]
        Node::argument("amount", Parser::Integer(0, 256), &[]).executable(),
        // 48: /whitelist
        Node::literal("whitelist", &[49, 50]),
        // 49: /whitelist add
        Node::literal("add", &[51]),
        // 50: /whitelist remove
        Node::literal("remove", &[51]),
        // 51: /whitelist add|remove [username]
        Node::argument("username", Parser::Entity(3), &[]).executable(),
        // 52-56: /container <type> <power>
        Node::literal("container", &[53, 54, 55]),
        Node::argument("type", Parser::String(0), &[56]).suggestions("minecraft:ask_server"),
        // Preserve the existing literal alternatives and their node indices.
        Node::literal("hopper", &[56]),
        Node::literal("furnace", &[56]),
        Node::argument("power", Parser::String(0), &[])
            .executable()
            .suggestions("minecraft:ask_server"),
        // 57: /plot lock
        Node::literal("lock", &[]).executable(),
        // 58: /plot unlock
        Node::literal("unlock", &[]).executable(),
        // 59: //wand
        Node::literal("/wand", &[]).executable(),
        // 60: //save
        Node::literal("/save", &[61]),
        // 61: //save [filename]
        Node::argument("filename", Parser::String(0), &[]).executable(),
        // 62: //load
        Node::literal("/load", &[63]),
        // 63: //load [filename]
        Node::argument("filename", Parser::String(0), &[])
            .executable()
            .suggestions("minecraft:ask_server"),
        // 64: /toggleautorp
        Node::literal("toggleautorp", &[]).executable(),
        // 65: /redpiler
        Node::literal("redpiler", &[67, 68, 69, 131, 147, 148, 149]),
        // 66: /rp
        Node::redirect("rp", 65),
        // 67: /redpiler compile
        Node::literal("compile", &[133]).executable(),
        // 68: /redpiler inspect
        Node::literal("inspect", &[]).executable(),
        // 69: /redpiler reset
        Node::literal("reset", &[]).executable(),
        // 70: /worldsendrate
        Node::literal("worldsendrate", &[71]).executable(),
        // 71: /worldsendrate [rticks]
        Node::argument("hertz", Parser::Integer(0, 1000), &[]).executable(),
        // 72: /wsr
        Node::redirect("wsr", 70),
        // 73: /curse
        Node::literal("curse", &[]),
        // 74: /bless
        Node::literal("bless", &[]),
        // 75: /adv nano
        Node::literal("nano", &[76]),
        // 76: /adv nano [nticks]
        Node::argument("nticks", Parser::Integer(0, 100000), &[]).executable(),
        // 77: /adv pico
        Node::literal("pico", &[78]),
        // 78: /adv pico [pticks]
        Node::argument("pticks", Parser::Integer(0, 100000), &[]).executable(),
        // 79: /plot select
        Node::literal("select", &[]).executable(),
        // 80: /plot sel
        Node::redirect("sel", 79),
        // 81: /version
        Node::literal("version", &[]).executable(),
        // 82–86: /say <message>, /tellraw <targets> <JSON text>
        Node::literal("say", &[83]),
        Node::argument("message", Parser::String(2), &[]).executable(),
        Node::literal("tellraw", &[85]),
        Node::argument("targets", Parser::Entity(2), &[86]),
        Node::argument("message", Parser::String(2), &[]).executable(),
        // 87–89: animation preference and alias
        Node::literal("piston_anim", &[88]).executable(),
        Node::argument("mode", Parser::String(0), &[]).executable(),
        Node::redirect("bisdon_anim", 87).executable(),
        // 90–99: /help and its topic suggestions
        Node::literal("help", &[91, 92, 93, 94, 95, 96, 97, 98, 99]).executable(),
        Node::argument("topic", Parser::String(0), &[]).executable(),
        Node::literal("plots", &[]).executable(),
        Node::literal("tps", &[]).executable(),
        Node::literal("we", &[]).executable(),
        Node::literal("schematics", &[]).executable(),
        Node::literal("pistons", &[]).executable(),
        Node::literal("rewind", &[]).executable(),
        Node::literal("chat", &[]).executable(),
        Node::literal("redpiler", &[]).executable(),
        // 100–106: tick history and whole-game-tick rewind
        Node::literal("rhistory", &[101, 103, 104, 107]).executable(),
        Node::literal("on", &[102]).executable(),
        Node::argument(
            messages::ADV_GAME_TICKS_LABEL,
            Parser::Integer(1, i32::MAX),
            &[],
        )
        .executable(),
        Node::literal("off", &[]).executable(),
        Node::literal("status", &[]).executable(),
        Node::literal("back", &[106]).executable(),
        Node::argument(
            messages::ADV_GAME_TICKS_LABEL,
            Parser::Integer(1, i32::MAX),
            &[],
        )
        .executable(),
        // 107–108: server history memory limit, in MiB
        Node::literal("limit", &[108]).executable(),
        Node::argument("MiB", Parser::Integer(0, i32::MAX), &[]).executable(),
        // 109: flexible tool arguments, validated by the command handler
        Node::argument("arguments", Parser::String(2), &[])
            .executable()
            .suggestions("minecraft:ask_server"),
        // 110: //find
        Node::literal("/find", &[109]).executable(),
        // 111: //signsearch
        Node::literal("/signsearch", &[109]).executable(),
        // 112: //ss
        Node::literal("/ss", &[109]).executable(),
        // 113: //rstack
        Node::literal("/rstack", &[109]).executable(),
        // 114: //rs
        Node::literal("/rs", &[109]).executable(),
        // 115: /cursel
        Node::literal("cursel", &[109]).executable(),
        // 116: /tps timings
        Node::literal("timings", &[]).executable(),
        // 117: //update
        Node::literal("/update", &[118]).executable(),
        // 118: //update -p
        Node::literal("-p", &[]).executable(),
        // 119: //invalidatecaches
        Node::literal("/invalidatecaches", &[]).executable(),
        // 120-122: /screenonly [on|off]
        Node::literal("screenonly", &[121, 122]).executable(),
        Node::literal("on", &[]).executable(),
        Node::literal("off", &[]).executable(),
        // 123: /autostack
        Node::literal("autostack", &[109]).executable(),
        // 124-129: /p home, /p h, /p add <nick>, /p remove <nick>.
        Node::literal("home", &[]).executable(),
        Node::redirect("h", 124),
        Node::literal("add", &[127]),
        Node::argument("nick", Parser::String(0), &[])
            .executable()
            .suggestions("minecraft:ask_server"),
        Node::literal("remove", &[129]),
        Node::argument("nick", Parser::String(0), &[])
            .executable()
            .suggestions("minecraft:ask_server"),
        // 130: /git uses the existing greedy, server-completed arguments node.
        Node::literal("git", &[109]).executable(),
        // 131: /redpiler analyze
        Node::literal("analyze", &[132, 133]).executable(),
        // 132-133: read-only graph preparation and ordinary optimization flags.
        Node::literal("--graph", &[133]).executable(),
        Node::argument("options", Parser::String(2), &[])
            .executable()
            .suggestions("minecraft:ask_server"),
        // 134-143: gamemode alias, names, IDs and legacy shortcuts.
        Node::redirect("gm", 135),
        Node::literal("gamemode", &[136, 137, 138, 139, 140, 141]),
        Node::literal("creative", &[]).executable(),
        Node::literal("adventure", &[]).executable(),
        Node::literal("spectator", &[]).executable(),
        Node::literal("1", &[]).executable(),
        Node::literal("2", &[]).executable(),
        Node::literal("3", &[]).executable(),
        Node::literal("gmc", &[]).executable(),
        Node::literal("gmsp", &[]).executable(),
        // 144-145: clear the WorldEdit selection.
        Node::redirect("/desel", 36).executable(),
        Node::redirect("desel", 36).executable(),
        // 146: /set accepts the same block patterns as //set.
        Node::redirect("set", 24),
        // 147-149: Redpiler subcommand aliases share arguments and suggestions.
        Node::redirect("c", 67).executable(),
        Node::redirect("i", 68).executable(),
        Node::redirect("r", 69).executable(),
        // 150-152: shared, saved destinations.
        Node::literal("warp", &[151]).executable(),
        Node::argument("name", Parser::String(0), &[])
            .executable()
            .suggestions("minecraft:ask_server"),
        Node::literal("setwarp", &[151]),
        // 153-159: administrator diagnostics and optional page/plot arguments.
        Node::literal("serverinfo", &[154, 156, 159]).executable(),
        Node::literal("plots", &[155]).executable(),
        Node::argument("page", Parser::Integer(1, i32::MAX), &[]).executable(),
        Node::literal("plot", &[157]).executable(),
        Node::argument("x", Parser::Integer(i32::MIN, i32::MAX), &[158]),
        Node::argument("z", Parser::Integer(i32::MIN, i32::MAX), &[]).executable(),
        Node::literal("settings", &[]).executable(),
    ]
}

pub static DECLARE_COMMANDS: Lazy<PacketEncoder> = Lazy::new(|| {
    CDeclareCommands {
        nodes: &declared_command_nodes(),
        root_index: 0,
    }
    .encode()
});

#[cfg(test)]
mod security_tests {
    use super::*;
    #[test]
    fn teleport_autocomplete_suggests_player_names_without_selectors() {
        let nodes = declared_command_nodes();
        assert_eq!(nodes[1].children, &[3, 2]);
        assert!(matches!(nodes[3].parser, Some(Parser::String(0))));
        assert_eq!(nodes[3].suggestions_type, Some("minecraft:ask_server"));
        let names = ["Zoe", "Alice", "Alex"];
        for (text, expected) in [
            ("/tp ", vec!["Alex", "Alice", "Zoe"]),
            ("/teleport al", vec!["Alex", "Alice"]),
            ("/tp @", vec![]),
            ("/tp 1 2 ", vec![]),
        ] {
            let response = complete_teleport(42, text, names.into_iter()).unwrap();
            let start = text.find(' ').unwrap() + 1;
            assert_eq!(response.id, 42);
            assert_eq!(response.start, start as i32);
            assert_eq!(response.length, text[start..].encode_utf16().count() as i32);
            assert_eq!(
                response
                    .matches
                    .into_iter()
                    .map(|item| item.match_)
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }
    #[test]
    fn serverinfo_autocomplete_declares_subcommands_and_numeric_arguments() {
        let nodes = declared_command_nodes();
        let serverinfo = nodes
            .iter()
            .find(|node| node.name == Some("serverinfo"))
            .unwrap();
        assert_eq!(
            serverinfo
                .children
                .iter()
                .map(|&id| nodes[id as usize].name.unwrap())
                .collect::<Vec<_>>(),
            ["plots", "plot", "settings"]
        );
        for &id in serverinfo.children {
            let node = &nodes[id as usize];
            assert_eq!(
                node.flags & 0x07,
                0x05,
                "subcommands must be executable literals"
            );
        }
        let page = &nodes[nodes[154].children[0] as usize];
        assert_eq!(page.name, Some("page"));
        assert!(matches!(page.parser, Some(Parser::Integer(1, i32::MAX))));
        let x = &nodes[nodes[156].children[0] as usize];
        let z = &nodes[x.children[0] as usize];
        assert_eq!((x.name, z.name), (Some("x"), Some("z")));
        for node in [x, z] {
            assert!(matches!(
                node.parser,
                Some(Parser::Integer(i32::MIN, i32::MAX))
            ));
        }
        assert_eq!(x.flags & 0x04, 0);
        assert_eq!(z.flags & 0x04, 0x04);
    }
    #[test]
    fn command_declarations_have_valid_edges_and_no_legacy_tick_aliases() {
        let nodes = declared_command_nodes();
        assert_eq!(nodes.len(), 160);
        for node in &nodes {
            for edge in node.children.iter().copied().chain(node.redirect_node) {
                assert!(edge >= 0 && (edge as usize) < nodes.len());
            }
        }
        let names: Vec<_> = nodes[0]
            .children
            .iter()
            .filter_map(|index| nodes[*index as usize].name)
            .collect();
        for removed in ["rtps", "radv", "radvance", "rback"] {
            assert!(!nodes.iter().any(|node| node.name == Some(removed)));
            let command = format!("/{removed}");
            assert!(native_command_permission(&command, &["1"]).is_none());
            assert!(!changes_plot(&command, &["1"]));
        }
        for retained in [
            "tps",
            "adv",
            "back",
            "rhistory",
            "redpiler",
            "rp",
            "gm",
            "warp",
            "setwarp",
            "serverinfo",
        ] {
            assert!(names.contains(&retained), "missing command {retained}");
        }
        for (alias, target) in [
            ("rp", "redpiler"),
            ("gm", "gamemode"),
            ("tp", "teleport"),
            ("/desel", "/sel"),
            ("desel", "/sel"),
            ("set", "/set"),
        ] {
            let node = nodes.iter().find(|node| node.name == Some(alias)).unwrap();
            assert_eq!(
                nodes[node.redirect_node.unwrap() as usize].name,
                Some(target)
            );
        }
        assert_eq!(NO_COMMANDS.packet_id, 0x10);
        assert_eq!(
            format!("{:x}", md5::compute(&NO_COMMANDS.buffer)),
            "4352d88a78aa39750bf70cd6f27bcaa5"
        );
    }
    #[test]
    fn warp_names_and_permissions_are_checked() {
        for name in ["spawn", "CPU-1", "my_plot", &"a".repeat(32)] {
            assert!(valid_warp_name(name));
        }
        for name in ["", "two words", "../spawn", "café", "'", &"a".repeat(33)] {
            assert!(!valid_warp_name(name));
        }
        let nodes = declared_command_nodes();
        for command in ["warp", "setwarp"] {
            let node = nodes
                .iter()
                .find(|node| node.name == Some(command))
                .unwrap();
            assert_eq!(
                nodes[node.children[0] as usize].suggestions_type,
                Some("minecraft:ask_server")
            );
            assert_eq!(
                native_command_permission(&format!("/{command}"), &["spawn"]),
                Some(format!("commands.{command}"))
            );
            assert!(!changes_plot(&format!("/{command}"), &["spawn"]));
        }
    }

    #[test]
    fn advance_arguments_preserve_units_limits_and_error_messages() {
        for (args, expected) in [
            (vec!["0"], (AdvanceUnit::Game, 0)),
            (vec!["+10"], (AdvanceUnit::Game, 10)),
            (vec!["NaNo", "10"], (AdvanceUnit::Nano, 10)),
            (vec!["PICO", "1"], (AdvanceUnit::Pico, 1)),
        ] {
            assert_eq!(AdvanceUnit::parse(&args, 10), Ok(expected));
        }
        for args in [vec![], vec!["game", "1"], vec!["nano", "1", "extra"]] {
            assert_eq!(
                AdvanceUnit::parse(&args, 10).unwrap_err(),
                messages::USAGE_ADV
            );
        }
        for args in [
            vec!["nano"],
            vec!["11"],
            vec!["-1"],
            vec!["pico", "invalid"],
            vec!["nano", "4294967296"],
        ] {
            assert_eq!(
                AdvanceUnit::parse(&args, 10).unwrap_err(),
                messages::advance_tick_count_limit(10)
            );
        }
    }

    #[test]
    fn queued_commands_stop_when_the_actor_leaves() {
        let mut calls = Vec::new();
        assert!(run_command_queue(
            vec!["/tp Admin".into(), "/stop".into()],
            |command, _| {
                calls.push(command.to_owned());
                command == "/tp"
            }
        ));
        assert_eq!(calls, ["/tp"]);
        assert!(!run_command_queue(
            vec!["  ".into(), "/help".into()],
            |_, _| false
        ));
    }
    #[test]
    fn teleport_coordinates_center_blocks_and_keep_precision() {
        for (input, reference, center, expected) in [
            ("10", 0.0, true, 10.5),
            ("-10", 0.0, true, -9.5),
            ("64", 0.0, false, 64.0),
            ("10.0", 0.0, true, 10.0),
            ("10.25", 0.0, true, 10.25),
            ("~", 2.25, true, 2.25),
            ("~1", 2.25, true, 3.25),
        ] {
            assert_eq!(parse_teleport_coord(input, reference, center), Ok(expected));
        }
        for input in ["NaN", "inf", "1e309", "~1e309", "invalid"] {
            assert!(parse_teleport_coord(input, 0.0, true).is_err());
        }
        assert_eq!(Gamemode::Adventure.get_id(), 2);
    }

    #[test]
    fn relative_plot_coordinates_cannot_overflow() {
        assert!(parse_relative_coord("~2147483647", 1i32).is_err());
        assert!(parse_relative_coord("~-2147483648", -1i32).is_err());
        assert!(parse_relative_coord("~1e309", 0.0f64).is_err());
        assert_eq!(parse_relative_coord("~-1", 2i32).unwrap(), 1);
    }
    #[test]
    fn native_aliases_share_permissions_and_ownership_checks() {
        for (alias, canonical, args) in [
            ("/bisdon_anim", "/piston_anim", vec!["off"]),
            ("/wsr", "/worldsendrate", vec!["100"]),
            ("/rp", "/redpiler", vec!["compile"]),
            ("/tp", "/teleport", vec!["Admin"]),
            ("/gm", "/gamemode", vec!["2"]),
        ] {
            assert_eq!(
                native_command_permission(alias, &args),
                native_command_permission(canonical, &args)
            );
            assert_eq!(changes_plot(alias, &args), changes_plot(canonical, &args));
        }
    }

    #[test]
    fn redpiler_analysis_has_a_read_only_permission() {
        for command in ["/rp", "/redpiler"] {
            assert_eq!(
                native_command_permission(command, &["analyze"]).as_deref(),
                Some("commands.redpiler.analyze")
            );
            assert!(!changes_plot(command, &["analyze"]));
            assert!(changes_plot(command, &["compile"]));
        }
    }

    #[test]
    fn redpiler_analysis_reports_graph_passes_and_compile_statistics_to_the_client() {
        use crate::world::World;
        use mchprs_blocks::blocks::{Block, Lever, LeverFace};
        use mchprs_blocks::{BlockDirection, BlockPos};
        use mchprs_network::packets::PacketDecoderExt;
        use mchprs_network::test_support::read_frame;

        for compressed in [false, true] {
            let (mut plot, mut peer) = crate::plot::client_sync_tests::fixture(compressed);
            let control = BlockPos::new(32, 21, 32);
            let output = BlockPos::new(33, 21, 32);
            plot.world.set_block(
                control,
                Block::Lever {
                    lever: Lever::new(LeverFace::Floor, BlockDirection::North, true),
                },
            );
            plot.world
                .set_block(output, Block::RedstoneLamp { lit: true });
            let before = [plot.world.get_block(control), plot.world.get_block(output)];
            let mut messages = |last: &str| {
                let mut messages = Vec::new();
                for _ in 0..32 {
                    let (id, mut frame) = read_frame(&mut peer, compressed).unwrap();
                    assert_eq!(id, 0x72, "analysis must send system chat");
                    // Wire text is an unnamed NBT compound; restore its empty
                    // root name for the storage NBT reader.
                    assert_eq!(frame.read_unsigned_byte().unwrap(), 10);
                    let mut named = vec![10, 0, 0];
                    named.extend(frame.read_to_end().unwrap());
                    let mut named = std::io::Cursor::new(named);
                    let component = nbt::Blob::from_reader(&mut named).unwrap();
                    let nbt::Value::String(text) = &component.content["text"] else {
                        panic!("system chat is missing its text");
                    };
                    assert!(!named.read_bool().unwrap());
                    assert_eq!(named.position() as usize, named.get_ref().len());
                    let text = text.clone();
                    let complete = text.contains(last);
                    messages.push(text);
                    if complete {
                        return messages;
                    }
                }
                panic!("analysis response did not complete");
            };
            for (args, folding) in [
                (vec!["--graph"], "Constant folding [skipped]"),
                (
                    vec!["--graph", "--optimize", "--io-only"],
                    "Constant folding [enabled]",
                ),
            ] {
                plot.handle_redpiler_command(0, "analyze", &args);
                let lines = messages("Exporting graph [skipped]");
                assert!(lines
                    .iter()
                    .any(|line| line.starts_with("Candidate graph:")));
                assert!(lines
                    .iter()
                    .any(|line| line.starts_with("Graph after required preparation:")));
                assert!(lines.iter().any(|line| line.contains(folding)
                    && line.contains("links")
                    && line.ends_with("ms")));
                assert!(!plot.redpiler.is_active());
            }
            plot.handle_redpiler_command(0, "analyze", &[]);
            let lines = messages("This plot can compile with these flags.");
            assert!(lines.iter().any(|line| line.starts_with("0 pistons,")));
            assert!(lines
                .iter()
                .any(|line| line.starts_with("Compile:") && line.contains("backend nodes")));
            assert!(!plot.redpiler.is_active());
            assert_eq!(
                [plot.world.get_block(control), plot.world.get_block(output)],
                before
            );
        }
    }

    #[test]
    fn redpiler_flags_complete_aliases_prefixes_and_multiple_options() {
        let nodes = declared_command_nodes();
        for command in ["compile", "analyze"] {
            let node = nodes.iter().find(|n| n.name == Some(command)).unwrap();
            assert!(node.children.iter().any(|&id| {
                nodes[id as usize].suggestions_type == Some("minecraft:ask_server")
            }));
        }
        assert_eq!(
            nodes[nodes[147].redirect_node.unwrap() as usize].name,
            Some("compile")
        );
        for (text, expected) in [
            ("/rp compile --ass", vec!["--assume-instant"]),
            ("/redpiler c --optimize --i", vec!["--io-only"]),
            (
                "/rp analyze --graph --assume-instant --o",
                vec!["--optimize"],
            ),
            (
                "/rp compile -",
                vec![
                    "--assume-instant",
                    "--optimize",
                    "--io-only",
                    "--update",
                    "--export",
                    "--export-dot",
                    "-o",
                    "-i",
                    "-u",
                    "-e",
                ],
            ),
            ("/rp analyze --ex", vec![]),
            ("/rp compile --unknown", vec![]),
        ] {
            let response = complete_redpiler(42, text).unwrap();
            let start = text.rfind(' ').unwrap() + 1;
            assert_eq!(response.id, 42);
            assert_eq!(response.start, text[..start].encode_utf16().count() as i32);
            assert_eq!(response.length, text[start..].encode_utf16().count() as i32);
            assert_eq!(
                response
                    .matches
                    .iter()
                    .map(|m| m.match_.as_str())
                    .collect::<Vec<_>>(),
                expected
            );
            for item in response.matches {
                assert!(item.match_ == "--graph" || CompilerOptions::parse(&item.match_).is_ok());
            }
        }
        let response = complete_redpiler(0, "/rp analyze --graph ").unwrap();
        assert!(!response.matches.iter().any(|m| m.match_ == "--graph"));
        assert!(complete_redpiler(0, "/rp reset ").is_none());
        assert!(complete_redpiler(0, "/git compile ").is_none());
    }
}
