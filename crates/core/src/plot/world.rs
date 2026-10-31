use super::{PLOT_BLOCK_HEIGHT, PLOT_BLOCK_WIDTH, PLOT_SCALE, PLOT_WIDTH, block_light, history, in_bounds, screen_updates};
use crate::messages;
use crate::redstone;
use crate::world::scheduler::{ScheduledBlockTick, TickScheduler};
use crate::world::storage::Chunk;
use crate::world::{BlockAction, RedstoneUpdateObservation, World};
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::Block;
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_network::PlayerPacketSender;
use mchprs_network::packets::clientbound::*;
use mchprs_world::{PistonMotion, PistonState, TickEntry, TickPriority};
use std::collections::HashSet;
use std::time::{Duration, Instant};

mod interpreter;

pub struct PlotWorld {
    pub(super) x: i32,
    pub(super) z: i32,
    pub(super) chunks: Vec<Chunk>,
    pub(super) to_be_ticked: TickScheduler<ScheduledBlockTick>,
    tick_index: interpreter::TickIndex,
    pub(super) piston_state: PistonState,
    piston_index: std::cell::RefCell<interpreter::PistonIndex>,
    pub(super) wire_topology: std::cell::RefCell<crate::world::wire_cache::Topology>,
    pub(super) instant_pistons: crate::world::InstantPistonCache,
    pub(super) screen_updates: Option<screen_updates::ScreenUpdates>,
    pub(super) packet_senders: Vec<PlayerPacketSender>,
    pub(super) is_cursed: bool,
    pub(super) fast_rendering: bool,
    pub(super) command_messages: Vec<crate::chat_commands::ChatCommand>,
    pub(super) sounds: Vec<crate::sound::Emission>,
    pub(super) level_events: Vec<(u128, CEffect)>,
    pub(super) open_chests: HashSet<BlockPos>,
    pub(super) command_output_window: Instant,
    pub(super) command_output_count: usize,
    pub(super) command_output_bytes: usize,
    pub(super) command_output_limits_enabled: bool,
    pub(super) history: history::TickHistory,
    pub(super) waveform: crate::waveform::Recorder,
    pub(super) update_stats: UpdateStats,
    pub(super) bulb_light: block_light::BulbLight,
    pub(super) redstone_update_observations: Option<Vec<RedstoneUpdateObservation>>,
    pub(super) scratch_capture_bounds: Option<(BlockPos, BlockPos)>,
    pub(super) scratch_read_bounds: Option<(BlockPos, BlockPos)>,
    pub(super) scratch_read_chunks: Option<HashSet<(i32, i32)>>,
    pub(super) scratch_contract_air: Option<HashSet<BlockPos>>,
    pub(super) scratch_boundary_read: std::cell::Cell<Option<BlockPos>>,
    pub(super) scratch_boundary_effect: std::cell::Cell<Option<BlockPos>>,
    pub(super) scratch_touched_blocks: Option<HashSet<BlockPos>>,
    pub(super) scratch_touched_entities: Option<HashSet<BlockPos>>,
    pub(super) scratch_suppress_commands: bool,
}

#[derive(Default)]
pub(super) struct UpdateStats {
    pub(super) collection: Duration,
    pub(super) enqueue: Duration,
    pub(super) flushes: u64,
    pub(super) sections: u64,
    pub(super) records: u64,
    pub(super) simulation: Duration,
    pub(super) projection: Duration,
    pub(super) plot: Duration,
    pub(super) simulated_ticks: u64,
}

