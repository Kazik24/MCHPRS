use super::ScratchError;
use crate::world::scheduler::NUM_QUEUES;
use mchprs_world::TickPriority;
use std::num::NonZeroUsize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ScheduledComponentTick {
    /// component index into `Program::positions` whose scheduled tick will run.
    pub(super) component: usize,
    /// block identity used to discard stale ticks after the component changes type.
    pub(super) block_type: Option<u32>,
}

pub(super) struct PendingTick<'a> {
    pub(super) tick: &'a ScheduledComponentTick,
    pub(super) delay_half_ticks: usize,
    pub(super) priority: TickPriority,
}

#[derive(Clone, Copy, Default)]
struct Queue {
    /// first slot in this FIFO chain, encoded as its index plus one.
    first: Option<NonZeroUsize>,
    /// last slot in this FIFO chain, encoded as its index plus one.
    last: Option<NonZeroUsize>,
}

struct Slot {
    /// component tick stored in this slot.
    tick: ScheduledComponentTick,
    /// next FIFO slot or next free-list slot, encoded as its index plus one.
    next: Option<NonZeroUsize>,
}

/// A bounded ring of FIFO queues, ordered by delay and then tick priority.
///
/// Slots are linked by one-based indices so removed ticks can be reused without
/// allocating. Delay zero represents the current half-tick.
pub(super) struct Scheduler {
    /// fixed-capacity tick slots, reused through the free list.
    slots: Vec<Slot>,
    /// head of the free-slot chain, encoded as its index plus one.
    free: Option<NonZeroUsize>,
    /// ring buckets by delay and native tick priority.
    queues: [[Queue; TickPriority::COUNT]; NUM_QUEUES],
    /// current ring position representing this tick's delay-zero queue.
    current_bucket: usize,
    /// maximum number of allocated slot records accepted by this scheduler.
    limit: usize,
}

