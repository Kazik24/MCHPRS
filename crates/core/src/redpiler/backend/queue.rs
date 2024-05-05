use std::mem;

use mchprs_blocks::blocks::Block;
use mchprs_blocks::BlockPos;
use mchprs_world::{TickEntry, TickPriority};
use tracing::warn;

use crate::world::World;

use super::direct::node::NodeId;

const NUM_QUEUES: usize = 32;

#[derive(Debug, Clone)]
pub struct Queues<T>([Vec<T>; TickPriority::COUNT]);

impl<T> Queues<T> {
    pub fn drain_iter(&mut self) -> impl Iterator<Item = T> + '_ {
        self.0.each_mut().into_iter().flat_map(|q| q.drain(..))
    }
    #[inline]
    pub fn len(&self) -> usize {
        self.0.iter().map(|v| v.len()).sum()
    }
    #[inline]
    pub fn count_for(&self, priority: TickPriority) -> usize {
        self.0[priority as usize].len()
    }
    #[inline]
    pub fn is_empty_for(&self, priority: TickPriority) -> bool {
        self.count_for(priority) == 0
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|q| q.is_empty())
    }

    pub fn pop_first(&mut self) -> Option<T> {
        for queue in &mut self.0 {
            if !queue.is_empty() {
                return Some(queue.remove(0)); //no need to be fast for now
            }
        }
        None
    }

    pub fn clear(&mut self) {
        self.0.iter_mut().for_each(|q| q.clear());
    }

    pub fn contains(&self, node: &T) -> bool
    where
        T: PartialEq,
    {
        self.0.iter().any(|q| q.contains(node))
    }

    pub fn iter(&self) -> impl Iterator<Item = (&T, TickPriority)> {
        self.0
            .each_ref()
            .into_iter()
            .enumerate()
            .flat_map(|(i, q)| {
                let priority = TickPriority::ALL[i];
                q.iter().map(move |n| (n, priority))
            })
    }
}

//todo use this tick scheduler also for interpreted backend
#[derive(Debug, Clone)]
pub struct TickScheduler<T> {
    queues_deque: [Queues<T>; NUM_QUEUES],
    pos: usize,
}

impl<T> Default for Queues<T> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<T> Default for TickScheduler<T> {
    fn default() -> Self {
        Self {
            queues_deque: Default::default(),
            pos: 0,
        }
    }
}

impl TickScheduler<NodeId> {
    pub fn reset<W: World>(&mut self, world: &mut W, blocks: &[Option<(BlockPos, Block)>]) {
        for (node, delay, priority) in self.iter() {
            let Some((pos, _)) = blocks[node.index()] else {
                warn!(
                    "Cannot schedule tick for node {node:?} because block information is missing"
                );
                continue;
            };
            world.schedule_half_tick(pos, delay as u32, priority);
        }
        // for (idx, queues) in self.queues_deque.iter().enumerate() {
        //     let delay = if self.pos >= idx {
        //         idx + NUM_QUEUES
        //     } else {
        //         idx
        //     } - self.pos;
        //     for (entries, priority) in queues.0.iter().zip(TickPriority::ALL) {
        //         for node in entries {
        //             let Some((pos, _)) = blocks[node.index()] else {
        //                 warn!("Cannot schedule tick for node {node:?} because block information is missing");
        //                 continue;
        //             };
        //             world.schedule_tick(pos, delay as u32, priority);
        //         }
        //     }
        // }
        self.clear();
    }
}

impl TickScheduler<BlockPos> {
    pub fn iter_entries(&self) -> impl Iterator<Item = TickEntry> + '_ where {
        self.iter().map(|(pos, d, p)| TickEntry {
            pos: *pos,
            ticks_left: d as u32,
            tick_priority: p,
        })
    }
}