impl PlotWorld {
    #[inline]
    pub fn from_chunks(x: i32, z: i32, chunks: Vec<Chunk>, to_be_ticked: TickScheduler<ScheduledBlockTick>) -> Self {
        let mut world = Self {
            x,
            z,
            chunks,
            to_be_ticked,
            tick_index: Default::default(),
            piston_state: PistonState::default(),
            piston_index: Default::default(),
            wire_topology: Default::default(),
            instant_pistons: Default::default(),
            screen_updates: None,
            packet_senders: Vec::new(),
            is_cursed: false,
            fast_rendering: false,
            command_messages: Vec::new(),
            sounds: Vec::new(),
            level_events: Vec::new(),
            open_chests: HashSet::new(),
            command_output_window: Instant::now(),
            command_output_count: 0,
            command_output_bytes: 0,
            command_output_limits_enabled: true,
            history: Default::default(),
            waveform: Default::default(),
            update_stats: Default::default(),
            bulb_light: Default::default(),
            redstone_update_observations: None,
            scratch_capture_bounds: None,
            scratch_read_bounds: None,
            scratch_read_chunks: None,
            scratch_contract_air: None,
            scratch_boundary_read: std::cell::Cell::new(None),
            scratch_boundary_effect: std::cell::Cell::new(None),
            scratch_touched_blocks: None,
            scratch_touched_entities: None,
            scratch_suppress_commands: false,
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
        let repeating: Vec<_> = world
            .chunks
            .iter()
            .flat_map(|chunk| {
                chunk.block_entities.iter().filter_map(move |(local, entity)| {
                    let pos = BlockPos::new(chunk.x * 16 + local.x, local.y, chunk.z * 16 + local.z);
                    matches!(entity, BlockEntity::CommandBlock(_)).then_some(pos)
                })
            })
            .filter(|&pos| world.get_block(pos).get_name() == "repeating_command_block")
            .collect();
        for pos in repeating {
            redstone::command_block::update(&mut world, pos);
        }
        world.rebuild_bulb_light();
        world
    }

    pub(super) fn get_chunk_index_for_chunk(&self, chunk_x: i32, chunk_z: i32) -> usize {
        let local_x = chunk_x - self.x * PLOT_WIDTH;
        let local_z = chunk_z - self.z * PLOT_WIDTH;
        (local_x * PLOT_WIDTH + local_z).unsigned_abs() as usize
    }

    pub(super) fn get_chunk_index_for_block(&self, block_x: i32, block_z: i32) -> Option<usize> {
        let chunk_x = (block_x - (self.x * PLOT_BLOCK_WIDTH)) >> 4;
        let chunk_z = (block_z - (self.z * PLOT_BLOCK_WIDTH)) >> 4;
        if !(0..PLOT_WIDTH).contains(&chunk_x) || !(0..PLOT_WIDTH).contains(&chunk_z) {
            return None;
        }
        Some(((chunk_x << PLOT_SCALE) + chunk_z).unsigned_abs() as usize)
    }

    pub fn flush_block_changes(&mut self) {
        self.flush_bulb_light();
        if self.screen_only() {
            self.flush_screen_changes();
            return;
        }
        let started = Instant::now();
        let mut enqueue_time = Duration::ZERO;
        self.update_stats.flushes += 1;
        let fast = self.fast_rendering;
        let moving_states: std::collections::HashMap<_, _> = if fast {
            self.piston_state
                .motions
                .iter()
                .filter_map(|motion| match self.get_block_entity(motion.pos) {
                    Some(BlockEntity::MovingPiston(entity)) => Some((motion.pos, entity.block_state)),
                    _ => None,
                })
                .collect()
        } else {
            Default::default()
        };
        for packet in self.chunks.iter_mut().flat_map(|c| c.multi_blocks()) {
            let transformed = if fast {
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
                Some(CMultiBlockChange {
                    chunk_x: packet.chunk_x,
                    chunk_y: packet.chunk_y,
                    chunk_z: packet.chunk_z,
                    records,
                })
            } else {
                None
            };
            let packet = transformed.as_ref().unwrap_or(packet);
            self.update_stats.sections += 1;
            self.update_stats.records += packet.records.len() as u64;
            let enqueue_started = Instant::now();
            for player in &self.packet_senders {
                player.send_block_changes(packet);
            }
            enqueue_time += enqueue_started.elapsed();
        }
        for chunk in &mut self.chunks {
            chunk.reset_multi_blocks();
        }
        self.update_stats.enqueue += enqueue_time;
        self.update_stats.collection += started.elapsed().saturating_sub(enqueue_time);
    }

    pub fn get_corners(&self) -> (BlockPos, BlockPos) {
        const W: i32 = PLOT_BLOCK_WIDTH;
        let first_pos = BlockPos::new(self.x * W, 0, self.z * W);
        let second_pos = BlockPos::new((self.x + 1) * W - 1, PLOT_BLOCK_HEIGHT - 1, (self.z + 1) * W - 1);
        (first_pos, second_pos)
    }

    pub(crate) fn activation_snapshot(&self) -> Self {
        let mut snapshot = Self::from_chunks(
            self.x,
            self.z,
            self.chunks.iter().map(Chunk::staging_clone).collect(),
            self.to_be_ticked.clone(),
        );
        snapshot.piston_state = self.piston_state.clone();
        snapshot.is_cursed = self.is_cursed;
        snapshot.fast_rendering = self.fast_rendering;
        snapshot.screen_updates = self.screen_updates.clone();
        snapshot
    }

    pub(super) fn publish_activation_changes(&mut self, staged: &Self) {
        let changes: Vec<_> = staged.chunks.iter().flat_map(Chunk::changed_blocks).collect();
        for (pos, block) in changes {
            self.set_block(pos, Block::from_id(block));
        }
    }

    pub fn get_chunks(&self) -> &[Chunk] {
        &self.chunks
    }

    /// Commit and drain pending writes for a headless world, without sending packets.
    pub fn take_block_changes(&mut self) -> Vec<(BlockPos, u32)> {
        self.chunks.iter_mut().flat_map(Chunk::take_changed_blocks).collect()
    }

    pub fn scheduler(&self) -> &TickScheduler<ScheduledBlockTick> {
        &self.to_be_ticked
    }

    pub(crate) fn observe_redstone_updates(&mut self, enabled: bool) {
        self.redstone_update_observations = enabled.then(Vec::new);
    }

    pub(crate) fn take_redstone_update_observations(&mut self) -> Vec<RedstoneUpdateObservation> {
        self.redstone_update_observations.take().unwrap_or_default()
    }

    pub(crate) fn configure_scratch(
        &mut self,
        bounds: (BlockPos, BlockPos),
        read_bounds: (BlockPos, BlockPos),
        read_chunks: HashSet<(i32, i32)>,
        contract_air: HashSet<BlockPos>,
    ) {
        self.scratch_capture_bounds = Some(bounds);
        self.scratch_read_bounds = Some(read_bounds);
        self.scratch_read_chunks = Some(read_chunks);
        self.scratch_contract_air = Some(contract_air);
        self.scratch_boundary_read.set(None);
        self.scratch_boundary_effect.set(None);
        self.scratch_touched_blocks = Some(HashSet::new());
        self.scratch_touched_entities = Some(HashSet::new());
        self.scratch_suppress_commands = true;
    }

    pub(crate) fn take_scratch_changes(&mut self) -> (HashSet<BlockPos>, HashSet<BlockPos>) {
        (
            self.scratch_touched_blocks.take().unwrap_or_default(),
            self.scratch_touched_entities.take().unwrap_or_default(),
        )
    }

    pub(crate) fn reset_scratch_probe_state(
        &mut self,
        piston: &mchprs_world::PistonState,
        bounds: (BlockPos, BlockPos),
        read_bounds: (BlockPos, BlockPos),
        read_chunks: HashSet<(i32, i32)>,
        contract_air: HashSet<BlockPos>,
    ) {
        self.clear_scheduled_ticks();
        self.piston_state_mut().clone_from(piston);
        self.observe_redstone_updates(false);
        #[cfg(test)]
        self.piston_index.get_mut().invalidate();
        self.wire_topology.get_mut().clear();
        self.instant_pistons.clear();
        self.reset_waveform();
        self.configure_scratch(bounds, read_bounds, read_chunks, contract_air);
    }

    pub(crate) fn take_scratch_boundary_read(&self) -> Option<BlockPos> {
        self.scratch_boundary_read.take()
    }

    pub(crate) fn take_scratch_boundary_effect(&self) -> Option<BlockPos> {
        self.scratch_boundary_effect.take()
    }

    pub(crate) fn clear_scheduled_ticks(&mut self) {
        self.to_be_ticked.clear();
        self.tick_index.invalidate();
    }

    /// Queued command-block chat, in emission order.
    pub fn command_output(&self) -> impl Iterator<Item = &str> {
        self.command_messages.iter().map(|message| message.message.as_str())
    }

    /// Disable wall-clock and queue output limits for deterministic offline replay.
    /// Live plots retain their default limits; this is never called by the server.
    pub fn disable_command_output_limits_for_replay(&mut self) {
        self.command_output_limits_enabled = false;
    }

    pub(super) fn register_motion(&mut self, pos: BlockPos, progress: f32) {
        self.remove_motions_at(pos);
        let state = &mut self.piston_state;
        state.next_identity += 1;
        state.motions.push_back(PistonMotion {
            pos,
            identity: state.next_identity,
            progress,
            previous_progress: progress,
            last_tick: state.logical_tick,
            carried_entity: None,
        });
        self.piston_index.get_mut().inserted(pos, state.motions.len() - 1);
    }

    pub(super) fn remove_motions_at(&mut self, pos: BlockPos) {
        if let Some(index) = self.piston_motion_index(pos, None) {
            if self.piston_state.motions.len() <= 8 || self.piston_index.get_mut().has_duplicate_motions(pos) {
                // Legacy saves can contain repeated positions; remove all of
                // them as before. Small banks are cheap to retain in order.
                self.piston_state.motions.retain(|m| m.pos != pos);
            } else {
                self.piston_state.motions.remove(index);
            }
            self.piston_index.get_mut().removed(pos);
        }
    }

    pub(super) fn invalidate_interpreter_caches(&mut self) {
        self.reset_waveform();
        self.rebuild_bulb_light();
        self.tick_index.invalidate();
        self.piston_index.get_mut().invalidate();
        self.wire_topology.get_mut().clear();
        self.instant_pistons.clear();
    }

    pub(super) fn clear_interpreter_caches(&mut self) {
        self.reset_waveform();
        self.rebuild_bulb_light();
        self.tick_index = Default::default();
        *self.piston_index.get_mut() = Default::default();
        self.wire_topology.get_mut().clear();
        self.instant_pistons.clear();
        redstone::wire::invalidate_turbo_cache();
    }
}

impl World for PlotWorld {
    fn plot_coordinates(&self) -> (i32, i32) {
        (self.x, self.z)
    }

