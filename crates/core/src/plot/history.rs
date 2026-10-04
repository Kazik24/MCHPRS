//! Interpreter snapshots. The vector is a ring; snapshots are allocated as ticks run.
#[cfg(test)]
mod tests;
use super::data::sleep_time_for_tps;
use super::{Plot, PlotWorld, PLOT_SECTIONS};
use crate::config::CONFIG;
use crate::player::PacketSender;
use crate::redpiler::backend::ScheduledBlockTick;
use crate::redpiler::TickScheduler;
use crate::world::storage::Chunk;
use mchprs_blocks::block_entities::{
    BlockEntity, CommandBlockEntity, InventoryEntry, SignBlockEntity,
};
use mchprs_blocks::BlockPos;
use mchprs_save_data::plot_data::{ChunkData, Tps};
use mchprs_world::{AdvancePhase, PistonEvent, PistonMotion, PistonState};
use std::mem::size_of;
use std::time::Instant;

pub(super) const DEFAULT_HISTORY_TICKS: usize = 100;
const NORMAL_HISTORY_LIMIT: usize = 1_000;
const UNLIMITED_HISTORY_PERMISSION: &str = "plots.admin.rewind.unlimited";

fn validate_tick_limit(ticks: usize, unlimited: bool) -> Result<(), String> {
    if ticks > NORMAL_HISTORY_LIMIT && !unlimited {
        return Err(format!(
            "More than {NORMAL_HISTORY_LIMIT} game ticks requires {UNLIMITED_HISTORY_PERMISSION} permission."
        ));
    }
    Ok(())
}

struct Snapshot {
    chunks: Vec<(i32, i32, ChunkData<PLOT_SECTIONS>)>,
    scheduler: TickScheduler<ScheduledBlockTick>,
    piston_state: PistonState,
    heap_bytes: usize,
}

impl Snapshot {
    fn capture(world: &mut PlotWorld) -> Self {
        let chunks: Vec<_> = world
            .chunks
            .iter_mut()
            .map(|chunk| (chunk.x, chunk.z, chunk.save()))
            .collect();
        let scheduler = world.to_be_ticked.clone();
        let piston_state = world.piston_state.clone();
        let mut heap_bytes = chunks.capacity() * size_of::<(i32, i32, ChunkData<PLOT_SECTIONS>)>();
        for (_, _, chunk) in &chunks {
            for section in chunk.sections.iter().flatten() {
                heap_bytes += section.data.capacity() * size_of::<i64>()
                    + section.palette.capacity() * size_of::<i32>();
            }
            // Approximate hash-table control bytes; allocator padding is excluded.
            heap_bytes +=
                chunk.block_entities.capacity() * (size_of::<(BlockPos, BlockEntity)>() + 1);
            heap_bytes += chunk
                .block_entities
                .values()
                .map(entity_heap_bytes)
                .sum::<usize>();
        }
        // Cloned queue vectors contain their live entries. Queue structs are inline.
        heap_bytes += scheduler.iter().count() * size_of::<ScheduledBlockTick>();
        heap_bytes += piston_state.events.capacity() * size_of::<PistonEvent>()
            + piston_state.motions.capacity() * size_of::<PistonMotion>()
            + piston_state.movement_work.capacity() * size_of::<(BlockPos, u64)>();
        for motion in &piston_state.motions {
            if let Some(entity) = &motion.carried_entity {
                heap_bytes += size_of::<BlockEntity>() + entity_heap_bytes(entity);
            }
        }
        Self {
            chunks,
            scheduler,
            piston_state,
            heap_bytes,
        }
    }

    fn restore(self, world: &mut PlotWorld) {
        world.chunks = self
            .chunks
            .into_iter()
            .map(|(x, z, data)| Chunk::load(x, z, data))
            .collect();
        world.to_be_ticked = self.scheduler;
        world.piston_state = self.piston_state;
        world.command_messages.clear();
    }
}

fn entity_heap_bytes(entity: &BlockEntity) -> usize {
    match entity {
        BlockEntity::Comparator { .. } | BlockEntity::MovingPiston(_) => 0,
        BlockEntity::Container { inventory, .. } => {
            let header = if inventory.capacity() == 0 {
                0
            } else {
                2 * size_of::<usize>()
            };
            header
                + inventory.capacity() * size_of::<InventoryEntry>()
                + inventory
                    .iter()
                    .filter_map(|item| item.nbt.as_ref())
                    .map(Vec::capacity)
                    .sum::<usize>()
        }
        BlockEntity::Sign(sign) => {
            size_of::<SignBlockEntity>()
                + sign
                    .rows
                    .iter()
                    .chain(&sign.back_rows)
                    .map(String::capacity)
                    .sum::<usize>()
                + sign.front_color.capacity()
                + sign.back_color.capacity()
        }
        BlockEntity::CommandBlock(command) => {
            size_of::<CommandBlockEntity>()
                + command.command.capacity()
                + command.custom_name.capacity()
                + command.last_output.as_ref().map_or(0, String::capacity)
        }
    }
}

