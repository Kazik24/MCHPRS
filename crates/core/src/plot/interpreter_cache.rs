use mchprs_blocks::BlockPos;
use mchprs_world::{PistonEvent, PistonState};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::redpiler::backend::{ScheduledBlockTick, TickScheduler};

/// Membership only: the scheduler retains priority, FIFO and ring ordering.
/// Counts also preserve duplicate entries imported from older saves.
pub(super) struct TickIndex {
    pending: FxHashMap<ScheduledBlockTick, usize>,
    dirty: bool,
}

impl Default for TickIndex {
    fn default() -> Self {
        Self {
            pending: Default::default(),
            dirty: true,
        }
    }
}

impl TickIndex {
    /// Rebuild lazily after loading, history restoration or compiled handoff.
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }

    pub fn contains(
        &mut self,
        scheduler: &TickScheduler<ScheduledBlockTick>,
        tick: ScheduledBlockTick,
    ) -> bool {
        if self.dirty {
            self.pending.clear();
            for (&node, _, _) in scheduler.iter() {
                *self.pending.entry(node).or_default() += 1;
            }
            self.dirty = false;
        }
        self.pending.contains_key(&tick)
    }

    pub fn pushed(&mut self, tick: ScheduledBlockTick) {
        if !self.dirty {
            *self.pending.entry(tick).or_default() += 1;
        }
    }

    pub fn popped(&mut self, tick: ScheduledBlockTick) {
        if !self.dirty {
            let count = self.pending.get_mut(&tick).expect("tick index out of sync");
            *count -= 1;
            if *count == 0 {
                self.pending.remove(&tick);
            }
        }
    }
}

/// Derived indexes only; the ordered serialized collections remain authoritative.
pub(super) struct PistonIndex {
    motions: FxHashMap<BlockPos, usize>,
    duplicates: FxHashSet<BlockPos>,
    events: FxHashMap<PistonEvent, usize>,
    motions_dirty: bool,
    events_dirty: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use mchprs_world::{PistonAction, PistonMotion};

    fn motion(pos: BlockPos, identity: u64) -> PistonMotion {
        PistonMotion {
            pos,
            identity,
            progress: 0.0,
            previous_progress: 0.0,
            last_tick: 0,
            carried_entity: None,
        }
    }

    #[test]
    fn tick_membership_matches_ring_through_wrap_duplicates_and_restore() {
        use mchprs_world::TickPriority;
        let mut scheduler = TickScheduler::default();
        let mut index = TickIndex::default();
        let a = ScheduledBlockTick {
            pos: BlockPos::new(1, 2, 3),
            block_type: Some(1),
        };
        let b = ScheduledBlockTick {
            block_type: Some(2),
            ..a
        };
        let legacy = ScheduledBlockTick {
            block_type: None,
            ..a
        };
        for turn in 0..70 {
            for (node, delay, priority) in [
                (a, 0, TickPriority::Normal),
                (a, 31, TickPriority::Highest),
                (b, 2, TickPriority::Higher),
                (legacy, 1, TickPriority::High),
            ] {
                // Imported duplicates are permitted by the generic scheduler.
                assert_eq!(index.contains(&scheduler, node), scheduler.contains(&node));
                scheduler.schedule_half_tick(node, delay, priority);
                index.pushed(node);
            }
            while let Some(node) = scheduler.this_tick().pop_first() {
                index.popped(node);
                for queried in [a, b, legacy] {
                    assert_eq!(
                        index.contains(&scheduler, queried),
                        scheduler.contains(&queried)
                    );
                }
            }
            scheduler.end_last_tick_move_next();
            if turn == 35 {
                // History/load can replace the scheduler independently of the index.
                scheduler = scheduler.iter_entries().collect();
                index.invalidate();
            }
        }
        scheduler.clear();
        index.invalidate();
        assert!(!index.contains(&scheduler, a));
        // A consumed request no longer prevents immediate rescheduling.
        scheduler.schedule_half_tick(a, 0, TickPriority::Normal);
        index.pushed(a);
        assert!(index.contains(&scheduler, a));
        index.popped(scheduler.this_tick().pop_first().unwrap());
        assert!(!index.contains(&scheduler, a));
    }