    fn observe_redstone_update(&mut self, pos: BlockPos, block: Block, direction: Option<BlockFace>) {
        if self.redstone_update_observations.is_none() {
            return;
        }
        let input_on = match block {
            Block::RedstoneRepeater { repeater } => {
                let rear = crate::redstone::diode_get_input_strength(self, pos, repeater.facing) > 0;
                let side = crate::redstone::repeater::get_state_for_placement(self, pos, repeater.facing).locked;
                Some((rear, side))
            }
            Block::Piston { piston } => Some((crate::redstone::piston::should_piston_extend(self, piston.facing, pos), false)),
            Block::RedstoneTorch { .. } => Some((crate::redstone::torch_should_be_off(self, pos), false)),
            Block::RedstoneWallTorch { facing, .. } => Some((crate::redstone::wall_torch_should_be_off(self, pos, facing), false)),
            Block::RedstoneLamp { .. } => Some((
                BlockFace::values().iter().any(|&face| {
                    let at = pos.offset(face);
                    crate::redstone::get_redstone_power(self.get_block(at), self, at, face) > 0
                }),
                false,
            )),
            _ => None,
        };
        if let Some(observations) = &mut self.redstone_update_observations {
            observations.push(RedstoneUpdateObservation {
                pos,
                block,
                direction,
                input_on,
            });
        }
    }