#[derive(Default)]
pub(super) struct TickHistory {
    slots: Vec<Option<Snapshot>>,
    next_write: usize,
    len: usize,
    heap_bytes: usize,
}

impl TickHistory {
    pub fn enabled(&self) -> bool {
        !self.slots.is_empty()
    }
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn memory_bytes(&self) -> usize {
        self.slots.capacity() * size_of::<Option<Snapshot>>() + self.heap_bytes
    }

    fn prepare(capacity: usize) -> Result<Self, String> {
        if capacity == 0 || capacity > i32::MAX as usize {
            return Err("History capacity must be between 1 and 2147483647 game ticks.".into());
        }
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(capacity)
            .map_err(|_| "Unable to allocate the history buffer.".to_owned())?;
        slots.resize_with(capacity, || None);
        Ok(Self {
            slots,
            ..Self::default()
        })
    }

    pub fn disable(&mut self) -> usize {
        let bytes = self.memory_bytes();
        *self = Self::default();
        bytes
    }

    fn release_next(&mut self) {
        if let Some(old) = self.slots[self.next_write].take() {
            self.heap_bytes -= old.heap_bytes;
            self.len -= 1;
        }
    }

    fn push(&mut self, snapshot: Snapshot) {
        debug_assert!(self.slots[self.next_write].is_none());
        self.heap_bytes += snapshot.heap_bytes;
        self.slots[self.next_write] = Some(snapshot);
        self.len += 1;
        self.next_write = (self.next_write + 1) % self.capacity();
    }

    pub fn validate_rewind(&self, ticks: usize) -> Result<(), String> {
        if !self.enabled() {
            return Err("Tick history is disabled. Use /rhistory on [ticks].".into());
        }
        if ticks == 0 {
            return Err("Specify a positive number of game ticks to rewind.".into());
        }
        if ticks > self.len {
            return Err(format!(
                "Only {} game ticks are available to rewind.",
                self.len
            ));
        }
        Ok(())
    }

    fn take_back(&mut self, ticks: usize) -> Snapshot {
        let mut target = None;
        for _ in 0..ticks {
            self.next_write = if self.next_write == 0 {
                self.capacity() - 1
            } else {
                self.next_write - 1
            };
            let snapshot = self.slots[self.next_write]
                .take()
                .expect("occupied history slot");
            self.heap_bytes -= snapshot.heap_bytes;
            self.len -= 1;
            target = Some(snapshot);
        }
        target.expect("positive rewind count")
    }
}

impl PlotWorld {
    /// Start a fresh session. Returns projected full-buffer bytes for the current plot.
    pub(super) fn enable_history(
        &mut self,
        capacity: usize,
        unlimited: bool,
    ) -> Result<usize, String> {
        validate_tick_limit(capacity, unlimited)?;
        self.require_tick_boundary()?;
        let history = TickHistory::prepare(capacity)?;
        let sample = Snapshot::capture(self);
        let projected = sample
            .heap_bytes
            .checked_mul(capacity)
            .and_then(|bytes| bytes.checked_add(history.memory_bytes()))
            .ok_or_else(|| "History memory estimate exceeds the supported size.".to_owned())?;
        self.history = history;
        Ok(projected)
    }

    fn require_tick_boundary(&self) -> Result<(), String> {
        if self.piston_state.phase != AdvancePhase::BetweenTicks {
            Err("Finish the partial tick with /radvance 1 before using tick history.".into())
        } else {
            Ok(())
        }
    }

    pub(super) fn record_tick(&mut self) {
        if self.history.enabled() {
            debug_assert_eq!(self.piston_state.phase, AdvancePhase::BetweenTicks);
            self.history.release_next();
            let snapshot = Snapshot::capture(self);
            self.history.push(snapshot);
        }
    }

    pub(super) fn validate_rewind(&self, ticks: usize) -> Result<(), String> {
        self.require_tick_boundary()?;
        self.history.validate_rewind(ticks)
    }

    pub(super) fn rewind_ticks(&mut self, ticks: usize, unlimited: bool) -> Result<(), String> {
        validate_tick_limit(ticks, unlimited)?;
        self.validate_rewind(ticks)?;
        let snapshot = self.history.take_back(ticks);
        snapshot.restore(self);
        Ok(())
    }
}

