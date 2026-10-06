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
    ClientBoundPacket,
};
use mchprs_network::packets::PacketEncoder;
use mchprs_network::PlayerPacketSender;
use mchprs_save_data::plot_data::{Tps, WorldSendRate};
use once_cell::sync::Lazy;
use std::str::FromStr;
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
impl RelativeCoordinate for f64 {
    fn checked_offset(self, offset: Self) -> Option<Self> {
        let result = self + offset;
        result.is_finite().then_some(result)
    }
}
fn parse_relative_coord<F: RelativeCoordinate>(
    coord: &str,
    ref_coord: F,
) -> Result<F, &'static str> {
    if coord == "~" {
        Ok(ref_coord)
    } else if let Some(offset_str) = coord.strip_prefix('~') {
        let offset = offset_str.parse::<F>().map_err(|_| "Invalid coordinate")?;
        ref_coord
            .checked_offset(offset)
            .ok_or("Coordinate overflow")
    } else {
        coord.parse::<F>().map_err(|_| "Invalid coordinate")
    }
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
            _ => return Err("Usage: /adv [nano|pico] <ticks>".into()),
        };
        let ticks = count
            .parse::<u32>()
            .ok()
            .filter(|&ticks| ticks <= limit)
            .ok_or_else(|| format!("Tick count must be between 0 and {limit}"))?;
        Ok((unit, ticks))
    }

    fn label(self) -> &'static str {
        match self {
            Self::Game => "ticks",
            Self::Nano => "nano-ticks",
            Self::Pico => "pico-ticks",
        }
    }
}