    fn execute_command_block(&mut self, command: &str, source: &str) -> Result<(), String> {
        self.execute_command_block_at(BlockPos::new(0, 0, 0), command, source)
    }
    fn execute_command_block_at(&mut self, pos: BlockPos, command: &str, source: &str) -> Result<(), String> {
        if self.scratch_suppress_commands {
            return Err("command-block execution is suppressed in scratch worlds".into());
        }
        if self.command_output_window.elapsed() >= Duration::from_secs(1) {
            self.command_output_window = Instant::now();
            self.command_output_count = 0;
            self.command_output_bytes = 0;
        }
        if (self.command_output_limits_enabled && (self.command_messages.len() >= 64 || self.command_output_count >= 64))
            || command.len() > 131_068
            || source.len() > 256
        {
            return Err(messages::COMMAND_BLOCK_OUTPUT_LIMIT.into());
        }
        let message = crate::chat_commands::parse(command, source, None)?;
        if self.command_output_limits_enabled && self.command_output_bytes.saturating_add(message.message.len()) > 65_536 {
            return Err(messages::COMMAND_BLOCK_OUTPUT_BYTE_LIMIT.into());
        }
        self.command_output_count += 1;
        self.command_output_bytes += message.message.len();
        self.waveform.text(pos, &message.message, self.piston_state.logical_tick);
        self.command_messages.push(message);
        Ok(())
    }
    /// Sets a block in storage. Returns true if a block was changed.
    fn set_block_raw(&mut self, pos: BlockPos, block: u32) -> bool {
        if self.scratch_capture_bounds.is_some_and(|bounds| !in_bounds(bounds, pos)) {
            self.scratch_boundary_effect.set(Some(pos));
            return false;
        }
        if self.scratch_capture_bounds.is_some() {
            if let Some(touched) = &mut self.scratch_touched_blocks {
                touched.insert(pos);
            }
        }
        let chunk_index = match self.get_chunk_index_for_block(pos.x, pos.z) {
            Some(idx) => idx,
            None => return false,
        };

        // Check to see if block is within height limit
        if pos.y >= PLOT_BLOCK_HEIGHT || pos.y < 0 {
            return false;
        }

        let old = self.get_block(pos);
        let new = Block::from_id(block);
        let screen_change = self.screen_updates.as_ref().is_some_and(|updates| {
            updates.authoritative || old.is_screen() || new.is_screen() || (matches!(old, Block::MovingPiston { .. }) && updates.contains(pos))
        });
        let previous_screen = screen_change.then(|| self.screen_state(pos));
        if mchprs_blocks::block_entities::ContainerType::from_block(old).is_some()
            && mchprs_blocks::block_entities::ContainerType::from_block(new).is_none()
        {
            self.delete_block_entity(pos);
        }
        if matches!(old, Block::MovingPiston { .. }) && old.get_id() != block {
            self.delete_block_entity(pos);
        }
        if old.is_sign() && !new.is_sign() && matches!(self.get_block_entity(pos), Some(BlockEntity::Sign(_))) {
            self.delete_block_entity(pos);
        }
        let chunk = &mut self.chunks[chunk_index];
        let changed = chunk.set_block((pos.x & 0xF) as u32, pos.y as u32, (pos.z & 0xF) as u32, block);
        let local_pos = BlockPos::new(pos.x & 15, pos.y, pos.z & 15);
        if let Some(ty) = mchprs_blocks::block_entities::ContainerType::from_block(new) {
            if !matches!(chunk.get_block_entity(local_pos), Some(BlockEntity::Container { ty: existing, .. }) if *existing == ty) {
                chunk.set_block_entity(
                    local_pos,
                    BlockEntity::Container {
                        ty,
                        inventory: Default::default(),
                        comparator_override: 0,
                    },
                );
            }
        }
        if new.is_sign() && !matches!(chunk.get_block_entity(local_pos), Some(BlockEntity::Sign(_))) {
            // The client creates signs from chunk block-entity entries on join.
            // Keep an existing sign's text when only its block state changes.
            chunk.set_block_entity(local_pos, BlockEntity::Sign(Default::default()));
        }
        if changed || self.screen_updates.as_ref().is_some_and(|updates| updates.authoritative) {
            if let Some(previous) = previous_screen {
                self.track_screen_change(pos, previous);
            }
        }
        if changed {
            self.record_waveform_change(pos);
            self.track_bulb_light(pos, old, new);
            if !self.instant_pistons.is_empty() {
                self.instant_pistons.block_changed(pos, old, new);
            }
        }
        changed
    }