pub(super) fn format_memory(bytes: usize) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else {
        format!("{:.2} MiB", bytes as f64 / (1024.0 * 1024.0))
    }
}

impl Plot {
    fn authorize_history(
        &self,
        player: usize,
        permission: &str,
        mutating: bool,
    ) -> Result<(), String> {
        let player = &self.players[player];
        if !player.has_permission(permission) {
            return Err("You do not have permission to use this command.".into());
        }
        if mutating {
            let allowed = match self.owner {
                Some(owner) => {
                    owner == player.uuid || player.has_permission("plots.admin.interact.other")
                }
                None => player.has_permission("plots.admin.interact.unowned"),
            };
            if !allowed {
                return Err("You do not have permission to change this plot.".into());
            }
        }
        Ok(())
    }

    pub(super) fn control_history(
        &mut self,
        player: usize,
        args: &[&str],
    ) -> Result<String, String> {
        self.authorize_history(
            player,
            "commands.rhistory",
            !args.is_empty() && args != ["status"],
        )?;
        match args {
            [] | ["status"] => Ok(format!(
                "Tick history: {}. Available: {}/{} game ticks. Approximate memory: {}.",
                if self.world.history.enabled() {
                    "on"
                } else {
                    "off"
                },
                self.world.history.len(),
                self.world.history.capacity(),
                format_memory(self.world.history.memory_bytes())
            )),
            ["off"] => {
                let ticks = self.world.history.len();
                let bytes = self.world.history.disable();
                Ok(format!(
                    "History disabled. Released approximately {} from {ticks} snapshots.",
                    format_memory(bytes)
                ))
            }
            ["on"] | ["on", _] => {
                if self.redpiler.is_active() {
                    return Err(
                        "Tick history is only available during interpreted execution.".into(),
                    );
                }
                let capacity = match args.get(1) {
                    Some(value) => value.parse::<usize>().map_err(|_| {
                        "Specify a positive history capacity in game ticks.".to_owned()
                    })?,
                    None => DEFAULT_HISTORY_TICKS,
                };
                let unlimited =
                    self.players[player].has_explicit_permission(UNLIMITED_HISTORY_PERMISSION);
                let projected = self.world.enable_history(capacity, unlimited)?;
                self.reset_timings();
                Ok(format!(
                    "History enabled: up to {capacity} game ticks. Approximate current memory: {}. Estimated memory at full capacity: {} (based on the current plot).",
                    format_memory(self.world.history.memory_bytes()), format_memory(projected)
                ))
            }
            _ => Err("Usage: /rhistory [on [ticks]|off|status]".into()),
        }
    }

    pub(super) fn rewind_plot(&mut self, player: usize, args: &[&str]) -> Result<(), String> {
        self.authorize_history(player, "commands.rback", true)?;
        let ticks = match args {
            [] => 1,
            [value] => value
                .parse::<usize>()
                .map_err(|_| "Specify a positive number of game ticks.".to_owned())?,
            _ => return Err("Usage: /rback [ticks]".into()),
        };
        if self.redpiler.is_active() {
            return Err("Tick rewind is only available during interpreted execution.".into());
        }
        let unlimited = self.players[player].has_explicit_permission(UNLIMITED_HISTORY_PERMISSION);
        self.world.rewind_ticks(ticks, unlimited)?;
        self.close_all_containers();
        self.tps = Tps::Limited(0);
        self.sleep_time = sleep_time_for_tps(self.tps);
        self.timings.set_tps(self.tps);
        self.reset_timings();
        self.update_render_mode();
        self.last_world_send_time = Instant::now();
        for player in &mut self.players {
            if !player.worldedit_undo.is_empty() || !player.worldedit_redo.is_empty() {
                player.worldedit_undo.clear();
                player.worldedit_redo.clear();
                player.send_system_message("WorldEdit undo/redo cleared after tick rewind.");
            }
        }
        // Explicit authoritative refresh also works when /wsr 0 disables periodic sends.
        for chunk in &self.world.chunks {
            let visible: Vec<_> = self
                .players
                .iter()
                .filter(|player| {
                    Self::get_chunk_distance(
                        chunk.x,
                        chunk.z,
                        player.last_chunk_x,
                        player.last_chunk_z,
                    ) <= CONFIG.view_distance.max(0) as u32
                })
                .collect();
            if visible.is_empty() {
                continue;
            }
            let packet = chunk.encode_packet_for_client(self.world.fast_rendering);
            for player in visible {
                player.client.send_packet(&packet);
            }
        }
        self.broadcast_plot_chat_message(&format!(
            "Plot rewound by {ticks} game ticks and paused. {} game ticks remain in history.",
            self.world.history.len()
        ));
        Ok(())
    }
}