    #[test]
    fn wrapped_motion_deque_keeps_vec_save_bytes_and_order() {
        let mut state = PistonState::default();
        for i in 0..20 {
            state
                .motions
                .push_back(motion(BlockPos::new(i, 1, 1), i as u64));
        }
        for _ in 0..state.motions.capacity() - 5 {
            let m = state.motions.pop_front().unwrap();
            state.motions.push_back(m);
        }
        assert!(!state.motions.as_slices().1.is_empty());
        state.motions[0].carried_entity = Some(Box::new(
            mchprs_blocks::block_entities::BlockEntity::Comparator { output_strength: 7 },
        ));
        state.logical_tick = 40;
        state.phase = mchprs_world::AdvancePhase::MovingEntities;
        state.scheduled_advanced = true;
        state.next_identity = 20;
        state.movement_work = state.motions.iter().map(|m| (m.pos, m.identity)).collect();
        state.movement_cursor = 3;
        // Bincode encodes structs/tuples identically; this is the old Vec layout.
        let legacy = (
            state.logical_tick,
            state.phase,
            state.scheduled_advanced,
            &state.events,
            state.motions.iter().cloned().collect::<Vec<_>>(),
            state.next_identity,
            &state.movement_work,
            state.movement_cursor,
        );
        let old_bytes = bincode::serialize(&legacy).unwrap();
        assert_eq!(bincode::serialize(&state).unwrap(), old_bytes);
        let restored: PistonState = bincode::deserialize(&old_bytes).unwrap();
        assert_eq!(bincode::serialize(&restored).unwrap(), old_bytes);
    }

    #[test]
    fn ordered_removals_replacements_and_external_mutations_match_linear_lookup() {
        let mut s = PistonState::default();
        for i in 0..100 {
            s.motions
                .push_back(motion(BlockPos::new(i, 1, 1), i as u64));
        }
        let mut cache = PistonIndex::default();
        for removed in [33, 0, 99, 47, 4] {
            for i in 0..100 {
                let pos = BlockPos::new(i, 1, 1);
                assert_eq!(
                    cache.motion(&s, pos, Some(i as u64)),
                    s.motions.iter().position(|m| m.pos == pos)
                );
            }
            let pos = BlockPos::new(removed, 1, 1);
            s.motions.retain(|m| m.pos != pos);
            cache.removed(pos);
            assert!(cache.motion(&s, pos, None).is_none());
            s.motions.push_back(motion(pos, 1000));
            cache.inserted(pos, s.motions.len() - 1);
            assert!(cache.motion(&s, pos, Some(removed as u64)).is_none());
            assert_eq!(cache.motion(&s, pos, Some(1000)), Some(s.motions.len() - 1));
            s.motions.pop_back();
            cache.removed(pos);
        }
        s.motions.make_contiguous().reverse();
        cache.invalidate();
        for m in &s.motions {
            assert_eq!(
                cache.motion(&s, m.pos, Some(m.identity)),
                s.motions.iter().position(|a| a.identity == m.identity)
            );
        }
        // Shrink through the linear/indexed boundary, then grow back through it.
        while !s.motions.is_empty() {
            let removed = s.motions.pop_front().unwrap();
            cache.removed(removed.pos);
            for m in &s.motions {
                assert_eq!(
                    cache.motion(&s, m.pos, Some(m.identity)),
                    s.motions.iter().position(|a| a.identity == m.identity)
                );
            }
        }
        for i in 0..20 {
            let pos = BlockPos::new(i, 1, 1);
            s.motions.push_back(motion(pos, i as u64));
            cache.inserted(pos, s.motions.len() - 1);
            assert_eq!(cache.motion(&s, pos, Some(i as u64)), Some(i as usize));
        }
    }