    /// Returns the block state id of the block at `pos`
    fn get_block_raw(&self, pos: BlockPos) -> u32 {
        if self.scratch_read_bounds.is_some_and(|bounds| !in_bounds(bounds, pos))
            && !self.scratch_contract_air.as_ref().is_some_and(|positions| positions.contains(&pos))
        {
            let first = self.scratch_boundary_read.replace(Some(pos)).is_none();
            if first && std::env::var_os("MCHPRS_REDPILER_BOUNDARY_TRACE").is_some() {
                eprintln!("Redpiler unknown block read at {pos:?}\n{}", std::backtrace::Backtrace::force_capture());
            }
            return 0;
        }
        if self.scratch_read_bounds.is_some() {
            let chunk = (pos.x.div_euclid(16), pos.z.div_euclid(16));
            if !self.scratch_read_chunks.as_ref().is_some_and(|chunks| chunks.contains(&chunk))
                && !self.scratch_contract_air.as_ref().is_some_and(|positions| positions.contains(&pos))
            {
                let first = self.scratch_boundary_read.replace(Some(pos)).is_none();
                if first && std::env::var_os("MCHPRS_REDPILER_BOUNDARY_TRACE").is_some() {
                    eprintln!(
                        "Redpiler unknown unloaded block read at {pos:?}\n{}",
                        std::backtrace::Backtrace::force_capture()
                    );
                }
                return 0;
            }
        }
        let chunk_index = match self.get_chunk_index_for_block(pos.x, pos.z) {
            Some(idx) => idx,
            None => return 0,
        };
        let chunk = &self.chunks[chunk_index];
        chunk.get_block((pos.x & 0xF) as u32, pos.y as u32, (pos.z & 0xF) as u32)
    }

    fn delete_block_entity(&mut self, pos: BlockPos) {
        if self.scratch_capture_bounds.is_some_and(|bounds| !in_bounds(bounds, pos)) {
            self.scratch_boundary_effect.set(Some(pos));
            return;
        }
        if let Some(touched) = &mut self.scratch_touched_entities {
            touched.insert(pos);
        }
        if self.get_block_entity(pos).is_some() {
            self.instant_pistons.entity_changed(pos);
        }
        self.delete_piston_entity(pos);
    }