impl Plot {
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
                    self.players[player].send_error_message("Usage: /p home");
                    return;
                }
                match database::get_owned_plots_by_uuid(self.players[player].uuid) {
                    Ok(plots) => {
                        if let Some(&(x, z)) = plots.first() {
                            let center = Plot::get_center(x, z);
                            self.players[player].teleport(PlayerPos::new(center.0, 64.0, center.1));
                        } else {
                            self.players[player].send_error_message(
                                "You do not own a plot. Use /p auto to claim one.",
                            );
                        }
                    }
                    Err(error) => self.players[player]
                        .send_error_message(&format!("Could not read plots: {error}")),
                }
            }
            "add" | "remove" => {
                let [name] = args else {
                    self.players[player].send_error_message(&format!("Usage: /p {command} <nick>"));
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
                        .send_system_message(&format!(
                            "{name} {} plot ({}, {}).",
                            if add {
                                "can now build on"
                            } else {
                                "was removed from"
                            },
                            self.world.x,
                            self.world.z,
                        )),
                    Ok(MembershipResult::Unchanged) => {
                        self.players[player].send_system_message(if add {
                            "That player is already a plot member."
                        } else {
                            "That player is not a plot member."
                        })
                    }
                    Ok(MembershipResult::UnknownPlayer) => self.players[player].send_error_message(
                        "Unknown player. They must have joined this server at least once.",
                    ),
                    Ok(MembershipResult::AmbiguousPlayer) => self.players[player].send_error_message(
                        "Multiple cached players have that nickname. They must rejoin with distinct current names before access can be changed.",
                    ),
                    Ok(MembershipResult::PlotUnclaimed) => {
                        self.players[player].send_error_message(messages::PLOT_UNCLAIMED)
                    }
                    Ok(MembershipResult::NotOwner) => {
                        self.players[player].send_no_permission_message()
                    }
                    Ok(MembershipResult::IsOwner) => self.players[player].send_error_message(
                        "The plot owner cannot be added or removed as a member.",
                    ),
                    Err(error) => self.players[player]
                        .send_error_message(&format!("Could not update plot members: {error}")),
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
                            self.players[player].send_error_message("Could not read plot claims");
                            return;
                        }
                    }
                }
                self.players[player].send_error_message(
                    "No free plot in the automatic search area; choose a plot and use /plot claim",
                );
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
                        self.players[player]
                            .send_error_message(&format!("Could not read plots: {error}"));
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
            "compile" | "c" => {
                let start_time = Instant::now();
                let args = args.join(" ");
                let options = CompilerOptions::parse(&args);

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
        if self.git_checkout_locked() && command != "/git" && command != "/help" {
            let message = self.git_lock_message();
            self.players[player].send_error_message(message);
            return false;
        }
        if !self.players[player].can_use_commands() {
            self.players[player].send_no_permission_message();
            return false;
        }
        if crate::permissions::dedicated_permissions() {
            if native_command_permission(command, &args)
                .is_some_and(|node| !self.players[player].has_permission(&node))
                || (changes_plot(command, &args)
                    && !self.players[player]
                        .can_edit_plot(self.owner, (self.world.x, self.world.z)))
            {
                self.players[player].send_no_permission_message();
                return false;
            }
        }
        info!(
            "{} issued command: {} {}",
            self.players[player].username,
            command,
            args.join(" ")
        );

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
                            .send_error_message(&format!("Could not save visual setting: {error}"));
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
                    self.players[player].send_system_message(page);
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
            "/tps" | "/rtps" => {
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
                Ok(message) => self.players[player].send_system_message(&message),
                Err(error) => self.players[player].send_error_message(&error),
            },
            "/back" | "/rback" => {
                if let Err(error) = self.rewind_plot(player, &args) {
                    self.players[player].send_error_message(&error);
                }
            }
            "/adv" | "/radv" | "/radvance" => {
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
                let progress = format!("{advanced} of {ticks} {}", unit.label());
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
            "/teleport" | "/tp" => {
                if args.len() == 3 {
                    let player_pos = self.players[player].pos;
                    let x;
                    let y;
                    let z;
                    if let Ok(x_arg) = parse_relative_coord(args[0], player_pos.x) {
                        x = x_arg;
                    } else {
                        self.players[player].send_error_message(messages::INVALID_X_COORDINATE);
                        return false;
                    }
                    if let Ok(y_arg) = parse_relative_coord(args[1], player_pos.y) {
                        y = y_arg;
                    } else {
                        self.players[player].send_error_message(messages::INVALID_Y_COORDINATE);
                        return false;
                    }
                    if let Ok(z_arg) = parse_relative_coord(args[2], player_pos.z) {
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
            "/gamemode" => {
                if args.is_empty() {
                    self.players[player].send_error_message(messages::INVALID_ARGUMENT_COUNT);
                    return false;
                }
                let name = args.remove(0);
                let gamemode = match name {
                    "creative" | "1" => Gamemode::Creative,
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
        "/speed" => "speed".to_owned(),
        "/gmsp" | "/gmc" | "/gamemode" => "gamemode".to_owned(),
        "/stop" => "stop".to_owned(),
        "/whitelist" => "whitelist".to_owned(),
        "/tellraw" => "tellraw".to_owned(),
        "/say" => "say".to_owned(),
        "/tps" | "/rtps" => format!(
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
        "/adv" | "/radv" | "/radvance" => "radvance".to_owned(),
        "/toggleautorp" => "toggleautorp".to_owned(),
        "/curse" => "curse".to_owned(),
        "/bless" => "bless".to_owned(),
        "/redpiler" | "/rp" => format!(
            "redpiler.{}",
            match args.first().copied() {
                Some("c" | "compile") => "compile",
                Some("r" | "reset") => "reset",
                Some("i" | "inspect") => "inspect",
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
        "/tps" | "/rtps" => !args.is_empty() && args != ["timings"],
        "/worldsendrate" | "/wsr" | "/screenonly" | "/piston_anim" | "/bisdon_anim" => {
            !args.is_empty()
        }
        "/adv" | "/radv" | "/radvance" | "/toggleautorp" | "/curse" | "/bless" => true,
        "/redpiler" | "/rp" => !matches!(args.first().copied(), Some("inspect" | "i")),
        _ => false,
    }
}

#[cfg(test)]
mod security_tests {
    use super::*;
    #[test]
    fn command_declarations_preserve_original_wire_bytes() {
        // Remove only the new Git root edge and node, then verify the original
        // declarations still have identical flags, parsers, aliases and edges.
        use mchprs_network::packets::{PacketDecoderExt, PacketEncoderExt};
        use std::io::Cursor;
        let mut cursor = Cursor::new(&DECLARE_COMMANDS.buffer);
        assert_eq!(cursor.read_varint().unwrap(), 135);
        assert_eq!(cursor.read_byte().unwrap(), 0);
        let children = cursor.read_varint().unwrap();
        let mut edges = Vec::new();
        for _ in 0..children {
            edges.push(cursor.read_varint().unwrap());
        }
        assert_eq!(edges.pop(), Some(134));
        let rest = cursor.position() as usize;
        let git_node = [5, 1, 110, 3, b'g', b'i', b't'];
        let end = DECLARE_COMMANDS.buffer.len() - 1 - git_node.len();
        assert_eq!(
            &DECLARE_COMMANDS.buffer[end..end + git_node.len()],
            &git_node
        );
        let mut original = Vec::new();
        original.write_varint(134);
        original.push(0);
        original.write_varint(children - 1);
        for edge in edges {
            original.write_varint(edge);
        }
        original.extend_from_slice(&DECLARE_COMMANDS.buffer[rest..end]);
        original.push(0);
        assert_eq!(original.len(), 1553);
        assert_eq!(
            format!("{:x}", md5::compute(original)),
            "f78c2d87c05142f9056cc44014e2a3ff"
        );
        for (packet, length, digest) in [(&*NO_COMMANDS, 4, "4352d88a78aa39750bf70cd6f27bcaa5")] {
            assert_eq!(packet.packet_id, 0x10);
            assert_eq!(packet.buffer.len(), length);
            assert_eq!(format!("{:x}", md5::compute(&packet.buffer)), digest);
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
                "Usage: /adv [nano|pico] <ticks>"
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
                "Tick count must be between 0 and 10"
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
            ("/radv", "/radvance", vec!["1"]),
            ("/rp", "/redpiler", vec!["compile"]),
            ("/tp", "/teleport", vec!["Admin"]),
        ] {
            assert_eq!(
                native_command_permission(alias, &args),
                native_command_permission(canonical, &args)
            );
            assert_eq!(changes_plot(alias, &args), changes_plot(canonical, &args));
        }
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
pub static DECLARE_COMMANDS: Lazy<PacketEncoder> = Lazy::new(|| {
    CDeclareCommands {
        nodes: &[
            // 0: Root Node
            Node::root(&[
                1, 4, 5, 6, 11, 12, 14, 16, 18, 19, 20, 21, 22, 23, 24, 26, 29, 31, 32, 34, 36, 47,
                49, 53, 60, 61, 63, 65, 66, 67, 71, 73, 74, 75, 82, 83, 85, 88, 90, 91, 101, 106,
                111, 112, 113, 114, 115, 116, 118, 120, 121, 124, 125, 126, 127, 134,
            ]),
            // 1: /teleport
            Node::literal("teleport", &[2, 3]),
            // 2: /teleport [x, y, z]
            Node::argument("x, y, z", Parser::Vec3, &[]).executable(),
            // 3: /teleport [player]
            Node::argument("player", Parser::Entity(3), &[]).executable(),
            // 4: /tp
            Node::redirect("tp", 1),
            // 5: /stop
            Node::literal("stop", &[]).executable(),
            // 6: /plot
            Node::literal(
                "plot",
                &[
                    7, 8, 9, 10, 38, 39, 40, 41, 43, 44, 46, 58, 59, 80, 81, 128, 129, 130, 132,
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
            Node::literal("tps", &[13, 117]).executable(),
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
            Node::argument("block", Parser::BlockState, &[]).executable(),
            // 26: //replace
            Node::literal("/replace", &[27]),
            // 27: //replace [oldblock]
            Node::argument("oldblock", Parser::BlockState, &[28]),
            // 28: //replace [oldblock] [newblock]
            Node::argument("newblock", Parser::BlockState, &[]).executable(),
            // 29: /adv
            Node::literal("adv", &[30, 76, 78]),
            // 30: /adv [rticks]
            Node::argument("rticks", Parser::Integer(0, 100000), &[]).executable(),
            // 31: /radv
            Node::redirect("radv", 29),
            // 32: /speed
            Node::literal("speed", &[33]),
            // 33: /speed [speed]
            Node::argument("speed", Parser::Float(0.0, 10.0), &[]).executable(),
            // 34: //stack
            Node::literal("/stack", &[35]).executable(),
            // 35: //stack [amount]
            Node::argument("amount", Parser::Integer(0, 256), &[]).executable(),
            // 36: //undo
            Node::literal("/undo", &[]).executable(),
            // 37: //sel
            Node::literal("/sel", &[]).executable(),
            // 38: /p auto
            Node::literal("auto", &[]).executable(),
            // 39: /p a
            Node::redirect("a", 9),
            // 40: /p middle
            Node::literal("middle", &[]).executable(),
            // 41: /p visit
            Node::literal("visit", &[42]),
            // 42: /p visit [player]
            Node::argument("player", Parser::Entity(3), &[]).executable(),
            // 43: /p v
            Node::redirect("v", 41),
            // 44: /p teleport
            Node::literal("teleport", &[45]),
            // 45: /p teleport [x, z]
            Node::argument("x, z", Parser::Vec2, &[]).executable(),
            // 46: /p tp
            Node::redirect("tp", 44),
            // 47: //shift
            Node::literal("/shift", &[35]).executable(),
            // 48: //shift [amount]
            Node::argument("amount", Parser::Integer(0, 256), &[]).executable(),
            // 49: /whitelist
            Node::literal("whitelist", &[50, 51]),
            // 50: /whitelist add
            Node::literal("add", &[52]),
            // 51: /whitelist remove
            Node::literal("remove", &[52]),
            // 52: /whitelist add|remove [username]
            Node::argument("username", Parser::Entity(3), &[]).executable(),
            // 53-57: /container <type> <power>
            Node::literal("container", &[54, 55, 56]),
            Node::argument("type", Parser::String(0), &[57]).suggestions("minecraft:ask_server"),
            // Preserve the existing literal alternatives and their node indices.
            Node::literal("hopper", &[57]),
            Node::literal("furnace", &[57]),
            Node::argument("power", Parser::String(0), &[])
                .executable()
                .suggestions("minecraft:ask_server"),
            // 58: /plot lock
            Node::literal("lock", &[]).executable(),
            // 59: /plot unlock
            Node::literal("unlock", &[]).executable(),
            // 60: //wand
            Node::literal("/wand", &[]).executable(),
            // 61: //save
            Node::literal("/save", &[62]),
            // 62: //save [filename]
            Node::argument("filename", Parser::String(0), &[]).executable(),
            // 63: //load
            Node::literal("/load", &[64]),
            // 64: //load [filename]
            Node::argument("filename", Parser::String(0), &[])
                .executable()
                .suggestions("minecraft:ask_server"),
            // 65: /toggleautorp
            Node::literal("toggleautorp", &[]).executable(),
            // 66: /redpiler
            Node::literal("redpiler", &[68, 69, 70]),
            // 67: /rp
            Node::redirect("rp", 66),
            // 68: /redpiler compile
            Node::literal("compile", &[]).executable(),
            // 69: /redpiler inspect
            Node::literal("inspect", &[]).executable(),
            // 70: /redpiler reset
            Node::literal("reset", &[]).executable(),
            // 71: /worldsendrate
            Node::literal("worldsendrate", &[72]).executable(),
            // 72: /worldsendrate [rticks]
            Node::argument("hertz", Parser::Integer(0, 1000), &[]).executable(),
            // 73: /wsr
            Node::redirect("wsr", 71),
            // 74: /curse
            Node::literal("curse", &[]),
            // 75: /bless
            Node::literal("bless", &[]),
            // 76: /adv nano
            Node::literal("nano", &[77]),
            // 77: /adv nano [nticks]
            Node::argument("nticks", Parser::Integer(0, 100000), &[]).executable(),
            // 78: /adv pico
            Node::literal("pico", &[79]),
            // 79: /adv pico [pticks]
            Node::argument("pticks", Parser::Integer(0, 100000), &[]).executable(),
            // 80: /plot select
            Node::literal("select", &[]).executable(),
            // 81: /plot sel
            Node::redirect("sel", 80),
            // 82: /version
            Node::literal("version", &[]).executable(),
            // 83–87: /say <message>, /tellraw <targets> <JSON text>
            Node::literal("say", &[84]),
            Node::argument("message", Parser::String(2), &[]).executable(),
            Node::literal("tellraw", &[86]),
            Node::argument("targets", Parser::Entity(2), &[87]),
            Node::argument("message", Parser::String(2), &[]).executable(),
            // 88–90: animation preference and alias
            Node::literal("piston_anim", &[89]).executable(),
            Node::argument("mode", Parser::String(0), &[]).executable(),
            Node::redirect("bisdon_anim", 88).executable(),
            // 91–100: /help and its topic suggestions
            Node::literal("help", &[92, 93, 94, 95, 96, 97, 98, 99, 100]).executable(),
            Node::argument("topic", Parser::String(0), &[]).executable(),
            Node::literal("plots", &[]).executable(),
            Node::literal("tps", &[]).executable(),
            Node::literal("we", &[]).executable(),
            Node::literal("schematics", &[]).executable(),
            Node::literal("pistons", &[]).executable(),
            Node::literal("rewind", &[]).executable(),
            Node::literal("chat", &[]).executable(),
            Node::literal("redpiler", &[]).executable(),
            // 101–107: tick history and whole-game-tick rewind
            Node::literal("rhistory", &[102, 104, 105, 108]).executable(),
            Node::literal("on", &[103]).executable(),
            Node::argument("ticks", Parser::Integer(1, i32::MAX), &[]).executable(),
            Node::literal("off", &[]).executable(),
            Node::literal("status", &[]).executable(),
            Node::literal("back", &[107]).executable(),
            Node::argument("ticks", Parser::Integer(1, i32::MAX), &[]).executable(),
            // 108–109: server history memory limit, in MiB
            Node::literal("limit", &[109]).executable(),
            Node::argument("MiB", Parser::Integer(0, i32::MAX), &[]).executable(),
            // 110: flexible tool arguments, validated by the command handler
            Node::argument("arguments", Parser::String(2), &[])
                .executable()
                .suggestions("minecraft:ask_server"),
            // 111: //find
            Node::literal("/find", &[110]).executable(),
            // 112: //signsearch
            Node::literal("/signsearch", &[110]).executable(),
            // 113: //ss
            Node::literal("/ss", &[110]).executable(),
            // 114: //rstack
            Node::literal("/rstack", &[110]).executable(),
            // 115: //rs
            Node::literal("/rs", &[110]).executable(),
            // 116: /cursel
            Node::literal("cursel", &[110]).executable(),
            // 117: /tps timings
            Node::literal("timings", &[]).executable(),
            // 118: //update
            Node::literal("/update", &[119]).executable(),
            // 119: //update -p
            Node::literal("-p", &[]).executable(),
            // 120: //invalidatecaches
            Node::literal("/invalidatecaches", &[]).executable(),
            // 121-123: /screenonly [on|off]
            Node::literal("screenonly", &[122, 123]).executable(),
            Node::literal("on", &[]).executable(),
            Node::literal("off", &[]).executable(),
            // 124: /autostack
            Node::literal("autostack", &[110]).executable(),
            // 125-127: legacy aliases for /tps, /adv and /back.
            Node::redirect("rtps", 12).executable(),
            Node::redirect("radvance", 29),
            Node::redirect("rback", 106).executable(),
            // 128-133: /p home, /p h, /p add <nick>, /p remove <nick>.
            Node::literal("home", &[]).executable(),
            Node::redirect("h", 128),
            Node::literal("add", &[131]),
            Node::argument("nick", Parser::String(0), &[])
                .executable()
                .suggestions("minecraft:ask_server"),
            Node::literal("remove", &[133]),
            Node::argument("nick", Parser::String(0), &[])
                .executable()
                .suggestions("minecraft:ask_server"),
            // 134: /git uses the existing greedy, server-completed arguments node.
            Node::literal("git", &[110]).executable(),
        ],
        root_index: 0,
    }
    .encode()
});
