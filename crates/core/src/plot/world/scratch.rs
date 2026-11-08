use crate::world::RedstoneUpdateObservation;
use mchprs_blocks::BlockPos;
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
};

#[derive(Default)]
pub(super) struct ScratchState {
    pub(super) observations: Option<Vec<RedstoneUpdateObservation>>,
    pub(super) capture_bounds: Option<(BlockPos, BlockPos)>,
    pub(super) read_bounds: Option<(BlockPos, BlockPos)>,
    pub(super) read_chunks: Option<HashSet<(i32, i32)>>,
    pub(super) contract_air: Option<HashSet<BlockPos>>,
    pub(super) boundary_read: Cell<Option<BlockPos>>,
    pub(super) boundary_effect: Cell<Option<BlockPos>>,
    pub(super) touched_blocks: Option<HashSet<BlockPos>>,
    pub(super) touched_entities: Option<HashSet<BlockPos>>,
    pub(super) suppress_commands: bool,
    source_probe_reads: RefCell<Option<HashSet<usize>>>,
    source_probe_active: Cell<bool>,
    source_probe_unsafe: Cell<bool>,
}

impl ScratchState {
    pub(super) fn begin_source_probe_reads(&mut self) {
        *self.source_probe_reads.get_mut() = Some(HashSet::new());
        self.source_probe_unsafe.set(false);
        self.source_probe_active.set(true);
    }

    #[inline]
    pub(super) fn source_probe_active(&self) -> bool {
        self.source_probe_active.get()
    }

    #[inline]
    pub(super) fn decline_source_probe(&self) {
        if self.source_probe_active.get() {
            self.source_probe_unsafe.set(true);
        }
    }

    #[inline]
    pub(super) fn record_source_probe_chunk(&self, chunk: Option<usize>) {
        if !self.source_probe_active.get() || self.source_probe_unsafe.get() {
            return;
        }
        let Some(chunk) = chunk else {
            self.source_probe_unsafe.set(true);
            return;
        };
        let mut reads = self.source_probe_reads.borrow_mut();
        let Some(reads) = reads.as_mut() else { return };
        if !reads.contains(&chunk) {
            if reads.try_reserve(1).is_err() {
                self.source_probe_unsafe.set(true);
                return;
            }
            reads.insert(chunk);
        }
    }

    pub(super) fn record_all_source_probe_chunks(&self, chunks: usize) {
        if !self.source_probe_active.get() || self.source_probe_unsafe.get() {
            return;
        }
        let mut reads = self.source_probe_reads.borrow_mut();
        let Some(reads) = reads.as_mut() else { return };
        if reads.try_reserve(chunks).is_err() {
            self.source_probe_unsafe.set(true);
            return;
        }
        reads.extend(0..chunks);
    }

    pub(super) fn reset_source_probe_reads(&mut self) {
        self.source_probe_active.set(false);
        *self.source_probe_reads.get_mut() = None;
        self.source_probe_unsafe.set(false);
    }

    pub(super) fn take_source_probe_reads(&mut self) -> Option<Vec<usize>> {
        self.source_probe_active.set(false);
        let unsafe_read = self.source_probe_unsafe.replace(false);
        let reads = self.source_probe_reads.get_mut().take()?;
        if unsafe_read {
            return None;
        }
        let mut chunks = Vec::new();
        chunks.try_reserve_exact(reads.len()).ok()?;
        chunks.extend(reads);
        chunks.sort_unstable();
        Some(chunks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        plot::{PLOT_WIDTH, PlotWorld},
        world::{WireNeighbor, World, storage::Chunk},
    };
    use mchprs_blocks::blocks::Block;

    fn world() -> PlotWorld {
        let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
        PlotWorld::from_chunks(0, 0, chunks, Default::default())
    }

    #[test]
    fn compact_wire_reads_and_missing_data_remain_dependencies() {
        let mut world = world();
        let wire = BlockPos::new(32, 15, 15);
        world.set_block(wire, Block::Stone {});
        let address = WireNeighbor {
            pos: wire,
            cell: world.wire_location(wire),
        };
        world.begin_source_probe_reads();
        assert_eq!(world.get_wire_block_raw(address), Block::Stone {}.get_id());
        let missing = BlockPos::new(48, 15, 15);
        assert!(world.get_block_entity(missing).is_none());
        assert!(world.get_block_entity_mut(missing).is_none());
        assert_eq!(world.get_block(BlockPos::new(64, 15, 15)), Block::Air);
        world.delete_block_entity(BlockPos::new(80, 15, 15));
        world.set_block(BlockPos::new(96, 15, 15), Block::Stone {});
        assert_eq!(
            world.take_source_probe_reads(),
            Some((2..=6).map(|x| (x * PLOT_WIDTH) as usize).collect())
        );
    }

    #[test]
    fn warm_wire_neighborhood_records_cross_chunk_footprint() {
        let mut world = world();
        let pos = BlockPos::new(15, 15, 15);
        let warm = world.wire_neighborhood(pos).unwrap();
        world.begin_source_probe_reads();
        let repeated = world.wire_neighborhood(pos).unwrap();
        assert!(std::sync::Arc::ptr_eq(&warm, &repeated));
        let chunks = world.take_source_probe_reads().unwrap();
        assert_eq!(chunks, vec![0, 1, PLOT_WIDTH as usize, PLOT_WIDTH as usize + 1]);
    }

    #[test]
    fn boundaries_and_shortcuts_decline_and_reset_disables_tracking() {
        let mut world = world();
        world.configure_scratch(
            (BlockPos::new(8, 8, 8), BlockPos::new(20, 20, 20)),
            (BlockPos::new(8, 8, 8), BlockPos::new(20, 20, 20)),
            HashSet::from([(0, 0), (1, 0), (0, 1), (1, 1)]),
            HashSet::new(),
        );
        world.begin_source_probe_reads();
        assert_eq!(world.get_block(BlockPos::new(21, 15, 15)), Block::Air);
        assert!(world.take_scratch_boundary_read().is_some());
        assert!(
            world.take_source_probe_reads().is_none(),
            "consuming boundary diagnostics must not admit the probe"
        );
        world.begin_source_probe_reads();
        let _ = world.instant_piston_cache_mut();
        assert!(world.take_source_probe_reads().is_none());
        world.begin_source_probe_reads();
        let _ = world.get_block(BlockPos::new(15, 15, 15));
        world.reset_scratch_probe_state(&Default::default());
        assert!(world.take_source_probe_reads().is_none());
        world.begin_source_probe_reads();
        assert!(world.instant_piston_cache().is_some());
        let _ = world.get_block(BlockPos::new(16, 15, 15));
        assert_eq!(world.take_source_probe_reads(), Some(vec![PLOT_WIDTH as usize]));
        world.begin_source_probe_reads();
        assert_eq!(world.get_chunks().len(), (PLOT_WIDTH * PLOT_WIDTH) as usize);
        assert_eq!(world.take_source_probe_reads().unwrap().len(), (PLOT_WIDTH * PLOT_WIDTH) as usize);
    }
}
