//! Independent LZ4 snapshots with a shared per-session dictionary and bounded storage.
use crate::messages;
mod budget;
mod codec;
#[cfg(test)]
mod tests;

use super::data::sleep_time_for_tps;
use super::{Plot, PlotWorld, PLOT_SECTIONS};
use crate::config::CONFIG;
use crate::player::PacketSender;
use budget::{Budget, Bytes, Reservation};
use codec::{capture_raw, Encoded, Stored};
use mchprs_network::packets::clientbound::{CChatMessage, ClientBoundPacket};
use mchprs_save_data::plot_data::Tps;
use mchprs_world::AdvancePhase;
use std::mem::size_of;
use std::sync::Arc;
use std::time::Instant;

pub(super) const DEFAULT_HISTORY_TICKS: usize = 100;
const NORMAL_HISTORY_LIMIT: usize = 1_000;
const UNLIMITED_HISTORY_PERMISSION: &str = "plots.admin.rewind.unlimited";
const MEMORY_PERMISSION: &str = "plots.admin.rewind.memory";

fn validate_tick_limit(ticks: usize, unlimited: bool) -> Result<(), String> {
    if ticks > NORMAL_HISTORY_LIMIT && !unlimited {
        return Err(messages::history_tick_limit_permission(
            NORMAL_HISTORY_LIMIT,
            UNLIMITED_HISTORY_PERMISSION,
        ));
    }

    Ok(())
}

pub(super) struct TickHistory {
    slots: Vec<Option<Stored>>,
    dictionary: Option<Bytes>,
    _slots_reservation: Option<Reservation>,
    budget: Arc<Budget>,
    work: Arc<Budget>,
    next_write: usize,
    len: usize,
    heap_bytes: usize,
    raw_bytes: usize,
}

impl Default for TickHistory {
    fn default() -> Self {
        Self::with_budgets(budget::STORED.clone(), budget::WORK.clone())
    }
}

impl TickHistory {
    fn with_budgets(budget: Arc<Budget>, work: Arc<Budget>) -> Self {
        Self {
            slots: Vec::new(),
            dictionary: None,
            _slots_reservation: None,
            budget,
            work,
            next_write: 0,
            len: 0,
            heap_bytes: 0,
            raw_bytes: 0,
        }
    }

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
        self.slots.capacity() * size_of::<Option<Stored>>()
            + self.heap_bytes
            + self.dictionary.as_ref().map_or(0, |d| d.data.capacity())
    }

    fn dictionary(&self) -> &[u8] {
        self.dictionary.as_ref().map_or(&[], |d| &d.data)
    }

    fn status(&self) -> String {
        let (used, limit) = self.budget.stats();
        let state = match self.enabled() {
            true => "on",
            false => "off",
        };

        messages::history_status(
            state,
            self.len(),
            self.capacity(),
            format_memory(self.raw_bytes),
            format_memory(self.heap_bytes),
            format_memory(used),
            format_memory(limit),
        )
    }

    fn prepare(capacity: usize, budget: Arc<Budget>, work: Arc<Budget>) -> Result<Self, String> {
        if capacity == 0 || capacity > i32::MAX as usize {
            return Err(messages::HISTORY_CAPACITY_MUST_BETWEEN_GAME_TICKS.into());
        }
        let bytes = capacity
            .checked_mul(size_of::<Option<Stored>>())
            .ok_or(messages::HISTORY_SIZE_OVERFLOW)?;
        let reservation = budget.reserve(bytes)?;
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(capacity)
            .map_err(|_| messages::UNABLE_ALLOCATE_HISTORY_BUFFER.to_owned())?;
        debug_assert_eq!(slots.capacity(), capacity);
        slots.resize_with(capacity, || None);
        Ok(Self {
            slots,
            _slots_reservation: Some(reservation),
            ..Self::with_budgets(budget, work)
        })
    }

    pub fn disable(&mut self) -> usize {
        let bytes = self.memory_bytes();
        *self = Self::with_budgets(self.budget.clone(), self.work.clone());
        bytes
    }

    fn release(&mut self, index: usize) {
        let old = self.slots[index].take().expect("occupied history slot");
        self.heap_bytes -= old.bytes.data.capacity();
        self.raw_bytes -= old.raw_len;
        self.len -= 1;
    }

    fn release_oldest(&mut self) {
        self.release((self.next_write + self.capacity() - self.len) % self.capacity());
    }

    fn push(&mut self, encoded: Encoded) -> Result<(), String> {
        if self.len == self.capacity() {
            self.release_oldest();
        }
        // Admission happens after compression. Evict only this plot's oldest ticks.
        let bytes = loop {
            match Bytes::copy(&self.budget, &encoded.bytes.data) {
                Ok(bytes) => break bytes,
                Err(_) if self.len > 0 => self.release_oldest(),
                Err(error) => return Err(error),
            }
        };
        self.heap_bytes += bytes.data.capacity();
        self.raw_bytes += encoded.raw_len;
        self.slots[self.next_write] = Some(Stored {
            bytes,
            raw_len: encoded.raw_len,
            checksum: encoded.checksum,
            compressed: encoded.compressed,
        });
        self.len += 1;
        self.next_write = (self.next_write + 1) % self.capacity();
        Ok(())
    }

    pub fn validate_rewind(&self, ticks: usize) -> Result<(), String> {
        if !self.enabled() {
            return Err(messages::TICK_HISTORY_DISABLED_USE_RHISTORY_ON.into());
        }
        if ticks == 0 {
            return Err(messages::SPECIFY_POSITIVE_NUMBER_GAME_TICKS_REWIND.into());
        }
        if ticks > self.len {
            return Err(messages::rewind_available(self.len));
        }
        Ok(())
    }

    fn decode_back(&mut self, ticks: usize) -> Result<codec::Snapshot, String> {
        let index = (self.next_write + self.capacity() - ticks) % self.capacity();
        let snapshot = self.slots[index]
            .as_ref()
            .expect("occupied history slot")
            .decode(self.dictionary(), &self.work)?;
        // Keep history intact until decoding has succeeded.
        for _ in 0..ticks {
            self.next_write = if self.next_write == 0 {
                self.capacity() - 1
            } else {
                self.next_write - 1
            };
            self.release(self.next_write);
        }
        Ok(snapshot)
    }
}

