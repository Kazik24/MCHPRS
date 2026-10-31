use crate::messages;
#[cfg(any(test, feature = "plot-bench"))]
#[doc(hidden)]
pub mod benchmark;
mod block_light;
#[cfg(test)]
#[path = "tests/client_sync.rs"]
mod client_sync_tests;
#[cfg(test)]
#[path = "tests/client_test_utils.rs"]
mod client_test_utils;
mod command_block;
pub mod commands;
use redstone_tools::compass;
mod containers;
mod data;
pub mod database;
mod diagnostics;
mod git;
mod help;
mod history;
pub(crate) mod memory_budget;
mod monitor;
mod neighbors;
mod packet_handlers;
mod picking;
#[cfg(test)]
#[path = "tests/piston.rs"]
mod piston_tests;
mod player_model;
mod preview;
mod redpiler_compilation;
pub(crate) mod redstone_tools;
mod scoreboard;
mod screen_updates;
#[cfg(test)]
mod sign_tests;
mod visuals;
mod world;
use redstone_tools::waveform;
pub mod worldedit;

use crate::chat::ChatComponent;
use crate::config::CONFIG;
use crate::player::{EntityId, Gamemode, PacketSender, Player, PlayerPos};
use crate::redpiler::{Compiler, CompilerOptions};
use crate::redstone;
use crate::server::{BroadcastMessage, Message, PrivMessage};
use crate::utils::HyphenatedUUID;
use crate::world::World;
use crate::world::storage::Chunk;
use anyhow::Context;
use bus::BusReader;
use mchprs_blocks::BlockColorVariant;
#[cfg(test)]
use mchprs_blocks::BlockFace;
use mchprs_blocks::BlockPos;
#[cfg(test)]
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::Block;
use mchprs_network::PlayerPacketSender;
use mchprs_network::packets::clientbound::*;
use mchprs_save_data::plot_data::{ChunkData, PistonAnimation, PlotData, Tps, WorldSendRate};
#[cfg(test)]
use mchprs_world::TickPriority;
use mchprs_world::{AdvancePhase, TickEntry};
use monitor::TimingsMonitor;
use scoreboard::RedpilerState;
use serde_json::json;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::mpsc::{Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use tracing::{debug, error, info, warn};

pub use self::data::empty_plot;
use self::data::sleep_time_for_tps;
use self::scoreboard::Scoreboard;
pub use world::PlotWorld;

/// The width of a plot (2^n)
pub const PLOT_SCALE: u32 = 5;

/// The width of a plot counted in chunks
pub const PLOT_WIDTH: i32 = 2i32.pow(PLOT_SCALE);
/// The plot width in blocks
pub const PLOT_BLOCK_WIDTH: i32 = PLOT_WIDTH * 16;
pub const NUM_CHUNKS: usize = PLOT_WIDTH.pow(2) as usize;

/// The height of the world in sections (Default: 16, Max: 127)
pub const PLOT_SECTIONS: usize = 16;
/// The plot height in blocks
pub const PLOT_BLOCK_HEIGHT: i32 = PLOT_SECTIONS as i32 * 16;

fn in_bounds((min, max): (BlockPos, BlockPos), pos: BlockPos) -> bool {
    (min.x..=max.x).contains(&pos.x) && (min.y..=max.y).contains(&pos.y) && (min.z..=max.z).contains(&pos.z)
}

pub struct Plot {
    #[cfg(any(test, feature = "plot-bench"))]
    transient_test_fixture: bool,
    // TODO Route plot-specific world packets through the active plot when a player enters it.
    pub world: PlotWorld,
    pub players: Vec<Player>,
    pub redpiler: Compiler,
    /// In-flight compilation owns a snapshot; the live plot stays on this thread.
    compilation: Option<redpiler_compilation::CompilationJob>,

    // Thread communication
    message_receiver: BusReader<BroadcastMessage>,
    message_sender: Sender<Message>,
    priv_message_receiver: Receiver<PrivMessage>,

    locked_players: HashSet<EntityId>,

    // Timings
    tps: Tps,
    world_send_rate: WorldSendRate,
    piston_animation: PistonAnimation,
    last_update_time: Instant,
    lag_time: Duration,
    last_nspt: Option<Duration>,
    timings: TimingsMonitor,
    /// The last time a player was in this plot
    last_player_time: Instant,
    /// The last time the world changes were sent to the player
    last_world_send_time: Instant,
    /// The duration we should sleep for after every update
    sleep_time: Duration,
    /// When this is false, the update loop will end and the thread will stop.
    /// This will be set to false if no players are on the plot for a certain amount of time.
    running: bool,
    /// If true, the plot will remain running even if no players are on for a long time.
    always_running: bool,
    /// Whether the update loop may automatically compile eligible plots.
    auto_redpiler: bool,

    owner: Option<u128>,
    /// Runtime entered by this plot's worker for its async helpers.
    async_rt: Runtime,
    scoreboard: Scoreboard,
    last_sidebar_update: Instant,
    /// The last time player positions were reported to the RedBox proxy.
    last_redbox_state: Instant,
    neighbor_views: neighbors::Views,
    neighbor_source: Option<neighbors::LiveSource>,
    waveform_source: Option<crate::waveform::api::Source>,
    git: git::State,
    wire_cursor: usize,
}

impl Plot {
    fn update_timing_report(&self) -> String {
        let p = &self.world.update_stats;
        let mut encode_ns = 0u64;
        let mut compress_ns = 0u64;
        let mut write_ns = 0u64;
        let mut packets = 0u64;
        let mut bytes = 0u64;
        let mut queued_bytes = 0usize;
        let mut coalesced = 0u64;
        let mut failures = 0u64;
        for sender in &self.world.packet_senders {
            let s = sender.send_stats();
            encode_ns += s.encode_ns;
            compress_ns += s.compress_ns;
            write_ns += s.write_ns;
            packets += s.packets;
            bytes += s.bytes;
            queued_bytes += s.queued_bytes;
            coalesced += s.coalesced_blocks;
            failures += s.failures;
        }
        messages::plot_update_timings(
            p.simulated_ticks,
            p.simulation.as_secs_f64(),
            p.simulation.saturating_sub(p.projection).as_secs_f64(),
            p.projection.as_secs_f64(),
            p.plot.as_secs_f64(),
            p.flushes,
            p.sections,
            p.records,
            p.collection.as_secs_f64(),
            p.enqueue.as_secs_f64(),
            self.effective_send_rate(),
            if self.world.screen_only() { "screen" } else { "all" },
            packets,
            bytes,
            encode_ns as f64 / 1e9,
            compress_ns as f64 / 1e9,
            write_ns as f64 / 1e9,
            queued_bytes,
            coalesced,
            failures,
        )
    }

    fn effective_send_rate(&self) -> u32 {
        visuals::visual_send_rate(
            self.world_send_rate.0,
            self.tps,
            CONFIG.fast_render_threshold,
            CONFIG.fast_render_send_rate,
        )
    }

    fn refresh_sidebar(&mut self, force: bool) {
        if self.players.is_empty() || (!force && self.last_sidebar_update.elapsed() < Duration::from_secs(1)) {
            return;
        }

        self.last_sidebar_update = Instant::now();
        let actual_tps = if self.tps == Tps::Limited(0) {
            Some(0.0)
        } else {
            self.timings.generate_report().map(|report| report.two_s)
        };
        let visual_update_rate = (self.world_send_rate.0 != 0).then(|| self.effective_send_rate());
        let git_readers = (0..self.players.len())
            .filter(|&player| self.git_access(player, "read"))
            .map(|player| self.players[player].uuid)
            .collect();
        self.scoreboard.update_plot_metrics(
            &self.players,
            self.tps,
            actual_tps,
            self.world.history.enabled(),
            self.world.history.len(),
            self.world.history.capacity(),
            self.world.history.memory_bytes(),
            visual_update_rate,
            scoreboard::PlotStatus {
                piston_mode: self.piston_animation,
                pistons_animated: !self.world.fast_rendering && !self.world.screen_only(),
                screen_only: self.world.screen_only(),
                git_head: self.git.head.as_deref(),
                git_restoring: self.git.locked,
                git_recovery: self.git.fatal,
                git_readers,
            },
        );
    }

    fn update_render_mode(&mut self) {
        let fast = visuals::static_pistons(self.piston_animation, self.tps, CONFIG.fast_render_threshold);
        if fast == self.world.fast_rendering {
            return;
        }
        self.world.fast_rendering = fast;
        // Refresh only active movement chunks when switching presentation. This
        // also restores moving entities if the user pauses after leaving fast mode.
        let chunks: std::collections::BTreeSet<_> = self
            .world
            .piston_state
            .motions
            .iter()
            .filter_map(|motion| self.world.get_chunk_index_for_block(motion.pos.x, motion.pos.z))
            .collect();
        for index in chunks {
            let chunk = &self.world.chunks[index];
            let encoded = chunk.encode_packet_for_client(fast || self.world.screen_only());
            for player in &self.players {
                if Self::get_chunk_distance(chunk.x, chunk.z, player.last_chunk_x, player.last_chunk_z) <= CONFIG.view_distance.max(0) as u32 {
                    player.client.send_packet(&encoded);
                }
            }
        }
    }
    fn tick(&mut self) -> bool {
        if self.compiling_redpiler() {
            return false;
        }
        self.timings.tick();
        if self.redpiler.is_active() {
            if let Err(error) = self.redpiler.tick_with_world(&mut self.world) {
                tracing::error!(%error, "sampled execution failed");
                if let Err(export_error) = self.reset_redpiler() {
                    tracing::error!(%export_error, "compiled state export failed; keeping compiler ownership");
                    for player in &self.players {
                        player.send_error_message(&format!("Compiled state could not be returned to native simulation: {export_error}"));
                    }
                }
                self.auto_redpiler = false;
                return false;
            }
        } else {
            self.world.tick_interpreted();
        }
        true
    }

    /// Send a block change to all connected players
    pub fn send_block_change(&mut self, pos: BlockPos, id: u32) {
        if !Self::in_plot_bounds(self.world.x, self.world.z, pos.x, pos.z) {
            for player in 0..self.players.len() {
                self.restore_neighbor_chunk(player, pos);
            }
            return;
        }
        let block_change = CBlockChange {
            block_id: id as i32,
            pos: pos.packed(),
        }
        .encode();
        for player in &mut self.players {
            player.client.send_packet(&block_change);
        }
    }

    pub fn broadcast_chat_message(&mut self, message: String) {
        let broadcast_message = Message::ChatInfo(0, messages::plot_chat_source(self.world.x, self.world.z), message);
        self.message_sender.send(broadcast_message).unwrap();
    }

    pub fn broadcast_plot_chat_message(&mut self, message: &str) {
        for player in &mut self.players {
            player.send_chat_message(0, &ChatComponent::from_legacy_text(message));
        }
    }

    fn change_player_gamemode(&mut self, player_idx: usize, gamemode: Gamemode, small: bool) {
        if crate::permissions::dedicated_permissions() {
            let name = match gamemode {
                Gamemode::Creative => "creative",
                Gamemode::Adventure => "adventure",
                Gamemode::Spectator => "spectator",
                Gamemode::Survival => "survival",
            };
            if !self.players[player_idx].has_permission(&format!("commands.gamemode.{name}")) {
                self.players[player_idx].send_no_permission_message();
                return;
            }
            if small && !self.players[player_idx].has_permission("commands.small") {
                self.players[player_idx].send_no_permission_message();
                return;
            }
        }
        if self.players[player_idx].small_model.is_some() != small && !self.set_small(player_idx, small) {
            return;
        }
        self.players[player_idx].set_gamemode(gamemode);
        let _ = self
            .message_sender
            .send(Message::PlayerUpdateGamemode(self.players[player_idx].uuid, gamemode));
    }

    fn on_player_move(&mut self, player_idx: usize, old: PlayerPos, new: PlayerPos) {
        if self.git_checkout_locked() {
            return;
        }
        if matches!(self.players[player_idx].gamemode, Gamemode::Spectator)
            || (crate::permissions::dedicated_permissions()
                && !self.players[player_idx].can_build_action("interact", self.owner, (self.world.x, self.world.z)))
        {
            return;
        }
        let old_block = old.block_pos();
        let new_block = new.block_pos();

        if self.world.get_block(old_block).pressure_plate_powered().is_some() && !self.are_players_on_block(old_block) {
            self.set_pressure_plate(old_block, false);
        }

        if self.world.get_block(new_block).pressure_plate_powered().is_some() && self.players[player_idx].on_ground {
            self.set_pressure_plate(new_block, true);
        }
    }

    fn set_pressure_plate(&mut self, pos: BlockPos, powered: bool) {
        if self.redpiler.is_active() {
            if let Err(error) = self.reset_redpiler() {
                tracing::error!(%error, "cannot hand off compiled state for a pressure-plate input");
                return;
            }
        }

        let block = self.world.get_block(pos);
        match block {
            block if block.pressure_plate_powered().is_some() => {
                redstone::set_pressure_plate(&mut self.world, pos, block, powered);
            }
            _ => warn!("Block at {} is not a pressure plate", pos),
        }
    }

    fn are_players_on_block(&mut self, pos: BlockPos) -> bool {
        for player in &self.players {
            if player.pos.block_pos() == pos
                && player.on_ground
                && !matches!(player.gamemode, Gamemode::Spectator)
                && (!crate::permissions::dedicated_permissions() || player.can_build_action("interact", self.owner, (self.world.x, self.world.z)))
            {
                return true;
            }
        }
        false
    }

    fn spawn_player(viewer: &Player, player: &Player) {
        viewer.send_packet(
            &CSpawnPlayer {
                entity_id: player.entity_id as i32,
                uuid: player.uuid,
                pitch: player.pitch,
                yaw: player.yaw,
                x: player.pos.x,
                y: player.pos.y,
                z: player.pos.z,
            }
            .encode(),
        );
        viewer.send_packet(&player.entity_metadata_packet(false));
        viewer.send_packet(
            &CEntityScale {
                entity_id: player.entity_id as i32,
                scale: player.scale(),
            }
            .encode(),
        );
        viewer.send_packet(&player.entity_equipment_packet());
        player_model::spawn_model(viewer, player);
    }

    fn enter_plot(&mut self, player: Player) {
        info!(player = %player.username, uuid = %format_args!("{:032x}", player.uuid),
            plot_x = self.world.x, plot_z = self.world.z, "Player entered plot");
        // Another running plot can claim this plot before teleporting here.
        // Refresh the cached owner so permission checks reflect that claim.
        self.owner = database::get_plot_owner(self.world.x, self.world.z).map(|s| s.parse::<HyphenatedUUID>().unwrap().0);
        self.save();
        let status = CEntityStatus {
            entity_id: player.entity_id as i32,
            entity_status: if player.can_use_commands() { 26 } else { 24 },
        }
        .encode();
        player.send_packet(&status);
        player.send_packet(&player.entity_metadata_packet(true));
        player.send_packet(
            &CEntityScale {
                entity_id: player.entity_id as i32,
                scale: player.scale(),
            }
            .encode(),
        );
        for other in &self.players {
            Self::spawn_player(other, &player);
            Self::spawn_player(&player, other);
        }

        player.send_system_message(&messages::plot_entered(self.world.x, self.world.z));
        self.world.packet_senders.push(PlayerPacketSender::new(&player.client));
        self.players.push(player);
        let player = self.players.len() - 1;
        let git_read = self.git_access(player, "read");
        self.scoreboard.add_player(&self.players[player], git_read);
        self.refresh_sidebar(true);
        self.update_view_pos_for_player(self.players.len() - 1, true);
    }

    fn get_chunk_distance(x1: i32, z1: i32, x2: i32, z2: i32) -> u32 {
        let x = x1 - x2;
        let z = z1 - z2;
        x.abs().max(z.abs()) as u32
    }

    fn set_chunk_loaded_at_player(&mut self, player_idx: usize, chunk_x: i32, chunk_z: i32, was_loaded: bool, should_be_loaded: bool) {
        if was_loaded && !should_be_loaded {
            self.unload_git_chunk(player_idx, chunk_x, chunk_z);
            self.unload_wire_chunk(player_idx, chunk_x, chunk_z);
            self.neighbor_views.unload(self.players[player_idx].uuid, (chunk_x, chunk_z));
            let unload_chunk = CUnloadChunk { chunk_x, chunk_z }.encode();
            self.players[player_idx].client.send_packet(&unload_chunk);
        } else if !was_loaded && should_be_loaded {
            if !Plot::chunk_in_plot_bounds(self.world.x, self.world.z, chunk_x, chunk_z) {
                if CONFIG.neighbor_update_interval_ms != 0 {
                    self.neighbor_views.load(self.players[player_idx].uuid, (chunk_x, chunk_z));
                }
                self.players[player_idx].client.send_packet(&Chunk::encode_empty_packet(chunk_x, chunk_z));
            } else {
                let chunk_data = self.world.chunks[self.world.get_chunk_index_for_chunk(chunk_x, chunk_z)]
                    .encode_packet_for_client(self.world.fast_rendering || self.world.screen_only());
                self.players[player_idx].client.send_packet(&chunk_data);
            }
        }
    }

    pub fn update_view_pos_for_player(&mut self, player_idx: usize, force_load: bool) {
        let view_distance = CONFIG.view_distance as i32;
        let (chunk_x, chunk_z) = self.players[player_idx].pos.chunk_pos();
        let last_chunk_x = self.players[player_idx].last_chunk_x;
        let last_chunk_z = self.players[player_idx].last_chunk_z;

        let update_view = CUpdateViewPosition { chunk_x, chunk_z }.encode();
        self.players[player_idx].client.send_packet(&update_view);

        if ((last_chunk_x - chunk_x).abs() <= view_distance * 2 && (last_chunk_z - chunk_z).abs() <= view_distance * 2) && !force_load {
            let nx = chunk_x.min(last_chunk_x) - view_distance;
            let nz = chunk_z.min(last_chunk_z) - view_distance;
            let px = chunk_x.max(last_chunk_x) + view_distance;
            let pz = chunk_z.max(last_chunk_z) + view_distance;
            for x in nx..=px {
                for z in nz..=pz {
                    let was_loaded = Self::get_chunk_distance(x, z, last_chunk_x, last_chunk_z) <= view_distance as u32;
                    let should_be_loaded = Self::get_chunk_distance(x, z, chunk_x, chunk_z) <= view_distance as u32;
                    // TODO Spread chunk loading over time to avoid client hitches when a large view is loaded.
                    self.set_chunk_loaded_at_player(player_idx, x, z, was_loaded, should_be_loaded);
                }
            }
        } else {
            for x in last_chunk_x - view_distance..=last_chunk_x + view_distance {
                for z in last_chunk_z - view_distance..=last_chunk_z + view_distance {
                    self.set_chunk_loaded_at_player(player_idx, x, z, true, false);
                }
            }
            for x in chunk_x - view_distance..=chunk_x + view_distance {
                for z in chunk_z - view_distance..=chunk_z + view_distance {
                    self.set_chunk_loaded_at_player(player_idx, x, z, false, true);
                }
            }
        }
        self.players[player_idx].last_chunk_x = chunk_x;
        self.players[player_idx].last_chunk_z = chunk_z;
    }

    /// After an expensive operation or change in timings, it's important to
    /// call this function so our timings monitor doesn't think we're running
    /// behind.
    fn reset_timings(&mut self) {
        self.lag_time = Duration::ZERO;
        self.last_update_time = Instant::now();
        self.last_nspt = None;
        self.timings.reset_timings();
    }

    /// Redpiler needs to reset implicitly in the case of any block changes done by a player. This can be
    fn reset_redpiler(&mut self) -> Result<(), crate::redpiler::ExecutionError> {
        if self.compiling_redpiler() {
            self.cancel_redpiler_compilation();
            self.scoreboard.set_redpiler_state(&self.players, RedpilerState::Stopped);
            self.scoreboard.set_redpiler_options(&self.players, &Default::default());
            self.reset_timings();
        }
        if self.redpiler.is_active() {
            debug!("Resetting redpiler");
            let bounds = self.world.get_corners();
            self.redpiler.try_reset(&mut self.world, bounds)?;
            self.scoreboard.set_redpiler_state(&self.players, RedpilerState::Stopped);
            self.scoreboard.set_redpiler_options(&self.players, &Default::default());

            // reseting redpiler could cause a large amount of block updates
            self.reset_timings();
        }
        Ok(())
    }

    fn destroy_entity(&mut self, entity_id: u32) {
        let destroy_entity = CDestroyEntities {
            entity_ids: vec![entity_id as i32],
        }
        .encode();
        for player in &mut self.players {
            player.client.send_packet(&destroy_entity);
        }
    }

    fn leave_plot(&mut self, uuid: u128) -> Player {
        let player_idx = self.players.iter().position(|p| p.uuid == uuid).unwrap();
        self.clear_wire_tool(player_idx);
        self.players[player_idx].redstone_tools.waveform = None;
        self.hide_git(player_idx, true);
        self.close_open_container(player_idx);
        self.world.packet_senders.remove(player_idx);
        let mut player = self.players.remove(player_idx);
        self.neighbor_views.remove_player(player.uuid);
        self.disable_empty_plot_history();

        let destroy_other_entities = CDestroyEntities {
            entity_ids: self
                .players
                .iter()
                .flat_map(|p| std::iter::once(p.entity_id as i32).chain(p.small_model.as_ref().map(|model| model.entity_id as i32)))
                .collect(),
        }
        .encode();
        player.client.send_packet(&destroy_other_entities);

        for chunk in &self.world.chunks {
            player.client.send_packet(
                &CUnloadChunk {
                    chunk_x: chunk.x,
                    chunk_z: chunk.z,
                }
                .encode(),
            );
        }
        self.destroy_entity(player.entity_id);
        if let Some(model) = &player.small_model {
            self.destroy_entity(model.entity_id);
        }
        self.locked_players.remove(&player.entity_id);
        self.scoreboard.remove_player(&player);
        redstone_tools::selection::remove(&mut player);
        player.redstone_tools.block_search = None;
        player.redstone_tools.sign_search = None;
        player.redstone_tools.auto_stack = None;
        player
    }

    fn disable_empty_plot_history(&mut self) {
        if self.players.is_empty() {
            self.world.waveform.stop("empty_plot");
        }
        if self.players.is_empty() && self.world.history.enabled() {
            let bytes = self.world.history.disable();
            debug!(
                plot_x = self.world.x,
                plot_z = self.world.z,
                bytes,
                "Disabled tick history because the plot is empty"
            );
        }
    }

    fn chunk_in_plot_bounds(plot_x: i32, plot_z: i32, chunk_x: i32, chunk_z: i32) -> bool {
        let (x, z) = (chunk_x >> PLOT_SCALE, chunk_z >> PLOT_SCALE);
        plot_x == x && plot_z == z
    }

    fn in_plot_bounds(plot_x: i32, plot_z: i32, x: i32, z: i32) -> bool {
        Plot::chunk_in_plot_bounds(plot_x, plot_z, x >> 4, z >> 4)
    }

    pub fn claim_plot(&mut self, plot_x: i32, plot_z: i32, player: usize) {
        let player = &mut self.players[player];
        let limit = if player.has_permission("plots.limit.unlimited") {
            None
        } else {
            let prefix = if crate::permissions::dedicated_permissions() {
                "mchprs.plots.limit."
            } else {
                "plots.plot."
            };
            Some(player.numeric_permission_limit(prefix).unwrap_or(1))
        };
        match database::claim_plot(plot_x, plot_z, &format!("{:032x}", player.uuid), limit) {
            Ok(database::ClaimResult::Claimed) => {}
            Ok(database::ClaimResult::AlreadyClaimed) => {
                player.send_system_message(messages::PLOT_ALREADY_CLAIMED);
                return;
            }
            Ok(database::ClaimResult::LimitReached(limit)) => {
                player.send_error_message(&messages::plot_claim_limit(limit));
                return;
            }
            Err(error) => {
                player.send_error_message(&messages::plot_claim_failed(error));
                return;
            }
        }
        if self.world.x == plot_x && self.world.z == plot_z {
            self.owner = Some(player.uuid);
            for chunk in &mut self.world.chunks {
                Self::color_claimed_chunk(chunk, plot_x, plot_z);
            }
        }
        let center = Plot::get_center(plot_x, plot_z);
        player.teleport(PlayerPos::new(center.0, 64.0, center.1));
        player.send_system_message(&messages::plot_claimed(plot_x, plot_z));
    }

    pub fn get_center(plot_x: i32, plot_z: i32) -> (f64, f64) {
        const WIDTH: f64 = PLOT_BLOCK_WIDTH as f64;
        (plot_x as f64 * WIDTH + WIDTH / 2.0, plot_z as f64 * WIDTH + WIDTH / 2.0)
    }

    pub fn get_next_plot(plot_x: i32, plot_z: i32) -> (i32, i32) {
        let x = plot_x.abs();
        let z = plot_z.abs();

        match x.cmp(&z) {
            Ordering::Greater => {
                if plot_x > 0 {
                    (plot_x, plot_z + 1)
                } else {
                    (plot_x, plot_z - 1)
                }
            }
            Ordering::Less => {
                if plot_z > 0 {
                    (plot_x - 1, plot_z)
                } else {
                    (plot_x + 1, plot_z)
                }
            }
            Ordering::Equal => {
                if plot_x == plot_z && plot_x > 0 || plot_x == x {
                    (plot_x, plot_z + 1)
                } else if plot_z == z {
                    (plot_x, plot_z - 1)
                } else {
                    (plot_x + 1, plot_z)
                }
            }
        }
    }

    fn handle_commands(&mut self) {
        let mut removal_offset = 0;
        for player_idx in 0..self.players.len() {
            let player_idx = player_idx - removal_offset;
            let commands: Vec<String> = std::mem::take(&mut self.players[player_idx].command_queue);
            if commands::run_command_queue(commands, |command, args| self.handle_command(player_idx, command, args)) {
                removal_offset += 1;
            }
        }
    }

    fn handle_messages(&mut self) {
        while let Ok(message) = self.message_receiver.try_recv() {
            match message {
                BroadcastMessage::Chat(sender, message) => {
                    for player in &mut self.players {
                        player.send_chat_message(sender, &message);
                    }
                }
                BroadcastMessage::PlayerJoinedInfo(player_join_info) => {
                    let join_message = (CONFIG.native_chat && crate::permissions::ranked_chat() && !crate::redbox::enabled())
                        .then(|| ChatComponent::player_joined(&player_join_info.username));
                    let player_info = CPlayerInfo::AddPlayer(vec![CPlayerInfoAddPlayer {
                        name: player_join_info.username,
                        properties: player_join_info.properties,
                        gamemode: player_join_info.gamemode.get_id(),
                        ping: 0,
                        uuid: player_join_info.uuid,
                        display_name: None,
                    }])
                    .encode();
                    for player in &mut self.players {
                        player.client.send_packet(&player_info);
                        if player.uuid != player_join_info.uuid {
                            if let Some(message) = &join_message {
                                player.send_chat_message(0, message);
                            }
                        }
                    }
                }
                BroadcastMessage::PlayerLeft(uuid) => {
                    let player_info = CPlayerInfo::RemovePlayer(vec![uuid]).encode();
                    for player in &mut self.players {
                        player.client.send_packet(&player_info);
                    }
                }
                BroadcastMessage::Shutdown => {
                    self.close_all_containers();
                    let mut players: Vec<Player> = std::mem::take(&mut self.players);
                    self.disable_empty_plot_history();
                    for player in players.iter_mut() {
                        player.save();
                        player.kick(
                            json!({
                                "text": messages::SERVER_CLOSED
                            })
                            .to_string(),
                        );
                    }
                    self.always_running = false;
                    self.running = false;
                    return;
                }
                BroadcastMessage::PlayerUpdateGamemode(uuid, gamemode) => {
                    let player_info = CPlayerInfo::UpdateGamemode(uuid, gamemode.get_id()).encode();
                    for player in &mut self.players {
                        player.client.send_packet(&player_info);
                    }
                }
            }
        }
        // Handle messages from the private message channel
        while let Ok(message) = self.priv_message_receiver.try_recv() {
            match message {
                PrivMessage::Diagnostics(sender, span) => {
                    let _entered = span.enter();
                    let text = self.diagnostics();
                    info!(details = %text, "Plot diagnostics");
                    sender.send_system_message(&text);
                }
                PrivMessage::PlayerEnterPlot(player) => {
                    self.enter_plot(player);
                }
                PrivMessage::PlayerTeleportOther(mut player, username) => {
                    if let Some(other) = self.players.iter().find(|p| p.username == username) {
                        player.teleport(other.pos);
                    }
                    self.enter_plot(player);
                }
                PrivMessage::RedBoxTeleport { uuid, position } => {
                    if let Some(player) = self.players.iter().position(|p| p.uuid == uuid) {
                        if position.is_valid() {
                            self.teleport_player_to(player, position.player_pos(), position.yaw, position.pitch);
                        }
                    }
                }
                PrivMessage::RedBoxGamemode(uuid, gamemode) => {
                    if let Some(player) = self.players.iter().position(|p| p.uuid == uuid) {
                        self.players[player].set_gamemode(gamemode);
                        let _ = self.message_sender.send(Message::PlayerUpdateGamemode(uuid, gamemode));
                    }
                }
                PrivMessage::RedBoxPermissions {
                    uuid,
                    group,
                    prefix,
                    permissions,
                } => {
                    if let Some(player) = self.players.iter().position(|p| p.uuid == uuid) {
                        self.players[player].apply_proxy_permissions(group, prefix, permissions);
                    }
                }
            }
        }
    }

    /// Remove players outside of the plot
    fn remove_oob_players(&mut self) {
        let mut outside_players = Vec::new();
        for player in 0..self.players.len() {
            let player = &mut self.players[player];
            if self.locked_players.contains(&player.entity_id) {
                continue;
            }
            let (plot_x, plot_z) = player.pos.plot_pos();
            if plot_x != self.world.x || plot_z != self.world.z {
                if player.should_transfer_plot((self.world.x, self.world.z), (plot_x, plot_z), player.pos) {
                    outside_players.push(player.uuid);
                }
            }
        }

        for uuid in outside_players {
            let player = self.leave_plot(uuid);
            let player_leave_plot = Message::PlayerLeavePlot(player);
            self.message_sender.send(player_leave_plot).unwrap();
        }
    }

    /// Remove disconnected players
    fn remove_dc_players(&mut self) {
        for player in 0..self.players.len() {
            if !self.players[player].client.alive() {
                self.clear_wire_tool(player);
                self.close_open_container(player);
            }
        }
        let message_sender = &mut self.message_sender;

        let mut disconnected_players = Vec::new();
        self.players.retain(|player| {
            let alive = player.client.alive();
            if !alive {
                player.save();
                message_sender.send(Message::PlayerLeft(player.uuid)).unwrap();
                disconnected_players.push(player.entity_id);
                if let Some(model) = &player.small_model {
                    disconnected_players.push(model.entity_id);
                }
            }
            alive
        });
        for entity_id in disconnected_players {
            self.destroy_entity(entity_id);
        }
        self.disable_empty_plot_history();
    }

    /// Report player positions to the RedBox proxy a few times per second so it
    /// can resolve `/tp`, `/home`, `/warp`, and `/back`.
    fn report_redbox_states(&mut self) {
        if !crate::redbox::enabled() {
            return;
        }
        let now = Instant::now();
        if now.duration_since(self.last_redbox_state) < Duration::from_millis(500) {
            return;
        }
        self.last_redbox_state = now;
        let world = format!("plot_{}_{}", self.world.x, self.world.z);
        for player in &self.players {
            let position = crate::redbox::Position::new(world.clone(), player.pos, player.yaw, player.pitch);
            let packet = CPluginMessage {
                channel: crate::redbox::CHANNEL.to_owned(),
                data: crate::redbox::encode_player_state(player.uuid, &position),
            }
            .encode();
            player.client.send_packet(&packet);
        }
    }

    /// Update player view positions and handle packets
    fn update_players(&mut self) {
        for player_idx in 0..self.players.len() {
            if self.players[player_idx].update() {
                self.update_view_pos_for_player(player_idx, false);
            }
        }
        // Handle received packets
        for player_idx in 0..self.players.len() {
            self.handle_packets_for_player(player_idx);
        }
        self.sync_small_models();
    }

    fn interpreter_idle(&self) -> bool {
        !self.redpiler.is_active()
            && !self.world.history.enabled()
            && !self.world.waveform.enabled()
            && self.world.piston_state.phase == AdvancePhase::BetweenTicks
            && self.world.piston_state.events.is_empty()
            && self.world.piston_state.motions.is_empty()
            && self.world.to_be_ticked.queues_iter().all(|queue| queue.is_empty())
    }

    fn update(&mut self) {
        if let Some(source) = &mut self.waveform_source {
            source.poll(&self.world.waveform, self.redpiler.is_active() || self.compilation.is_some(), self.owner);
        }
        if self.world.waveform.take_overflow_notice() {
            for player in &mut self.players {
                if player.can_edit_plot(self.owner, (self.world.x, self.world.z)) {
                    player.send_error_message(
                        "Waveform buffer full; recording stopped. Captured history is still available. Use /waveform on to restart.",
                    );
                }
            }
        }
        let update_started = Instant::now();
        self.update_git();
        self.handle_messages();
        self.update_render_mode();
        self.poll_redpiler_compilation();

        // Only tick if there are players in the plot
        if !self.players.is_empty() {
            let idle = self.interpreter_idle();
            self.timings.set_ticking(!idle);
            let now = Instant::now();
            self.last_player_time = now;

            let effective_send_rate = self.effective_send_rate();
            let world_send_rate = Duration::from_nanos(1_000_000_000 / effective_send_rate.max(1) as u64);

            let max_batch_size = match self.last_nspt {
                Some(Duration::ZERO) | None => 1,
                Some(last_nspt) => {
                    let ticks_fit = (world_send_rate.as_nanos() / last_nspt.as_nanos()) as u64;
                    // A tick previously took longer than the world send rate.
                    // Run at least one just so we're not stuck doing nothing
                    ticks_fit.max(1)
                }
            };

            let tick_duration = match self.tps {
                Tps::Limited(tps) if tps != 0 => Some(Duration::from_nanos((1_000_000_000 / tps as u64).max(1))),
                _ => None,
            };
            let batch_size = match self.tps {
                Tps::Limited(tps) if tps != 0 => {
                    let dur_per_tick = Duration::from_nanos((1_000_000_000 / tps as u64).max(1));
                    self.lag_time += now - self.last_update_time;
                    let batch_size = (self.lag_time.as_nanos() / dur_per_tick.as_nanos()) as u64;
                    batch_size.min(max_batch_size)
                }
                Tps::Unlimited => max_batch_size,
                _ => 0,
            };

            self.last_update_time = now;
            if self.compiling_redpiler() || idle {
                // Idle time must not become tick debt when a player wakes the circuit.
                self.lag_time = Duration::ZERO;
            } else if batch_size != 0 {
                let simulation_started = Instant::now();
                // 50_000 (= 3.33 MHz) here is arbitrary.
                // We just need a number that's not too high so we actually get around to sending block updates.
                let batch_size = batch_size.min(50_000) as u32;
                let slice = if self.wire_tools_active() {
                    Duration::from_millis(5)
                } else {
                    Duration::from_millis(200)
                };
                let mut ticks_completed = 0;
                for _ in 0..batch_size {
                    if !self.tick() {
                        break;
                    }
                    ticks_completed += 1;
                    if simulation_started.elapsed() >= slice {
                        break;
                    }
                }
                if self.redpiler.is_active() {
                    let projection_started = Instant::now();
                    self.redpiler.flush(&mut self.world);
                    self.world.update_stats.projection += projection_started.elapsed();
                }
                if let Some(duration) = tick_duration {
                    self.lag_time = self.lag_time.saturating_sub(duration * ticks_completed);
                }
                if ticks_completed > 0 {
                    self.last_nspt = Some(simulation_started.elapsed() / ticks_completed);
                }
                self.world.update_stats.simulation += simulation_started.elapsed();
                self.world.update_stats.simulated_ticks += u64::from(ticks_completed);
            }

            if self.auto_redpiler
                && !self.world.waveform.enabled()
                && !idle
                && self.compilation.is_none()
                && !self.git_checkout_locked()
                && !self.redpiler.is_active()
                && (self.tps == Tps::Unlimited || self.timings.is_running_behind())
                && !self.world.chunks.iter().any(Chunk::requires_interpreter)
            {
                self.start_redpiler(Default::default());
            }

            let now = Instant::now();
            let time_since_last_world_send = now - self.last_world_send_time;
            if self.world_send_rate.0 != 0 && time_since_last_world_send > world_send_rate {
                self.last_world_send_time = now;
                self.world.flush_block_changes();
            }
        } else {
            self.timings.set_ticking(false);
            // Unload plot after 600 seconds unless the plot should be always loaded
            if self.last_player_time.elapsed().as_secs() > 600 && !self.always_running {
                self.running = false;
                self.timings.stop();
            }
        }

        self.update_players();
        self.report_redbox_states();

        // Handle commands before removing players just in case they ran a command before leaving
        self.handle_commands();
        self.update_wire_tools();
        self.update_open_containers();
        for player in &mut self.players {
            redstone_tools::selection::update(player);
        }

        self.refresh_sidebar(false);

        for (excluded, event) in self.world.level_events.drain(..) {
            let packet = event.encode();
            for player in &self.players {
                if player.uuid != excluded {
                    player.send_packet(&packet);
                }
            }
        }

        for sound in self.world.sounds.drain(..) {
            let packet = sound.packet();
            for player in &self.players {
                if sound.audible(player.uuid, player.pos) {
                    player.send_packet(&packet);
                }
            }
        }

        for command in self.world.command_messages.drain(..) {
            // Automated plot circuits cannot flood chat on unrelated plots.
            for player in &self.players {
                if command.recipient.matches(&player.username) {
                    player.send_raw_system_message(command.message.clone());
                }
            }
        }

        self.remove_dc_players();
        self.remove_oob_players();
        self.update_neighbor_views();
        self.world.update_stats.plot += update_started.elapsed();
    }

    fn create_async_rt() -> Runtime {
        Runtime::new().unwrap()
    }

    fn generate_chunk(layers: i32, x: i32, z: i32) -> Chunk {
        let border = Block::Wool {
            color: BlockColorVariant::Black,
        }
        .get_id();
        let fill = Block::Wool {
            color: BlockColorVariant::White,
        }
        .get_id();

        let mut chunk = Chunk::empty(x, z);

        for ry in 0..layers {
            for rx in 0..16 {
                for rz in 0..16 {
                    let block_x = (x << 4) | rx;
                    let block_z = (z << 4) | rz;

                    if block_x % PLOT_BLOCK_WIDTH == 0
                        || block_z % PLOT_BLOCK_WIDTH == 0
                        || (block_x + 1) % PLOT_BLOCK_WIDTH == 0
                        || (block_z + 1) % PLOT_BLOCK_WIDTH == 0
                    {
                        chunk.set_block(rx as u32, ry as u32, rz as u32, border);
                    } else {
                        chunk.set_block(rx as u32, ry as u32, rz as u32, fill);
                    }
                }
            }
        }
        chunk
    }

    fn color_claimed_chunk(chunk: &mut Chunk, plot_x: i32, plot_z: i32) {
        let black = Block::Wool {
            color: BlockColorVariant::Black,
        }
        .get_id();
        let red = Block::Wool {
            color: BlockColorVariant::Red,
        }
        .get_id();

        for ry in 0..8 {
            for rx in 0..16 {
                for rz in 0..16 {
                    let block_x = (chunk.x << 4) | rx;
                    let block_z = (chunk.z << 4) | rz;
                    if (block_x == plot_x * PLOT_BLOCK_WIDTH
                        || block_x == (plot_x + 1) * PLOT_BLOCK_WIDTH - 1
                        || block_z == plot_z * PLOT_BLOCK_WIDTH
                        || block_z == (plot_z + 1) * PLOT_BLOCK_WIDTH - 1)
                        && chunk.get_block(rx as u32, ry as u32, rz as u32) == black
                    {
                        chunk.set_block(rx as u32, ry as u32, rz as u32, red);
                    }
                }
            }
        }
    }

    fn from_data(
        plot_data: PlotData<PLOT_SECTIONS>,
        x: i32,
        z: i32,
        rx: BusReader<BroadcastMessage>,
        tx: Sender<Message>,
        priv_rx: Receiver<PrivMessage>,
        always_running: bool,
    ) -> Plot {
        let chunk_x_offset = x << PLOT_SCALE;
        let chunk_z_offset = z << PLOT_SCALE;
        let owner = database::get_plot_owner(x, z).map(|s| s.parse::<HyphenatedUUID>().unwrap().0);
        let mut chunks: Vec<Chunk> = plot_data
            .chunk_data
            .into_iter()
            .enumerate()
            .map(|(i, c)| Chunk::load(chunk_x_offset + i as i32 / PLOT_WIDTH, chunk_z_offset + i as i32 % PLOT_WIDTH, c))
            .collect();
        if owner.is_some() {
            for chunk in &mut chunks {
                Self::color_claimed_chunk(chunk, x, z);
            }
        }
        if chunks.len() != NUM_CHUNKS {
            error!("This plot has the wrong number of chunks!");
            let possible_scale = (chunks.len() as f64).sqrt().log2();
            error!(
                "Note: it most likely came from a server running plot scale {}, this server is running a plot scale of {}",
                possible_scale, PLOT_SCALE
            );
        }

        let mut world = PlotWorld::from_chunks(x, z, chunks, plot_data.pending_ticks.into_iter().collect());
        let probes = database::waveform_probes(x, z).expect("Cannot load saved waveform definitions");
        assert!(probes.len() <= 1024, "Saved waveform definition exceeds the hard signal limit");
        let mut names = HashSet::new();
        let mut positions = HashSet::new();
        assert!(
            probes.iter().all(|p| crate::waveform::Probe::valid_name(&p.name)
                && Self::in_plot_bounds(x, z, p.pos.x, p.pos.z)
                && (0..PLOT_BLOCK_HEIGHT).contains(&p.pos.y)
                && names.insert(p.name.as_str())
                && positions.insert(p.pos)),
            "Invalid saved waveform definitions"
        );
        world.waveform.replace(probes);
        if plot_data.piston_state.logical_tick != 0
            || plot_data.piston_state.next_identity != 0
            || !plot_data.piston_state.events.is_empty()
            || !plot_data.piston_state.motions.is_empty()
        {
            world.piston_state = plot_data.piston_state;
            world.invalidate_interpreter_caches();
        }
        let tps = plot_data.tps;
        world.fast_rendering = visuals::static_pistons(plot_data.piston_animation, tps, CONFIG.fast_render_threshold);
        let world_send_rate = plot_data.world_send_rate;
        world.set_screen_only(database::get_screen_only(x, z));
        Plot {
            #[cfg(any(test, feature = "plot-bench"))]
            transient_test_fixture: false,
            last_player_time: Instant::now(),
            last_update_time: Instant::now(),
            last_world_send_time: Instant::now(),
            lag_time: Duration::new(0, 0),
            sleep_time: sleep_time_for_tps(tps),
            last_nspt: None,
            message_receiver: rx,
            message_sender: tx,
            priv_message_receiver: priv_rx,
            players: Vec::new(),
            locked_players: HashSet::new(),
            running: true,
            auto_redpiler: CONFIG.auto_redpiler,
            tps,
            world_send_rate,
            piston_animation: plot_data.piston_animation,
            always_running,
            redpiler: Default::default(),
            compilation: None,
            timings: TimingsMonitor::new(tps),
            owner,
            async_rt: Plot::create_async_rt(),
            scoreboard: Default::default(),
            last_sidebar_update: Instant::now(),
            last_redbox_state: Instant::now(),
            neighbor_views: Default::default(),
            neighbor_source: None,
            waveform_source: None,
            git: Default::default(),
            wire_cursor: 0,
            world,
        }
    }

    fn save(&mut self) {
        // The checkout worker owns the durable save until its result is applied.
        if self.git_checkout_locked() {
            return;
        }
        let world = &mut self.world;
        let chunk_data: Vec<ChunkData<PLOT_SECTIONS>> = world.chunks.iter_mut().map(|c| c.save()).collect();
        let data = PlotData {
            tps: self.tps,
            world_send_rate: self.world_send_rate,
            chunk_data,
            pending_ticks: world.to_be_ticked.iter_entries().collect(),
            piston_state: world.piston_state.clone(),
            piston_animation: self.piston_animation,
        };
        data.save_to_file(format!("./world/plots/p{},{}", world.x, world.z)).unwrap();

        self.reset_timings();
    }

    fn run(&mut self, initial_player: Option<Player>) {
        if CONFIG.waveform_enabled {
            self.waveform_source =
                Some(crate::waveform::api::Source::register((self.world.x, self.world.z)).expect("Cannot register waveform source"));
        }
        let _guard = self.async_rt.enter();

        if CONFIG.neighbor_update_interval_ms != 0 {
            self.neighbor_source = Some(neighbors::LiveSource::register((self.world.x, self.world.z)));
        }

        if let Some(player) = initial_player {
            self.enter_plot(player);
        }

        while self.running {
            // Fast path, for super high RTPS
            if self.sleep_time <= Duration::from_millis(5) && !self.players.is_empty() && !self.compiling_redpiler() && !self.interpreter_idle() {
                self.update();
                if self.tps != Tps::Unlimited {
                    thread::yield_now();
                }
                continue;
            }

            let before = Instant::now();
            self.update();
            let delta = before.elapsed();

            let cadence = if self.compiling_redpiler() || self.interpreter_idle() {
                Duration::from_millis(20)
            } else if self.wire_tools_active() {
                self.sleep_time.min(Duration::from_millis(20))
            } else {
                self.sleep_time
            };
            if delta < cadence {
                let sleep_time = cadence - delta;
                thread::sleep(sleep_time);
            } else {
                thread::yield_now();
            }
        }
    }

    pub fn load_and_run(
        x: i32,
        z: i32,
        rx: BusReader<BroadcastMessage>,
        tx: Sender<Message>,
        priv_rx: Receiver<PrivMessage>,
        always_running: bool,
        initial_player: Option<Player>,
    ) {
        thread::Builder::new()
            .name(format!("p{},{}", x, z))
            .spawn(move || {
                let loaded = data::load_plot(format!("./world/plots/p{},{}", x, z)).with_context(|| format!("error loading plot {},{}", x, z));
                let data = match loaded {
                    Ok(data) => data,
                    Err(error) => {
                        let _ = tx.send(Message::PlotLoadFailed(x, z, format!("{:#}", error), initial_player, priv_rx));
                        return;
                    }
                };
                let mut plot = Plot::from_data(data, x, z, rx, tx, priv_rx, always_running);
                plot.run(initial_player);
            })
            .unwrap();
    }
}

impl Drop for Plot {
    fn drop(&mut self) {
        self.cancel_redpiler_compilation();
        // Packet-handler tests own an in-memory world, with no persistent saves
        // or server thread to receive crash-recovery messages.
        #[cfg(any(test, feature = "plot-bench"))]
        if self.transient_test_fixture {
            return;
        }
        self.finish_git();
        self.close_all_containers();
        if !self.players.is_empty() {
            for player in &mut self.players {
                player.save(); // just in case

                let world = &self.world;
                let (px, pz) = if world.x == 0 && world.z == 0 {
                    // Can't send players to spawn if spawn crashed!
                    Plot::get_center(1, 0)
                } else {
                    Plot::get_center(0, 0)
                };
                player.teleport(PlayerPos::new(px, 64.0, pz));
                player.send_error_message(messages::PLOT_YOU_WERE_PREVIOUSLY_CRASHED);
            }

            while !self.players.is_empty() {
                let uuid = self.players[0].uuid;
                let player = self.leave_plot(uuid);
                self.message_sender.send(Message::PlayerLeavePlot(player)).unwrap();
            }
        }

        if let Err(error) = self.reset_redpiler() {
            tracing::error!(%error, "compiled state export failed during plot shutdown; skipping save");
        } else {
            self.world.chunks.iter_mut().for_each(|chunk| chunk.compress());
            self.save();
        }
        let world = &self.world;
        self.message_sender.send(Message::PlotUnload(world.x, world.z)).unwrap();
    }
}

#[test]
fn chunk_save_and_load_test() {
    let mut chunk = Chunk::empty(1, 1);
    chunk.set_block(13, 63, 12, 332);
    chunk.set_block(13, 62, 12, 331);
    let chunk_data = chunk.save();
    let loaded_chunk = Chunk::load(1, 1, chunk_data);
    assert_eq!(loaded_chunk.get_block(13, 63, 12), 332);
    assert_eq!(loaded_chunk.get_block(13, 62, 12), 331);
    assert_eq!(loaded_chunk.get_block(13, 64, 12), 0);
}

#[test]
fn copper_bulb_item_actions_preserve_state_and_exclude_predicted_feedback() {
    use crate::interaction::{ItemUseResult, UseOnBlockContext, use_item_on_block};
    use mchprs_blocks::items::{Item, ItemStack};
    let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
    let pos = BlockPos::new(4, 30, 4);
    world.set_block(
        pos,
        Block::from_name("exposed_copper_bulb")
            .unwrap()
            .with_copper_bulb_state(true, false)
            .unwrap(),
    );
    let connection = mchprs_network::test_support::connection(false).unwrap();
    let mut player = Player::test_player(connection.player);
    for (item, variant, event) in [
        ("honeycomb", "waxed_exposed_copper_bulb", 3003),
        ("iron_axe", "exposed_copper_bulb", 3004),
        ("iron_axe", "copper_bulb", 3005),
    ] {
        let result = use_item_on_block(
            &ItemStack {
                item_type: Item::from_name(item).unwrap(),
                count: 1,
                nbt: None,
            },
            &mut world,
            UseOnBlockContext {
                block_pos: pos,
                block_face: BlockFace::Top,
                player: &mut player,
                cursor_y: 0.5,
            },
        );
        assert!(matches!(result, ItemUseResult::Used));
        assert_eq!(world.get_block(pos).get_name(), variant);
        assert_eq!(world.get_block(pos).copper_bulb_state(), Some((true, false)));
        let (excluded, feedback) = world.level_events.last().unwrap();
        assert_eq!(*excluded, player.uuid);
        assert_eq!(feedback.effect_id, event);
    }
    assert_eq!(world.level_events.len(), 3);
    assert_eq!(world.sounds.len(), 2);
    assert!(world.sounds.iter().all(|sound| sound.excluded == Some(player.uuid)));
}