    fn delete_piston_entity(&mut self, pos: BlockPos) {
        if self.scratch_capture_bounds.is_some_and(|bounds| !in_bounds(bounds, pos)) {
            self.scratch_boundary_effect.set(Some(pos));
            return;
        }
        self.remove_motions_at(pos);
        let chunk_index = match self.get_chunk_index_for_block(pos.x, pos.z) {
            Some(idx) => idx,
            None => return,
        };
        let chunk = &mut self.chunks[chunk_index];
        chunk.delete_block_entity(BlockPos::new(pos.x & 0xF, pos.y, pos.z & 0xF));
        self.record_waveform_change(pos);
    }

    fn get_block_entity(&self, pos: BlockPos) -> Option<&BlockEntity> {
        if self.scratch_read_bounds.is_some_and(|bounds| !in_bounds(bounds, pos))
            && !self.scratch_contract_air.as_ref().is_some_and(|positions| positions.contains(&pos))
        {
            self.scratch_boundary_read.set(Some(pos));
            return None;
        }
        if self.scratch_read_bounds.is_some() {
            let chunk = (pos.x.div_euclid(16), pos.z.div_euclid(16));
            if !self.scratch_read_chunks.as_ref().is_some_and(|chunks| chunks.contains(&chunk))
                && !self.scratch_contract_air.as_ref().is_some_and(|positions| positions.contains(&pos))
            {
                self.scratch_boundary_read.set(Some(pos));
                return None;
            }
        }
        let chunk_index = self.get_chunk_index_for_block(pos.x, pos.z)?;
        let chunk = &self.chunks[chunk_index];
        chunk.get_block_entity(BlockPos::new(pos.x & 0xF, pos.y, pos.z & 0xF))
    }

    fn get_block_entity_mut(&mut self, pos: BlockPos) -> Option<&mut BlockEntity> {
        if self.scratch_capture_bounds.is_some_and(|bounds| !in_bounds(bounds, pos)) {
            self.scratch_boundary_effect.set(Some(pos));
            return None;
        }
        if let Some(touched) = &mut self.scratch_touched_entities {
            touched.insert(pos);
        }
        let index = self.get_chunk_index_for_block(pos.x, pos.z)?;
        self.instant_pistons.entity_changed(pos);
        self.chunks[index].get_block_entity_mut(BlockPos::new(pos.x & 15, pos.y, pos.z & 15))
    }

    fn set_piston_progress(&mut self, pos: BlockPos, progress: f32) {
        if self.scratch_capture_bounds.is_some_and(|bounds| !in_bounds(bounds, pos)) {
            self.scratch_boundary_effect.set(Some(pos));
            return;
        }
        if let Some(touched) = &mut self.scratch_touched_entities {
            touched.insert(pos);
        }
        let Some(index) = self.get_chunk_index_for_block(pos.x, pos.z) else {
            return;
        };
        if let Some(BlockEntity::MovingPiston(entity)) = self.chunks[index].get_block_entity_mut(BlockPos::new(pos.x & 15, pos.y, pos.z & 15)) {
            entity.set_progress(progress);
        }
    }

    fn piston_state(&self) -> &PistonState {
        &self.piston_state
    }
    fn piston_state_mut(&mut self) -> &mut PistonState {
        self.piston_index.get_mut().invalidate();
        self.instant_pistons.clear();
        &mut self.piston_state
    }

    fn piston_motion_index(&self, pos: BlockPos, identity: Option<u64>) -> Option<usize> {
        self.piston_index.borrow_mut().motion(&self.piston_state, pos, identity)
    }

    fn advance_piston_motion(&mut self, index: usize) -> (bool, f32) {
        let pos = self.piston_state.motions[index].pos;
        if self
            .scratch_capture_bounds
            .is_some_and(|(min, max)| pos.x < min.x || pos.y < min.y || pos.z < min.z || pos.x > max.x || pos.y > max.y || pos.z > max.z)
        {
            self.scratch_boundary_effect.set(Some(pos));
            return (false, 0.0);
        }
        let state = &mut self.piston_state;
        state.motions[index].advance(state.logical_tick)
    }

    fn remove_piston_motion(&mut self, index: usize) {
        let pos = self.piston_state.motions[index].pos;
        if self
            .scratch_capture_bounds
            .is_some_and(|(min, max)| pos.x < min.x || pos.y < min.y || pos.z < min.z || pos.x > max.x || pos.y > max.y || pos.z > max.z)
        {
            self.scratch_boundary_effect.set(Some(pos));
            return;
        }
        let pos = self.piston_state.motions.remove(index).unwrap().pos;
        self.piston_index.get_mut().invalidate();
        self.piston_index.get_mut().removed(pos);
    }