impl PlotWorld {
    pub(super) fn enable_history(
        &mut self,
        capacity: usize,
        unlimited: bool,
    ) -> Result<usize, String> {
        validate_tick_limit(capacity, unlimited)?;
        self.require_tick_boundary()?;
        let mut history = TickHistory::prepare(
            capacity,
            self.history.budget.clone(),
            self.history.work.clone(),
        )?;
        let raw = capture_raw(self, &history.work)?;
        // LZ4 references the last 64 KiB. Keep the dictionary unchanged throughout the session.
        let start = raw.data.len().saturating_sub(65535);
        history.dictionary = Some(Bytes::copy(&history.budget, &raw.data[start..])?);
        let sample = Encoded::encode(raw, history.dictionary(), &history.work)?;
        let stored = sample.bytes.data.len();
        let minimum = history
            .memory_bytes()
            .checked_add(stored)
            .ok_or(messages::HISTORY_SIZE_OVERFLOW)?;
        if minimum > history.budget.stats().1 {
            return Err(messages::ONE_COMPRESSED_SNAPSHOT_CANNOT_FIT_HISTORY.into());
        }
        let projected = stored
            .checked_mul(capacity)
            .and_then(|n| n.checked_add(history.memory_bytes()))
            .ok_or_else(|| messages::HISTORY_MEMORY_ESTIMATE_EXCEEDS_SUPPORTED_SIZE.to_owned())?;
        self.history = history;
        Ok(projected)
    }

    fn require_tick_boundary(&self) -> Result<(), String> {
        if self.piston_state.phase != AdvancePhase::BetweenTicks {
            return Err(messages::FINISH_PARTIAL_TICK_RADVANCE_BEFORE_USING.into());
        }

        Ok(())
    }

    pub(super) fn record_tick(&mut self) {
        if !self.history.enabled() {
            return;
        }
        debug_assert_eq!(self.piston_state.phase, AdvancePhase::BetweenTicks);
        let result = capture_raw(self, &self.history.work.clone())
            .and_then(|raw| Encoded::encode(raw, self.history.dictionary(), &self.history.work))
            .and_then(|encoded| self.history.push(encoded));
        if let Err(error) = result {
            self.history.disable();
            tracing::warn!("Tick history stopped: {error}");
            let message = serde_json::json!({
                "text": messages::history_stopped(error),
                "color": "red",
            })
            .to_string();
            let packet = CChatMessage {
                message,
                position: 1,
                sender: 0,
            }
            .encode();

            for sender in &self.packet_senders {
                sender.send_packet(&packet);
            }
        }
    }

    pub(super) fn validate_rewind(&self, ticks: usize) -> Result<(), String> {
        self.require_tick_boundary()?;
        self.history.validate_rewind(ticks)
    }

    pub(super) fn rewind_ticks(&mut self, ticks: usize, unlimited: bool) -> Result<(), String> {
        validate_tick_limit(ticks, unlimited)?;
        self.validate_rewind(ticks)?;
        let snapshot = self.history.decode_back(ticks)?;
        snapshot.restore(self);
        Ok(())
    }
}

pub(super) fn format_memory(bytes: usize) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.2} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.2} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

impl Plot {
    fn validate_player_history_limit(&self, player: usize, ticks: usize) -> Result<(), String> {
        let player = &self.players[player];
        if player.has_explicit_permission(UNLIMITED_HISTORY_PERMISSION) {
            return Ok(());
        }
        let (limit, permission) = if crate::permissions::dedicated_permissions() {
            (
                player
                    .numeric_permission_limit("mchprs.history.limit.")
                    .unwrap_or(0),
                "mchprs.plots.admin.rewind.unlimited",
            )
        } else {
            (NORMAL_HISTORY_LIMIT, UNLIMITED_HISTORY_PERMISSION)
        };
        if ticks > limit {
            return Err(messages::history_tick_limit_permission(limit, permission));
        }
        Ok(())
    }

