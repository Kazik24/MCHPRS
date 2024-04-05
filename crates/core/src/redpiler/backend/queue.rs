use std::mem;

use mchprs_blocks::blocks::Block;
use mchprs_blocks::BlockPos;
use mchprs_world::{TickEntry, TickPriority};
use tracing::warn;

use crate::world::World;

use super::direct::node::NodeId;

const NUM_PRIORITIES: usize = 4;
const NUM_QUEUES: usize = 16;

#[derive(Debug, Clone)]
pub struct Queues<T>([Vec<T>; NUM_PRIORITIES]);

impl<T> Queues<T> {
    pub fn drain_iter(&mut self) -> impl Iterator<Item = T> + '_ {
        let [q0, q1, q2, q3] = &mut self.0;
        let [q0, q1, q2, q3] = [q0, q1, q2, q3].map(|q| q.drain(..));
        q0.chain(q1).chain(q2).chain(q3)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&T, TickPriority)> + '_ {
        let [q0, q1, q2, q3] = &self.0;
        let q1 = q1.iter().map(|q| (q, TickPriority::Highest));
        let q2 = q2.iter().map(|q| (q, TickPriority::Higher));
        let q3 = q3.iter().map(|q| (q, TickPriority::High));
        let q4 = q0.iter().map(|q| (q, TickPriority::Normal));
        q1.chain(q2).chain(q3).chain(q4)
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
        for (idx, queues) in self.queues_deque.iter().enumerate() {
            let delay = if self.pos >= idx {
                idx + NUM_QUEUES
            } else {
                idx
            } - self.pos;
            for (entries, priority) in queues.0.iter().zip(Self::priorities()) {
                for node in entries {
                    let Some((pos, _)) = blocks[node.index()] else {
                        warn!("Cannot schedule tick for node {:?} because block information is missing", node);
                        continue;
                    };
                    world.schedule_tick(pos, delay as u32, priority);
                }
            }
        }
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
            scheduler.schedule_tick(entry.pos, entry.ticks_left as usize, entry.tick_priority);
        }
        scheduler
    }
}

impl<T> TickScheduler<T> {
    #[inline]
    pub fn schedule_tick(&mut self, node: T, delay: usize, priority: TickPriority) {
        self.queues_deque[(self.pos + delay) % NUM_QUEUES].0[priority as usize].push(node);
    }

    fn next_pos(&self) -> usize {
        (self.pos + 1) % NUM_QUEUES
    }

    pub fn queues_this_tick_move_next(&mut self) -> Queues<T> {
        self.pos = self.next_pos();
        mem::take(&mut self.queues_deque[self.pos])
    }

    // for interpreted ticks
    pub fn pop_one_this_tick(&mut self) -> Option<T> {
        for queue in &mut self.queues_deque[self.pos].0 {
            if !queue.is_empty() {
                return Some(queue.remove(0)); //no need to be fast for now
            }
        }
        None
    }

    // for interpreted ticks
    pub fn end_last_tick_move_next(&mut self) {
        for queue in &mut self.queues_deque[self.pos].0 {
            queue.clear();
        }
        self.pos = self.next_pos();
    }

    pub fn contains(&self, node: &T) -> bool
    where
        T: PartialEq,
    {
        for queues in &self.queues_deque {
            for queue in &queues.0 {
                if queue.contains(node) {
                    return true;
                }
            }
        }
        false
    }

    pub fn queues_iter(&self) -> impl Iterator<Item = &Queues<T>> + '_ {
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

    pub fn iter(&self) -> impl Iterator<Item = (&T, usize, TickPriority)> + '_ {
        self.queues_iter()
            .enumerate()
            .flat_map(|(d, q)| q.iter().map(move |(n, p)| (n, d, p)))
    }

    pub fn end_tick(&mut self, mut queues: Queues<T>) {
        for queue in &mut queues.0 {
            queue.clear();
        }
        self.queues_deque[self.pos] = queues;
    }

    pub fn clear(&mut self) {
        for queues in &mut self.queues_deque {
            for queue in &mut queues.0 {
                queue.clear();
            }
        }
    }

    fn priorities() -> [TickPriority; NUM_PRIORITIES] {
        [
            TickPriority::Highest,
            TickPriority::Higher,
            TickPriority::High,
            TickPriority::Normal,
        ]
    }
}
