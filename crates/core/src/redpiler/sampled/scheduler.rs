use super::executor::ScheduledComponentTick;
use crate::world::scheduler::NUM_QUEUES;
use mchprs_world::TickPriority;
use std::num::NonZeroUsize;

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

pub(super) struct Scheduler {
    /// fixed-capacity tick slots, reused through the free list.
    slots: Vec<Slot>,
    /// head of the free-slot chain, encoded as its index plus one.
    free: Option<NonZeroUsize>,
    /// ring buckets by delay and native tick priority.
    queues: [[Queue; TickPriority::COUNT]; NUM_QUEUES],
    /// current ring position representing this tick's delay-zero queue.
    pos: usize,
    /// maximum number of allocated slot records accepted by this scheduler.
    limit: usize,
}

impl Scheduler {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            slots: Vec::with_capacity(limit),
            free: None,
            queues: [[Queue::default(); TickPriority::COUNT]; NUM_QUEUES],
            pos: 0,
            limit,
        }
    }

    pub(super) fn reset(&mut self, limit: usize) {
        self.slots.clear();
        self.slots.reserve(limit);
        self.free = None;
        self.queues = [[Queue::default(); TickPriority::COUNT]; NUM_QUEUES];
        self.pos = 0;
        self.limit = limit;
    }

    pub(super) fn schedule_half_tick(&mut self, tick: ScheduledComponentTick, delay: usize, priority: TickPriority) -> Result<(), &'static str> {
        if delay >= NUM_QUEUES || (delay == 0 && priority != TickPriority::NanoTick) {
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
            NonZeroUsize::new(self.slots.len()).unwrap()
        };
        let queue = &mut self.queues[(self.pos + delay) % NUM_QUEUES][priority as usize];
        if let Some(last) = queue.last {
            self.slots[last.get() - 1].next = Some(slot);
        } else {
            queue.first = Some(slot);
        }
        queue.last = Some(slot);
        Ok(())
    }

    pub(super) fn pop_first(&mut self) -> Option<ScheduledComponentTick> {
        for queue in &mut self.queues[self.pos] {
            if let Some(slot) = queue.first {
                queue.first = self.slots[slot.get() - 1].next;
                if queue.first.is_none() {
                    queue.last = None;
                }
                let tick = self.slots[slot.get() - 1].tick;
                self.slots[slot.get() - 1].next = self.free;
                self.free = Some(slot);
                return Some(tick);
            }
        }
        None
    }

    pub(super) fn end_last_tick_move_next(&mut self) {
        debug_assert!(self.queues[self.pos].iter().all(|queue| queue.first.is_none()));
        self.pos = (self.pos + 1) % NUM_QUEUES;
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (&ScheduledComponentTick, usize, TickPriority)> {
        (0..NUM_QUEUES).flat_map(move |delay| {
            TickPriority::ALL.into_iter().flat_map(move |priority| {
                let mut slot = self.queues[(self.pos + delay) % NUM_QUEUES][priority as usize].first;
                std::iter::from_fn(move || {
                    let current = slot?;
                    slot = self.slots[current.get() - 1].next;
                    Some((&self.slots[current.get() - 1].tick, delay, priority))
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
            assert_eq!(compiled.iter().collect::<Vec<_>>(), native.iter().collect::<Vec<_>>());
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
