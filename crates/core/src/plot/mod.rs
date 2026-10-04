pub mod commands;
mod data;
pub mod database;
mod monitor;
mod packet_handlers;
mod picking;
#[cfg(test)]
mod piston_tests;
mod scoreboard;
mod visuals;
pub mod worldedit;

use crate::chat::ChatComponent;
use crate::config::CONFIG;
use crate::player::{EntityId, Gamemode, PacketSender, Player, PlayerPos};
use crate::redpiler::backend::ScheduledBlockTick;
use crate::redpiler::{Compiler, CompilerOptions, TickScheduler};
use crate::redstone;
use crate::server::{BroadcastMessage, Message, PrivMessage};
use crate::utils::HyphenatedUUID;
use crate::world::storage::Chunk;
use crate::world::{BlockAction, World};
use anyhow::Context;
use bus::BusReader;
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::Block;
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_network::packets::clientbound::*;
use mchprs_network::packets::SlotData;
use mchprs_network::PlayerPacketSender;
use mchprs_save_data::plot_data::{ChunkData, PlotData, Tps, WorldSendRate};
use mchprs_world::{AdvancePhase, PistonMotion, PistonState, TickPriority};
use monitor::TimingsMonitor;
use scoreboard::RedpilerState;
use serde_json::json;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::mpsc::{Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use tracing::{debug, error, warn};

pub use self::data::empty_plot;
use self::data::sleep_time_for_tps;
use self::scoreboard::Scoreboard;

/// The width of a plot (2^n)
pub const PLOT_SCALE: u32 = 4;

/// The width of a plot counted in chunks
pub const PLOT_WIDTH: i32 = 2i32.pow(PLOT_SCALE);
/// The plot width in blocks
pub const PLOT_BLOCK_WIDTH: i32 = PLOT_WIDTH * 16;
pub const NUM_CHUNKS: usize = PLOT_WIDTH.pow(2) as usize;

/// The height of the world in sections (Default: 16, Max: 127)
pub const PLOT_SECTIONS: usize = 16;
/// The plot height in blocks
pub const PLOT_BLOCK_HEIGHT: i32 = PLOT_SECTIONS as i32 * 16;

pub struct Plot {
    //todo plots are worlds on its own, when player enters the plot, all network packets are dispatched by that plot
    pub world: PlotWorld,
    pub players: Vec<Player>,
    pub redpiler: Compiler,

    // Thread communication
    message_receiver: BusReader<BroadcastMessage>,
    message_sender: Sender<Message>,
    priv_message_receiver: Receiver<PrivMessage>,

    locked_players: HashSet<EntityId>,

    // Timings
    tps: Tps,
    world_send_rate: WorldSendRate,
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
    auto_redpiler: bool, //todo disables/enables automatic redpiler engaging

    owner: Option<u128>,
    async_rt: Runtime, //todo use one runtime for all plots since it's heavy object used just for some small requests
    scoreboard: Scoreboard,
}

pub struct PlotWorld {
    x: i32,
    z: i32,
    chunks: Vec<Chunk>,
    to_be_ticked: TickScheduler<ScheduledBlockTick>,
    piston_state: PistonState,
    packet_senders: Vec<PlayerPacketSender>,
    is_cursed: bool,
    fast_rendering: bool,
}

impl PlotWorld {
    #[inline]
    pub fn from_chunks(
        x: i32,
        z: i32,
        chunks: Vec<Chunk>,
        to_be_ticked: TickScheduler<ScheduledBlockTick>,
    ) -> Self {
        let mut world = Self {
            x,
            z,
            chunks,
            to_be_ticked,
            piston_state: PistonState::default(),
            packet_senders: Vec::new(),
            is_cursed: false,
            fast_rendering: false,
        };
        // Position-only old saves bind to the loaded type once. They never
        // dispatch an observer tick into a subsequently moved/replaced block.
        let ticks: Vec<_> = world.to_be_ticked.iter_entries().collect();
        world.to_be_ticked = ticks
            .into_iter()
            .filter_map(|mut tick| {
                if tick.block_type.is_none() {
                    let block = world.get_block(tick.pos);
                    if matches!(block, Block::MovingPiston { .. }) {
                        return None;
                    }
                    if matches!(block, Block::Piston { .. }) {
                        tick.ticks_left = 0;
                        tick.tick_priority = TickPriority::NanoTick;
                    }
                    tick.block_type = Some(block.registry_id());
                }
                Some(tick)
            })
            .collect();
        let mut entities = Vec::new();
        for (i, chunk) in world.chunks.iter().enumerate() {
            for (&local, entity) in &chunk.block_entities {
                let pos = BlockPos::new(
                    x * PLOT_BLOCK_WIDTH + (i as i32 / PLOT_WIDTH) * 16 + local.x,
                    local.y,
                    z * PLOT_BLOCK_WIDTH + (i as i32 % PLOT_WIDTH) * 16 + local.z,
                );
                if let BlockEntity::MovingPiston(e) = entity {
                    if matches!(world.get_block(pos), Block::MovingPiston { .. }) {
                        entities.push((pos, e.get_progress()));
                    }
                }
            }
        }
        entities.sort_by_key(|(p, _)| (p.x, p.y, p.z));
        for (pos, progress) in entities {
            world.register_motion(pos, progress);
        }
        world
    }

    fn get_chunk_index_for_chunk(&self, chunk_x: i32, chunk_z: i32) -> usize {
        let local_x = chunk_x - self.x * PLOT_WIDTH;
        let local_z = chunk_z - self.z * PLOT_WIDTH;
        (local_x * PLOT_WIDTH + local_z).unsigned_abs() as usize
    }

    fn get_chunk_index_for_block(&self, block_x: i32, block_z: i32) -> Option<usize> {
        let chunk_x = (block_x - (self.x * PLOT_BLOCK_WIDTH)) >> 4;
        let chunk_z = (block_z - (self.z * PLOT_BLOCK_WIDTH)) >> 4;
        if !(0..PLOT_WIDTH).contains(&chunk_x) || !(0..PLOT_WIDTH).contains(&chunk_z) {
            return None;
        }
        Some(((chunk_x << PLOT_SCALE) + chunk_z).unsigned_abs() as usize)
    }

    fn flush_block_changes(&mut self) {
        let fast = self.fast_rendering;
        let moving_states: std::collections::HashMap<_, _> = if fast {
            self.piston_state
                .motions
                .iter()
                .filter_map(|motion| match self.get_block_entity(motion.pos) {
                    Some(BlockEntity::MovingPiston(entity)) => {
                        Some((motion.pos, entity.block_state))
                    }
                    _ => None,
                })
                .collect()
        } else {
            Default::default()
        };
        for packet in self.chunks.iter_mut().flat_map(|c| c.multi_blocks()) {
            let encoded = if fast {
                let records: Vec<_> = packet
                    .records
                    .iter()
                    .map(|record| {
                        let mut record = C3BMultiBlockChangeRecord {
                            x: record.x,
                            y: record.y,
                            z: record.z,
                            block_id: record.block_id,
                        };
                        if matches!(Block::from_id(record.block_id), Block::MovingPiston { .. }) {
                            let pos = BlockPos::new(
                                packet.chunk_x * 16 + i32::from(record.x),
                                packet.chunk_y as i32 * 16 + i32::from(record.y),
                                packet.chunk_z * 16 + i32::from(record.z),
                            );
                            record.block_id = moving_states.get(&pos).copied().unwrap_or(0);
                        }
                        record
                    })
                    .collect();
                CMultiBlockChange {
                    chunk_x: packet.chunk_x,
                    chunk_y: packet.chunk_y,
                    chunk_z: packet.chunk_z,
                    records,
                }
                .encode()
            } else {
                packet.encode()
            };
            for player in &self.packet_senders {
                player.send_packet(&encoded);
            }
        }
        for chunk in &mut self.chunks {
            chunk.reset_multi_blocks();
        }
    }

    pub fn get_corners(&self) -> (BlockPos, BlockPos) {
        const W: i32 = PLOT_BLOCK_WIDTH;
        let first_pos = BlockPos::new(self.x * W, 0, self.z * W);
        let second_pos = BlockPos::new(
            (self.x + 1) * W - 1,
            PLOT_BLOCK_HEIGHT - 1,
            (self.z + 1) * W - 1,
        );
        (first_pos, second_pos)
    }

    pub fn get_chunks(&self) -> &[Chunk] {
        &self.chunks
    }

    pub fn scheduler(&self) -> &TickScheduler<ScheduledBlockTick> {
        &self.to_be_ticked
    }

    fn register_motion(&mut self, pos: BlockPos, progress: f32) {
        let state = &mut self.piston_state;
        state.motions.retain(|m| m.pos != pos);
        state.next_identity += 1;
        state.motions.push(PistonMotion {
            pos,
            identity: state.next_identity,
            progress,
            previous_progress: progress,
            last_tick: state.logical_tick,
            carried_entity: None,
        });
    }

    /// Find the next operation in this game tick. All stepping commands share
    /// these phases, so pausing between operations does not alter their order.
    fn prepare_operation(&mut self) -> bool {
        loop {
            match self.piston_state.phase {
                AdvancePhase::BetweenTicks => {
                    self.piston_state.logical_tick += 1;
                    self.piston_state.scheduled_advanced = false;
                    self.piston_state.phase = AdvancePhase::ScheduledTicks;
                }
                AdvancePhase::ScheduledTicks => {
                    if !self.to_be_ticked.this_tick_ref().is_empty() {
                        return true;
                    }
                    if !self.piston_state.scheduled_advanced {
                        self.to_be_ticked.end_last_tick_move_next();
                        self.piston_state.scheduled_advanced = true;
                    } else {
                        self.piston_state.phase = AdvancePhase::PistonEvents;
                    }
                }
                AdvancePhase::PistonEvents => {
                    if !self.piston_state.events.is_empty() {
                        return true;
                    }
                    self.piston_state.movement_work = self
                        .piston_state
                        .motions
                        .iter()
                        .map(|m| (m.pos, m.identity))
                        .collect();
                    self.piston_state.movement_cursor = 0;
                    self.piston_state.phase = AdvancePhase::MovingEntities;
                }
                AdvancePhase::MovingEntities => {
                    if self.piston_state.movement_cursor < self.piston_state.movement_work.len() {
                        return true;
                    }
                    self.piston_state.movement_work.clear();
                    self.piston_state.movement_cursor = 0;
                    self.piston_state.phase = AdvancePhase::BetweenTicks;
                    return false;
                }
            }
        }
    }

    fn advance_operation(&mut self) -> bool {
        if !self.prepare_operation() {
            return false;
        }
        match self.piston_state.phase {
            AdvancePhase::ScheduledTicks => {
                let tick = self.to_be_ticked.this_tick().pop_first().unwrap();
                let block = self.get_block(tick.pos);
                if tick.block_type == Some(block.registry_id()) {
                    redstone::tick(block, self, tick.pos);
                }
            }
            AdvancePhase::PistonEvents => {
                let event = self.piston_state.events.pop_front().unwrap();
                redstone::piston::execute_event(self, event);
            }
            AdvancePhase::MovingEntities => {
                let (pos, identity) =
                    self.piston_state.movement_work[self.piston_state.movement_cursor];
                self.piston_state.movement_cursor += 1;
                redstone::piston::tick_motion(self, pos, identity);
            }
            AdvancePhase::BetweenTicks => unreachable!(),
        }
        true
    }

    pub fn tick_interpreted(&mut self) {
        while self.advance_operation() {}
    }

    pub fn nanotick_advance(&mut self, amount: u32) {
        for _ in 0..amount {
            if !self.prepare_operation() {
                continue;
            }
            let count = match self.piston_state.phase {
                AdvancePhase::ScheduledTicks => self.to_be_ticked.this_tick_ref().len(),
                AdvancePhase::PistonEvents => self.piston_state.events.len(),
                AdvancePhase::MovingEntities => {
                    self.piston_state.movement_work.len() - self.piston_state.movement_cursor
                }
                AdvancePhase::BetweenTicks => 0,
            };
            for _ in 0..count {
                self.advance_operation();
            }
        }
    }

    pub fn picotick_advance(&mut self, amount: u32) {
        for _ in 0..amount {
            self.advance_operation();
        }
    }
}

impl World for PlotWorld {
    /// Sets a block in storage. Returns true if a block was changed.
    fn set_block_raw(&mut self, pos: BlockPos, block: u32) -> bool {
        let chunk_index = match self.get_chunk_index_for_block(pos.x, pos.z) {
            Some(idx) => idx,
            None => return false,
        };

        // Check to see if block is within height limit
        if pos.y >= PLOT_BLOCK_HEIGHT || pos.y < 0 {
            return false;
        }

        let old = self.get_block(pos);
        if matches!(old, Block::MovingPiston { .. }) && old.get_id() != block {
            self.delete_block_entity(pos);
        }
        let chunk = &mut self.chunks[chunk_index];
        chunk.set_block(
            (pos.x & 0xF) as u32,
            pos.y as u32,
            (pos.z & 0xF) as u32,
            block,
        )
    }

    /// Returns the block state id of the block at `pos`
    fn get_block_raw(&self, pos: BlockPos) -> u32 {
        let chunk_index = match self.get_chunk_index_for_block(pos.x, pos.z) {
            Some(idx) => idx,
            None => return 0,
        };
        let chunk = &self.chunks[chunk_index];
        chunk.get_block((pos.x & 0xF) as u32, pos.y as u32, (pos.z & 0xF) as u32)
    }

    fn delete_block_entity(&mut self, pos: BlockPos) {
        self.piston_state.motions.retain(|m| m.pos != pos);
        let chunk_index = match self.get_chunk_index_for_block(pos.x, pos.z) {
            Some(idx) => idx,
            None => return,
        };
        let chunk = &mut self.chunks[chunk_index];
        chunk.delete_block_entity(BlockPos::new(pos.x & 0xF, pos.y, pos.z & 0xF));
    }

    fn get_block_entity(&self, pos: BlockPos) -> Option<&BlockEntity> {
        let chunk_index = match self.get_chunk_index_for_block(pos.x, pos.z) {
            Some(idx) => idx,
            None => return None,
        };
        let chunk = &self.chunks[chunk_index];
        chunk.get_block_entity(BlockPos::new(pos.x & 0xF, pos.y, pos.z & 0xF))
    }

    fn get_block_entity_mut(&mut self, pos: BlockPos) -> Option<&mut BlockEntity> {
        let index = self.get_chunk_index_for_block(pos.x, pos.z)?;
        self.chunks[index]
            .block_entities
            .get_mut(&BlockPos::new(pos.x & 15, pos.y, pos.z & 15))
    }

    fn piston_state(&self) -> &PistonState {
        &self.piston_state
    }
    fn piston_state_mut(&mut self) -> &mut PistonState {
        &mut self.piston_state
    }

    fn set_block_entity(&mut self, pos: BlockPos, block_entity: BlockEntity) {
        let chunk_index = match self.get_chunk_index_for_block(pos.x, pos.z) {
            Some(idx) => idx,
            None => return,
        };
        self.piston_state.motions.retain(|m| m.pos != pos);
        if let BlockEntity::MovingPiston(e) = &block_entity {
            self.register_motion(pos, e.get_progress());
        }
        if let Some(nbt) = block_entity.to_nbt(true) {
            let block_entity_data = CBlockEntityData {
                pos: pos.packed(),
                // For now the only nbt we send to the client is sign data
                ty: block_entity.ty(),
                nbt,
            }
            .encode();
            for player in &self.packet_senders {
                player.send_packet(&block_entity_data);
            }
        }
        let chunk = &mut self.chunks[chunk_index];
        chunk.set_block_entity(BlockPos::new(pos.x & 0xF, pos.y, pos.z & 0xF), block_entity);
    }

    fn get_chunk(&self, x: i32, z: i32) -> Option<&Chunk> {
        if !(self.x * PLOT_WIDTH..(self.x + 1) * PLOT_WIDTH).contains(&x)
            || !(self.z * PLOT_WIDTH..(self.z + 1) * PLOT_WIDTH).contains(&z)
        {
            return None;
        }
        self.chunks.get(self.get_chunk_index_for_chunk(x, z))
    }

    fn get_chunk_mut(&mut self, x: i32, z: i32) -> Option<&mut Chunk> {
        self.get_chunk(x, z)?;
        let chunk_idx = self.get_chunk_index_for_chunk(x, z);
        self.chunks.get_mut(chunk_idx)
    }

    fn schedule_tick(&mut self, pos: BlockPos, delay: u32, priority: TickPriority) {
        self.schedule_half_tick(pos, delay * 2, priority);
    }

    fn schedule_half_tick(&mut self, pos: BlockPos, delay: u32, priority: TickPriority) {
        let node = ScheduledBlockTick {
            pos,
            block_type: Some(self.get_block(pos).registry_id()),
        };
        if !self.to_be_ticked.contains(&node) {
            self.to_be_ticked
                .schedule_half_tick(node, delay as usize, priority);
        }
    }

    fn pending_tick_at(&mut self, pos: BlockPos) -> bool {
        self.to_be_ticked.contains(&ScheduledBlockTick {
            pos,
            block_type: Some(self.get_block(pos).registry_id()),
        })
    }

    fn block_action(&mut self, pos: BlockPos, block_action: BlockAction) {
        match block_action {
            BlockAction::Piston { action, piston } => {
                if self.fast_rendering {
                    return;
                }
                let piston_action_data = CBlockAction {
                    pos: pos.packed(),
                    action_id: action as u8,
                    action_param: BlockFace::from(piston.facing) as u8,
                    block_id: Block::Piston { piston }.registry_id(),
                }
                .encode();
                for player in &self.packet_senders {
                    player.send_packet(&piston_action_data);
                }
            }
            BlockAction::BlockChange { pos, block_id } => {
                let block_action_data = CBlockChange {
                    pos: pos.packed(),
                    block_id: block_id as i32,
                }
                .encode();
                for player in &self.packet_senders {
                    player.send_packet(&block_action_data);
                }
            }
        }
    }

    fn play_sound(
        &mut self,
        pos: BlockPos,
        sound_id: i32,
        sound_category: i32,
        volume: f32,
        pitch: f32,
    ) {
        if self.fast_rendering {
            return;
        }
        // FIXME: We do not know the players location here, so we send the sound packet to all players
        // A notchian server would only send to players in hearing distance (volume.clamp(0.0, 1.0) * 16.0)
        let sound_effect_data = CSoundEffect {
            sound_id,
            sound_category,
            x: pos.x * 8 + 4,
            y: pos.y * 8 + 4,
            z: pos.z * 8 + 4,
            volume,
            pitch,
        }
        .encode();

        for player in &self.packet_senders {
            player.send_packet(&sound_effect_data);
        }
    }
}

impl Plot {
    fn effective_send_rate(&self) -> u32 {
        visuals::send_rate(
            self.world_send_rate.0,
            self.world.fast_rendering,
            CONFIG.fast_render_send_rate,
        )
    }

    fn update_render_mode(&mut self) {
        let fast = visuals::fast_rendering(self.tps, CONFIG.fast_render_threshold);
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
            .filter_map(|motion| {
                self.world
                    .get_chunk_index_for_block(motion.pos.x, motion.pos.z)
            })
            .collect();
        for index in chunks {
            let chunk = &self.world.chunks[index];
            let encoded = chunk.encode_packet_for_client(fast);
            for player in &self.players {
                if Self::get_chunk_distance(
                    chunk.x,
                    chunk.z,
                    player.last_chunk_x,
                    player.last_chunk_z,
                ) <= CONFIG.view_distance.max(0) as u32
                {
                    player.client.send_packet(&encoded);
                }
            }
        }
    }
    fn tick(&mut self) {
        self.timings.tick();
        if self.redpiler.is_active() {
            self.redpiler.tick();
        } else {
            self.world.tick_interpreted();
        }
    }

    /// Send a block change to all connected players
    pub fn send_block_change(&mut self, pos: BlockPos, id: u32) {
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
        let broadcast_message = Message::ChatInfo(
            0,
            format!("Plot {}-{}", self.world.x, self.world.z),
            message,
        );
        self.message_sender.send(broadcast_message).unwrap();
    }

    pub fn broadcast_plot_chat_message(&mut self, message: &str) {
        for player in &mut self.players {
            player.send_chat_message(0, &ChatComponent::from_legacy_text(message));
        }
    }

    fn change_player_gamemode(&mut self, player_idx: usize, gamemode: Gamemode) {
        self.players[player_idx].set_gamemode(gamemode);
        let _ = self.message_sender.send(Message::PlayerUpdateGamemode(
            self.players[player_idx].uuid,
            gamemode,
        ));
    }

    fn on_player_move(&mut self, player_idx: usize, old: PlayerPos, new: PlayerPos) {
        let old_block = old.block_pos();
        let new_block = new.block_pos();

        if self.world.get_block(old_block).pressure_plate_powered() == Some(true) {
            if !self.are_players_on_block(old_block) {
                self.set_pressure_plate(old_block, false);
            }
        }

        if self.world.get_block(new_block).pressure_plate_powered() == Some(false) {
            if self.players[player_idx].on_ground {
                self.set_pressure_plate(new_block, true);
            }
        }
    }

    fn set_pressure_plate(&mut self, pos: BlockPos, powered: bool) {
        if self.redpiler.is_active() {
            self.redpiler.set_pressure_plate(pos, powered);
            return;
        }

        let block = self.world.get_block(pos);
        match block {
            block if block.pressure_plate_powered().is_some() => {
                self.world
                    .set_block(pos, block.with_pressure_plate_power(powered).unwrap());
                redstone::update_surrounding_blocks(&mut self.world, pos);
                redstone::update_surrounding_blocks(&mut self.world, pos.offset(BlockFace::Bottom));
            }
            _ => warn!("Block at {} is not a pressure plate", pos),
        }
    }

    fn are_players_on_block(&mut self, pos: BlockPos) -> bool {
        for player in &self.players {
            if player.pos.block_pos() == pos && player.on_ground {
                return true;
            }
        }
        false
    }

    fn enter_plot(&mut self, player: Player) {
        self.save();
        let spawn_player = CSpawnPlayer {
            entity_id: player.entity_id as i32,
            uuid: player.uuid,
            pitch: player.pitch,
            yaw: player.yaw,
            x: player.pos.x,
            y: player.pos.y,
            z: player.pos.z,
        }
        .encode();
        let metadata_entries = vec![CEntityMetadataEntry {
            index: 17,
            metadata_type: 0,
            value: vec![player.skin_parts.bits() as u8],
        }];
        let metadata = CEntityMetadata {
            entity_id: player.entity_id as i32,
            metadata: metadata_entries,
        }
        .encode();
        let status = CEntityStatus {
            entity_id: player.entity_id as i32,
            entity_status: 26, // op level 2 to use F3+N, this is `24 + op_level` (there are 4 possible levels)
        }
        .encode();
        player.client.send_packet(&status);
        for other_player in &mut self.players {
            other_player.client.send_packet(&spawn_player);
            other_player.client.send_packet(&metadata);

            let spawn_other_player = CSpawnPlayer {
                entity_id: other_player.entity_id as i32,
                uuid: other_player.uuid,
                pitch: other_player.pitch,
                yaw: other_player.yaw,
                x: other_player.pos.x,
                y: other_player.pos.y,
                z: other_player.pos.z,
            }
            .encode();
            player.client.send_packet(&spawn_other_player);

            if let Some(item) = &other_player.inventory[other_player.selected_slot as usize + 36] {
                let other_entity_equipment = CEntityEquipment {
                    entity_id: other_player.entity_id as i32,
                    equipment: vec![CEntityEquipmentEquipment {
                        slot: 0, // Main hand
                        item: Some(SlotData {
                            item_count: item.count as i8,
                            item_id: item.item_type.get_id() as i32,
                            nbt: item.nbt.clone(),
                        }),
                    }],
                }
                .encode();
                player.client.send_packet(&other_entity_equipment);
            }

            let other_metadata_entries = vec![CEntityMetadataEntry {
                index: 17,
                metadata_type: 0,
                value: vec![other_player.skin_parts.bits() as u8],
            }];
            let other_metadata = CEntityMetadata {
                entity_id: other_player.entity_id as i32,
                metadata: other_metadata_entries,
            }
            .encode();
            player.client.send_packet(&other_metadata);
        }

        if let Some(item) = &player.inventory[player.selected_slot as usize + 36] {
            let entity_equipment = CEntityEquipment {
                entity_id: player.entity_id as i32,
                equipment: vec![CEntityEquipmentEquipment {
                    slot: 0, // Main hand
                    item: Some(SlotData {
                        item_count: item.count as i8,
                        item_id: item.item_type.get_id() as i32,
                        nbt: item.nbt.clone(),
                    }),
                }],
            }
            .encode();
            for other_player in &mut self.players {
                other_player.client.send_packet(&entity_equipment);
            }
        }

        player.send_system_message(&format!(
            "Entering plot ({}, {})",
            self.world.x, self.world.z
        ));
        self.world
            .packet_senders
            .push(PlayerPacketSender::new(&player.client));
        self.scoreboard.add_player(&player);
        self.players.push(player);
        self.update_view_pos_for_player(self.players.len() - 1, true);
    }

    fn get_chunk_distance(x1: i32, z1: i32, x2: i32, z2: i32) -> u32 {
        let x = x1 - x2;
        let z = z1 - z2;
        x.abs().max(z.abs()) as u32
    }

    fn set_chunk_loaded_at_player(
        &mut self,
        player_idx: usize,
        chunk_x: i32,
        chunk_z: i32,
        was_loaded: bool,
        should_be_loaded: bool,
    ) {
        if was_loaded && !should_be_loaded {
            let unload_chunk = CUnloadChunk { chunk_x, chunk_z }.encode();
            self.players[player_idx].client.send_packet(&unload_chunk);
        } else if !was_loaded && should_be_loaded {
            if !Plot::chunk_in_plot_bounds(self.world.x, self.world.z, chunk_x, chunk_z) {
                self.players[player_idx]
                    .client
                    .send_packet(&Chunk::encode_empty_packet(chunk_x, chunk_z));
            } else {
                let chunk_data = self.world.chunks
                    [self.world.get_chunk_index_for_chunk(chunk_x, chunk_z)]
                .encode_packet_for_client(self.world.fast_rendering);
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

        if ((last_chunk_x - chunk_x).abs() <= view_distance * 2
            && (last_chunk_z - chunk_z).abs() <= view_distance * 2)
            && !force_load
        {
            let nx = chunk_x.min(last_chunk_x) - view_distance;
            let nz = chunk_z.min(last_chunk_z) - view_distance;
            let px = chunk_x.max(last_chunk_x) + view_distance;
            let pz = chunk_z.max(last_chunk_z) + view_distance;
            for x in nx..=px {
                for z in nz..=pz {
                    let was_loaded = Self::get_chunk_distance(x, z, last_chunk_x, last_chunk_z)
                        <= view_distance as u32;
                    let should_be_loaded =
                        Self::get_chunk_distance(x, z, chunk_x, chunk_z) <= view_distance as u32;
                    // todo make this little bit async, so that chunks dont load all at the same time
                    // causing lag spikes in client when there is a massive redstone contraption on plot
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

    fn start_redpiler(&mut self, options: CompilerOptions) {
        if self.world.chunks.iter().any(Chunk::requires_interpreter)
            || !self.world.piston_state.events.is_empty()
            || !self.world.piston_state.motions.is_empty()
        {
            for player in &self.players {
                player.send_system_message("This plot contains pistons or observers and runs with the interpreter to preserve their behavior.");
            }
            return;
        }
        debug!("Starting redpiler");
        self.scoreboard
            .set_redpiler_state(&self.players, RedpilerState::Compiling);
        self.scoreboard
            .set_redpiler_options(&self.players, &options);

        let bounds = self.world.get_corners();
        // TODO: use monitor
        let monitor = Default::default();
        let ticks = self.world.to_be_ticked.iter_entries().collect();
        self.world.to_be_ticked.clear();

        let mut players_need_updates = HashSet::new();
        thread::scope(|s| {
            let handle = s.spawn(|| {
                self.redpiler
                    .compile(&mut self.world, bounds, options, ticks, monitor)
            });
            while !handle.is_finished() {
                // We'll update the players so that they don't time out.
                for player_idx in 0..self.players.len() {
                    if self.players[player_idx].update() {
                        // Unforunately we can't update a players view position
                        // since we don't have access to the world, but we can
                        // save the players that need updating for later.
                        players_need_updates.insert(player_idx);
                    }
                }
                thread::sleep(Duration::from_millis(20));
            }
        });

        // Now that we have ownership of the world again, we can update player view positions
        for player_idx in players_need_updates {
            self.update_view_pos_for_player(player_idx, false);
        }

        self.scoreboard
            .set_redpiler_state(&self.players, RedpilerState::Running);

        self.reset_timings();
    }

    /// Redpiler needs to reset implicitly in the case of any block changes done by a player. This can be
    fn reset_redpiler(&mut self) {
        if self.redpiler.is_active() {
            debug!("Discarding redpiler");
            let bounds = self.world.get_corners();
            self.redpiler.reset(&mut self.world, bounds);
            self.scoreboard
                .set_redpiler_state(&self.players, RedpilerState::Stopped);
            self.scoreboard
                .set_redpiler_options(&self.players, &Default::default());

            // reseting redpiler could cause a large amount of block updates
            self.reset_timings();
        }
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
        self.world.packet_senders.remove(player_idx);
        let player = self.players.remove(player_idx);

        let destroy_other_entities = CDestroyEntities {
            entity_ids: self.players.iter().map(|p| p.entity_id as i32).collect(),
        }
        .encode();
        player.client.send_packet(&destroy_other_entities);

        let chunk_offset_x = self.world.x << PLOT_SCALE;
        let chunk_offset_z = self.world.z << PLOT_SCALE;
        for chunk in &self.world.chunks {
            player.client.send_packet(
                &CUnloadChunk {
                    chunk_x: chunk_offset_x + chunk.x,
                    chunk_z: chunk_offset_z + chunk.z,
                }
                .encode(),
            );
        }
        self.destroy_entity(player.entity_id);
        self.locked_players.remove(&player.entity_id);
        self.scoreboard.remove_player(&player);
        player
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
        database::claim_plot(plot_x, plot_z, &format!("{:032x}", player.uuid));
        let center = Plot::get_center(plot_x, plot_z);
        player.teleport(PlayerPos::new(center.0, 64.0, center.1));
        player.send_system_message(&format!("Claimed plot {},{}", plot_x, plot_z));
    }

    pub fn get_center(plot_x: i32, plot_z: i32) -> (f64, f64) {
        const WIDTH: f64 = PLOT_BLOCK_WIDTH as f64;
        (
            plot_x as f64 * WIDTH + WIDTH / 2.0,
            plot_z as f64 * WIDTH + WIDTH / 2.0,
        )
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
            let commands: Vec<String> = self.players[player_idx].command_queue.drain(..).collect();
            for command in commands {
                let mut args: Vec<&str> = command.split(' ').collect();
                let command = args.remove(0);
                if self.handle_command(player_idx, command, args) {
                    removal_offset += 1;
                }
            }
        }
    }

    fn handle_messages(&mut self) {
        while let Ok(message) = self.message_receiver.try_recv() {
            match message {
                BroadcastMessage::CommandChat(command) => {
                    for player in &self.players {
                        if command.recipient.matches(&player.username) {
                            player.send_raw_system_message(command.message.clone());
                        }
                    }
                }
                BroadcastMessage::Chat(sender, message) => {
                    for player in &mut self.players {
                        player.send_chat_message(sender, &message);
                    }
                }
                BroadcastMessage::PlayerJoinedInfo(player_join_info) => {
                    let player_info = CPlayerInfo::AddPlayer(vec![CPlayerInfoAddPlayer {
                        name: player_join_info.username,
                        properties: Vec::new(),
                        gamemode: 1,
                        ping: 0,
                        uuid: player_join_info.uuid,
                        display_name: None,
                    }])
                    .encode();
                    for player in &mut self.players {
                        player.client.send_packet(&player_info);
                    }
                }
                BroadcastMessage::PlayerLeft(uuid) => {
                    let player_info = CPlayerInfo::RemovePlayer(vec![uuid]).encode();
                    for player in &mut self.players {
                        player.client.send_packet(&player_info);
                    }
                }
                BroadcastMessage::Shutdown => {
                    let mut players: Vec<Player> = self.players.drain(..).collect();
                    for player in players.iter_mut() {
                        player.save();
                        player.kick(
                            json!({
                                "text": "Server closed"
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
                PrivMessage::PlayerEnterPlot(player) => {
                    self.enter_plot(player);
                }
                PrivMessage::PlayerTeleportOther(mut player, username) => {
                    if let Some(other) = self.players.iter().find(|p| p.username == username) {
                        player.teleport(other.pos);
                    }
                    self.enter_plot(player);
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
                outside_players.push(player.uuid);
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
        let message_sender = &mut self.message_sender;

        let mut disconnected_players = Vec::new();
        self.players.retain(|player| {
            let alive = player.client.alive();
            if !alive {
                player.save();
                message_sender
                    .send(Message::PlayerLeft(player.uuid))
                    .unwrap();
                disconnected_players.push(player.entity_id);
            }
            alive
        });
        for entity_id in disconnected_players {
            self.destroy_entity(entity_id);
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
    }

    fn update(&mut self) {
        self.handle_messages();
        self.update_render_mode();

        // Only tick if there are players in the plot
        if !self.players.is_empty() {
            self.timings.set_ticking(true);
            let now = Instant::now();
            self.last_player_time = now;

            let effective_send_rate = self.effective_send_rate();
            let world_send_rate =
                Duration::from_nanos(1_000_000_000 / effective_send_rate.max(1) as u64);

            let max_batch_size = match self.last_nspt {
                Some(Duration::ZERO) | None => 1,
                Some(last_nspt) => {
                    let ticks_fit = (world_send_rate.as_nanos() / last_nspt.as_nanos()) as u64;
                    // A tick previously took longer than the world send rate.
                    // Run at least one just so we're not stuck doing nothing
                    ticks_fit.max(1)
                }
            };

            let batch_size = match self.tps {
                Tps::Limited(tps) if tps != 0 => {
                    let dur_per_tick = Duration::from_nanos((1_000_000_000 / tps as u64).max(1));
                    self.lag_time += now - self.last_update_time;
                    let batch_size = (self.lag_time.as_nanos() / dur_per_tick.as_nanos()) as u64;
                    self.lag_time -= dur_per_tick * batch_size as u32;
                    batch_size.min(max_batch_size)
                }
                Tps::Unlimited => max_batch_size,
                _ => 0,
            };

            self.last_update_time = now;
            if batch_size != 0 {
                // 50_000 (= 3.33 MHz) here is arbitrary.
                // We just need a number that's not too high so we actually get around to sending block updates.
                let batch_size = batch_size.min(50_000) as u32;
                let mut ticks_completed = batch_size;
                if self.redpiler.is_active() {
                    for _ in 0..batch_size {
                        self.tick();
                    }
                    self.redpiler.flush(&mut self.world);
                } else {
                    for i in 0..batch_size {
                        self.tick();
                        if now.elapsed() > Duration::from_millis(200) {
                            ticks_completed = i + 1;
                            break;
                        }
                    }
                }
                self.last_nspt = Some(self.last_update_time.elapsed() / ticks_completed);
            }

            if self.auto_redpiler
                && !self.world.chunks.iter().any(Chunk::requires_interpreter)
                && !self.redpiler.is_active()
                && (self.tps == Tps::Unlimited || self.timings.is_running_behind())
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

        // Handle commands before removing players just in case they ran a command before leaving
        self.handle_commands();

        self.remove_dc_players();
        self.remove_oob_players();
    }

    fn create_async_rt() -> Runtime {
        Runtime::new().unwrap()
    }

    fn generate_chunk(layers: i32, x: i32, z: i32) -> Chunk {
        let border: u32 = Block::StoneBrick {}.get_id();
        let fill: u32 = Block::Sandstone {}.get_id();

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
        let chunks: Vec<Chunk> = plot_data
            .chunk_data
            .into_iter()
            .enumerate()
            .map(|(i, c)| {
                Chunk::load(
                    chunk_x_offset + i as i32 / PLOT_WIDTH,
                    chunk_z_offset + i as i32 % PLOT_WIDTH,
                    c,
                )
            })
            .collect();
        if chunks.len() != NUM_CHUNKS {
            error!("This plot has the wrong number of chunks!");
            let possible_scale = (chunks.len() as f64).sqrt().log2();
            error!("Note: it most likely came from a server running plot scale {}, this server is running a plot scale of {}", possible_scale, PLOT_SCALE);
        }

        let mut world =
            PlotWorld::from_chunks(x, z, chunks, plot_data.pending_ticks.into_iter().collect());
        if plot_data.piston_state.logical_tick != 0
            || plot_data.piston_state.next_identity != 0
            || !plot_data.piston_state.events.is_empty()
            || !plot_data.piston_state.motions.is_empty()
        {
            world.piston_state = plot_data.piston_state;
        }
        let tps = plot_data.tps;
        world.fast_rendering = visuals::fast_rendering(tps, CONFIG.fast_render_threshold);
        let world_send_rate = plot_data.world_send_rate;
        Plot {
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
            always_running,
            redpiler: Default::default(),
            timings: TimingsMonitor::new(tps),
            owner: database::get_plot_owner(x, z).map(|s| s.parse::<HyphenatedUUID>().unwrap().0),
            async_rt: Plot::create_async_rt(),
            scoreboard: Default::default(),
            world,
        }
    }

    fn save(&mut self) {
        let world = &mut self.world;
        let chunk_data: Vec<ChunkData<PLOT_SECTIONS>> =
            world.chunks.iter_mut().map(|c| c.save()).collect();
        let data = PlotData {
            tps: self.tps,
            world_send_rate: self.world_send_rate,
            chunk_data,
            pending_ticks: world.to_be_ticked.iter_entries().collect(),
            piston_state: world.piston_state.clone(),
        };
        data.save_to_file(format!("./world/plots/p{},{}", world.x, world.z))
            .unwrap();

        self.reset_timings();
    }

    fn run(&mut self, initial_player: Option<Player>) {
        let _guard = self.async_rt.enter();

        if let Some(player) = initial_player {
            self.enter_plot(player);
        }

        while self.running {
            // Fast path, for super high RTPS
            if self.sleep_time <= Duration::from_millis(5) && !self.players.is_empty() {
                self.update();
                if self.tps != Tps::Unlimited {
                    thread::yield_now();
                }
                continue;
            }

            let before = Instant::now();
            self.update();
            let delta = before.elapsed();

            if delta < self.sleep_time {
                let sleep_time = self.sleep_time - delta;
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
                let loaded = data::load_plot(format!("./world/plots/p{},{}", x, z))
                    .with_context(|| format!("error loading plot {},{}", x, z));
                let data = match loaded {
                    Ok(data) => data,
                    Err(error) => {
                        let _ = tx.send(Message::PlotLoadFailed(
                            x,
                            z,
                            format!("{:#}", error),
                            initial_player,
                            priv_rx,
                        ));
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
                player.send_error_message("The plot you were previously in has crashed!");
            }

            while !self.players.is_empty() {
                let uuid = self.players[0].uuid;
                let player = self.leave_plot(uuid);
                self.message_sender
                    .send(Message::PlayerLeavePlot(player))
                    .unwrap();
            }
        }

        self.reset_redpiler();
        self.world
            .chunks
            .iter_mut()
            .for_each(|chunk| chunk.compress());
        self.save();
        let world = &self.world;
        self.message_sender
            .send(Message::PlotUnload(world.x, world.z))
            .unwrap();
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
