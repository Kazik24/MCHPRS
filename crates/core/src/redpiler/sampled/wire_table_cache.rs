//! Process-local experimental cache for position-based wire tables.
use crate::redpiler::scratch::WirePorts;
use mchprs_blocks::BlockPos;
use std::collections::{HashMap, VecDeque};
use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::sync::Arc;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub(super) struct TokenKey {
    pub(super) block_id: u32,
    pub(super) initial: Option<BlockPos>,
    pub(super) cells: Vec<BlockPos>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub(super) struct WireTableKey {
    pub(super) wire: BlockPos,
    pub(super) block_ids: [u32; 25],
    pub(super) axes: Vec<TokenKey>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CachedWireTable {
    pub(super) constant: bool,
    pub(super) ports: Vec<Option<WirePorts>>,
}

struct CacheEntry {
    table: Arc<CachedWireTable>,
    bytes: usize,
}

pub(super) struct WireTableCache<S = RandomState> {
    entries: HashMap<WireTableKey, CacheEntry, S>,
    order: VecDeque<WireTableKey>,
    retained: usize,
    budget: usize,
    pub(super) hits: usize,
    pub(super) misses: usize,
    pub(super) evictions: usize,
    pub(super) oversized: usize,
    pub(super) duplicates: usize,
    pub(super) peak: usize,
}

impl WireTableCache {
    pub(super) fn new(budget: usize) -> Self {
        Self::with_hasher(budget, RandomState::new())
    }
}

impl<S: BuildHasher> WireTableCache<S> {
    fn with_hasher(budget: usize, hasher: S) -> Self {
        Self { entries: HashMap::with_hasher(hasher), order: VecDeque::new(), retained: 0, budget, hits: 0, misses: 0, evictions: 0, oversized: 0, duplicates: 0, peak: 0 }
    }

    pub(super) fn get(&mut self, key: &WireTableKey) -> Option<Arc<CachedWireTable>> {
        let result = self.entries.get(key).map(|entry| entry.table.clone());
        if result.is_some() { self.hits += 1; } else { self.misses += 1; }
        result
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize { self.entries.len() }

    #[cfg(test)]
    pub(super) fn retained(&self) -> usize { self.retained }

    pub(super) fn insert(&mut self, key: WireTableKey, table: CachedWireTable) {
        let key_bytes = std::mem::size_of::<WireTableKey>()
            + key.axes.capacity() * std::mem::size_of::<TokenKey>()
            + key.axes.iter().map(|axis| axis.cells.capacity() * std::mem::size_of::<BlockPos>()).sum::<usize>();
        let table_bytes = std::mem::size_of::<CachedWireTable>()
            + table.ports.capacity() * std::mem::size_of::<Option<WirePorts>>()
            + table.ports.iter().flatten().map(|ports|
                (ports.wires.capacity() + ports.sources.capacity()) * std::mem::size_of::<BlockPos>()).sum::<usize>();
        // Account for spare hash buckets and deque slots as well as both key copies.
        let slots = 8 * (std::mem::size_of::<WireTableKey>() + std::mem::size_of::<CacheEntry>() + 16);
        let bytes = key_bytes.saturating_mul(2).saturating_add(table_bytes).saturating_add(slots);
        if bytes > self.budget { self.oversized += 1; return; }
        if self.entries.contains_key(&key) { self.duplicates += 1; return; }
        while self.retained > self.budget - bytes {
            let oldest = self.order.pop_front().expect("cache accounting without an entry");
            self.retained -= self.entries.remove(&oldest).expect("cache order without an entry").bytes;
            self.evictions += 1;
        }
        let capacity_limit = self.entries.len().saturating_add(1).saturating_mul(4);
        if self.entries.capacity() > capacity_limit || self.order.capacity() > capacity_limit {
            self.entries.shrink_to_fit();
            self.order.shrink_to_fit();
        }
        self.order.push_back(key.clone());
        self.entries.insert(key, CacheEntry { table: Arc::new(table), bytes });
        self.retained += bytes;
        self.peak = self.peak.max(self.retained);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(x: i32) -> WireTableKey {
        WireTableKey { wire: BlockPos::new(x, 0, 0), block_ids: [0; 25], axes: vec![] }
    }

    fn table() -> CachedWireTable {
        CachedWireTable { constant: true, ports: vec![Some(WirePorts { wires: vec![], sources: vec![] })] }
    }

    #[test]
    fn cache_respects_budget_and_evicts_in_insertion_order() {
        let mut probe = WireTableCache::new(usize::MAX);
        probe.insert(key(0), table());
        let entry_size = probe.retained;
        let mut cache = WireTableCache::new(entry_size * 2);
        cache.insert(key(0), table());
        cache.insert(key(1), table());
        cache.insert(key(0), table());
        cache.insert(key(2), table());
        assert!(cache.get(&key(0)).is_none());
        assert_eq!(*cache.get(&key(1)).unwrap(), table());
        assert_eq!(*cache.get(&key(2)).unwrap(), table());
        assert_eq!(cache.retained, entry_size * 2);
        let mut too_small = WireTableCache::new(entry_size - 1);
        too_small.insert(key(0), table());
        assert!(too_small.get(&key(0)).is_none());
    }

    #[test]
    fn colliding_hashes_still_require_equal_keys() {
        #[derive(Default)]
        struct CollisionHasher;
        impl std::hash::Hasher for CollisionHasher {
            fn finish(&self) -> u64 { 0 }
            fn write(&mut self, _: &[u8]) {}
        }
        let mut cache = WireTableCache::with_hasher(1_000_000, std::hash::BuildHasherDefault::<CollisionHasher>::default());
        let mut different = key(0);
        different.block_ids[0] = 1;
        cache.insert(key(0), table());
        assert!(cache.get(&different).is_none());
        cache.insert(different.clone(), table());
        assert!(cache.get(&key(0)).is_some());
        assert!(cache.get(&different).is_some());
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn concurrent_duplicate_inserts_keep_one_entry() {
        let cache = std::sync::Mutex::new(WireTableCache::new(1_000_000));
        std::thread::scope(|scope| {
            for _ in 0..16 {
                scope.spawn(|| cache.lock().unwrap().insert(key(0), table()));
            }
        });
        let mut cache = cache.lock().unwrap();
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.duplicates, 15);
        assert_eq!(*cache.get(&key(0)).unwrap(), table());
        assert!(cache.retained <= cache.budget);
    }
}