impl Scheduler {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            slots: Vec::with_capacity(limit),
            free: None,
            queues: [[Queue::default(); TickPriority::COUNT]; NUM_QUEUES],
            current_bucket: 0,
            limit,
        }
    }

    pub(super) fn reset(&mut self, limit: usize) {
        self.slots.clear();
        self.slots.reserve(limit);
        self.free = None;
        self.queues = [[Queue::default(); TickPriority::COUNT]; NUM_QUEUES];
        self.current_bucket = 0;
        self.limit = limit;
    }

    /// Append a tick to its delay and priority queue, or return an error without scheduling it.
    ///
    /// Inputs: The scheduled component tick, delay and tick priority.
    ///
    /// Validate delay, reuse a free slot or allocate below the limit, then append the slot to the selected
    /// FIFO chain in the timing ring.
    pub(super) fn schedule_half_tick(
        &mut self,
        tick: ScheduledComponentTick,
        delay_half_ticks: usize,
        priority: TickPriority,
    ) -> Result<(), &'static str> {
        if delay_half_ticks >= NUM_QUEUES || (delay_half_ticks == 0 && priority != TickPriority::NanoTick) {
            return Err("invalid runtime scheduled delay or priority");
        }

        let slot = if let Some(slot) = self.free {
            self.free = self.slots[slot.get() - 1].next;
            self.slots[slot.get() - 1] = Slot { tick, next: None };
            slot
        } else {
            if self.slots.len() == self.limit {
                return Err("runtime scheduled tick capacity exceeded");
            }
            self.slots.push(Slot { tick, next: None });
            NonZeroUsize::new(self.slots.len()).expect("a newly pushed slot has a nonzero one-based index")
        };
        let queue = &mut self.queues[(self.current_bucket + delay_half_ticks) % NUM_QUEUES][priority as usize];
        if let Some(last) = queue.last {
            self.slots[last.get() - 1].next = Some(slot);
        } else {
            queue.first = Some(slot);
        }
        queue.last = Some(slot);
        Ok(())
    }

    /// Remove the next tick in the current half-tick, respecting priority and FIFO order.
    ///
    /// Inputs: This scheduler's current priority buckets and free-slot chain.
    ///
    /// Select the first nonempty priority queue, detach its head, repair the tail when emptied, and return
    /// the slot to the free list.
    pub(super) fn pop_first(&mut self) -> Option<ScheduledComponentTick> {
        for queue in &mut self.queues[self.current_bucket] {
            let Some(slot_id) = queue.first else {
                continue;
            };
            let slot = &mut self.slots[slot_id.get() - 1];
            queue.first = slot.next;
            if queue.first.is_none() {
                queue.last = None;
            }
            let tick = slot.tick;
            slot.next = self.free;
            self.free = Some(slot_id);
            return Some(tick);
        }
        None
    }

    /// Advance the ring after all ticks in the current half-tick have been drained.
    pub(super) fn end_last_tick_move_next(&mut self) {
        debug_assert!(self.queues[self.current_bucket].iter().all(|queue| queue.first.is_none()));
        self.current_bucket = (self.current_bucket + 1) % NUM_QUEUES;
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = PendingTick<'_>> {
        (0..NUM_QUEUES).flat_map(move |delay_half_ticks| {
            TickPriority::ALL.into_iter().flat_map(move |priority| {
                let mut slot = self.queues[(self.current_bucket + delay_half_ticks) % NUM_QUEUES][priority as usize].first;
                std::iter::from_fn(move || {
                    let current = slot?;
                    slot = self.slots[current.get() - 1].next;
                    Some(PendingTick {
                        tick: &self.slots[current.get() - 1].tick,
                        delay_half_ticks,
                        priority,
                    })
                })
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::scheduler::TickScheduler;

    #[test]
    fn slot_links_use_one_word() {
        use std::mem::size_of;
        assert_eq!(size_of::<Option<NonZeroUsize>>(), size_of::<usize>());
        assert_eq!(size_of::<Queue>(), 2 * size_of::<usize>());
        assert_eq!(size_of::<Slot>(), size_of::<ScheduledComponentTick>() + size_of::<usize>());
        assert!(size_of::<Slot>() < size_of::<(ScheduledComponentTick, Option<usize>)>());
    }

    #[test]
    fn rejected_delays_leave_queued_work_unchanged() {
        let mut scheduler = Scheduler::new(1);
        let tick = ScheduledComponentTick {
            component: 7,
            block_type: Some(1),
        };
        scheduler.schedule_half_tick(tick, 0, TickPriority::NanoTick).unwrap();
        assert!(scheduler.schedule_half_tick(tick, 0, TickPriority::Normal).is_err());
        assert!(scheduler.schedule_half_tick(tick, NUM_QUEUES, TickPriority::NanoTick).is_err());
        assert_eq!(scheduler.pop_first(), Some(tick));
        assert_eq!(scheduler.pop_first(), None);
    }

    #[test]
    fn component_queues_match_native_priorities_fifo_and_ring_rotations() {
        let mut compiled = Scheduler::new(256);
        let mut native = TickScheduler::default();
        for tick in 0..128 {
            for offset in 0..4 {
                let delay = (tick + offset * 7) % NUM_QUEUES;
                let priority = if delay == 0 { TickPriority::NanoTick } else { TickPriority::ALL[offset] };
                let entry = ScheduledComponentTick {
                    component: tick * 4 + offset,
                    block_type: Some(1),
                };
                compiled.schedule_half_tick(entry, delay, priority).unwrap();
                native.schedule_half_tick(entry, delay, priority);
            }
            for phase in 0..2 {
                if phase == 1 {
                    compiled.end_last_tick_move_next();
                    native.end_last_tick_move_next();
                }
                loop {
                    let actual = compiled.pop_first();
                    assert_eq!(actual, native.this_tick().pop_first());
                    let Some(entry) = actual else { break };
                    if entry.component % 7 == 0 {
                        let same_phase = ScheduledComponentTick {
                            component: entry.component + 1,
                            ..entry
                        };
                        compiled.schedule_half_tick(same_phase, 0, TickPriority::NanoTick).unwrap();
                        native.schedule_half_tick(same_phase, 0, TickPriority::NanoTick);
                    }
                }
            }
            let compiled_entries: Vec<_> = compiled
                .iter()
                .map(|entry| (entry.tick, entry.delay_half_ticks, entry.priority))
                .collect();
            assert_eq!(compiled_entries, native.iter().collect::<Vec<_>>());
        }
        assert_eq!(compiled.slots.capacity(), 256);
    }

    #[test]
    fn exhausted_slots_fail_without_dropping_work_and_are_reused() {
        let mut scheduler = Scheduler::new(2);
        let entry = ScheduledComponentTick {
            component: 0,
            block_type: Some(1),
        };
        for _ in 0..2 {
            scheduler.schedule_half_tick(entry, 0, TickPriority::NanoTick).unwrap();
        }
        assert!(scheduler.schedule_half_tick(entry, 0, TickPriority::NanoTick).is_err());
        assert_eq!(scheduler.pop_first(), Some(entry));
        scheduler.schedule_half_tick(entry, 0, TickPriority::NanoTick).unwrap();
        assert_eq!(scheduler.pop_first(), Some(entry));
        assert_eq!(scheduler.pop_first(), Some(entry));
        assert_eq!(scheduler.pop_first(), None);
        assert_eq!(scheduler.slots.len(), 2);
        assert!(scheduler.schedule_half_tick(entry, NUM_QUEUES, TickPriority::Normal).is_err());
        scheduler.schedule_half_tick(entry, 1, TickPriority::Normal).unwrap();
        scheduler.reset(1);
        assert_eq!(scheduler.pop_first(), None);
        assert_eq!(scheduler.iter().count(), 0);
        scheduler.schedule_half_tick(entry, 0, TickPriority::NanoTick).unwrap();
        assert_eq!(scheduler.pop_first(), Some(entry));
        assert_eq!(scheduler.pop_first(), None);
    }
}

impl super::Runtime {
    pub(in crate::redpilator) fn set_pending(&mut self, ticks: Vec<mchprs_world::TickEntry>) -> Result<(), ScratchError> {
        // Pending work has a fixed admission limit; exhaustion is an execution error.
        let limit = self
            .blocks
            .len()
            .checked_mul(2)
            .and_then(|n| n.checked_add(ticks.len()))
            .ok_or(ScratchError {
                position: None,
                reason: "runtime scheduled tick capacity overflow",
            })?
            .max(64);
        self.set_pending_with_capacity(ticks, limit)
    }

    /// Import host scheduled ticks into the runtime's bounded scheduler.
    ///
    /// Inputs: Host TickEntry records, the slot limit and this program's component-position index.
    ///
    /// Reset pending state, assign extra IDs to passive positions, enqueue every original delay/priority,
    /// and count only ticks matching a live component's block identity.
    pub(in crate::redpilator) fn set_pending_with_capacity(&mut self, ticks: Vec<mchprs_world::TickEntry>, limit: usize) -> Result<(), ScratchError> {
        self.pending.reset(limit);
        self.pending_counts.fill(0);
        self.pending_passive_positions.clear();
        let mut indices = self.program.component_indices.clone();
        for tick in ticks {
            let component = *indices.entry(tick.pos).or_insert_with(|| {
                let id = self.blocks.len() + self.pending_passive_positions.len();
                self.pending_passive_positions.push(tick.pos);
                id
            });
            self.pending
                .schedule_half_tick(
                    ScheduledComponentTick {
                        component,
                        block_type: tick.block_type,
                    },
                    tick.ticks_left as usize,
                    tick.tick_priority,
                )
                .map_err(|reason| ScratchError {
                    position: Some(tick.pos),
                    reason,
                })?;
            if component < self.blocks.len() && tick.block_type == Some(self.blocks[component].registry_id()) {
                self.pending_counts[component] += 1;
            }
        }
        Ok(())
    }

    pub(in crate::redpilator) fn pending_entries(&self) -> Vec<mchprs_world::TickEntry> {
        self.pending
            .iter()
            .map(|entry| mchprs_world::TickEntry {
                pos: self.pending_position(entry.tick.component),
                block_type: entry.tick.block_type,
                ticks_left: entry.delay_half_ticks as u32,
                tick_priority: entry.priority,
            })
            .collect()
    }
}

impl super::Runtime {
    pub(super) fn has_pending(&self, tick: ScheduledComponentTick) -> bool {
        debug_assert_eq!(tick.block_type, Some(self.blocks[tick.component].registry_id()));
        self.pending_counts[tick.component] != 0
    }

    pub(super) fn schedule_tick(&mut self, tick: ScheduledComponentTick, delay_ticks: usize, priority: TickPriority) {
        self.schedule_half_tick(tick, delay_ticks * 2, priority);
    }

    pub(super) fn schedule_half_tick(&mut self, tick: ScheduledComponentTick, delay_half_ticks: usize, priority: TickPriority) {
        if let Err(reason) = self.pending.schedule_half_tick(tick, delay_half_ticks, priority) {
            self.failure = Some(ScratchError {
                position: Some(self.program.positions[tick.component]),
                reason,
            });
        } else {
            self.pending_counts[tick.component] += 1;
        }
    }

    pub(super) fn pending_position(&self, component: usize) -> mchprs_blocks::BlockPos {
        self.program
            .positions
            .get(component)
            .copied()
            .unwrap_or_else(|| self.pending_passive_positions[component - self.blocks.len()])
    }
}
