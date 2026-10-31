mod declaration;
mod plot;
mod redpiler;
pub(super) use redpiler::complete_redpiler;

use super::{Plot, database, worldedit};
use crate::chat::ChatComponent;
use crate::messages;
use crate::player::{Gamemode, PacketSender, PlayerPos};
use crate::plot::data::sleep_time_for_tps;
use crate::profile::PlayerProfile;
#[cfg(test)]
use crate::redpiler::CompilerOptions;
use crate::server::Message;
use mchprs_network::PlayerPacketSender;
use mchprs_network::packets::clientbound::{CTabComplete, CTabCompleteMatch};
use mchprs_save_data::plot_data::{Tps, WorldSendRate};
use std::str::FromStr;
use std::time::Instant;
use tracing::{debug, info, warn};

// Parses a relative or absolute coordinate relative to a reference coordinate
trait RelativeCoordinate: FromStr {
    fn checked_offset(self, offset: Self) -> Option<Self>;
}

/// Stop dispatch as soon as a command transfers the indexed player away.
pub(super) fn run_command_queue(commands: Vec<String>, mut handle: impl FnMut(&str, Vec<&str>) -> bool) -> bool {
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

impl RelativeCoordinate for f64 {
    fn checked_offset(self, offset: Self) -> Option<Self> {
        let result = self + offset;
        result.is_finite().then_some(result)
    }
}

fn parse_relative_coord<F: RelativeCoordinate>(coord: &str, ref_coord: F) -> Result<F, &'static str> {
    if coord == "~" {
        Ok(ref_coord)
    } else if let Some(offset_str) = coord.strip_prefix('~') {
        let offset = offset_str.parse::<F>().map_err(|_| messages::INVALID_COORDINATE)?;
        ref_coord.checked_offset(offset).ok_or(messages::COORDINATE_OVERFLOW)
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

pub(crate) fn complete_teleport<'a>(id: i32, text: &str, names: impl Iterator<Item = &'a str>) -> Option<CTabComplete> {
    let (command, prefix) = text.split_once(' ')?;
    if !matches!(command, "/tp" | "/teleport") {
        return None;
    }
    let normalized_prefix = prefix.to_ascii_lowercase();
    let mut names: Vec<_> = names.filter(|name| name.to_ascii_lowercase().starts_with(&normalized_prefix)).collect();
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

fn valid_warp_name(name: &str) -> bool {
    (1..=32).contains(&name.len()) && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn advance_bounded(ticks: u32, mut step: impl FnMut() -> bool) -> u32 {
    let started = Instant::now();
    let budget = std::time::Duration::from_millis(crate::config::CONFIG.command_work_time_ms);
    let mut advanced = 0;
    for _ in 0..ticks {
        if started.elapsed() >= budget {
            break;
        }
        if !step() {
            break;
        }
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
    // Returns true if packets should stop being handled
    pub(super) fn handle_command(&mut self, player: usize, command: &str, mut args: Vec<&str>) -> bool {
        let span = tracing::info_span!("command", player = %self.players[player].username,
            uuid = %format_args!("{:032x}", self.players[player].uuid),
            plot_x = self.world.x, plot_z = self.world.z, command,
            arguments = ?args.join(" ").chars().take(1024).collect::<String>());
        let _entered = span.enter();
        info!("Command requested");
        if self.git_checkout_locked() && !matches!(command, "/git" | "/rv" | "/help" | "/serverinfo") {
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
        if native_command_permission(command, &args).is_some_and(|node| !self.players[player].has_permission(&node))
            || (changes_plot(command, &args) && !self.players[player].can_edit_plot(self.owner, (self.world.x, self.world.z)))
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
        if admin_permission.is_some_and(|node| !self.players[player].has_explicit_permission(node)) {
            self.players[player].send_no_permission_message();
            return false;
        }

        if matches!(command, "/git" | "/rv") {
            self.handle_git_command(player, &args);
            return false;
        }
        if command == "/waveform" {
            match self.handle_waveform(player, &args) {
                Ok(text) if args == ["list"] => self.players[player].send_raw_system_message(crate::player::diagnostic_component(&text).to_string()),
                Ok(text) if args == ["web"] => self.players[player].send_raw_system_message(super::waveform::viewer_link(&text).to_string()),
                Ok(text) => self.players[player].send_system_message(&text),
                Err(error) => self.players[player].send_error_message(&error),
            };
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
            "/hl" => {
                let teleport = args.last() == Some(&"--tp");
                if teleport {
                    args.pop();
                }
                let pos = match args.as_slice() {
                    [] => worldedit::ray_trace_block(
                        &self.world,
                        self.players[player].eye_position(),
                        self.players[player].pitch as f64,
                        self.players[player].yaw as f64,
                        10.0,
                    ),
                    [x, y, z] => match (x.parse(), y.parse(), z.parse()) {
                        (Ok(x), Ok(y), Ok(z)) => Some(mchprs_blocks::BlockPos::new(x, y, z)),
                        _ => None,
                    },
                    _ => None,
                };
                let Some(pos) = pos else {
                    self.players[player].send_error_message(if args.is_empty() {
                        messages::BLOCK_TRACE_FAILED
                    } else {
                        messages::INVALID_ARGUMENT_COUNT
                    });
                    return false;
                };
                if !Self::in_plot_bounds(self.world.x, self.world.z, pos.x, pos.z) || !(0..super::PLOT_BLOCK_HEIGHT).contains(&pos.y) {
                    self.players[player].send_error_message(messages::INVALID_TELEPORT_COORDINATES);
                    return false;
                }
                if teleport {
                    let player_pos = self.players[player].pos;
                    let destination = PlayerPos::new(
                        f64::from(pos.x) + 0.5,
                        f64::from(pos.y) + 0.5 - (self.players[player].eye_position().y - player_pos.y),
                        f64::from(pos.z) + 0.5,
                    );
                    self.players[player].teleport(destination);
                }
                self.show_block_highlight(player, pos, false);
            }
            "/visual" => {
                if !self.players[player].has_permission("commands.visual") {
                    self.players[player].send_no_permission_message();
                    return false;
                }
                match args.as_slice() {
                    [] => self.players[player].send_system_message(&format!(
                        "Visual settings: pistons {}; update rate {} Hz; display-only {}.",
                        self.piston_animation,
                        self.world_send_rate.0,
                        if self.world.screen_only() { "on" } else { "off" },
                    )),
                    ["pistons"] => self.players[player].send_system_message(&messages::piston_animation(
                        self.piston_animation,
                        if self.world.fast_rendering { "off" } else { "on" },
                    )),
                    ["pistons", mode] => {
                        self.piston_animation = match *mode {
                            "auto" => mchprs_save_data::plot_data::PistonAnimation::Auto,
                            "on" => mchprs_save_data::plot_data::PistonAnimation::On,
                            "off" => mchprs_save_data::plot_data::PistonAnimation::Off,
                            _ => {
                                self.players[player].send_error_message(messages::USAGE_VISUAL);
                                return false;
                            }
                        };
                        self.update_render_mode();
                        self.players[player].send_system_message(&messages::piston_animation(
                            self.piston_animation,
                            if self.world.fast_rendering { "off" } else { "on" },
                        ));
                    }
                    ["rate"] => {
                        self.players[player].send_system_message(&messages::world_send_rate(self.world_send_rate.0, self.effective_send_rate()))
                    }
                    ["rate", rate] => {
                        let Ok(hertz) = rate.parse::<u32>() else {
                            self.players[player].send_error_message(messages::UNABLE_PARSE_SEND_RATE);
                            return false;
                        };
                        if hertz > 1000 {
                            self.players[player].send_error_message(messages::WORLD_SEND_RATE_CANNOT_GO_HIGHER);
                            return false;
                        }
                        self.world_send_rate = WorldSendRate(hertz);
                        self.reset_timings();
                        self.players[player].send_system_message(messages::WORLD_SEND_RATE_WAS_SUCCESSFULLY_SET);
                    }
                    ["displayonly"] => self.players[player].send_system_message(if self.world.screen_only() {
                        messages::SCREEN_ONLY_ON
                    } else {
                        messages::SCREEN_ONLY_OFF
                    }),
                    ["displayonly", enabled] => {
                        let enabled = match *enabled {
                            "on" => true,
                            "off" => false,
                            _ => {
                                self.players[player].send_error_message(messages::USAGE_VISUAL);
                                return false;
                            }
                        };
                        if self.owner != Some(self.players[player].uuid) && !self.players[player].has_permission("plots.worldedit.bypass") {
                            self.players[player].send_no_permission_message();
                            return false;
                        }
                        if let Err(error) = database::set_screen_only(self.world.x, self.world.z, enabled) {
                            self.players[player].send_error_message(&messages::visual_setting_save_failed(error));
                            return false;
                        }
                        self.world.set_screen_only(enabled);
                        self.players[player].send_system_message(if enabled { messages::SCREEN_ONLY_ON } else { messages::SCREEN_ONLY_OFF });
                    }
                    _ => self.players[player].send_error_message(messages::USAGE_VISUAL),
                }
            }
            "/help" => {
                if args.len() > 1 {
                    self.players[player].send_error_message(messages::USAGE_HELP_TOPIC);
                } else if let Some(page) = super::help::page(args.first().copied()) {
                    if page == messages::HELP_GIT {
                        for line in page.lines() {
                            self.players[player].send_color_message(crate::chat::ColorCode::Gray, line);
                        }
                    } else {
                        self.players[player].send_system_message(page);
                    }
                } else {
                    self.players[player].send_error_message(messages::UNKNOWN_HELP_TOPIC_USE_HELP_TOPICS);
                }
            }
            "/tellraw" | "/say" => {
                let permission = if command == "/say" { "commands.say" } else { "commands.tellraw" };
                if !self.players[player].has_permission(permission) {
                    self.players[player].send_no_permission_message();
                    return false;
                }
                let source = &self.players[player].username;
                match crate::chat_commands::parse(&format!("{command} {}", args.join(" ")), source, Some(source)) {
                    Ok(message) => {
                        for player in &self.players {
                            if message.recipient.matches(&player.username) {
                                player.send_raw_system_message(message.message.clone());
                            }
                        }
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
                                    Message::WhitelistAdd(profile.uuid.0, profile.username, packet_sender)
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
                    self.players[player].send_error_message(messages::USAGE_WHITELIST_ADD_REMOVE_USERNAME);
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
                            &ChatComponent::from_legacy_text(&messages::rtps_report(report.two_s, report.ten_s, report.one_m, self.tps)),
                        );
                    } else {
                        self.players[player].send_chat_message(0, &ChatComponent::from_legacy_text(&messages::rtps_no_data(self.tps)));
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
            "/rhistory" => {
                if args.first() == Some(&"rewind") {
                    if let Err(error) = self.rewind_plot(player, &args[1..]) {
                        warn!(%error, "Rewind rejected");
                        self.players[player].send_error_message(&error);
                    } else {
                        info!("Rewind completed");
                    }
                } else {
                    match self.control_history(player, &args) {
                        Ok(message) => {
                            info!(%message, "Tick history command completed");
                            self.players[player].send_system_message(&message);
                        }
                        Err(error) => {
                            warn!(%error, "Tick history command rejected");
                            self.players[player].send_error_message(&error);
                        }
                    }
                }
            }
            "/rev" => {
                if let Err(error) = self.rewind_plot(player, &args) {
                    warn!(%error, "Rewind rejected");
                    self.players[player].send_error_message(&error);
                } else {
                    info!("Rewind completed");
                }
            }
            "/adv" => {
                if self.compiling_redpiler() {
                    self.players[player].send_error_message(messages::REDPILER_COMPILATION_PAUSED);
                    return false;
                }
                let (unit, ticks) = match AdvanceUnit::parse(&args, crate::config::CONFIG.max_command_ticks) {
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
                        self.players[player].send_error_message(messages::DISABLE_TICK_HISTORY_BEFORE_NANO_PICO);
                        return false;
                    }
                }
                let advanced = match unit {
                    AdvanceUnit::Game => advance_bounded(ticks, || self.tick()),
                    AdvanceUnit::Nano => advance_bounded(ticks, || {
                        self.world.nanotick_advance(1);
                        true
                    }),
                    AdvanceUnit::Pico => advance_bounded(ticks, || {
                        self.world.picotick_advance(1);
                        true
                    }),
                };
                if advanced > 0 {
                    if self.redpiler.is_active() {
                        self.redpiler.flush(&mut self.world);
                    }
                    self.world.flush_block_changes();
                }
                let progress = messages::advance_progress(advanced, ticks, unit.label());
                self.players[player].send_system_message(&messages::plot_advanced(progress, start_time.elapsed()));
            }

            "/toggleautorp" => {
                self.auto_redpiler = !self.auto_redpiler;
                if self.auto_redpiler {
                    self.players[player].send_system_message(messages::REDPILER_AUTO_ENABLED);
                } else {
                    self.players[player].send_system_message(messages::REDPILER_AUTO_DISABLED);
                }
            }
            "/setwarp" if !crate::redbox::enabled() => {
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
                match database::set_warp(name, warp) {
                    Ok(()) => actor.send_system_message(&messages::warp_saved(name)),
                    Err(error) => {
                        warn!("Could not save warp {name}: {error}");
                        actor.send_error_message(messages::WARP_STORAGE_FAILED);
                    }
                }
            }
            "/warp" if !crate::redbox::enabled() => {
                if args.is_empty() {
                    match database::warp_names() {
                        Ok(names) if names.is_empty() => self.players[player].send_system_message(messages::NO_WARPS),
                        Ok(names) => self.players[player].send_system_message(&messages::warp_list(names.join(", "))),
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
                        if !warp.is_valid() {
                            self.players[player].send_error_message(messages::INVALID_TELEPORT_COORDINATES);
                            return false;
                        }
                        self.players[player].send_system_message(&messages::warp_teleport(name));
                        return self.teleport_player_to(player, warp.pos, warp.yaw, warp.pitch);
                    }
                    Ok(None) => self.players[player].send_error_message(&messages::warp_not_found(name)),
                    Err(error) => {
                        warn!("Could not read warp {name}: {error}");
                        self.players[player].send_error_message(messages::WARP_STORAGE_FAILED);
                    }
                }
            }
            "/teleport" | "/tp" => {
                let redbox = crate::redbox::enabled();
                let highlight_only = args.len() == 4 && args[3] == "--highlight-only";
                let highlight = highlight_only || (args.len() == 4 && args[3] == "--highlight");
                if redbox && !highlight {
                    self.players[player].send_error_message(messages::COMMAND_NOT_FOUND);
                    return false;
                }
                if !redbox && args.len() == 1 && !highlight {
                    self.players[player].send_system_message(&messages::teleport_player(args[0]));
                    let uuid = self.players[player].uuid;
                    let player = self.leave_plot(uuid);
                    let _ = self.message_sender.send(Message::PlayerTeleportOther(player, args[0].to_owned()));
                    return true;
                }
                if args.len() != 3 + usize::from(highlight) {
                    self.players[player].send_error_message(messages::INVALID_NUMBER_ARGUMENTS_TELEPORT_COMMAND);
                    return false;
                }
                if highlight {
                    args.pop();
                }
                let player_pos = self.players[player].pos;
                let x = match parse_teleport_coord(args[0], player_pos.x, true) {
                    Ok(value) => value,
                    Err(_) => {
                        self.players[player].send_error_message(messages::INVALID_X_COORDINATE);
                        return false;
                    }
                };
                let y = match parse_teleport_coord(args[1], player_pos.y, false) {
                    Ok(value) => value,
                    Err(_) => {
                        self.players[player].send_error_message(messages::INVALID_Y_COORDINATE);
                        return false;
                    }
                };
                let z = match parse_teleport_coord(args[2], player_pos.z, true) {
                    Ok(value) => value,
                    Err(_) => {
                        self.players[player].send_error_message(messages::INVALID_Z_COORDINATE);
                        return false;
                    }
                };
                let target = PlayerPos::new(x, y, z);
                let mut destination = target;
                if highlight && !highlight_only {
                    // Error links target the block center with the viewer's current head height.
                    destination.y -= self.players[player].eye_position().y - player_pos.y;
                }
                if !destination.is_valid() {
                    self.players[player].send_error_message(messages::INVALID_TELEPORT_COORDINATES);
                    return false;
                }
                if !highlight_only {
                    self.players[player].send_system_message(&messages::teleport_coordinates(x, y, z));
                    self.players[player].teleport(destination);
                }
                if highlight {
                    self.show_block_highlight(player, target.block_pos(), false);
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
            "/small" => {
                let enabled = match args.as_slice() {
                    [] => self.players[player].small_model.is_none(),
                    ["on"] => true,
                    ["off"] => false,
                    ["wolf" | "fox" | "cat" | "ocelot" | "baby"] => {
                        let animal = match args[0] {
                            "wolf" => crate::player::SmallAnimal::Wolf,
                            "fox" => crate::player::SmallAnimal::Fox,
                            "cat" => crate::player::SmallAnimal::Cat,
                            "baby" => crate::player::SmallAnimal::BabyOcelot,
                            _ => crate::player::SmallAnimal::Ocelot,
                        };
                        self.set_small_animal(player, animal);
                        return false;
                    }
                    _ => {
                        self.players[player].send_error_message(messages::USAGE_SMALL);
                        return false;
                    }
                };
                self.set_small(player, enabled);
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
                        self.players[player].send_error_message(messages::PERFORMANCE_REASONS_PLAYER_SPEED_CANNOT_HIGHER);
                        return false;
                    }
                    if speed_arg.is_nan() {
                        self.players[player].send_error_message(messages::YOU_CAN_T_SET_SPEED_NAN);
                        return false;
                    }
                    self.players[player].fly_speed = speed_arg;
                    self.players[player].update_player_abilities();
                    let username = self.players[player].username.clone();
                    self.players[player].send_system_message(&messages::flying_speed(speed_arg, username));
                } else {
                    self.players[player].send_error_message(messages::UNABLE_PARSE_SPEED_VALUE);
                }
            }
            "/gmsp" => self.change_player_gamemode(player, Gamemode::Spectator, false),
            "/gmc" => self.change_player_gamemode(player, Gamemode::Creative, false),
            "/gamemode" | "/gm" => {
                if args.len() != 1 {
                    self.players[player].send_error_message(messages::INVALID_ARGUMENT_COUNT);
                    return false;
                }
                let name = args.remove(0);
                let gamemode = match name {
                    "c" | "creative" | "1" | "cat" => Gamemode::Creative,
                    "a" | "adventure" | "2" => Gamemode::Adventure,
                    "s" | "spectator" | "3" => Gamemode::Spectator,
                    "survival" | "0" => Gamemode::Survival,
                    _ => {
                        self.players[player].send_error_message(messages::UNKNOWN_GAMEMODE);
                        return false;
                    }
                };
                self.change_player_gamemode(player, gamemode, name == "cat");
            }
            "/curse" => {
                if self.world.is_cursed {
                    self.players[player].send_system_message(messages::WORLD_ALREADY_CURSED);
                } else {
                    self.world.is_cursed = true;
                    self.auto_redpiler = false;
                    self.players[player].send_system_message(messages::WORLD_BEEN_CURSED_REDPILER_DISABLED_BLESS);
                }
                return false;
            }
            "/bless" => {
                if self.world.is_cursed {
                    self.world.is_cursed = false;
                    self.players[player].send_system_message(messages::WORLD_BEEN_BLESSED);
                } else {
                    self.players[player].send_system_message(messages::WORLD_NOT_CURSED_CURSE_CURSE);
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
    let name = match command {
        "/help" => "help".to_owned(),
        "/git" | "/rv" => "git".to_owned(),
        "/version" => "version".to_owned(),
        "/teleport" | "/tp" => "teleport".to_owned(),
        "/hl" => "teleport".to_owned(),
        "/warp" if !crate::redbox::enabled() => "warp".to_owned(),
        "/setwarp" if !crate::redbox::enabled() => "setwarp".to_owned(),
        "/speed" => "speed".to_owned(),
        "/small" => "small".to_owned(),
        "/gmsp" | "/gmc" | "/gamemode" | "/gm" => "gamemode".to_owned(),
        // These two use explicit Minecraft nodes in the admin guard below.
        "/stop" | "/whitelist" => return None,
        "/tellraw" => "tellraw".to_owned(),
        "/say" => "say".to_owned(),
        "/tps" => format!("rtps.{}", if args.is_empty() || args == ["timings"] { "view" } else { "set" }),
        "/visual" => format!("visual.{}", if args.len() > 1 { "set" } else { "view" }),
        "/adv" => "radvance".to_owned(),
        "/toggleautorp" => "toggleautorp".to_owned(),
        "/curse" => "curse".to_owned(),
        "/bless" => "bless".to_owned(),
        "/redpiler" | "/rp" => format!(
            "redpiler.{}",
            match args.first().copied() {
                Some("c" | "compile") => "compile",
                Some("r" | "reset") => "reset",
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
        "/visual" => args.len() > 1,
        "/adv" | "/toggleautorp" | "/curse" | "/bless" => true,
        "/redpiler" | "/rp" => true,
        _ => false,
    }
}

#[cfg(test)]
use declaration::declared_command_nodes;
pub use declaration::{DECLARE_COMMANDS, NO_COMMANDS};
#[cfg(test)]
use mchprs_network::packets::clientbound::CDeclareCommandsNodeParser as Parser;
#[cfg(test)]
use mchprs_network::packets::clientbound::ClientBoundPacket;

#[cfg(test)]
mod security_tests {
    #[test]
    fn waveform_actions_require_base_action_and_plot_access() {
        let (mut plot, _peer) = super::super::client_sync_tests::fixture(false);
        plot.owner = Some(plot.players[0].uuid);
        for (nodes, error) in [
            (
                vec!["mchprs.access.commands", "commands.waveform"],
                "No permission for /waveform add; ask staff.",
            ),
            (vec!["mchprs.access.commands", "commands.waveform.add"], "No waveform access; ask staff."),
        ] {
            plot.players[0].set_test_permissions(&nodes);
            assert_eq!(plot.handle_waveform(0, &["add", "clock"]).unwrap_err(), error);
        }
        plot.players[0].set_test_permissions(&["mchprs.access.commands", "commands.waveform", "commands.waveform.add"]);
        plot.owner = Some(999);
        assert_eq!(
            plot.handle_waveform(0, &["add", "clock"]).unwrap_err(),
            "You need edit access to this plot."
        );
        assert!(plot.world.waveform.probes.is_empty());
    }

    #[test]
    fn stop_and_whitelist_require_explicit_permissions_without_luckperms() {
        let (mut plot, _peer) = super::super::client_sync_tests::fixture(false);
        let (sender, receiver) = std::sync::mpsc::channel();
        plot.message_sender = sender;

        for command in ["/stop", "/whitelist"] {
            assert!(!plot.handle_command(0, command, vec![]));
            assert!(receiver.try_recv().is_err(), "{command} must not reach the server");
        }
    }

    #[test]
    fn explicit_stop_and_whitelist_grants_reach_their_handlers() {
        let (mut plot, mut peer) = super::super::client_sync_tests::fixture(false);
        plot.players[0].set_test_permissions(&["minecraft.command.stop", "minecraft.command.whitelist"]);
        let (sender, receiver) = std::sync::mpsc::channel();
        plot.message_sender = sender;

        assert!(!plot.handle_command(0, "/whitelist", vec![]));
        assert!(receiver.try_recv().is_err());
        assert!(!plot.handle_command(0, "/stop", vec![]));
        assert!(matches!(receiver.try_recv(), Ok(crate::server::Message::Shutdown)));
        assert!(mchprs_network::test_support::read_frame(&mut peer, false).is_ok());
    }

    #[test]
    fn legacy_plot_mutations_require_permission_and_allow_the_owner() {
        assert!(!crate::permissions::dedicated_permissions());
        let (mut plot, _) = super::super::client_sync_tests::fixture(false);
        let owner = 2;
        plot.owner = Some(owner);
        plot.players[0].set_test_permissions(&["commands.rtps.set", "commands.radvance", "commands.redpiler.reset", "commands.curse"]);

        let commands = [("/tps", vec!["42"]), ("/adv", vec!["1"]), ("/rp", vec!["reset"]), ("/curse", vec![])];
        for (command, args) in &commands {
            assert!(!plot.handle_command(0, command, args.clone()));
            assert_ne!(plot.tps, Tps::Limited(42));
            assert!(!plot.world.is_cursed);
        }

        plot.owner = Some(plot.players[0].uuid);
        for (command, args) in &commands {
            assert!(!plot.handle_command(0, command, args.clone()));
        }
        assert_eq!(plot.tps, Tps::Limited(42));
        assert!(plot.world.is_cursed);
    }

    #[test]
    fn bounded_advance_stops_on_execution_failure() {
        let mut calls = 0;
        let advanced = super::advance_bounded(10, || {
            calls += 1;
            calls < 2
        });
        assert_eq!(advanced, 1);
        assert_eq!(calls, 2);
    }

    use super::*;

    #[test]
    fn say_and_tellraw_deliver_only_to_matching_players_on_the_executing_plot() {
        use mchprs_network::packets::clientbound::CChatMessage;
        use mchprs_network::test_support::{connection, read_frame};
        use std::io::Read;

        let (mut plot, mut actor_peer) = super::super::client_sync_tests::fixture(false);
        plot.players[0].set_test_permissions(&["mchprs.access.commands", "commands.say", "commands.tellraw"]);
        let viewer = connection(false).unwrap();
        let mut viewer_peer = viewer.peer;
        let mut player = crate::player::Player::test_player(viewer.player);
        player.uuid = 2;
        player.username = "Viewer".into();
        plot.players.push(player);
        let (mut other_plot, mut other_peer) = super::super::client_sync_tests::fixture(false);
        other_plot.world.x = 1;
        other_plot.players[0].username = "OtherPlot".into();
        let (sender, receiver) = std::sync::mpsc::channel();
        plot.message_sender = sender;

        for (command, args) in [("/say", vec!["hello"]), ("/tellraw", vec!["@a", "{\"text\":\"hello\"}"])] {
            let expected = crate::chat_commands::parse(&format!("{command} {}", args.join(" ")), "SyncTest", Some("SyncTest")).unwrap();
            let packet = CChatMessage {
                message: expected.message,
                position: 1,
                sender: 0,
            }
            .encode();
            assert!(!plot.handle_command(0, command, args));
            for peer in [&mut actor_peer, &mut viewer_peer] {
                let (id, frame) = read_frame(peer, false).unwrap();
                assert_eq!(id, 0x72);
                assert_eq!(&frame.get_ref()[frame.position() as usize..], packet.buffer);
            }
            assert!(receiver.try_recv().is_err());
        }
        plot.handle_command(0, "/tellraw", vec!["Viewer", "\"targeted\""]);
        assert_eq!(read_frame(&mut viewer_peer, false).unwrap().0, 0x72);
        plot.handle_command(0, "/tellraw", vec!["OtherPlot", "\"hidden\""]);
        assert!(receiver.try_recv().is_err());
        for peer in [&mut actor_peer, &mut viewer_peer, &mut other_peer] {
            peer.set_nonblocking(true).unwrap();
            assert_eq!(peer.read(&mut [0]).unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
        }
    }

    #[test]
    fn error_coordinate_teleport_centers_the_head_and_highlights_the_exact_block() {
        use mchprs_network::packets::PacketDecoderExt;
        use mchprs_network::test_support::read_frame;
        use std::io::Read;

        for (crouching, small) in [(false, false), (true, false), (false, true), (true, true)] {
            let (mut plot, mut peer) = super::super::client_sync_tests::fixture(false);
            plot.players[0].crouching = crouching;
            plot.players[0].small_model = small.then(crate::player::SmallModel::new);
            let before = plot.players[0].pos;
            assert!(!plot.handle_command(0, "/tp", vec!["33.5", "21.5", "35.5", "--highlight-only"],));
            let after = plot.players[0].pos;
            assert_eq!((after.x, after.y, after.z), (before.x, before.y, before.z));
            assert!(!plot.players[0].awaiting_teleport());
            assert_eq!(read_frame(&mut peer, false).unwrap().0, 0x01);
            assert_eq!(read_frame(&mut peer, false).unwrap().0, 0x5c);
            assert!(!plot.handle_command(0, "/tp", vec!["33.5", "21.5", "35.5", "--highlight"],));
            let head = plot.players[0].eye_position();
            assert_eq!((head.x, head.z), (33.5, 35.5));
            assert!((head.y - 21.5).abs() < 1e-9);
            assert_eq!(read_frame(&mut peer, false).unwrap().0, 0x72);
            assert_eq!(read_frame(&mut peer, false).unwrap().0, 0x41);
            assert_eq!(read_frame(&mut peer, false).unwrap().0, 0x46);
            let (packet_id, mut spawn) = read_frame(&mut peer, false).unwrap();
            assert_eq!(packet_id, 0x01);
            spawn.read_varint().unwrap();
            spawn.read_uuid().unwrap();
            assert_eq!(spawn.read_varint().unwrap(), 15);
            assert_eq!(spawn.read_double().unwrap(), 33.0);
            assert_eq!(spawn.read_double().unwrap(), 21.0);
            assert_eq!(spawn.read_double().unwrap(), 35.0);
            assert_eq!(read_frame(&mut peer, false).unwrap().0, 0x5c);

            let before = plot.players[0].pos;
            plot.handle_command(0, "/tp", vec!["33.5", "21.5", "35.5"]);
            if crate::redbox::enabled() {
                assert_eq!(plot.players[0].pos, before);
                assert_eq!(read_frame(&mut peer, false).unwrap().0, 0x72);
            } else {
                assert_eq!(plot.players[0].pos, PlayerPos::new(33.5, 21.5, 35.5));
                assert_eq!(read_frame(&mut peer, false).unwrap().0, 0x72);
                assert_eq!(read_frame(&mut peer, false).unwrap().0, 0x41);
            }
            let position_after_teleport = plot.players[0].pos;

            for args in [
                vec!["invalid", "21.5", "35.5", "--highlight"],
                vec!["1e309", "21.5", "35.5", "--highlight"],
                vec!["30000001.5", "21.5", "35.5", "--highlight"],
                vec!["33.5", "-2048", "35.5", "--highlight"],
                vec!["invalid", "21.5", "35.5", "--highlight-only"],
                vec!["30000001.5", "21.5", "35.5", "--highlight-only"],
                vec!["33.5", "-2049", "35.5", "--highlight-only"],
            ] {
                assert!(!plot.handle_command(0, "/tp", args));
                assert_eq!(read_frame(&mut peer, false).unwrap().0, 0x72);
                assert_eq!(plot.players[0].pos, position_after_teleport);
            }
            peer.set_nonblocking(true).unwrap();
            assert_eq!(peer.read(&mut [0]).unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
        }
        assert_eq!(
            native_command_permission("/tp", &["33.5", "21.5", "35.5", "--highlight"]).as_deref(),
            Some("commands.teleport")
        );
        assert!(!changes_plot("/tp", &["33.5", "21.5", "35.5", "--highlight"]));
        let nodes = declared_command_nodes();
        assert!(!nodes.iter().any(|node| node.name == Some("inspect")));
    }

    #[test]
    fn highlight_command_targets_coordinates_and_can_teleport() {
        let (mut plot, _) = super::super::client_sync_tests::fixture(false);
        assert!(!plot.handle_command(0, "/hl", vec!["33", "21", "35", "--tp"]));
        let eye = plot.players[0].eye_position();
        assert_eq!((eye.x, eye.y, eye.z), (33.5, 21.5, 35.5));
        assert_eq!(native_command_permission("/hl", &[]).as_deref(), Some("commands.teleport"));
        assert!(declared_command_nodes().iter().any(|node| node.name == Some("hl")));
    }

    #[test]
    fn small_and_cat_commands_have_permissions_and_completion() {
        assert_eq!(native_command_permission("/small", &["on"]).as_deref(), Some("commands.small"));
        assert_eq!(native_command_permission("/gm", &["cat"]).as_deref(), Some("commands.gamemode"));
        assert!(!changes_plot("/small", &["on"]));
        let nodes = declared_command_nodes();
        let small = nodes.iter().find(|node| node.name == Some("small")).unwrap();
        assert_eq!(
            small.children.iter().map(|&id| nodes[id as usize].name.unwrap()).collect::<Vec<_>>(),
            ["on", "off", "wolf", "fox", "cat", "ocelot", "baby"]
        );
        let gm = nodes.iter().find(|node| node.name == Some("gamemode")).unwrap();
        assert!(gm.children.iter().any(|&id| nodes[id as usize].name == Some("cat")));
    }
    #[test]
    fn teleport_declarations_follow_the_redbox_setting_and_plot_teleport_remains() {
        let nodes = declared_command_nodes();
        let root_names: Vec<_> = nodes[0].children.iter().filter_map(|&id| nodes[id as usize].name).collect();
        assert_eq!(root_names.contains(&"teleport"), !crate::redbox::enabled());
        assert_eq!(root_names.contains(&"tp"), !crate::redbox::enabled());
        let plot = nodes.iter().find(|node| node.name == Some("plot")).unwrap();
        assert!(plot.children.iter().any(|&id| {
            nodes[id as usize].name == Some("teleport")
                && nodes[id as usize]
                    .children
                    .iter()
                    .any(|&child| matches!(nodes[child as usize].parser, Some(Parser::Vec2)))
        }));
    }
    #[test]
    fn teleport_autocomplete_suggests_player_names_without_selectors() {
        let nodes = declared_command_nodes();
        let tp = nodes[0]
            .children
            .iter()
            .map(|&id| &nodes[id as usize])
            .find(|node| node.name == Some("tp"));
        if crate::redbox::enabled() {
            assert!(tp.is_none());
        } else {
            let tp = tp.unwrap();
            let player = &nodes[tp.children[0] as usize];
            assert!(matches!(player.parser, Some(Parser::String(0))));
            assert_eq!(player.suggestions_type, Some("minecraft:ask_server"));
            assert!(matches!(nodes[tp.children[1] as usize].parser, Some(Parser::Vec3)));
        }
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
            assert_eq!(response.matches.into_iter().map(|item| item.match_).collect::<Vec<_>>(), expected);
        }
    }
    #[test]
    fn warp_names_and_permissions_are_checked() {
        for name in ["spawn", "CPU-1", "my_plot", &"a".repeat(32)] {
            assert!(valid_warp_name(name));
        }
        for name in ["", "two words", "../spawn", "café", "'", &"a".repeat(33)] {
            assert!(!valid_warp_name(name));
        }
        let redbox = crate::redbox::enabled();
        let nodes = declared_command_nodes();
        for command in ["warp", "setwarp"] {
            let node = nodes.iter().find(|node| node.name == Some(command));
            assert_eq!(node.is_some(), !redbox);
            assert_eq!(
                native_command_permission(&format!("/{command}"), &["spawn"]),
                (!redbox).then(|| format!("commands.{command}"))
            );
            assert!(!changes_plot(&format!("/{command}"), &["spawn"]));
            if let Some(node) = node {
                assert_eq!(nodes[node.children[0] as usize].suggestions_type, Some("minecraft:ask_server"));
            }
        }
    }
    #[test]
    fn serverinfo_autocomplete_declares_subcommands_and_numeric_arguments() {
        let nodes = declared_command_nodes();
        let serverinfo = nodes.iter().find(|node| node.name == Some("serverinfo")).unwrap();
        assert_eq!(
            serverinfo.children.iter().map(|&id| nodes[id as usize].name.unwrap()).collect::<Vec<_>>(),
            ["plots", "plot", "settings"]
        );
        for &id in serverinfo.children {
            let node = &nodes[id as usize];
            assert_eq!(node.flags & 0x07, 0x05, "subcommands must be executable literals");
        }
        let plots = &nodes[serverinfo.children[0] as usize];
        let page = &nodes[plots.children[0] as usize];
        assert_eq!(page.name, Some("page"));
        assert!(matches!(page.parser, Some(Parser::Integer(1, i32::MAX))));
        let plot = &nodes[serverinfo.children[1] as usize];
        let x = &nodes[plot.children[0] as usize];
        let z = &nodes[x.children[0] as usize];
        assert_eq!((x.name, z.name), (Some("x"), Some("z")));
        for node in [x, z] {
            assert!(matches!(node.parser, Some(Parser::Integer(i32::MIN, i32::MAX))));
        }
        assert_eq!(x.flags & 0x04, 0);
        assert_eq!(z.flags & 0x04, 0x04);
    }
    #[test]
    fn command_declarations_have_valid_edges_and_no_legacy_tick_aliases() {
        let nodes = declared_command_nodes();
        for node in &nodes {
            for edge in node.children.iter().copied().chain(node.redirect_node) {
                assert!(edge >= 0 && (edge as usize) < nodes.len());
            }
        }
        let names: Vec<_> = nodes[0].children.iter().filter_map(|index| nodes[*index as usize].name).collect();
        for removed in ["rtps", "radv", "radvance", "rback"] {
            assert!(!nodes.iter().any(|node| node.name == Some(removed)));
            let command = format!("/{removed}");
            assert!(native_command_permission(&command, &["1"]).is_none());
            assert!(!changes_plot(&command, &["1"]));
        }
        for proxied in ["warp", "setwarp", "tp", "teleport"] {
            assert_eq!(names.contains(&proxied), !crate::redbox::enabled(), "incorrect declaration for {proxied}");
        }
        for retained in ["tps", "adv", "rhistory", "redpiler", "rp", "gm", "serverinfo", "wire", "waveform"] {
            assert!(names.contains(&retained), "missing command {retained}");
        }
        assert!(names.contains(&"rev"));
        assert!(!names.contains(&"back"));
        assert!(names.contains(&"rv"));
        for (alias, target) in [
            ("rp", "redpiler"),
            ("gm", "gamemode"),
            ("/desel", "/sel"),
            ("desel", "/sel"),
            ("set", "/set"),
            ("/wire", "wire"),
        ] {
            let node = nodes.iter().find(|node| node.name == Some(alias)).unwrap();
            assert_eq!(nodes[node.redirect_node.unwrap() as usize].name, Some(target));
        }
        let wire = nodes.iter().find(|node| node.name == Some("wire")).unwrap();
        assert_eq!(
            wire.children.iter().map(|&id| nodes[id as usize].name.unwrap()).collect::<Vec<_>>(),
            ["free", "plane", "off"]
        );
        assert!(wire.children.iter().all(|&id| nodes[id as usize].flags & 0x04 != 0));
        assert_eq!(NO_COMMANDS.packet_id, 0x10);
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
            assert_eq!(AdvanceUnit::parse(&args, 10).unwrap_err(), messages::USAGE_ADV);
        }
        for args in [vec!["nano"], vec!["11"], vec!["-1"], vec!["pico", "invalid"], vec!["nano", "4294967296"]] {
            assert_eq!(AdvanceUnit::parse(&args, 10).unwrap_err(), messages::advance_tick_count_limit(10));
        }
    }

    #[test]
    fn queued_commands_stop_when_the_actor_leaves() {
        let mut calls = Vec::new();
        assert!(run_command_queue(vec!["/tp Admin".into(), "/stop".into()], |command, _| {
            calls.push(command.to_owned());
            command == "/tp"
        }));
        assert_eq!(calls, ["/tp"]);
        assert!(!run_command_queue(vec!["  ".into(), "/help".into()], |_, _| false));
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
            ("/rp", "/redpiler", vec!["compile"]),
            ("/tp", "/teleport", vec!["Admin"]),
            ("/gm", "/gamemode", vec!["2"]),
        ] {
            assert_eq!(native_command_permission(alias, &args), native_command_permission(canonical, &args));
            assert_eq!(changes_plot(alias, &args), changes_plot(canonical, &args));
        }
    }

    #[test]
    fn retired_redpiler_commands_and_graph_flags_are_not_exposed() {
        let nodes = declared_command_nodes();
        assert!(!nodes.iter().any(|node| matches!(node.name, Some("analyze" | "inspect" | "--graph"))));
        assert_eq!(
            native_command_permission("/rp", &["compile"]).as_deref(),
            Some("commands.redpiler.compile")
        );
        assert_eq!(native_command_permission("/rp", &["reset"]).as_deref(), Some("commands.redpiler.reset"));
        for flag in ["-O3", "--optimize", "--graph", "--export-json"] {
            assert!(CompilerOptions::parse(flag).is_err(), "retired option {flag} must be rejected");
        }
        let options = CompilerOptions::parse("--io-only -u").unwrap();
        assert!(options.io_only && options.update);
    }

    #[test]
    fn redpiler_completion_offers_only_supported_compile_flags() {
        for (text, expected) in [
            ("/rp compile --", vec!["--io-only", "--update"]),
            ("/redpiler c -", vec!["--io-only", "--update", "-i", "-u"]),
            ("/rp compile --graph", vec![]),
        ] {
            let response = complete_redpiler(42, text).unwrap();
            assert_eq!(response.matches.iter().map(|m| m.match_.as_str()).collect::<Vec<_>>(), expected);
        }
        assert!(complete_redpiler(0, "/rp analyze ").is_none());
        assert!(complete_redpiler(0, "/rp inspect ").is_none());
        assert!(complete_redpiler(0, "/rp reset ").is_none());
    }
}
