use super::PlotWorld;
use crate::world::World;
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::Block;
use mchprs_blocks::BlockPos;
use mchprs_network::packets::clientbound::{C3BMultiBlockChangeRecord, CMultiBlockChange};
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::BTreeMap;
use std::time::Instant;

#[derive(Default)]
pub(super) struct ScreenUpdates {
    dirty: BTreeMap<u32, FxHashSet<BlockPos>>,
    visible: FxHashMap<BlockPos, u32>,
    forced: FxHashSet<BlockPos>,
    pub authoritative: bool,
}

impl ScreenUpdates {
    pub fn contains(&self, pos: BlockPos) -> bool {
        self.visible.contains_key(&pos)
    }

    fn mark(&mut self, section: u32, pos: BlockPos, previous: u32) {
        self.visible.entry(pos).or_insert(previous);
        self.dirty.entry(section).or_default().insert(pos);
        if self.authoritative {
            self.forced.insert(pos);
        }
    }
}

impl PlotWorld {
    /// External edits must reach clients even when simulation rendering is filtered.
    /// Returns the previous mode so nested edits (e.g. a WorldEdit paste) restore it.
    pub(super) fn set_authoritative_updates(&mut self, enabled: bool) -> bool {
        self.screen_updates
            .as_mut()
            .map(|updates| std::mem::replace(&mut updates.authoritative, enabled))
            .unwrap_or(false)
    }

    pub fn screen_only(&self) -> bool {
        self.screen_updates.is_some()
    }

    /// Number of collected flushes, sections, and block records.
    pub fn visual_update_counts(&self) -> (u64, u64, u64) {
        let s = &self.update_stats;
        (s.flushes, s.sections, s.records)
    }

    /// Controls presentation only. Turning off sends every suppressed position's
    /// latest state from the authoritative storage overlay.
    pub fn set_screen_only(&mut self, enabled: bool) {
        if self.screen_only() == enabled {
            return;
        }
        if enabled {
            self.flush_block_changes();
            self.screen_updates = Some(Default::default());
            let moving: Vec<_> = self.piston_state.motions.iter().map(|m| m.pos).collect();
            for pos in moving {
                let state = self.screen_state(pos);
                if matches!(Block::from_id(state), Block::RedstoneLamp { .. }) {
                    self.track_screen_change(pos, self.get_block_raw(pos));
                }
            }
        } else {
            let tracked = self.screen_updates.take().unwrap();
            self.flush_block_changes();
            // A lamp already in motion when this mode was enabled can have a
            // projected pixel without a pending storage delta. Restore it too.
            if !self.fast_rendering {
                use mchprs_network::packets::clientbound::{CBlockChange, ClientBoundPacket};
                for pos in tracked.visible.keys().copied() {
                    if matches!(self.get_block(pos), Block::MovingPiston { .. }) {
                        let packet = CBlockChange {
                            pos: pos.packed(),
                            block_id: self.get_block_raw(pos) as i32,
                        }
                        .encode();
                        for sender in &self.packet_senders {
                            sender.send_packet(&packet);
                        }
                    }
                }
            }
        }
    }

    pub(super) fn reset_screen_tracking(&mut self) {
        if self.screen_only() {
            self.screen_updates = Some(Default::default());
            let moving: Vec<_> = self.piston_state.motions.iter().map(|m| m.pos).collect();
            for pos in moving {
                let state = self.screen_state(pos);
                if matches!(Block::from_id(state), Block::RedstoneLamp { .. }) {
                    self.screen_updates
                        .as_mut()
                        .unwrap()
                        .visible
                        .insert(pos, state);
                }
            }
        }
    }

    pub(super) fn screen_state(&self, pos: BlockPos) -> u32 {
        if matches!(self.get_block(pos), Block::MovingPiston { .. }) {
            if let Some(BlockEntity::MovingPiston(entity)) = self.get_block_entity(pos) {
                return entity.block_state;
            }
        }
        self.get_block_raw(pos)
    }

    pub(super) fn track_screen_change(&mut self, pos: BlockPos, previous: u32) {
        if let Some(section) = self.wire_location(pos).map(|cell| cell >> 12) {
            if let Some(updates) = &mut self.screen_updates {
                updates.mark(section, pos, previous);
            }
        }
    }