    fn set_piston_carried_entity(&mut self, pos: BlockPos, entity: Option<Box<BlockEntity>>) {
        if self
            .scratch_capture_bounds
            .is_some_and(|(min, max)| pos.x < min.x || pos.y < min.y || pos.z < min.z || pos.x > max.x || pos.y > max.y || pos.z > max.z)
        {
            self.scratch_boundary_effect.set(Some(pos));
            return;
        }
        if entity.is_some() {
            self.instant_pistons.entity_changed(pos);
        }
        if let Some(i) = self.piston_motion_index(pos, None) {
            self.piston_state.motions[i].carried_entity = entity;
        }
    }

    fn enqueue_piston_event(&mut self, event: mchprs_world::PistonEvent) {
        if self.scratch_capture_bounds.is_some_and(|(min, max)| {
            event.pos.x < min.x || event.pos.y < min.y || event.pos.z < min.z || event.pos.x > max.x || event.pos.y > max.y || event.pos.z > max.z
        }) {
            self.scratch_boundary_effect.set(Some(event.pos));
            return;
        }
        if !self.piston_index.borrow_mut().has_event(&self.piston_state, event) {
            self.piston_state.events.push_back(event);
            self.piston_index.get_mut().pushed(event);
            #[cfg(test)]
            redstone::instant_piston_tests::record_event(self, "event_enqueue", event);
        }
    }

    fn wire_location(&self, pos: BlockPos) -> Option<u32> {
        if !(0..PLOT_BLOCK_HEIGHT).contains(&pos.y) {
            return None;
        }
        let chunk = self.get_chunk_index_for_block(pos.x, pos.z)? as u32;
        let section = (chunk << 4) | (pos.y as u32 >> 4);
        Some((section << 12) | ((pos.y as u32 & 15) << 8) | ((pos.z as u32 & 15) << 4) | (pos.x as u32 & 15))
    }

    fn wire_neighborhood(&self, pos: BlockPos) -> Option<std::sync::Arc<[crate::world::WireNeighbor; 24]>> {
        let cell = self.wire_location(pos)?;
        Some(self.wire_topology.borrow_mut().neighborhood(cell, pos, |p| self.wire_location(p)))
    }

    fn get_wire_block_raw(&self, location: crate::world::WireNeighbor) -> u32 {
        if self.scratch_read_bounds.is_some() {
            return self.get_block_raw(location.pos);
        }
        let Some(cell) = location.cell else {
            return self.get_block_raw(location.pos);
        };
        let section = (cell >> 12) as usize;
        self.chunks[section >> 4].sections[section & 15].get_block_by_index((cell & 4095) as usize)
    }

    fn instant_piston_cache(&self) -> Option<&crate::world::InstantPistonCache> {
        Some(&self.instant_pistons)
    }

    fn instant_piston_cache_mut(&mut self) -> Option<&mut crate::world::InstantPistonCache> {
        Some(&mut self.instant_pistons)
    }

