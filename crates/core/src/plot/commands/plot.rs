use super::super::{Plot, PlotWorld, database};
use super::parse_relative_coord;
use crate::messages;
use crate::player::{PacketSender, PlayerPos};
use crate::server::Message;

impl Plot {
    /// Teleports an indexed player, transferring to another plot through the
    /// normal routing when the destination lies there.
    pub(in crate::plot) fn teleport_player_to(&mut self, player: usize, pos: PlayerPos, yaw: f32, pitch: f32) -> bool {
        self.close_open_container(player);
        self.players[player].yaw = yaw;
        self.players[player].pitch = pitch;
        self.players[player].teleport(pos);
        if pos.plot_pos() != (self.world.x, self.world.z) {
            let uuid = self.players[player].uuid;
            let player = self.leave_plot(uuid);
            let _ = self.message_sender.send(Message::PlayerLeavePlot(player));
            return true;
        }
        false
    }

    /// Handles a command that starts with `/plot` or `/p`
    pub(super) fn handle_plot_command(&mut self, player: usize, command: &str, args: &[&str]) {
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
                    Err(error) => self.players[player].send_error_message(&messages::plot_read_failed(error)),
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
                    Ok(MembershipResult::Changed { name, .. }) => self.players[player].send_system_message(&if add {
                        messages::plot_member_added(name, self.world.x, self.world.z)
                    } else {
                        messages::plot_member_removed(name, self.world.x, self.world.z)
                    }),
                    Ok(MembershipResult::Unchanged) => self.players[player].send_system_message(if add {
                        messages::PLOT_MEMBER_ALREADY_ADDED
                    } else {
                        messages::PLOT_MEMBER_NOT_ADDED
                    }),
                    Ok(MembershipResult::UnknownPlayer) => self.players[player].send_error_message(messages::PLOT_MEMBER_UNKNOWN_PLAYER),
                    Ok(MembershipResult::AmbiguousPlayer) => self.players[player].send_error_message(messages::PLOT_MEMBER_AMBIGUOUS_PLAYER),
                    Ok(MembershipResult::PlotUnclaimed) => self.players[player].send_error_message(messages::PLOT_UNCLAIMED),
                    Ok(MembershipResult::NotOwner) => self.players[player].send_no_permission_message(),
                    Ok(MembershipResult::IsOwner) => self.players[player].send_error_message(messages::PLOT_MEMBER_IS_OWNER),
                    Err(error) => self.players[player].send_error_message(&messages::plot_members_update_failed(error)),
                }
            }
            "info" | "i" => {
                if let Some(owner) = database::get_plot_owner(plot_x, plot_z) {
                    self.players[player].send_system_message(&messages::plot_owner(database::get_cached_username(owner.clone()).unwrap_or(owner)));
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
                            self.players[player].send_error_message(messages::PLOT_CLAIMS_READ_FAILED);
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
                        self.players[player].send_system_message(&messages::plot_index_range(plots.len()));
                    }
                } else {
                    self.players[player].send_system_message(&messages::player_has_no_plots(args[0]));
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
}
