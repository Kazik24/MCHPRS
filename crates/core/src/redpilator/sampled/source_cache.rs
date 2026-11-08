use crate::{
    plot::{
        PLOT_WIDTH, PlotWorld,
        memory_budget::{Budget, Reservation},
    },
    world::{RedstoneUpdateObservation, World, storage::Chunk},
};
use mchprs_blocks::{BlockPos, blocks::Block};
use mchprs_world::{PistonState, TickEntry};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;

const MAX_CACHE_BYTES: usize = 64 * 1024 * 1024;
const MAX_SOURCE_CHUNKS: usize = 64;
const MAX_ENTITY_FINGERPRINT_BYTES: usize = 1024 * 1024;
static CACHE_MEMORY: once_cell::sync::Lazy<Arc<Budget>> = once_cell::sync::Lazy::new(|| Budget::new(256 * 1024 * 1024));

pub(crate) struct SourceOrderCache {
    context: Vec<u8>,
    records: HashMap<BlockPos, CachedSource>,
    basis_chunks: HashMap<usize, [u8; 32]>,
    estimated_bytes: usize,
    hits: usize,
    misses: usize,
    declined: usize,
    _reservation: Option<Reservation>,
}

#[derive(Clone)]
pub(super) struct CachedSource {
    source_block: Block,
    signature: [u8; 32],
    chunks: Vec<usize>,
    edges: Vec<CachedEdge>,
}

#[derive(Clone)]
pub(super) struct CachedEdge {
    pub(super) powered: bool,
    pub(super) deliveries: Vec<RedstoneUpdateObservation>,
}

impl SourceOrderCache {
    pub(super) fn new(world: &impl World, bounds: (BlockPos, BlockPos), scheduled: &[TickEntry], piston: &PistonState, cursed: bool) -> Option<Self> {
        #[derive(Serialize)]
        struct Context<'a> {
            bounds: ((i32, i32, i32), (i32, i32, i32)),
            scheduled: &'a [TickEntry],
            piston: &'a PistonState,
            cursed: bool,
            loaded_chunks: Vec<bool>,
        }