    fn authorize_history(
        &self,
        player: usize,
        permission: &str,
        mutating: bool,
    ) -> Result<(), String> {
        let player = &self.players[player];
        if !player.has_permission(permission) {
            return Err(messages::COMMAND_PERMISSION_DENIED.into());
        }
        if mutating {
            if !player.can_edit_plot(self.owner) {
                return Err(messages::PLOT_PERMISSION_DENIED.into());
            }
        }
        Ok(())
    }

    pub(super) fn control_history(
        &mut self,
        player: usize,
        args: &[&str],
    ) -> Result<String, String> {
        if crate::permissions::dedicated_permissions() {
            let action = match args {
                [] | ["status"] => "status",
                ["limit"] => "limit.view",
                ["limit", _] => "limit.set",
                ["off"] => "disable",
                _ => "enable",
            };
            if !self.players[player].has_permission(&format!("commands.rhistory.{action}")) {
                return Err(messages::COMMAND_PERMISSION_DENIED.into());
            }
        }
        self.authorize_history(
            player,
            "commands.rhistory",
            !args.is_empty() && args != ["status"] && args.first() != Some(&"limit"),
        )?;
        match args {
            [] | ["status"] => Ok(self.world.history.status()),
            ["limit"] => {
                let (used, limit) = self.world.history.budget.stats();
                Ok(messages::server_history_usage(
                    format_memory(used),
                    format_memory(limit),
                ))
            }
            ["limit", value] => {
                if !self.players[player].has_explicit_permission(MEMORY_PERMISSION) {
                    return Err(messages::history_memory_permission(MEMORY_PERMISSION));
                }
                let mib = value
                    .parse::<i64>()
                    .map_err(|_| messages::SPECIFY_NONNEGATIVE_MEMORY_LIMIT_MIB.to_owned())?;
                let bytes = budget::mib_to_bytes(mib)?;
                self.world
                    .history
                    .budget
                    .set_limit(bytes, || crate::config::save_history_limit(mib))?;
                Ok(messages::history_limit_saved(format_memory(bytes)))
            }
            ["off"] => {
                let bytes = self.world.history.disable();
                Ok(messages::history_disabled(format_memory(bytes)))
            }
            ["on"] | ["on", _] => {
                if self.redpiler.is_active() {
                    return Err(messages::TICK_HISTORY_ONLY_AVAILABLE_DURING_INTERPRETED.into());
                }
                let capacity = match args.get(1) {
                    Some(value) => value.parse::<usize>().map_err(|_| {
                        messages::SPECIFY_POSITIVE_HISTORY_CAPACITY_GAME_TICKS.to_owned()
                    })?,
                    None => DEFAULT_HISTORY_TICKS,
                };
                let unlimited =
                    self.players[player].has_explicit_permission(UNLIMITED_HISTORY_PERMISSION);
                self.validate_player_history_limit(player, capacity)?;
                let projected = self.world.enable_history(
                    capacity,
                    unlimited || crate::permissions::dedicated_permissions(),
                )?;
                self.reset_timings();
                Ok(messages::history_enabled(
                    capacity,
                    format_memory(projected),
                ))
            }
            _ => Err(messages::USAGE_RHISTORY_ON_TICKS_OFF_STATUS.into()),
        }
    }

    pub(super) fn rewind_plot(&mut self, player: usize, args: &[&str]) -> Result<(), String> {
        self.authorize_history(player, "commands.rback", true)?;
        let ticks = match args {
            [] => 1,
            [value] => value
                .parse::<usize>()
                .map_err(|_| messages::SPECIFY_POSITIVE_NUMBER_GAME_TICKS.to_owned())?,
            _ => return Err(messages::USAGE_RBACK_TICKS.into()),
        };
        if self.redpiler.is_active() {
            return Err(messages::TICK_REWIND_ONLY_AVAILABLE_DURING_INTERPRETED.into());
        }
        let unlimited = self.players[player].has_explicit_permission(UNLIMITED_HISTORY_PERMISSION);
        self.validate_player_history_limit(player, ticks)?;
        self.world.rewind_ticks(
            ticks,
            unlimited || crate::permissions::dedicated_permissions(),
        )?;
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
                player.send_system_message(messages::WORLDEDIT_UNDO_REDO_CLEARED_AFTER_TICK);
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
            let packet = chunk
                .encode_packet_for_client(self.world.fast_rendering || self.world.screen_only());
            for player in visible {
                player.client.send_packet(&packet);
            }
        }
        self.broadcast_plot_chat_message(&messages::plot_rewound(ticks, self.world.history.len()));
        Ok(())
    }
}