impl FromIterator<TickEntry> for TickScheduler<BlockPos> {
    fn from_iter<T: IntoIterator<Item = TickEntry>>(iter: T) -> Self {
        let mut scheduler = Self::default();
        for entry in iter {
            scheduler.schedule_half_tick(entry.pos, entry.ticks_left as usize, entry.tick_priority);
        }
        scheduler
    }
}

impl<T> TickScheduler<T> {
    #[inline]
    pub fn schedule_tick(&mut self, node: T, delay: usize, priority: TickPriority) {
        self.schedule_half_tick(node, delay * 2, priority);
    }

    #[inline]
    pub fn schedule_half_tick(&mut self, node: T, delay: usize, priority: TickPriority) {
        debug_assert!(delay < NUM_QUEUES);
        if delay == 0 {
            debug_assert!(
                priority == TickPriority::NanoTick,
                "0 delay can only be scheduled with NanoTick priority"
            );
        }
        self.queues_deque[(self.pos + delay) % NUM_QUEUES].0[priority as usize].push(node);
    }

    fn next_pos(&self) -> usize {
        (self.pos + 1) % NUM_QUEUES
    }

    // for redpiler tick logic
    pub fn queues_this_tick_move_next(&mut self) -> Queues<T> {
        self.pos = self.next_pos();
        mem::take(&mut self.queues_deque[self.pos])
    }

    // for interpreted ticks
    pub fn this_tick_ref(&self) -> &Queues<T> {
        &self.queues_deque[self.pos]
    }
    // for interpreted ticks
    pub fn this_tick(&mut self) -> &mut Queues<T> {
        &mut self.queues_deque[self.pos]
    }
    // for interpreted ticks
    pub fn end_last_tick_move_next(&mut self) {
        self.this_tick().clear();
        self.pos = self.next_pos();
    }

    pub fn contains(&self, node: &T) -> bool
    where
        T: PartialEq,
    {
        self.queues_iter().any(|q| q.contains(node))
    }

    pub fn queues_iter(&self) -> impl Iterator<Item = &Queues<T>> {
        let mut i = 0;
        std::iter::from_fn(move || {
            if i < NUM_QUEUES {
                let idx = (i + self.pos) % NUM_QUEUES;
                let queue = &self.queues_deque[idx];
                i += 1;
                Some(queue)
            } else {
                None
            }
        })
    }

    pub fn iter(&self) -> impl Iterator<Item = (&T, usize, TickPriority)> {
        self.queues_iter()
            .enumerate()
            .flat_map(|(d, q)| q.iter().map(move |(n, p)| (n, d, p)))
    }

    pub fn end_tick(&mut self, mut queues: Queues<T>) {
        queues.clear();
        self.queues_deque[self.pos] = queues;
    }

    pub fn clear(&mut self) {
        self.queues_deque.iter_mut().for_each(|q| q.clear());
    }
}

#[cfg(test)]
mod tests {
    use std::iter;

    use super::*;
    use rand::prelude::*;

    #[test]
    fn test_reset_queue() {
        let rng = &mut StdRng::seed_from_u64(123456);
        let mut sch = TickScheduler::default();

        let entries = iter::repeat_with(|| TickEntry {
            pos: BlockPos::new(
                rng.gen_range(0..128),
                rng.gen_range(0..128),
                rng.gen_range(0..128),
            ),
            ticks_left: rng.gen_range(0..16),
            tick_priority: TickPriority::ALL[rng.gen_range(0..TickPriority::COUNT)],
        });
        let entries = entries.take(1000).collect::<Vec<_>>();
        for e in &entries {
            sch.schedule_tick(e.pos, e.ticks_left as usize, e.tick_priority);
        }
    }

    #[test]
    fn test_zero_tick() {
        let mut sch = TickScheduler::default();
        let pos = BlockPos::new(1, 1, 1);

        sch.schedule_tick(pos, 0, TickPriority::NanoTick);
        assert_eq!(Some(pos), sch.this_tick().pop_first())
    }
}
