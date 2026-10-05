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

impl Plot {
    /// Handles a command that starts with `/plot` or `/p`
    fn handle_plot_command(&mut self, player: usize, command: &str, args: &[&str]) {
        let (plot_x, plot_z) = self.players[player].pos.plot_pos();

        let permission_node = match command {
            "info" | "i" => "plots.info",
            "claim" | "c" => "plots.claim",
            "auto" | "a" => "plots.auto",
            "middle" => "plots.middle",
            "visit" | "v" => "plots.visit",
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
        if !self.players[player].can_use_commands() {
            self.players[player].send_no_permission_message();
            return false;
        }
        if crate::permissions::dedicated_permissions() {
            if native_command_permission(command, &args)
                .is_some_and(|node| !self.players[player].has_permission(&node))
                || (changes_plot(command, &args) && !self.players[player].can_edit_plot(self.owner))
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
                ["add", username] => {
                    let username = username.to_string();
                    let sender = self.message_sender.clone();
                    let packet_sender = PlayerPacketSender::new(&self.players[player].client);
                    self.async_rt.spawn(async move {
                        match PlayerProfile::lookup_by_username(&username).await {
                            Ok(profile) => sender
                                .send(Message::WhitelistAdd(
                                    profile.uuid.0,
                                    profile.username,
                                    packet_sender,
                                ))
                                .unwrap(),
                            Err(_) => {
                                debug!("Failed to look up profile for username {:?}", username)
                            }
                        }
                    });
                }
                ["remove", username] => {
                    let username = username.to_string();
                    let sender = self.message_sender.clone();
                    let packet_sender = PlayerPacketSender::new(&self.players[player].client);
                    self.async_rt.spawn(async move {
                        match PlayerProfile::lookup_by_username(&username).await {
                            Ok(profile) => sender
                                .send(Message::WhitelistRemove(profile.uuid.0, packet_sender))
                                .unwrap(),
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
                let count = match args.as_slice() {
                    [count] => count.parse::<u32>(),
                    [unit, count]
                        if matches!(unit.to_ascii_lowercase().as_str(), "nano" | "pico") =>
                    {
                        count.parse::<u32>()
                    }
                    _ => {
                        self.players[player].send_error_message("Usage: /adv [nano|pico] <ticks>");
                        return false;
                    }
                };
                if !matches!(count, Ok(ticks) if ticks <= crate::config::CONFIG.max_command_ticks) {
                    self.players[player].send_error_message(&format!(
                        "Tick count must be between 0 and {}",
                        crate::config::CONFIG.max_command_ticks
                    ));
                    return false;
                }
                let Some(arg0) = args.get(0) else {
                    self.players[player]
                        .send_error_message(messages::PLEASE_SPECIFY_NUMBER_TICKS_ADVANCE);
                    return false;
                };
                let start_time = Instant::now();
                let unit = match arg0.to_lowercase().as_str() {
                    "nano" => {
                        let Some(num) = args.get(1) else {
                            self.players[player].send_error_message(
                                messages::PLEASE_SPECIFY_NUMBER_NANO_TICKS_ADVANCE,
                            );
                            return false;
                        };
                        let Ok(ticks) = num.parse::<u32>() else {
                            self.players[player]
                                .send_error_message(messages::UNABLE_PARSE_NANO_TICKS);
                            return false;
                        };
                        if self.redpiler.is_active() {
                            self.players[player].send_error_message(
                                messages::CANNOT_ADVANCE_NANO_TICKS_WHILE_REDPILER,
                            );
                            return false;
                        }
                        if self.world.history.enabled() {
                            self.players[player].send_error_message(
                                messages::DISABLE_TICK_HISTORY_BEFORE_NANO_PICO,
                            );
                            return false;
                        }
                        let advanced = advance_bounded(ticks, || self.world.nanotick_advance(1));
                        format!("{advanced} of {ticks} nano-ticks")
                    }
                    "pico" => {
                        let Some(num) = args.get(1) else {
                            self.players[player].send_error_message(
                                messages::PLEASE_SPECIFY_NUMBER_PICO_TICKS_ADVANCE,
                            );
                            return false;
                        };
                        let Ok(ticks) = num.parse::<u32>() else {
                            self.players[player]
                                .send_error_message(messages::UNABLE_PARSE_PICO_TICKS);
                            return false;
                        };
                        if self.redpiler.is_active() {
                            self.players[player].send_error_message(
                                messages::CANNOT_ADVANCE_PICO_TICKS_WHILE_REDPILER,
                            );
                            return false;
                        }
                        if self.world.history.enabled() {
                            self.players[player].send_error_message(
                                messages::DISABLE_TICK_HISTORY_BEFORE_NANO_PICO,
                            );
                            return false;
                        }
                        let advanced = advance_bounded(ticks, || self.world.picotick_advance(1));
                        format!("{advanced} of {ticks} pico-ticks")
                    }
                    num => {
                        let Ok(ticks) = num.parse::<u32>() else {
                            self.players[player].send_error_message(messages::UNABLE_PARSE_TICKS);
                            return false;
                        };
                        let advanced = advance_bounded(ticks, || self.tick());
                        format!("{advanced} of {ticks} ticks")
                    }
                };

                self.players[player]
                    .send_system_message(&messages::plot_advanced(unit, start_time.elapsed()));
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
        nodes: &[Node {
            flags: 0,
            children: &[],
            redirect_node: None,
            name: None,
            parser: None,
            suggestions_type: None,
        }],
        root_index: 0,
    }
    .encode()
});

bitflags! {
    struct CommandFlags: u32 {
        const ROOT = 0x0;
        const LITERAL = 0x1;
        const ARGUMENT = 0x2;
        const EXECUTABLE = 0x4;
        const REDIRECT = 0x8;
        const HAS_SUGGESTIONS_TYPE = 0x10;
    }
}

// In the future a DSL or some type of generation would be much better.
// For more information, see https://wiki.vg/Command_Data
/// The `DeclareCommands` packet that is sent when the player joins.
/// This is used for command autocomplete.
pub static DECLARE_COMMANDS: Lazy<PacketEncoder> = Lazy::new(|| {
    CDeclareCommands {
        nodes: &[
            // 0: Root Node
            Node {
                flags: CommandFlags::ROOT.bits() as i8,
                children: &[
                    1, 4, 5, 6, 11, 12, 14, 16, 18, 19, 20, 21, 22, 23, 24, 26, 29, 31, 32, 34, 36,
                    47, 49, 53, 60, 61, 63, 65, 66, 67, 71, 73, 74, 75, 82, 83, 85, 88, 90, 91,
                    101, 106, 111, 112, 113, 114, 115, 116, 118, 120, 121, 124, 125, 126, 127,
                ],
                redirect_node: None,
                name: None,
                parser: None,
                suggestions_type: None,
            },
            // 1: /teleport
            Node {
                flags: CommandFlags::LITERAL.bits() as i8,
                children: &[2, 3],
                redirect_node: None,
                name: Some("teleport"),
                parser: None,
                suggestions_type: None,
            },
            // 2: /teleport [x, y, z]
            Node {
                flags: (CommandFlags::ARGUMENT | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("x, y, z"),
                parser: Some(Parser::Vec3),
                suggestions_type: None,
            },
            // 3: /teleport [player]
            Node {
                flags: (CommandFlags::ARGUMENT | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("player"),
                parser: Some(Parser::Entity(3)), // Only allow one player
                suggestions_type: None,
            },
            // 4: /tp
            Node {
                flags: (CommandFlags::REDIRECT | CommandFlags::LITERAL).bits() as i8,
                children: &[],
                redirect_node: Some(1),
                name: Some("tp"),
                parser: None,
                suggestions_type: None,
            },
            // 5: /stop
            Node {
                flags: (CommandFlags::EXECUTABLE | CommandFlags::LITERAL).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("stop"),
                parser: None,
                suggestions_type: None,
            },
            // 6: /plot
            Node {
                flags: (CommandFlags::LITERAL).bits() as i8,
                children: &[7, 8, 9, 10, 38, 39, 40, 41, 43, 44, 46, 58, 59, 80, 81],
                redirect_node: None,
                name: Some("plot"),
                parser: None,
                suggestions_type: None,
            },
            // 7: /plot info
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("info"),
                parser: None,
                suggestions_type: None,
            },
            // 8: /plot i
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::REDIRECT).bits() as i8,
                children: &[],
                redirect_node: Some(7),
                name: Some("i"),
                parser: None,
                suggestions_type: None,
            },
            // 9: /plot claim
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("claim"),
                parser: None,
                suggestions_type: None,
            },
            // 10: /plot c
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::REDIRECT).bits() as i8,
                children: &[],
                redirect_node: Some(9),
                name: Some("c"),
                parser: None,
                suggestions_type: None,
            },
            // 11: /p
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::REDIRECT).bits() as i8,
                children: &[],
                redirect_node: Some(6),
                name: Some("p"),
                parser: None,
                suggestions_type: None,
            },
            // 12: /tps
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[13, 117],
                redirect_node: None,
                name: Some("tps"),
                parser: None,
                suggestions_type: None,
            },
            // 13: /tps [tps]
            Node {
                flags: (CommandFlags::ARGUMENT | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("tps"),
                parser: Some(Parser::Integer(0, i32::MAX)),
                suggestions_type: None,
            },
            // 14: //pos1
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[15],
                redirect_node: None,
                name: Some("/pos1"),
                parser: None,
                suggestions_type: None,
            },
            // 15: //pos1 [pos]
            Node {
                flags: (CommandFlags::ARGUMENT | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("pos"),
                parser: Some(Parser::BlockPos),
                suggestions_type: None,
            },
            // 16: //pos2
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[17],
                redirect_node: None,
                name: Some("/pos2"),
                parser: None,
                suggestions_type: None,
            },
            // 17: //pos2 [pos]
            Node {
                flags: (CommandFlags::ARGUMENT | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("pos"),
                parser: Some(Parser::BlockPos),
                suggestions_type: None,
            },
            // 18: /1
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::REDIRECT).bits() as i8,
                children: &[],
                redirect_node: Some(14),
                name: Some("/1"),
                parser: None,
                suggestions_type: None,
            },
            // 19: /2
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::REDIRECT).bits() as i8,
                children: &[],
                redirect_node: Some(16),
                name: Some("/2"),
                parser: None,
                suggestions_type: None,
            },
            // 20: //copy
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("/copy"),
                parser: None,
                suggestions_type: None,
            },
            // 21: //c
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::REDIRECT).bits() as i8,
                children: &[],
                redirect_node: Some(20),
                name: Some("/c"),
                parser: None,
                suggestions_type: None,
            },
            // 22: //paste
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("/paste"),
                parser: None,
                suggestions_type: None,
            },
            // 23: //p
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::REDIRECT).bits() as i8,
                children: &[],
                redirect_node: Some(22),
                name: Some("/p"),
                parser: None,
                suggestions_type: None,
            },
            // 24: //set
            Node {
                flags: (CommandFlags::LITERAL).bits() as i8,
                children: &[25],
                redirect_node: None,
                name: Some("/set"),
                parser: None,
                suggestions_type: None,
            },
            // 25: //set [block]
            Node {
                flags: (CommandFlags::ARGUMENT | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("block"),
                parser: Some(Parser::BlockState),
                suggestions_type: None,
            },
            // 26: //replace
            Node {
                flags: (CommandFlags::LITERAL).bits() as i8,
                children: &[27],
                redirect_node: None,
                name: Some("/replace"),
                parser: None,
                suggestions_type: None,
            },
            // 27: //replace [oldblock]
            Node {
                flags: (CommandFlags::ARGUMENT).bits() as i8,
                children: &[28],
                redirect_node: None,
                name: Some("oldblock"),
                parser: Some(Parser::BlockState),
                suggestions_type: None,
            },
            // 28: //replace [oldblock] [newblock]
            Node {
                flags: (CommandFlags::ARGUMENT | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("newblock"),
                parser: Some(Parser::BlockState),
                suggestions_type: None,
            },
            // 29: /adv
            Node {
                flags: (CommandFlags::LITERAL).bits() as i8,
                children: &[30, 76, 78],
                redirect_node: None,
                name: Some("adv"),
                parser: None,
                suggestions_type: None,
            },
            // 30: /adv [rticks]
            Node {
                flags: (CommandFlags::ARGUMENT | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("rticks"),
                parser: Some(Parser::Integer(0, 100000)),
                suggestions_type: None,
            },
            // 31: /radv
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::REDIRECT).bits() as i8,
                children: &[],
                redirect_node: Some(29),
                name: Some("radv"),
                parser: None,
                suggestions_type: None,
            },
            // 32: /speed
            Node {
                flags: (CommandFlags::LITERAL).bits() as i8,
                children: &[33],
                redirect_node: None,
                name: Some("speed"),
                parser: None,
                suggestions_type: None,
            },
            // 33: /speed [speed]
            Node {
                flags: (CommandFlags::ARGUMENT | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("speed"),
                parser: Some(Parser::Float(0.0, 10.0)),
                suggestions_type: None,
            },
            // 34: //stack
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[35],
                redirect_node: None,
                name: Some("/stack"),
                parser: None,
                suggestions_type: None,
            },
            // 35: //stack [amount]
            Node {
                flags: (CommandFlags::ARGUMENT | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("amount"),
                parser: Some(Parser::Integer(0, 256)),
                suggestions_type: None,
            },
            // 36: //undo
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("/undo"),
                parser: None,
                suggestions_type: None,
            },
            // 37: //sel
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("/sel"),
                parser: None,
                suggestions_type: None,
            },
            // 38: /p auto
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("auto"),
                parser: None,
                suggestions_type: None,
            },
            // 39: /p a
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::REDIRECT).bits() as i8,
                children: &[],
                redirect_node: Some(9),
                name: Some("a"),
                parser: None,
                suggestions_type: None,
            },
            // 40: /p middle
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("middle"),
                parser: None,
                suggestions_type: None,
            },
            // 41: /p visit
            Node {
                flags: (CommandFlags::LITERAL).bits() as i8,
                children: &[42],
                redirect_node: None,
                name: Some("visit"),
                parser: None,
                suggestions_type: None,
            },
            // 42: /p visit [player]
            Node {
                flags: (CommandFlags::ARGUMENT | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("player"),
                parser: Some(Parser::Entity(3)),
                suggestions_type: None,
            },
            // 43: /p v
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::REDIRECT).bits() as i8,
                children: &[],
                redirect_node: Some(41),
                name: Some("v"),
                parser: None,
                suggestions_type: None,
            },
            // 44: /p teleport
            Node {
                flags: (CommandFlags::LITERAL).bits() as i8,
                children: &[45],
                redirect_node: None,
                name: Some("teleport"),
                parser: None,
                suggestions_type: None,
            },
            // 45: /p teleport [x, z]
            Node {
                flags: (CommandFlags::ARGUMENT | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("x, z"),
                parser: Some(Parser::Vec2),
                suggestions_type: None,
            },
            // 46: /p tp
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::REDIRECT).bits() as i8,
                children: &[],
                redirect_node: Some(44),
                name: Some("tp"),
                parser: None,
                suggestions_type: None,
            },
            // 47: //shift
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[35],
                redirect_node: None,
                name: Some("/shift"),
                parser: None,
                suggestions_type: None,
            },
            // 48: //shift [amount]
            Node {
                flags: (CommandFlags::ARGUMENT | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("amount"),
                parser: Some(Parser::Integer(0, 256)),
                suggestions_type: None,
            },
            // 49: /whitelist
            Node {
                flags: (CommandFlags::LITERAL).bits() as i8,
                children: &[50, 51],
                redirect_node: None,
                name: Some("whitelist"),
                parser: None,
                suggestions_type: None,
            },
            // 50: /whitelist add
            Node {
                flags: (CommandFlags::LITERAL).bits() as i8,
                children: &[52],
                redirect_node: None,
                name: Some("add"),
                parser: None,
                suggestions_type: None,
            },
            // 51: /whitelist remove
            Node {
                flags: (CommandFlags::LITERAL).bits() as i8,
                children: &[52],
                redirect_node: None,
                name: Some("remove"),
                parser: None,
                suggestions_type: None,
            },
            // 52: /whitelist add|remove [username]
            Node {
                flags: (CommandFlags::ARGUMENT | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("username"),
                parser: Some(Parser::Entity(3)),
                suggestions_type: None,
            },
            // 53-57: /container <type> <power>
            Node {
                flags: CommandFlags::LITERAL.bits() as i8,
                children: &[54, 55, 56],
                redirect_node: None,
                name: Some("container"),
                parser: None,
                suggestions_type: None,
            },
            Node {
                flags: (CommandFlags::ARGUMENT | CommandFlags::HAS_SUGGESTIONS_TYPE).bits() as i8,
                children: &[57],
                redirect_node: None,
                name: Some("type"),
                parser: Some(Parser::String(0)),
                suggestions_type: Some("minecraft:ask_server"),
            },
            // Preserve the existing literal alternatives and their node indices.
            Node {
                flags: CommandFlags::LITERAL.bits() as i8,
                children: &[57],
                redirect_node: None,
                name: Some("hopper"),
                parser: None,
                suggestions_type: None,
            },
            Node {
                flags: CommandFlags::LITERAL.bits() as i8,
                children: &[57],
                redirect_node: None,
                name: Some("furnace"),
                parser: None,
                suggestions_type: None,
            },
            Node {
                flags: (CommandFlags::ARGUMENT
                    | CommandFlags::EXECUTABLE
                    | CommandFlags::HAS_SUGGESTIONS_TYPE)
                    .bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("power"),
                parser: Some(Parser::String(0)),
                suggestions_type: Some("minecraft:ask_server"),
            },
            // 58: /plot lock
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("lock"),
                parser: None,
                suggestions_type: None,
            },
            // 59: /plot unlock
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("unlock"),
                parser: None,
                suggestions_type: None,
            },
            // 60: //wand
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("/wand"),
                parser: None,
                suggestions_type: None,
            },
            // 61: //save
            Node {
                flags: (CommandFlags::LITERAL).bits() as i8,
                children: &[62],
                redirect_node: None,
                name: Some("/save"),
                parser: None,
                suggestions_type: None,
            },
            // 62: //save [filename]
            Node {
                flags: (CommandFlags::ARGUMENT | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("filename"),
                parser: Some(Parser::String(0)),
                suggestions_type: None,
            },
            // 63: //load
            Node {
                flags: (CommandFlags::LITERAL).bits() as i8,
                children: &[64],
                redirect_node: None,
                name: Some("/load"),
                parser: None,
                suggestions_type: None,
            },
            // 64: //load [filename]
            Node {
                flags: (CommandFlags::ARGUMENT
                    | CommandFlags::EXECUTABLE
                    | CommandFlags::HAS_SUGGESTIONS_TYPE)
                    .bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("filename"),
                parser: Some(Parser::String(0)),
                suggestions_type: Some("minecraft:ask_server"),
            },
            // 65: /toggleautorp
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("toggleautorp"),
                parser: None,
                suggestions_type: None,
            },
            // 66: /redpiler
            Node {
                flags: CommandFlags::LITERAL.bits() as i8,
                children: &[68, 69, 70], // Children are compile, inspect, reset
                redirect_node: None,
                name: Some("redpiler"),
                parser: None,
                suggestions_type: None,
            },
            // 67: /rp
            Node {
                flags: (CommandFlags::REDIRECT | CommandFlags::LITERAL).bits() as i8,
                children: &[],
                redirect_node: Some(66), // Redirect to /redpiler
                name: Some("rp"),
                parser: None,
                suggestions_type: None,
            },
            // 68: /redpiler compile
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("compile"),
                parser: None,
                suggestions_type: None,
            },
            // 69: /redpiler inspect
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("inspect"),
                parser: None,
                suggestions_type: None,
            },
            // 70: /redpiler reset
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("reset"),
                parser: None,
                suggestions_type: None,
            },
            // 71: /worldsendrate
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[72],
                redirect_node: None,
                name: Some("worldsendrate"),
                parser: None,
                suggestions_type: None,
            },
            // 72: /worldsendrate [rticks]
            Node {
                flags: (CommandFlags::ARGUMENT | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("hertz"),
                parser: Some(Parser::Integer(0, 1000)),
                suggestions_type: None,
            },
            // 73: /wsr
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::REDIRECT).bits() as i8,
                children: &[],
                redirect_node: Some(71),
                name: Some("wsr"),
                parser: None,
                suggestions_type: None,
            },
            // 74: /curse
            Node {
                flags: (CommandFlags::LITERAL).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("curse"),
                parser: None,
                suggestions_type: None,
            },
            // 75: /bless
            Node {
                flags: (CommandFlags::LITERAL).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("bless"),
                parser: None,
                suggestions_type: None,
            },
            // 76: /adv nano
            Node {
                flags: (CommandFlags::LITERAL).bits() as i8,
                children: &[77],
                redirect_node: None,
                name: Some("nano"),
                parser: None,
                suggestions_type: None,
            },
            // 77: /adv nano [nticks]
            Node {
                flags: (CommandFlags::ARGUMENT | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("nticks"),
                parser: Some(Parser::Integer(0, 100000)),
                suggestions_type: None,
            },
            // 78: /adv pico
            Node {
                flags: (CommandFlags::LITERAL).bits() as i8,
                children: &[79],
                redirect_node: None,
                name: Some("pico"),
                parser: None,
                suggestions_type: None,
            },
            // 79: /adv pico [pticks]
            Node {
                flags: (CommandFlags::ARGUMENT | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("pticks"),
                parser: Some(Parser::Integer(0, 100000)),
                suggestions_type: None,
            },
            // 80: /plot select
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("select"),
                parser: None,
                suggestions_type: None,
            },
            // 81: /plot sel
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::REDIRECT).bits() as i8,
                children: &[],
                redirect_node: Some(80),
                name: Some("sel"),
                parser: None,
                suggestions_type: None,
            },
            // 82: /version
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("version"),
                parser: None,
                suggestions_type: None,
            },
            // 83–87: /say <message>, /tellraw <targets> <JSON text>
            Node {
                flags: CommandFlags::LITERAL.bits() as i8,
                children: &[84],
                redirect_node: None,
                name: Some("say"),
                parser: None,
                suggestions_type: None,
            },
            Node {
                flags: (CommandFlags::ARGUMENT | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("message"),
                parser: Some(Parser::String(2)),
                suggestions_type: None,
            },
            Node {
                flags: CommandFlags::LITERAL.bits() as i8,
                children: &[86],
                redirect_node: None,
                name: Some("tellraw"),
                parser: None,
                suggestions_type: None,
            },
            Node {
                flags: CommandFlags::ARGUMENT.bits() as i8,
                children: &[87],
                redirect_node: None,
                name: Some("targets"),
                parser: Some(Parser::Entity(2)),
                suggestions_type: None,
            },
            Node {
                flags: (CommandFlags::ARGUMENT | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("message"),
                parser: Some(Parser::String(2)),
                suggestions_type: None,
            },
            // 88–90: animation preference and alias
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[89],
                redirect_node: None,
                name: Some("piston_anim"),
                parser: None,
                suggestions_type: None,
            },
            Node {
                flags: (CommandFlags::ARGUMENT | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("mode"),
                parser: Some(Parser::String(0)),
                suggestions_type: None,
            },
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::REDIRECT | CommandFlags::EXECUTABLE)
                    .bits() as i8,
                children: &[],
                redirect_node: Some(88),
                name: Some("bisdon_anim"),
                parser: None,
                suggestions_type: None,
            },
            // 91–100: /help and its topic suggestions
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[92, 93, 94, 95, 96, 97, 98, 99, 100],
                redirect_node: None,
                name: Some("help"),
                parser: None,
                suggestions_type: None,
            },
            Node {
                flags: (CommandFlags::ARGUMENT | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("topic"),
                parser: Some(Parser::String(0)),
                suggestions_type: None,
            },
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("plots"),
                parser: None,
                suggestions_type: None,
            },
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("tps"),
                parser: None,
                suggestions_type: None,
            },
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("we"),
                parser: None,
                suggestions_type: None,
            },
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("schematics"),
                parser: None,
                suggestions_type: None,
            },
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("pistons"),
                parser: None,
                suggestions_type: None,
            },
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("rewind"),
                parser: None,
                suggestions_type: None,
            },
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("chat"),
                parser: None,
                suggestions_type: None,
            },
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("redpiler"),
                parser: None,
                suggestions_type: None,
            },
            // 101–107: tick history and whole-game-tick rewind
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[102, 104, 105, 108],
                redirect_node: None,
                name: Some("rhistory"),
                parser: None,
                suggestions_type: None,
            },
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[103],
                redirect_node: None,
                name: Some("on"),
                parser: None,
                suggestions_type: None,
            },
            Node {
                flags: (CommandFlags::ARGUMENT | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("ticks"),
                parser: Some(Parser::Integer(1, i32::MAX)),
                suggestions_type: None,
            },
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("off"),
                parser: None,
                suggestions_type: None,
            },
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("status"),
                parser: None,
                suggestions_type: None,
            },
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[107],
                redirect_node: None,
                name: Some("back"),
                parser: None,
                suggestions_type: None,
            },
            Node {
                flags: (CommandFlags::ARGUMENT | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("ticks"),
                parser: Some(Parser::Integer(1, i32::MAX)),
                suggestions_type: None,
            },
            // 108–109: server history memory limit, in MiB
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[109],
                redirect_node: None,
                name: Some("limit"),
                parser: None,
                suggestions_type: None,
            },
            Node {
                flags: (CommandFlags::ARGUMENT | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("MiB"),
                parser: Some(Parser::Integer(0, i32::MAX)),
                suggestions_type: None,
            },
            // 110: flexible tool arguments, validated by the command handler
            Node {
                flags: (CommandFlags::ARGUMENT
                    | CommandFlags::EXECUTABLE
                    | CommandFlags::HAS_SUGGESTIONS_TYPE)
                    .bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("arguments"),
                parser: Some(Parser::String(2)),
                suggestions_type: Some("minecraft:ask_server"),
            },
            // 111: //find
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[110],
                redirect_node: None,
                name: Some("/find"),
                parser: None,
                suggestions_type: None,
            },
            // 112: //signsearch
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[110],
                redirect_node: None,
                name: Some("/signsearch"),
                parser: None,
                suggestions_type: None,
            },
            // 113: //ss
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[110],
                redirect_node: None,
                name: Some("/ss"),
                parser: None,
                suggestions_type: None,
            },
            // 114: //rstack
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[110],
                redirect_node: None,
                name: Some("/rstack"),
                parser: None,
                suggestions_type: None,
            },
            // 115: //rs
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[110],
                redirect_node: None,
                name: Some("/rs"),
                parser: None,
                suggestions_type: None,
            },
            // 116: /cursel
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[110],
                redirect_node: None,
                name: Some("cursel"),
                parser: None,
                suggestions_type: None,
            },
            // 117: /tps timings
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("timings"),
                parser: None,
                suggestions_type: None,
            },
            // 118: //update
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[119],
                redirect_node: None,
                name: Some("/update"),
                parser: None,
                suggestions_type: None,
            },
            // 119: //update -p
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("-p"),
                parser: None,
                suggestions_type: None,
            },
            // 120: //invalidatecaches
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("/invalidatecaches"),
                parser: None,
                suggestions_type: None,
            },
            // 121-123: /screenonly [on|off]
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[122, 123],
                redirect_node: None,
                name: Some("screenonly"),
                parser: None,
                suggestions_type: None,
            },
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("on"),
                parser: None,
                suggestions_type: None,
            },
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[],
                redirect_node: None,
                name: Some("off"),
                parser: None,
                suggestions_type: None,
            },
            // 124: /autostack
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::EXECUTABLE).bits() as i8,
                children: &[110],
                redirect_node: None,
                name: Some("autostack"),
                parser: None,
                suggestions_type: None,
            },
            // 125-127: legacy aliases for /tps, /adv and /back.
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::REDIRECT | CommandFlags::EXECUTABLE)
                    .bits() as i8,
                children: &[],
                redirect_node: Some(12),
                name: Some("rtps"),
                parser: None,
                suggestions_type: None,
            },
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::REDIRECT).bits() as i8,
                children: &[],
                redirect_node: Some(29),
                name: Some("radvance"),
                parser: None,
                suggestions_type: None,
            },
            Node {
                flags: (CommandFlags::LITERAL | CommandFlags::REDIRECT | CommandFlags::EXECUTABLE)
                    .bits() as i8,
                children: &[],
                redirect_node: Some(106),
                name: Some("rback"),
                parser: None,
                suggestions_type: None,
            },
        ],
        root_index: 0,
    }
    .encode()
});