        let pos = |pos: BlockPos| (pos.x, pos.y, pos.z);
        let (plot_x, plot_z) = world.plot_coordinates();
        let mut loaded_chunks = Vec::with_capacity((PLOT_WIDTH * PLOT_WIDTH) as usize);
        for x in 0..PLOT_WIDTH {
            for z in 0..PLOT_WIDTH {
                loaded_chunks.push(world.get_chunk(plot_x * PLOT_WIDTH + x, plot_z * PLOT_WIDTH + z).is_some());
            }
        }
        let header = Context {
            bounds: (pos(bounds.0), pos(bounds.1)),
            scheduled,
            piston,
            cursed,
            loaded_chunks,
        };
        let bytes = usize::try_from(bincode::serialized_size(&header).ok()?).ok()?;
        let estimated_bytes = std::mem::size_of::<Self>().checked_add(bytes)?;
        if estimated_bytes > MAX_CACHE_BYTES {
            return None;
        }
        let mut context = Vec::new();
        context.try_reserve_exact(bytes).ok()?;
        bincode::serialize_into(&mut context, &header).ok()?;
        Some(Self {
            estimated_bytes,
            context,
            records: HashMap::new(),
            basis_chunks: HashMap::new(),
            hits: 0,
            misses: 0,
            declined: 0,
            _reservation: None,
        })
    }

    pub(super) fn retain(self) -> Option<Self> {
        self.retain_with_budget(&CACHE_MEMORY)
    }

    fn retain_with_budget(mut self, budget: &Arc<Budget>) -> Option<Self> {
        self._reservation = Some(budget.reserve(self.estimated_bytes)?);
        Some(self)
    }

    pub(super) fn context_matches(
        &self,
        world: &impl World,
        bounds: (BlockPos, BlockPos),
        scheduled: &[TickEntry],
        piston: &PistonState,
        cursed: bool,
    ) -> bool {
        let Some(current) = Self::new(world, bounds, scheduled, piston, cursed) else {
            return false;
        };
        self.context == current.context
    }

    pub(super) fn source(&self, pos: BlockPos) -> Option<&CachedSource> {
        self.records.get(&pos)
    }

    pub(super) fn source_matches(
        &self,
        source: &CachedSource,
        block: Block,
        signature: &[u8; 32],
        mut current_chunk: impl FnMut(usize) -> Option<[u8; 32]>,
    ) -> bool {
        source.source_block == block
            && &source.signature == signature
            && source
                .chunks
                .iter()
                .all(|&index| match (self.basis_chunks.get(&index), current_chunk(index)) {
                    (Some(before), Some(after)) => before == &after,
                    _ => false,
                })
    }

    pub(super) fn edges(source: &CachedSource) -> &[CachedEdge] {
        &source.edges
    }

    pub(super) fn chunks(source: &CachedSource) -> &[usize] {
        &source.chunks
    }

    pub(super) fn insert_source(
        &mut self,
        pos: BlockPos,
        source_block: Block,
        signature: [u8; 32],
        chunks: Vec<usize>,
        edges: Vec<CachedEdge>,
        mut current_chunk: impl FnMut(usize) -> Option<[u8; 32]>,
    ) -> bool {
        if chunks.is_empty()
            || chunks.len() > MAX_SOURCE_CHUNKS
            || chunks.windows(2).any(|pair| pair[0] >= pair[1])
            || self.records.contains_key(&pos)
        {
            return false;
        }
        let Some(record_bytes) = Self::record_size(&chunks, &edges) else {
            return false;
        };
        let new_chunks: Vec<_> = chunks.iter().copied().filter(|index| !self.basis_chunks.contains_key(index)).collect();
        let fingerprint_bytes = new_chunks
            .len()
            .checked_mul(std::mem::size_of::<(usize, [u8; 32])>())
            .unwrap_or(usize::MAX);
        let Some(additional) = record_bytes.checked_add(fingerprint_bytes) else {
            return false;
        };
        if self.estimated_bytes.checked_add(additional).is_none_or(|bytes| bytes > MAX_CACHE_BYTES) {
            return false;
        }
        let mut fingerprints = Vec::new();
        if fingerprints.try_reserve_exact(new_chunks.len()).is_err()
            || self.records.try_reserve(1).is_err()
            || self.basis_chunks.try_reserve(new_chunks.len()).is_err()
        {
            return false;
        }
        for index in new_chunks {
            let Some(fingerprint) = current_chunk(index) else {
                return false;
            };
            fingerprints.push((index, fingerprint));
        }
        for (index, fingerprint) in fingerprints {
            self.basis_chunks.insert(index, fingerprint);
        }
        self.estimated_bytes += additional;
        self.records.insert(
            pos,
            CachedSource {
                source_block,
                signature,
                chunks,
                edges,
            },
        );
        true
    }

    fn record_size(chunks: &Vec<usize>, edges: &Vec<CachedEdge>) -> Option<usize> {
        let mut bytes = std::mem::size_of::<CachedSource>()
            .checked_add(chunks.capacity().checked_mul(std::mem::size_of::<usize>())?)?
            .checked_add(edges.capacity().checked_mul(std::mem::size_of::<CachedEdge>())?)?
            .checked_add(128)?;
        for edge in edges {
            bytes = bytes
                .checked_add(edge.deliveries.capacity().checked_mul(std::mem::size_of::<RedstoneUpdateObservation>())?)?
                .checked_add(32)?;
        }
        Some(bytes)
    }

    pub(super) fn note_hit(&mut self) {
        self.hits = self.hits.saturating_add(1);
    }

    pub(super) fn note_miss(&mut self) {
        self.misses = self.misses.saturating_add(1);
    }

    pub(super) fn note_declined(&mut self) {
        self.declined = self.declined.saturating_add(1);
    }

    pub(crate) fn reuse_counts(&self) -> (usize, usize) {
        (self.hits, self.misses)
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        self.estimated_bytes
    }

    pub(crate) fn declined_sources(&self) -> usize {
        self.declined
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_declines_at_shared_limit_and_releases_its_reservation() {
        let world = crate::redpilator::sampled::tests::host();
        let make = || SourceOrderCache::new(&world, world.get_corners(), &[], world.piston_state(), false).unwrap();
        assert!(make().retain_with_budget(&Budget::new(0)).is_none());
        let budget = Budget::new(MAX_CACHE_BYTES);
        let cache = make().retain_with_budget(&budget).unwrap();
        assert_eq!(budget.stats().0, cache.estimated_bytes());
        drop(cache);
        assert_eq!(budget.stats().0, 0);
    }
}

pub(super) fn quiet_chunk_fingerprint(world: &PlotWorld, index: usize) -> Option<[u8; 32]> {
    let chunk = world.get_chunks().get(index)?;
    chunk_fingerprint(chunk)
}

fn chunk_fingerprint(chunk: &Chunk) -> Option<[u8; 32]> {
    let mut hasher = Sha256::new();
    hasher.update(chunk.x.to_le_bytes());
    hasher.update(chunk.z.to_le_bytes());
    for section in &chunk.sections {
        for index in 0..4096 {
            hasher.update(section.get_block_by_index(index).to_le_bytes());
        }
    }
    let mut entities = Vec::new();
    entities.try_reserve_exact(chunk.block_entities.len()).ok()?;
    entities.extend(chunk.block_entities.iter());
    entities.sort_unstable_by_key(|(pos, _)| (pos.x, pos.y, pos.z));
    for (pos, entity) in entities {
        hasher.update(pos.x.to_le_bytes());
        hasher.update(pos.y.to_le_bytes());
        hasher.update(pos.z.to_le_bytes());
        let bytes = usize::try_from(bincode::serialized_size(entity).ok()?).ok()?;
        if bytes > MAX_ENTITY_FINGERPRINT_BYTES {
            return None;
        }
        let mut encoded = Vec::new();
        encoded.try_reserve_exact(bytes).ok()?;
        bincode::serialize_into(&mut encoded, entity).ok()?;
        hasher.update(encoded);
    }
    Some(hasher.finalize().into())
}