    /// Visit only sections with changed screen pixels. Simulation changes stay
    /// in chunk storage and are still visible to reads, saves and chunk loads.
    pub(super) fn flush_screen_changes(&mut self) {
        let mut updates = self.screen_updates.take().unwrap();
        if updates.dirty.is_empty() {
            self.screen_updates = Some(updates);
            return;
        }
        let started = Instant::now();
        let mut enqueue = std::time::Duration::ZERO;
        self.update_stats.flushes += 1;
        for (_, positions) in std::mem::take(&mut updates.dirty) {
            let first = *positions.iter().next().unwrap();
            let mut packet = CMultiBlockChange {
                chunk_x: first.x.div_euclid(16),
                chunk_y: (first.y >> 4) as u32,
                chunk_z: first.z.div_euclid(16),
                records: Vec::with_capacity(positions.len()),
            };
            for pos in positions {
                let state = self.screen_state(pos);
                if updates.forced.remove(&pos) || updates.visible.get(&pos).copied() != Some(state) {
                    packet.records.push(C3BMultiBlockChangeRecord {
                        block_id: state,
                        x: (pos.x & 15) as u8,
                        y: (pos.y & 15) as u8,
                        z: (pos.z & 15) as u8,
                    });
                }
                if matches!(Block::from_id(state), Block::RedstoneLamp { .. }) {
                    updates.visible.insert(pos, state);
                } else {
                    updates.visible.remove(&pos);
                }
            }
            if packet.records.is_empty() {
                continue;
            }
            self.update_stats.sections += 1;
            self.update_stats.records += packet.records.len() as u64;
            let enqueue_started = Instant::now();
            for sender in &self.packet_senders {
                sender.send_block_changes(&packet);
            }
            enqueue += enqueue_started.elapsed();
        }
        self.update_stats.enqueue += enqueue;
        self.update_stats.collection += started.elapsed().saturating_sub(enqueue);
        self.screen_updates = Some(updates);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plot::PLOT_WIDTH;
    use crate::world::storage::Chunk;
    use mchprs_blocks::blocks::{RedstoneMovingPiston, RedstoneWire};

    fn world() -> PlotWorld {
        PlotWorld::from_chunks(
            0,
            0,
            (0..PLOT_WIDTH)
                .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
                .collect(),
            Default::default(),
        )
    }

    #[test]
    fn only_changed_screen_sections_are_sent_and_turning_off_catches_up() {
        let mut w = world();
        let lamp = BlockPos::new(32, 20, 32);
        let wire = BlockPos::new(5, 5, 5);
        w.set_block(lamp, Block::RedstoneLamp { lit: false });
        w.set_block(
            wire,
            Block::RedstoneWire {
                wire: RedstoneWire::default(),
            },
        );
        w.set_screen_only(true);
        let before = w.visual_update_counts();
        w.set_block(
            wire,
            Block::RedstoneWire {
                wire: RedstoneWire {
                    power: 15,
                    ..Default::default()
                },
            },
        );
        w.flush_block_changes();
        assert_eq!(w.visual_update_counts(), before);
        w.set_block(lamp, Block::RedstoneLamp { lit: true });
        w.flush_block_changes();
        assert_eq!(
            w.visual_update_counts(),
            (before.0 + 1, before.1 + 1, before.2 + 1)
        );
        assert_eq!(
            w.screen_updates.as_ref().unwrap().visible[&lamp],
            w.get_block_raw(lamp)
        );
        assert_eq!(
            w.get_block(wire),
            Block::RedstoneWire {
                wire: RedstoneWire {
                    power: 15,
                    ..Default::default()
                }
            }
        );
        // Changes returning to the displayed state need no new packet.
        w.set_block(lamp, Block::RedstoneLamp { lit: false });
        w.set_block(lamp, Block::RedstoneLamp { lit: true });
        w.flush_block_changes();
        assert_eq!(w.visual_update_counts().2, before.2 + 1);
        w.set_block(lamp, Block::Air);
        w.flush_block_changes();
        assert_eq!(w.visual_update_counts().2, before.2 + 2);
        assert!(!w.screen_updates.as_ref().unwrap().contains(lamp));
        let records = w.visual_update_counts().2;
        w.set_screen_only(false);
        assert!(!w.screen_only());
        assert_eq!(w.visual_update_counts().2, records + 2);
        // The storage overlay is committed, not discarded or rolled back.
        assert_eq!(w.chunks[0].get_block(5, 5, 5), w.get_block_raw(wire));
    }

    #[test]
    fn moved_lamp_projects_payload_and_is_cleared_after_entity_removal() {
        let mut w = world();
        let pos = BlockPos::new(40, 30, 40);
        w.set_screen_only(true);
        let before = w.visual_update_counts().2;
        let lamp = Block::RedstoneLamp { lit: true }.get_id();
        w.set_block(
            pos,
            Block::MovingPiston {
                moving: RedstoneMovingPiston::default(),
            },
        );
        w.set_block_entity(
            pos,
            BlockEntity::MovingPiston(mchprs_blocks::block_entities::MovingPistonEntity {
                block_state: lamp,
                ..Default::default()
            }),
        );
        w.flush_block_changes();
        assert_eq!(w.visual_update_counts().2, before + 1);
        assert_eq!(w.screen_updates.as_ref().unwrap().visible[&pos], lamp);
        w.delete_block_entity(pos);
        w.set_block(pos, Block::Air);
        w.flush_block_changes();
        assert_eq!(w.visual_update_counts().2, before + 2);
        assert!(!w.screen_updates.as_ref().unwrap().contains(pos));
    }
}