    fn set_block_entity(&mut self, pos: BlockPos, block_entity: BlockEntity) {
        if let Some((min, max)) = self.scratch_capture_bounds {
            if pos.x < min.x || pos.y < min.y || pos.z < min.z || pos.x > max.x || pos.y > max.y || pos.z > max.z {
                self.scratch_boundary_effect.set(Some(pos));
                return;
            }
            if let Some(touched) = &mut self.scratch_touched_entities {
                touched.insert(pos);
            }
        }
        let chunk_index = match self.get_chunk_index_for_block(pos.x, pos.z) {
            Some(idx) => idx,
            None => return,
        };
        if let BlockEntity::MovingPiston(e) = &block_entity {
            if let Some(index) = self.piston_motion_index(pos, None) {
                let identity = self.piston_state.motions[index].identity;
                self.instant_pistons.reject_motion(pos, identity);
            }
            // register_motion already removes the previous motion at this position.
            // Repeating that linear scan here doubles the work for large piston banks.
            self.register_motion(pos, e.get_progress());
        } else {
            self.instant_pistons.entity_changed(pos);
            self.remove_motions_at(pos);
        }
        let send_command = matches!(&block_entity, BlockEntity::CommandBlock(_));
        if let BlockEntity::MovingPiston(entity) = &block_entity {
            if self.screen_only() && Block::from_id(entity.block_state).is_screen() {
                // A newly moved lamp may replace a previously non-screen block.
                // Existing tracked pixels retain their actual previous state.
                self.track_screen_change(pos, 0);
            }
        }
        if let Some(nbt) = block_entity.to_nbt(!send_command) {
            let block_entity_data = CBlockEntityData {
                pos: pos.packed(),
                // Signs and command editors need their current block data.
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
        self.record_waveform_change(pos);
    }

    fn get_chunk(&self, x: i32, z: i32) -> Option<&Chunk> {
        if !(self.x * PLOT_WIDTH..(self.x + 1) * PLOT_WIDTH).contains(&x) || !(self.z * PLOT_WIDTH..(self.z + 1) * PLOT_WIDTH).contains(&z) {
            return None;
        }
        self.chunks.get(self.get_chunk_index_for_chunk(x, z))
    }

    fn get_chunk_mut(&mut self, x: i32, z: i32) -> Option<&mut Chunk> {
        self.get_chunk(x, z)?;
        self.instant_pistons.clear();
        let chunk_idx = self.get_chunk_index_for_chunk(x, z);
        self.chunks.get_mut(chunk_idx)
    }

    fn schedule_tick(&mut self, pos: BlockPos, delay: u32, priority: TickPriority) {
        self.schedule_half_tick(pos, delay * 2, priority);
    }

    fn schedule_half_tick(&mut self, pos: BlockPos, delay: u32, priority: TickPriority) {
        if self
            .scratch_capture_bounds
            .is_some_and(|(min, max)| pos.x < min.x || pos.y < min.y || pos.z < min.z || pos.x > max.x || pos.y > max.y || pos.z > max.z)
        {
            self.scratch_boundary_effect.set(Some(pos));
            return;
        }
        let node = ScheduledBlockTick {
            pos,
            block_type: Some(self.get_block(pos).registry_id()),
        };
        if !self.tick_index.contains(&self.to_be_ticked, node) {
            self.to_be_ticked.schedule_half_tick(node, delay as usize, priority);
            self.tick_index.pushed(node);
        }
    }

    fn restore_scheduled_tick(&mut self, entry: TickEntry) {
        if self.scratch_capture_bounds.is_some_and(|(min, max)| {
            entry.pos.x < min.x || entry.pos.y < min.y || entry.pos.z < min.z || entry.pos.x > max.x || entry.pos.y > max.y || entry.pos.z > max.z
        }) {
            self.scratch_boundary_effect.set(Some(entry.pos));
            return;
        }
        let node = ScheduledBlockTick {
            pos: entry.pos,
            block_type: entry.block_type,
        };
        self.to_be_ticked.schedule_half_tick(node, entry.ticks_left as usize, entry.tick_priority);
        self.tick_index.pushed(node);
    }

    fn pending_tick_at(&mut self, pos: BlockPos) -> bool {
        let node = ScheduledBlockTick {
            pos,
            block_type: Some(self.get_block(pos).registry_id()),
        };
        self.tick_index.contains(&self.to_be_ticked, node)
    }

    fn block_action(&mut self, pos: BlockPos, block_action: BlockAction) {
        if self
            .scratch_capture_bounds
            .is_some_and(|(min, max)| pos.x < min.x || pos.y < min.y || pos.z < min.z || pos.x > max.x || pos.y > max.y || pos.z > max.z)
        {
            self.scratch_boundary_effect.set(Some(pos));
            return;
        }
        match block_action {
            BlockAction::Piston { action, piston } => {
                if self.fast_rendering || self.screen_only() {
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

    fn level_event_for_action(&mut self, pos: BlockPos, event: i32, excluded: u128) {
        self.level_events.push((
            excluded,
            CEffect {
                effect_id: event,
                pos: pos.packed(),
                data: 0,
                disable_relative_volume: false,
            },
        ));
    }

    fn play_sound(&mut self, pos: BlockPos, sound_id: i32, sound_category: i32, volume: f32, pitch: f32) {
        // Fast rendering suppresses automated circuit sounds.
        if self.fast_rendering {
            return;
        }
        self.play_sound_for_action(pos, sound_id, sound_category, volume, pitch, None);
    }

    fn play_sound_for_action(&mut self, pos: BlockPos, sound_id: i32, sound_category: i32, volume: f32, pitch: f32, excluded: Option<u128>) {
        if self.sounds.len() >= 256 {
            return;
        }
        if let Some(sound) = crate::sound::Emission::new(pos, sound_id, sound_category, volume, pitch, excluded) {
            self.sounds.push(sound);
        }
    }
    fn container_opened(&mut self, pos: BlockPos, ty: mchprs_blocks::block_entities::ContainerType) {
        if ty == mchprs_blocks::block_entities::ContainerType::Chest && self.open_chests.insert(pos) {
            crate::sound::play(self, pos, "block.chest.open", 0.5, 0.95, None);
        }
    }
}