    #[test]
    fn duplicate_legacy_motions_and_events_keep_original_membership() {
        let pos = BlockPos::new(1, 2, 3);
        let mut s = PistonState::default();
        s.motions = vec![motion(pos, 1), motion(pos, 2)].into();
        s.motions
            .extend((0..9).map(|i| motion(BlockPos::new(i, 3, 4), i as u64 + 3)));
        let e = PistonEvent {
            pos,
            sticky: true,
            facing: mchprs_blocks::BlockFace::North,
            action: PistonAction::Extend,
        };
        s.events.extend([e, e]);
        s.events.extend((0..8).map(|i| PistonEvent {
            pos: BlockPos::new(i, 3, 4),
            ..e
        }));
        let mut cache = PistonIndex::default();
        assert_eq!(cache.motion(&s, pos, None), Some(0));
        assert_eq!(cache.motion(&s, pos, Some(2)), Some(1));
        assert!(cache.has_event(&s, e));
        s.events.pop_front();
        cache.popped(e);
        assert!(cache.has_event(&s, e));
        s.events.pop_front();
        cache.popped(e);
        assert!(!cache.has_event(&s, e));
    }
}

impl Default for PistonIndex {
    fn default() -> Self {
        Self {
            motions: Default::default(),
            duplicates: Default::default(),
            events: Default::default(),
            motions_dirty: true,
            events_dirty: true,
        }
    }
}

impl PistonIndex {
    pub fn invalidate(&mut self) {
        self.motions_dirty = true;
        self.events_dirty = true;
    }

    pub fn motion(
        &mut self,
        state: &PistonState,
        pos: BlockPos,
        identity: Option<u64>,
    ) -> Option<usize> {
        // Tiny banks are cheaper to scan than to hash. Keep the derived index
        // dirty until a larger bank actually needs it.
        if state.motions.len() <= 8 {
            self.motions_dirty = true;
            return state
                .motions
                .iter()
                .position(|m| m.pos == pos && identity.is_none_or(|id| id == m.identity));
        }
        if self.motions_dirty {
            self.motions.clear();
            self.duplicates.clear();
            for (i, m) in state.motions.iter().enumerate() {
                if self.motions.contains_key(&m.pos) {
                    self.duplicates.insert(m.pos);
                } else {
                    self.motions.insert(m.pos, i);
                }
            }
            self.motions_dirty = false;
        }
        if self.duplicates.contains(&pos) {
            return state
                .motions
                .iter()
                .position(|m| m.pos == pos && identity.is_none_or(|id| id == m.identity));
        }
        if state.motions.front().is_some_and(|m| m.pos == pos) {
            // Most completions consume the ordered front. Its old hash-map
            // index may have shifted after earlier removals in this phase.
            return identity
                .is_none_or(|id| state.motions[0].identity == id)
                .then_some(0);
        }
        let mut i = *self.motions.get(&pos)?;
        // Ordered removal shifts logical indices. Repair only the queried entry;
        // completing a phase in order usually finds its next motion at index 0.
        if !state.motions.get(i).is_some_and(|m| m.pos == pos) {
            i = state.motions.iter().position(|m| m.pos == pos)?;
            self.motions.insert(pos, i);
        }
        identity
            .is_none_or(|id| state.motions[i].identity == id)
            .then_some(i)
    }

    pub fn inserted(&mut self, pos: BlockPos, i: usize) {
        if !self.motions_dirty {
            self.motions.insert(pos, i);
        }
    }

    /// Call after motion lookup, which populates the index for larger banks.
    pub fn has_duplicate_motions(&self, pos: BlockPos) -> bool {
        self.duplicates.contains(&pos)
    }

    pub fn removed(&mut self, pos: BlockPos) {
        if self.motions_dirty {
            return;
        }
        self.motions.remove(&pos);
        self.duplicates.remove(&pos);
        if !self.duplicates.is_empty() {
            self.motions_dirty = true;
        }
    }

    pub fn has_event(&mut self, state: &PistonState, event: PistonEvent) -> bool {
        if state.events.len() <= 8 {
            self.events_dirty = true;
            return state.events.contains(&event);
        }
        if self.events_dirty {
            self.events.clear();
            for &e in &state.events {
                *self.events.entry(e).or_default() += 1;
            }
            self.events_dirty = false;
        }
        self.events.contains_key(&event)
    }

    pub fn pushed(&mut self, event: PistonEvent) {
        if !self.events_dirty {
            *self.events.entry(event).or_default() += 1;
        }
    }

    pub fn popped(&mut self, event: PistonEvent) {
        if self.events_dirty {
            return;
        }
        if let Some(count) = self.events.get_mut(&event) {
            *count -= 1;
            if *count == 0 {
                self.events.remove(&event);
            }
        }
    }
}
