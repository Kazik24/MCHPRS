//! Local dust connectivity for inputs that do not fit a dense arrangement table.
use super::{Program, ScratchError, ScratchWorld, compiler::PistonToken, group_probe::ProbeIndexes, program::ArrangementSource};
use crate::redpiler::{TaskMonitor, scratch::discover_wire_ports};
use crate::world::World;
use mchprs_blocks::{BlockPos, blocks::Block};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

#[path = "wire_table_cache.rs"]
mod wire_table_cache;
use wire_table_cache::{CachedWireTable, TokenKey, WireTableCache, WireTableKey};
use std::sync::Mutex;

#[derive(Default)]
#[repr(align(64))]
struct WireCacheWork {
    no_axes: AtomicUsize,
    eligible: AtomicUsize,
    unavailable: AtomicUsize,
    bypass: [AtomicUsize; 4],
    enumerated: AtomicUsize,
    reused: AtomicUsize,
    key_ns: AtomicU64,
    lookup_ns: AtomicU64,
    enumeration_ns: AtomicU64,
    conversion_ns: AtomicU64,
    insert_ns: AtomicU64,
}

fn add_elapsed(counter: &AtomicU64, started: std::time::Instant) {
    counter.fetch_add(started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64, Ordering::Relaxed);
}

#[derive(Clone, Debug, Default)]
#[cfg_attr(test, derive(serde::Serialize))]
struct WireCacheWorkSnapshot {
    eligible: usize, no_axes: usize, unavailable: usize, bypass: [usize; 4], enumerated: usize, reused: usize,
    key_ns: u64, lookup_ns: u64, enumeration_ns: u64, conversion_ns: u64, insert_ns: u64,
}

impl WireCacheWork {
    fn add_to(&self, total: &mut WireCacheWorkSnapshot) {
        total.eligible += self.eligible.load(Ordering::Relaxed);
        total.no_axes += self.no_axes.load(Ordering::Relaxed);
        total.unavailable += self.unavailable.load(Ordering::Relaxed);
        for (sum, counter) in total.bypass.iter_mut().zip(&self.bypass) { *sum += counter.load(Ordering::Relaxed); }
        total.enumerated += self.enumerated.load(Ordering::Relaxed);
        total.reused += self.reused.load(Ordering::Relaxed);
        total.key_ns += self.key_ns.load(Ordering::Relaxed);
        total.lookup_ns += self.lookup_ns.load(Ordering::Relaxed);
        total.enumeration_ns += self.enumeration_ns.load(Ordering::Relaxed);
        total.conversion_ns += self.conversion_ns.load(Ordering::Relaxed);
        total.insert_ns += self.insert_ns.load(Ordering::Relaxed);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Axis {
    Token(usize, usize),
}

impl Axis {
    fn radix(self) -> usize {
        match self { Self::Token(_, cells) => cells + 1 }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Ports {
    pub(super) wires: Vec<usize>,
    pub(super) sources: Vec<BlockPos>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Local<T> {
    pub(super) axes: Vec<Axis>,
    pub(super) ports: Vec<Option<T>>,
}

type Node = Local<Ports>;
type TokenCells = HashMap<BlockPos, Vec<usize>>;

fn wire_index(index: &HashMap<BlockPos, usize>, pos: BlockPos) -> Result<usize, ScratchError> {
    index.get(&pos).copied().ok_or(ScratchError { position: Some(pos), reason: "unrepresented routed wire" })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ReaderPorts {
    pub(super) roots: Vec<(usize, bool)>,
    pub(super) direct: Vec<(ArrangementSource, bool)>,
}

#[derive(Clone, Debug)]
pub(super) struct Reader {
    pub(super) component: usize,
    position: BlockPos,
    pub(super) table: Local<ReaderPorts>,
    dependencies: Vec<usize>,
    pub(super) links: Vec<usize>,
}

impl<T> Local<T> {
    pub(super) fn select(&self, digit: impl Fn(Axis) -> usize) -> Option<&T> {
        let mut state = 0;
        let mut stride = 1;
        for &axis in &self.axes {
            let value = digit(axis);
            if value >= axis.radix() { return None; }
            state += value * stride;
            stride *= axis.radix();
        }
        self.ports.get(state)?.as_ref()
    }
}

pub(super) fn compile_local<T: PartialEq>(scratch: &mut ScratchWorld, tokens: &[PistonToken], mut axes: Vec<Axis>, position: BlockPos,
    monitor: Option<&TaskMonitor>, evaluate: impl Fn(&crate::plot::PlotWorld) -> Result<T, ScratchError>) -> Result<Local<T>, ScratchError> {
    let states = axes.iter().try_fold(1usize, |count, axis| count.checked_mul(axis.radix()));
    let Some(states) = states.filter(|&count| count <= 4096) else {
        return Err(ScratchError { position: Some(position), reason: "local connectivity exceeds state bound" });
    };
    let mut ports = Vec::with_capacity(states);
    for state in 0..states {
        if monitor.is_some_and(TaskMonitor::cancelled) { return Err(ScratchError { position: None, reason: "compilation cancelled" }); }
        scratch.revert();
        for &Axis::Token(token, _) in &axes {
            if let Some(initial) = tokens[token].position { scratch.world_mut().set_block(initial, Block::Air); }
        }
        let mut digits = state;
        let mut occupied = HashSet::new();
        let mut valid = true;
        for &axis in &axes {
            let digit = digits % axis.radix();
            digits /= axis.radix();
            let Axis::Token(token, _) = axis;
            if digit != 0 {
                let cell = tokens[token].cells[digit - 1];
                valid &= occupied.insert(cell);
                scratch.world_mut().set_block(cell, tokens[token].block);
            }
        }
        if valid { ports.push(Some(evaluate(scratch.world())?)); } else { ports.push(None); }
        scratch.finish_probe()?;
    }
    if ports.iter().all(|entry| entry == &ports[0]) { axes.clear(); ports.truncate(1); }
    Ok(Local { axes, ports })
}

#[derive(Clone, Debug)]
pub(super) struct WireGraph {
    positions: Vec<BlockPos>,
    index: HashMap<BlockPos, usize>,
    token_cells: TokenCells,
    pub(super) nodes: Vec<Node>,
    pub(super) source_ids: HashMap<BlockPos, ArrangementSource>,
    pub(super) readers: Vec<Reader>,
    by_component: Vec<Option<usize>>,
    pub(super) by_token: Vec<Vec<usize>>,
    #[cfg(test)]
    cache_work: Option<WireCacheWorkSnapshot>,
    #[cfg(test)]
    node_elapsed_ns: u64,
}

#[derive(Default)]
pub(super) struct Search {
    distances: Vec<u8>,
    visited: Vec<usize>,
    queue: VecDeque<usize>,
    pub(super) remaining: Option<usize>,
    pub(super) spent: usize,
}

#[cfg(test)]
fn touches(a: BlockPos, b: BlockPos) -> bool {
    a.x.abs_diff(b.x) + a.y.abs_diff(b.y) + a.z.abs_diff(b.z) == 1
}

fn checked_offset(position: BlockPos, dx: i32, dy: i32, dz: i32) -> Option<BlockPos> {
    Some(BlockPos::new(position.x.checked_add(dx)?, position.y.checked_add(dy)?, position.z.checked_add(dz)?))
}

fn adjacent_positions(position: BlockPos) -> impl Iterator<Item = BlockPos> {
    [
        checked_offset(position, 0, -1, 0), checked_offset(position, 0, 1, 0),
        checked_offset(position, 0, 0, -1), checked_offset(position, 0, 0, 1),
        checked_offset(position, -1, 0, 0), checked_offset(position, 1, 0, 0),
    ].into_iter().flatten()
}

fn manhattan_positions(center: BlockPos, radius: i32) -> Vec<BlockPos> {
    let mut positions = Vec::with_capacity(match radius { 2 => 25, 3 => 63, _ => 1 });
    for dx in -radius..=radius {
        for dy in -radius..=radius {
            for dz in -radius..=radius {
                if dx.abs() + dy.abs() + dz.abs() <= radius {
                    if let Some(position) = checked_offset(center, dx, dy, dz) { positions.push(position); }
                }
            }
        }
    }
    positions
}

fn index_token_cells(tokens: &[PistonToken]) -> TokenCells {
    let mut cells: TokenCells = HashMap::new();
    for (token, record) in tokens.iter().enumerate() {
        for &cell in &record.cells { cells.entry(cell).or_default().push(token); }
    }
    cells
}

fn append_tokens_at(index: &TokenCells, position: BlockPos, into: &mut Vec<usize>) {
    if let Some(tokens) = index.get(&position) { into.extend_from_slice(tokens); }
}

fn axes_for_tokens(tokens: &[PistonToken], mut candidates: Vec<usize>) -> Vec<Axis> {
    candidates.sort_unstable();
    candidates.dedup();
    candidates.into_iter().map(|token| Axis::Token(token, tokens[token].cells.len())).collect()
}

fn wire_token_axes(position: BlockPos, tokens: &[PistonToken], token_cells: &TokenCells) -> Vec<Axis> {
    let mut candidates = Vec::new();
    for cell in adjacent_positions(position) { append_tokens_at(token_cells, cell, &mut candidates); }
    axes_for_tokens(tokens, candidates)
}

fn piston_contact_cells(position: BlockPos, block: Block) -> Option<HashSet<BlockPos>> {
    let Block::Piston { piston } = block else { return None; };
    let mut cells: HashSet<_> = mchprs_blocks::BlockFace::values().into_iter()
        .filter(|&face| face != piston.facing.into()).map(|face| position.offset(face)).collect();
    let above = position.offset(mchprs_blocks::BlockFace::Top);
    cells.extend(mchprs_blocks::BlockFace::values().into_iter().map(|face| above.offset(face)));
    Some(cells)
}

fn reader_contact_positions(position: BlockPos, piston_cells: Option<&HashSet<BlockPos>>,
    wire_index: &HashMap<BlockPos, usize>) -> HashSet<BlockPos> {
    let mut contacts = HashSet::new();
    if let Some(cells) = piston_cells {
        for &cell in cells {
            if wire_index.contains_key(&cell) { contacts.insert(cell); }
            for adjacent in adjacent_positions(cell) {
                if wire_index.contains_key(&adjacent) { contacts.insert(adjacent); }
            }
        }
    } else {
        for at in manhattan_positions(position, 2) {
            if wire_index.contains_key(&at) { contacts.insert(at); }
        }
    }
    contacts
}

fn reader_token_axes(position: BlockPos, tokens: &[PistonToken], token_cells: &TokenCells,
    piston_cells: Option<&HashSet<BlockPos>>, contacts: &HashSet<BlockPos>) -> Vec<Axis> {
    let mut candidates = Vec::new();
    if let Some(cells) = piston_cells {
        for &cell in cells { append_tokens_at(token_cells, cell, &mut candidates); }
    } else {
        for cell in manhattan_positions(position, 2) { append_tokens_at(token_cells, cell, &mut candidates); }
    }
    for &contact in contacts {
        for cell in adjacent_positions(contact) { append_tokens_at(token_cells, cell, &mut candidates); }
    }
    axes_for_tokens(tokens, candidates)
}

impl WireGraph {
    pub(super) fn reader_count(&self) -> usize { self.readers.len() }
    pub(super) fn reader_component(&self, reader: usize) -> usize { self.readers[reader].component }
    pub(super) fn reader_dependencies(&self, reader: usize) -> &[usize] { &self.readers[reader].dependencies }
    pub(super) fn retain_readers(&mut self, targets: &BTreeSet<usize>) {
        self.readers.retain(|reader| targets.contains(&reader.component));
        self.reindex();
    }
    fn reindex(&mut self) {
        self.by_component.fill(None);
        for readers in &mut self.by_token { readers.clear(); }
        for (index, reader) in self.readers.iter().enumerate() {
            self.by_component[reader.component] = Some(index);
            for &token in &reader.dependencies { self.by_token[token].push(index); }
        }
    }
    pub(super) fn reader_for_component(&self, component: usize) -> Option<usize> { self.by_component[component] }
    pub(super) fn readers_for_token(&self, token: usize) -> &[usize] { &self.by_token[token] }
    pub(super) fn links(&self, reader: usize) -> &[usize] { &self.readers[reader].links }
    pub(super) fn bind_links(&mut self, links: &[super::program::ArrangementLink]) {
        for reader in &mut self.readers { reader.links.clear(); }
        for (index, link) in links.iter().enumerate() {
            if let Some(reader) = self.reader_for_component(link.target) { self.readers[reader].links.push(index); }
        }
    }

    pub(super) fn potential_sources(&self, reader: usize) -> BTreeMap<(ArrangementSource, bool), u8> {
        let mut found = BTreeMap::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        for ports in self.readers[reader].table.ports.iter().flatten() {
            for &source in &ports.direct { found.insert(source, 0); }
            for &(root, side) in &ports.roots {
                if visited.insert((root, side)) { queue.push_back((root, side, 0u8)); }
            }
        }
        while let Some((wire, side, distance)) = queue.pop_front() {
            for ports in self.nodes[wire].ports.iter().flatten() {
                for pos in &ports.sources {
                    found.entry((self.source_ids[pos], side)).and_modify(|old| *old = (*old).min(distance)).or_insert(distance);
                }
                if distance == 14 { continue; }
                for &next in &ports.wires {
                    if visited.insert((next, side)) { queue.push_back((next, side, distance + 1)); }
                }
            }
        }
        found
    }

    pub(super) fn compile(
        scratch: &mut ScratchWorld,
        positions: Vec<BlockPos>,
        tokens: &[PistonToken],
        monitor: Option<&TaskMonitor>,
    ) -> Result<Self, ScratchError> {
        Self::compile_with_cache(scratch, positions, tokens, monitor, None)
    }

    fn compile_with_cache(
        scratch: &mut ScratchWorld,
        positions: Vec<BlockPos>,
        tokens: &[PistonToken],
        monitor: Option<&TaskMonitor>,
        cache: Option<&Mutex<WireTableCache>>,
    ) -> Result<Self, ScratchError> {
        let index: HashMap<_, _> = positions.iter().enumerate().map(|(node, &pos)| (pos, node)).collect();
        for token in tokens {
            if token.block != Block::RedstoneBlock && token.block.slab_type().is_none()
                && (token.block.pressure_plate_powered().is_some() || !super::compiler::is_passive_geometry(token.block)) {
                return Err(ScratchError { position: token.position, reason: "unsupported routed payload" });
            }
            let at = token.position.or_else(|| token.cells.first().copied()).ok_or(ScratchError { position: None, reason: "routed payload has no cells" })?;
            if mchprs_blocks::BlockFace::values().iter().any(|&face|
                crate::redstone::power::emits_strong_power(token.block, scratch.world(), at, face, false)) {
                return Err(ScratchError { position: Some(at), reason: "routed payload emits strong power" });
            }
        }
        if tokens.iter().flat_map(|token| &token.cells).any(|cell| index.contains_key(cell)) {
            return Err(ScratchError { position: None, reason: "movable payload overlaps a fixed wire" });
        }
        let token_cells = index_token_cells(tokens);
        let rows = AtomicUsize::new(0);
        let work = cache.map(|_| (0..rayon::current_num_threads()).map(|_| WireCacheWork::default()).collect::<Vec<_>>());
        let node_started = std::time::Instant::now();
        let nodes = scratch.probe_jobs(&positions, monitor, |scratch, &pos| {
            let work = work.as_ref().map(|workers| &workers[rayon::current_thread_index().unwrap_or(0)]);
            // Non-wire payloads cannot alter a distance-two strong-source read.
            let axes = wire_token_axes(pos, tokens, &token_cells);
            if cache.is_none() || axes.is_empty() {
                if let Some(work) = work { work.no_axes.fetch_add(1, Ordering::Relaxed); }
                let node = compile_local(scratch, tokens, axes, pos, monitor, |world| {
                    let found = discover_wire_ports(world, pos)?;
                    let wires = found.wires.into_iter().map(|at| wire_index(&index, at)).collect::<Result<_, _>>()?;
                    Ok(Ports { wires, sources: found.sources })
                })?;
                let rows = rows.fetch_add(node.ports.len(), Ordering::Relaxed) + node.ports.len();
                if rows > 1_048_576 { return Err(ScratchError { position: Some(pos), reason: "local routing data exceeds size bound" }); }
                return Ok(node);
            }
            let states = axes.iter().try_fold(1usize, |count, axis| count.checked_mul(axis.radix()));
            if states.is_none_or(|count| count > 4096) {
                return Err(ScratchError { position: Some(pos), reason: "local connectivity exceeds state bound" });
            }
            let work = work.unwrap();
            let key_started = std::time::Instant::now();
            // The wire-port read footprint is guarded by wire_port_reads_stay_within_two_blocks.
            let footprint = manhattan_positions(pos, 2);
            let writes: Vec<_> = axes.iter().flat_map(|&Axis::Token(token, _)|
                tokens[token].position.into_iter().chain(tokens[token].cells.iter().copied())).collect();
            let bypass = if footprint.len() != 25 { Some(crate::redpiler::scratch::WireCacheBypass::Context) }
                else { scratch.wire_cache_bypass(&footprint, &writes) };
            if let Some(reason) = bypass { work.bypass[reason as usize].fetch_add(1, Ordering::Relaxed); }
            else { work.eligible.fetch_add(1, Ordering::Relaxed); }
            let key = bypass.is_none().then(|| WireTableKey {
                    wire: pos,
                    block_ids: std::array::from_fn(|i| scratch.world().get_block_raw(footprint[i])),
                    axes: axes.iter().map(|&Axis::Token(token, _)| TokenKey {
                        block_id: tokens[token].block.get_id(), initial: tokens[token].position, cells: tokens[token].cells.clone(),
                    }).collect(),
                });
            add_elapsed(&work.key_ns, key_started);
            let lookup_started = std::time::Instant::now();
            let cached = match (key.as_ref(), cache) {
                (Some(key), Some(cache)) => match cache.lock() {
                    Ok(mut cache) => cache.get(key),
                    Err(_) => { work.unavailable.fetch_add(1, Ordering::Relaxed); None }
                },
                _ => None,
            };
            add_elapsed(&work.lookup_ns, lookup_started);
            let mut pending = None;
            let table = if let Some(table) = cached {
                if monitor.is_some_and(TaskMonitor::cancelled) {
                    return Err(ScratchError { position: None, reason: "compilation cancelled" });
                }
                scratch.finish_probe()?;
                work.reused.fetch_add(states.unwrap(), Ordering::Relaxed);
                Local { axes: if table.constant { vec![] } else { axes.clone() }, ports: table.ports.clone() }
            } else {
                let enumeration_started = std::time::Instant::now();
                let table = compile_local(scratch, tokens, axes, pos, monitor, |world| {
                    let found = discover_wire_ports(world, pos)?;
                    for &at in &found.wires {
                        wire_index(&index, at)?;
                    }
                    Ok(found)
                })?;
                add_elapsed(&work.enumeration_ns, enumeration_started);
                work.enumerated.fetch_add(states.unwrap(), Ordering::Relaxed);
                if let Some(key) = key { pending = Some((key, CachedWireTable { constant: table.axes.is_empty(), ports: table.ports.clone() })); }
                table
            };
            let conversion_started = std::time::Instant::now();
            let node = Local { axes: table.axes, ports: table.ports.into_iter().map(|row| row.map(|found| {
                let wires = found.wires.into_iter().map(|at| wire_index(&index, at)).collect::<Result<_, _>>()?;
                Ok(Ports { wires, sources: found.sources })
            }).transpose()).collect::<Result<Vec<_>, ScratchError>>()? };
            add_elapsed(&work.conversion_ns, conversion_started);
            let rows = rows.fetch_add(node.ports.len(), Ordering::Relaxed) + node.ports.len();
            if rows > 1_048_576 { return Err(ScratchError { position: Some(pos), reason: "local routing data exceeds size bound" }); }
            if let Some((key, table)) = pending && let Some(cache) = cache {
                let insert_started = std::time::Instant::now();
                if let Ok(mut cache) = cache.lock() && !monitor.is_some_and(TaskMonitor::cancelled) { cache.insert(key, table); }
                add_elapsed(&work.insert_ns, insert_started);
            }
            Ok(node)
        })?;
        let node_elapsed = node_started.elapsed();
        let work_summary = work.as_ref().map(|workers| {
            let mut total = WireCacheWorkSnapshot::default();
            for worker in workers { worker.add_to(&mut total); }
            total
        });
        if std::env::var_os("MCHPRS_REDPILER_PHASE_TIMINGS").is_some() {
            eprintln!("Redpiler wire nodes: wires={} rows={} cache={} elapsed_ms={}",
                positions.len(), rows.load(Ordering::Relaxed), cache.is_some(), node_elapsed.as_millis());
            if let Some(work) = &work_summary {
                eprintln!("Redpiler wire cache work (summed worker ns): eligible={} no_axes={} unavailable={} bypass_entry={} bypass_context={} bypass_writes={} bypass_entities={} enumerated={} reused={} key_ns={} lookup_ns={} enumeration_ns={} conversion_ns={} insert_ns={}",
                    work.eligible, work.no_axes, work.unavailable, work.bypass[0], work.bypass[1], work.bypass[2], work.bypass[3],
                    work.enumerated, work.reused, work.key_ns, work.lookup_ns, work.enumeration_ns, work.conversion_ns, work.insert_ns);
            }
        }
        scratch.revert();
        Ok(Self { positions, index, token_cells, nodes, source_ids: HashMap::new(), readers: Vec::new(), by_component: Vec::new(), by_token: Vec::new(),
            #[cfg(test)] cache_work: work_summary,
            #[cfg(test)] node_elapsed_ns: node_elapsed.as_nanos().min(u128::from(u64::MAX)) as u64 })
    }

    pub(super) fn compile_readers(&mut self, scratch: &mut ScratchWorld, program: &Program, targets: &[usize], indexes: &ProbeIndexes<'_>, monitor: Option<&TaskMonitor>) -> Result<(), ScratchError> {
        let token_cells = std::mem::take(&mut self.token_cells);
        let rows = AtomicUsize::new(self.nodes.iter().map(|node| node.ports.len()).sum());
        for node in &self.nodes {
            for ports in node.ports.iter().flatten() {
                for &pos in &ports.sources { self.source_ids.insert(pos, indexes.source_at(pos)?); }
            }
        }
        let readers = scratch.probe_jobs(targets, monitor, |scratch, &component| {
            let position = program.positions[component];
            let piston_contacts = piston_contact_cells(position, program.initial[component]);
            let contacts = reader_contact_positions(position, piston_contacts.as_ref(), &self.index);
            let axes = if matches!(program.initial[component], Block::RedstoneWire { .. }) { Vec::new() } else {
                reader_token_axes(position, &program.tokens, &token_cells, piston_contacts.as_ref(), &contacts)
            };
            let table = compile_local(scratch, &program.tokens, axes, position, monitor, |world| {
                let inputs = super::group_probe::ports_of(world, program, component, monitor, indexes)?;
                let mut ports = ReaderPorts { roots: Vec::new(), direct: Vec::new() };
                for ((source, side), _) in inputs {
                    if let ArrangementSource::Component(node) = source && matches!(program.initial[node], Block::RedstoneWire { .. }) {
                        let root = self.index.get(&program.positions[node]).copied().ok_or(ScratchError {
                            position: Some(program.positions[node]), reason: "unrepresented routed reader contact",
                        })?;
                        ports.roots.push((root, side));
                    } else { ports.direct.push((source, side)); }
                }
                Ok(ports)
            })?;
            let rows = rows.fetch_add(table.ports.len(), Ordering::Relaxed) + table.ports.len();
            if rows > 1_048_576 { return Err(ScratchError { position: Some(position), reason: "local routing data exceeds size bound" }); }
            let mut dependencies: BTreeMap<usize, ()> = table.axes.iter().map(|&Axis::Token(token, _)| (token, ())).collect();
            let mut reached = HashSet::new();
            let mut queue = VecDeque::new();
            for &(root, _) in table.ports.iter().flatten().flat_map(|ports| &ports.roots) {
                if reached.insert(root) { queue.push_back((root, 0)); }
            }
            while let Some((wire, distance)) = queue.pop_front() {
                for &Axis::Token(token, _) in &self.nodes[wire].axes { dependencies.insert(token, ()); }
                if distance == 14 { continue; }
                for &next in self.nodes[wire].ports.iter().flatten().flat_map(|ports| &ports.wires) {
                    if reached.insert(next) { queue.push_back((next, distance + 1)); }
                }
            }
            Ok(Reader { component, position, table, dependencies: dependencies.into_keys().collect(), links: Vec::new() })
        })?;
        self.readers.extend(readers);
        self.by_component = vec![None; program.initial.len()];
        self.by_token = vec![Vec::new(); program.tokens.len()];
        self.reindex();
        scratch.revert();
        Ok(())
    }

    pub(super) fn reader_sources(&self, reader: usize, digit: impl Fn(Axis) -> usize + Copy, search: &mut Search) -> Result<BTreeMap<(ArrangementSource, bool), u8>, ScratchError> {
        let reader = &self.readers[reader];
        let ports = reader.table.select(digit).ok_or(ScratchError {
            position: Some(reader.position), reason: "invalid routed reader state",
        })?;
        let mut sources: BTreeMap<_, u8> = ports.direct.iter().map(|&key| (key, 0)).collect();
        for &(root, side) in &ports.roots {
            for (pos, attenuation) in self.sources(self.positions[root], digit, search)? {
                let source = self.source_ids[&pos];
                sources.entry((source, side)).and_modify(|old| *old = (*old).min(attenuation)).or_insert(attenuation);
            }
        }
        Ok(sources)
    }

    pub(super) fn sources(
        &self,
        root: BlockPos,
        digit: impl Fn(Axis) -> usize,
        search: &mut Search,
    ) -> Result<Vec<(BlockPos, u8)>, ScratchError> {
        for wire in search.visited.drain(..) { search.distances[wire] = u8::MAX; }
        search.distances.resize(self.nodes.len(), u8::MAX);
        search.queue.clear();
        let root = *self.index.get(&root).ok_or(ScratchError { position: Some(root), reason: "unrepresented routed wire root" })?;
        search.distances[root] = 0;
        search.visited.push(root);
        search.queue.push_back(root);
        let mut found = BTreeMap::new();
        while let Some(wire) = search.queue.pop_front() {
            if let Some(remaining) = &mut search.remaining {
                if *remaining == 0 { return Err(ScratchError { position: Some(self.positions[wire]), reason: "runtime semantic operation limit exceeded" }); }
                *remaining -= 1;
            }
            search.spent += 1;
            let node = &self.nodes[wire];
            let mut state = 0;
            let mut stride = 1;
            for &axis in &node.axes {
                let value = digit(axis);
                if value >= axis.radix() {
                    return Err(ScratchError { position: Some(self.positions[wire]), reason: "invalid routing axis state" });
                }
                state += value * stride;
                stride *= axis.radix();
            }
            let ports = node.ports[state].as_ref().ok_or(ScratchError {
                position: Some(self.positions[wire]), reason: "conflicting routed token positions",
            })?;
            let distance = search.distances[wire];
            for &source in &ports.sources {
                found.entry((source.x, source.y, source.z)).and_modify(|old: &mut u8| *old = (*old).min(distance)).or_insert(distance);
            }
            if distance == 14 { continue; }
            for &next in &ports.wires {
                if search.distances[next] != u8::MAX { continue; }
                search.distances[next] = distance + 1;
                search.visited.push(next);
                search.queue.push_back(next);
            }
        }
        Ok(found.into_iter().map(|((x, y, z), attenuation)| (BlockPos::new(x, y, z), attenuation)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::scratch::{ScratchEntry, WirePorts, discover_wire_sources};
    use mchprs_blocks::{BlockFace, blocks::RedstoneWire};

    fn cache_benchmark_threads() -> usize {
        let threads = std::env::var("MCHPRS_WIRE_CACHE_THREADS").map_or(4, |value| value.parse().unwrap());
        assert!(threads > 0);
        threads
    }

    #[test]
    fn cached_wire_tables_survive_distant_edits_and_rebind_indices() {
        let mut world = super::super::tests::host();
        let wire = BlockPos::new(20, 15, 20);
        let neighbor = wire.offset(BlockFace::East);
        let cell = wire.offset(BlockFace::West);
        for pos in [wire, neighbor] {
            world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
            world.set_block(pos, Block::RedstoneWire { wire: RedstoneWire::default() });
        }
        world.set_block(cell, Block::RedstoneBlock);
        let tokens = vec![PistonToken { block: Block::RedstoneBlock, position: Some(cell), cells: vec![cell] }];
        let bounds = (BlockPos::new(8, 10, 8), BlockPos::new(32, 20, 32));
        let cache = Mutex::new(WireTableCache::new(1_000_000));
        let compile = |world: &crate::plot::PlotWorld, positions: Vec<BlockPos>, cached: bool| {
            let entry = ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() };
            let mut scratch = ScratchWorld::from_world_monitored(world, bounds, entry, None).unwrap();
            WireGraph::compile_with_cache(&mut scratch, positions, &tokens, None, cached.then_some(&cache)).unwrap().nodes
        };
        let positions = vec![wire, neighbor];
        assert_eq!(compile(&world, positions.clone(), false), compile(&world, positions.clone(), true));
        assert_eq!(cache.lock().unwrap().len(), 1);
        assert_eq!(cache.lock().unwrap().misses, 1);
        world.set_block(BlockPos::new(30, 15, 30), Block::Stone {});
        assert_eq!(compile(&world, positions.clone(), false), compile(&world, positions.clone(), true));
        assert_eq!(cache.lock().unwrap().len(), 1);
        assert_eq!(cache.lock().unwrap().hits, 1);
        let reversed = vec![neighbor, wire];
        assert_eq!(compile(&world, reversed.clone(), false), compile(&world, reversed, true));
        assert_eq!(cache.lock().unwrap().len(), 1);
        let shifted_tokens = vec![PistonToken { block: Block::Stone {}, position: Some(BlockPos::new(30, 15, 30)),
            cells: vec![BlockPos::new(30, 15, 30)] }, tokens[0].clone()];
        let shifted = |cached: bool| {
            let entry = ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() };
            let mut scratch = ScratchWorld::from_world_monitored(&world, bounds, entry, None).unwrap();
            WireGraph::compile_with_cache(&mut scratch, positions.clone(), &shifted_tokens, None, cached.then_some(&cache)).unwrap().nodes
        };
        assert_eq!(shifted(false), shifted(true));
        assert_eq!(cache.lock().unwrap().len(), 1);
        assert_eq!(cache.lock().unwrap().hits, 3);
        let error = |cached: bool| {
            let entry = ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() };
            let mut scratch = ScratchWorld::from_world_monitored(&world, bounds, entry, None).unwrap();
            WireGraph::compile_with_cache(&mut scratch, vec![wire], &tokens, None, cached.then_some(&cache)).unwrap_err()
        };
        assert_eq!(error(false), error(true));
        let mut ordered_errors = tokens.clone();
        ordered_errors[0].cells.push(BlockPos::new(33, 15, 20));
        let first_error = |cached: bool| {
            let entry = ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() };
            let mut scratch = ScratchWorld::from_world_monitored(&world, bounds, entry, None).unwrap();
            WireGraph::compile_with_cache(&mut scratch, vec![wire], &ordered_errors, None, cached.then_some(&cache)).unwrap_err()
        };
        let expected_error = first_error(false);
        assert_eq!(expected_error.reason, "unrepresented routed wire");
        assert_eq!(expected_error, first_error(true));
        world.set_block(wire.offset(BlockFace::Top), Block::Stone {});
        assert_eq!(compile(&world, positions.clone(), false), compile(&world, positions.clone(), true));
        assert_eq!(cache.lock().unwrap().len(), 2);
        let mut changed_tokens = tokens.clone();
        changed_tokens[0].cells.push(BlockPos::new(31, 15, 31));
        let changed = |cached: bool| {
            let entry = ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() };
            let mut scratch = ScratchWorld::from_world_monitored(&world, bounds, entry, None).unwrap();
            WireGraph::compile_with_cache(&mut scratch, positions.clone(), &changed_tokens, None, cached.then_some(&cache)).unwrap().nodes
        };
        assert_eq!(changed(false), changed(true));
        assert_eq!(cache.lock().unwrap().len(), 3);
        let too_many = vec![tokens[0].clone(); 13];
        for cached in [false, true] {
            let entry = ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() };
            let mut scratch = ScratchWorld::from_world_monitored(&world, bounds, entry, None).unwrap();
            assert_eq!(WireGraph::compile_with_cache(&mut scratch, positions.clone(), &too_many, None, cached.then_some(&cache))
                .unwrap_err().reason, "local connectivity exceeds state bound");
        }
        assert_eq!(cache.lock().unwrap().len(), 3);
        world.set_block(wire.offset(BlockFace::Top), Block::Air {});
        let hits_before = cache.lock().unwrap().hits;
        assert_eq!(compile(&world, positions.clone(), false), compile(&world, positions.clone(), true));
        assert_eq!(cache.lock().unwrap().hits, hits_before + 1);
        let mut variants = Vec::new();
        let mut block_changed = tokens.clone();
        block_changed[0].block = Block::Stone {};
        variants.push(block_changed);
        let mut initial_changed = tokens.clone();
        initial_changed[0].position = None;
        variants.push(initial_changed);
        let mut reordered = tokens.clone();
        reordered[0].cells = vec![BlockPos::new(31, 15, 31), cell];
        variants.push(reordered);
        for variant in variants {
            let build = |cached: bool| {
                let entry = ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() };
                let mut scratch = ScratchWorld::from_world_monitored(&world, bounds, entry, None).unwrap();
                WireGraph::compile_with_cache(&mut scratch, positions.clone(), &variant, None, cached.then_some(&cache)).unwrap().nodes
            };
            let before = cache.lock().unwrap().misses;
            assert_eq!(build(false), build(true));
            assert_eq!(cache.lock().unwrap().misses, before + 1);
        }
        let Block::RedstoneWire { wire: mut wire_state } = world.get_block(wire) else { unreachable!() };
        wire_state.power = 1;
        world.set_block(wire, Block::RedstoneWire { wire: wire_state });
        let misses_before = cache.lock().unwrap().misses;
        assert_eq!(compile(&world, positions.clone(), false), compile(&world, positions.clone(), true));
        assert_eq!(cache.lock().unwrap().misses, misses_before + 1);
        let monitor = TaskMonitor::default();
        monitor.cancel();
        let entry = ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() };
        let mut scratch = ScratchWorld::from_world_monitored(&world, bounds, entry, None).unwrap();
        assert_eq!(WireGraph::compile_with_cache(&mut scratch, positions, &tokens, Some(&monitor), Some(&cache)).unwrap_err().reason,
            "compilation cancelled");
        assert_eq!(cache.lock().unwrap().len(), 7);
    }

    #[test]
    fn cached_wire_tables_preserve_boundary_errors_and_bypass_pending_work() {
        let mut world = super::super::tests::host();
        let wire = BlockPos::new(20, 15, 20);
        let cell = wire.offset(BlockFace::West);
        world.set_block(wire, Block::RedstoneWire { wire: RedstoneWire::default() });
        world.set_block(cell, Block::RedstoneBlock);
        let tokens = vec![PistonToken { block: Block::RedstoneBlock, position: Some(cell), cells: vec![cell] }];
        let cache = Mutex::new(WireTableCache::new(1_000_000));
        let compile = |world: &crate::plot::PlotWorld, bounds, cached: bool| {
            let entry = ScratchEntry { scheduled: world.scheduler().iter_entries().collect(), piston: world.piston_state().clone() };
            let mut scratch = ScratchWorld::from_world_monitored(world, bounds, entry, None).unwrap();
            WireGraph::compile_with_cache(&mut scratch, vec![wire], &tokens, None, cached.then_some(&cache)).map(|graph| graph.nodes)
        };
        let bounds = (BlockPos::new(8, 10, 8), BlockPos::new(32, 20, 32));
        assert_eq!(compile(&world, bounds, false).unwrap(), compile(&world, bounds, true).unwrap());
        assert_eq!(cache.lock().unwrap().len(), 1);
        let restricted = (wire, wire);
        assert_eq!(compile(&world, restricted, false).unwrap_err(), compile(&world, restricted, true).unwrap_err());
        assert_eq!(cache.lock().unwrap().len(), 1);
        world.schedule_tick(wire, 1, mchprs_world::TickPriority::Normal);
        assert_eq!(compile(&world, bounds, false).unwrap(), compile(&world, bounds, true).unwrap());
        let stats = cache.lock().unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!((stats.hits, stats.misses), (0, 1));
        drop(stats);
        let mut with_entity = super::super::tests::host();
        with_entity.set_block(wire, Block::RedstoneWire { wire: RedstoneWire::default() });
        with_entity.set_block(cell, Block::RedstoneBlock);
        with_entity.set_block_entity(cell, mchprs_blocks::block_entities::BlockEntity::Comparator { output_strength: 5 });
        assert_eq!(compile(&with_entity, bounds, false).unwrap(), compile(&with_entity, bounds, true).unwrap());
        let stats = cache.lock().unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!((stats.hits, stats.misses), (0, 1));
        drop(stats);
        let mut edge_world = super::super::tests::host();
        let edge = BlockPos::new(0, 15, 0);
        let edge_cell = edge.offset(BlockFace::East);
        edge_world.set_block(edge, Block::RedstoneWire { wire: RedstoneWire::default() });
        edge_world.set_block(edge_cell, Block::RedstoneBlock);
        let edge_tokens = vec![PistonToken { block: Block::RedstoneBlock, position: Some(edge_cell), cells: vec![edge_cell] }];
        let edge_bounds = (BlockPos::new(0, 10, 0), BlockPos::new(8, 20, 8));
        let edge_compile = |cached: bool| {
            let entry = ScratchEntry { scheduled: vec![], piston: edge_world.piston_state().clone() };
            let mut scratch = ScratchWorld::from_world_monitored(&edge_world, edge_bounds, entry, None).unwrap();
            WireGraph::compile_with_cache(&mut scratch, vec![edge], &edge_tokens, None, cached.then_some(&cache)).map(|graph| graph.nodes)
        };
        assert_eq!(edge_compile(false).unwrap(), edge_compile(true).unwrap());
        let stats = cache.lock().unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!((stats.hits, stats.misses), (0, 1));
    }

    #[test]
    fn cached_wire_tables_preserve_overlapping_token_states() {
        let mut world = super::super::tests::host();
        let wire = BlockPos::new(20, 15, 20);
        let cell = wire.offset(BlockFace::West);
        world.set_block(wire, Block::RedstoneWire { wire: RedstoneWire::default() });
        world.set_block(cell, Block::RedstoneBlock);
        let tokens = vec![PistonToken { block: Block::RedstoneBlock, position: Some(cell), cells: vec![cell] }; 2];
        let bounds = (BlockPos::new(8, 10, 8), BlockPos::new(32, 20, 32));
        let cache = Mutex::new(WireTableCache::new(1_000_000));
        let compile = |cached: bool| {
            let entry = ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() };
            let mut scratch = ScratchWorld::from_world_monitored(&world, bounds, entry, None).unwrap();
            WireGraph::compile_with_cache(&mut scratch, vec![wire], &tokens, None, cached.then_some(&cache)).unwrap().nodes
        };
        let expected = compile(false);
        assert_eq!(expected[0].ports.len(), 4);
        assert!(expected[0].ports[3].is_none());
        assert_eq!(expected, compile(true));
        assert_eq!(expected, compile(true));
        assert_eq!(cache.lock().unwrap().hits, 1);
    }

    #[test]
    fn poisoned_wire_cache_falls_back_to_uncached_compilation() {
        let mut world = super::super::tests::host();
        let wire = BlockPos::new(20, 15, 20);
        let cell = wire.offset(BlockFace::West);
        world.set_block(wire, Block::RedstoneWire { wire: RedstoneWire::default() });
        world.set_block(cell, Block::RedstoneBlock);
        let tokens = vec![PistonToken { block: Block::RedstoneBlock, position: Some(cell), cells: vec![cell] }];
        let bounds = (BlockPos::new(8, 10, 8), BlockPos::new(32, 20, 32));
        let cache = Mutex::new(WireTableCache::new(1_000_000));
        std::thread::scope(|scope| {
            assert!(scope.spawn(|| { let _guard = cache.lock().unwrap(); panic!("poison cache"); }).join().is_err());
        });
        let build = |cached: bool| {
            let entry = ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() };
            let mut scratch = ScratchWorld::from_world_monitored(&world, bounds, entry, None).unwrap();
            WireGraph::compile_with_cache(&mut scratch, vec![wire], &tokens, None, cached.then_some(&cache)).unwrap()
        };
        let baseline = build(false);
        let recovered = build(true);
        assert_eq!(baseline.nodes, recovered.nodes);
        assert_eq!(recovered.cache_work.unwrap().unavailable, 1);
    }

    #[test]
    fn cancelling_a_running_cached_probe_does_not_publish_a_result() {
        let mut world = super::super::tests::host();
        let wire = BlockPos::new(20, 15, 20);
        let cell = wire.offset(BlockFace::West);
        world.set_block(wire, Block::RedstoneWire { wire: RedstoneWire::default() });
        world.set_block(cell, Block::RedstoneBlock);
        let tokens = vec![PistonToken { block: Block::RedstoneBlock, position: Some(cell), cells: vec![cell] }];
        let bounds = (BlockPos::new(8, 10, 8), BlockPos::new(32, 20, 32));
        let entry = ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() };
        let cache = Mutex::new(WireTableCache::new(1_000_000));
        let mut scratch = ScratchWorld::from_world_monitored(&world, bounds, entry.clone(), None).unwrap();
        WireGraph::compile_with_cache(&mut scratch, vec![wire], &tokens, None, Some(&cache)).unwrap();
        assert_eq!(cache.lock().unwrap().len(), 1);
        let mut scratch = ScratchWorld::from_world_monitored(&world, bounds, entry, None).unwrap();
        let monitor = TaskMonitor::default();
        std::thread::scope(|scope| {
            let guard = cache.lock().unwrap();
            let cache_ref = &cache;
            let monitor_ref = &monitor;
            let worker = scope.spawn(move || WireGraph::compile_with_cache(&mut scratch, vec![wire], &tokens, Some(monitor_ref), Some(cache_ref)));
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while !monitor.status().contains("0/1 probes") {
                assert!(!worker.is_finished() && std::time::Instant::now() < deadline, "worker did not start probing");
                std::thread::yield_now();
            }
            monitor.cancel();
            drop(guard);
            assert_eq!(worker.join().unwrap().unwrap_err().reason, "compilation cancelled");
        });
        assert_eq!(cache.lock().unwrap().len(), 1);
    }

    #[test]
    fn cached_wire_rows_still_obey_global_size_limit() {
        let mut world = super::super::tests::host();
        let mut positions = Vec::new();
        let mut tokens = Vec::new();
        for i in 0..257 {
            let wire = BlockPos::new(12 + (i % 16) * 4, 15, 12 + (i / 16) * 4);
            world.set_block(wire, Block::RedstoneWire { wire: RedstoneWire::default() });
            for face in BlockFace::values() {
                let cell = wire.offset(face);
                world.set_block(cell, Block::RedstoneBlock);
                for _ in 0..2 {
                    tokens.push(PistonToken { block: Block::RedstoneBlock, position: Some(cell), cells: vec![cell] });
                }
            }
            positions.push(wire);
        }
        let bounds = (BlockPos::new(8, 10, 8), BlockPos::new(80, 20, 80));
        let entry = ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() };
        let mut scratch = ScratchWorld::from_world_monitored(&world, bounds, entry, None).unwrap();
        let cache = Mutex::new(WireTableCache::new(128 * 1024 * 1024));
        let token_cells = index_token_cells(&tokens);
        for &wire in &positions {
            let axes = wire_token_axes(wire, &tokens, &token_cells);
            assert_eq!(axes.len(), 12);
            let footprint = manhattan_positions(wire, 2);
            let key = WireTableKey { wire, block_ids: std::array::from_fn(|i| scratch.world().get_block_raw(footprint[i])),
                axes: axes.iter().map(|&Axis::Token(token, _)| TokenKey { block_id: tokens[token].block.get_id(),
                    initial: tokens[token].position, cells: tokens[token].cells.clone() }).collect() };
            let ports = (0usize..4096).map(|state| {
                if state == 0 { Some(WirePorts { wires: vec![], sources: vec![] }) }
                else if state.count_ones() == 1 {
                    let Axis::Token(token, _) = axes[state.trailing_zeros() as usize];
                    Some(WirePorts { wires: vec![], sources: vec![tokens[token].cells[0]] })
                } else { None }
            }).collect();
            cache.lock().unwrap().insert(key, CachedWireTable { constant: false, ports });
        }
        assert_eq!(cache.lock().unwrap().len(), positions.len());
        let error = WireGraph::compile_with_cache(&mut scratch, positions, &tokens, None, Some(&cache)).unwrap_err();
        assert_eq!(error.reason, "local routing data exceeds size bound");
    }

    #[test]
    #[ignore = "diagnostic timing experiment"]
    fn benchmark_wire_table_cache_synthetic() {
        let mut world = super::super::tests::host();
        let mut positions = Vec::new();
        let mut tokens = Vec::new();
        for index in 0..512 {
            let wire = BlockPos::new(12 + (index % 16) * 4, 15, 12 + (index / 16) * 4);
            let cell = wire.offset(BlockFace::West);
            world.set_block(wire.offset(BlockFace::Bottom), Block::Stone {});
            world.set_block(wire, Block::RedstoneWire { wire: RedstoneWire::default() });
            world.set_block(cell, Block::RedstoneBlock);
            tokens.push(PistonToken { block: Block::RedstoneBlock, position: Some(cell), cells: vec![cell] });
            positions.push(wire);
        }
        let bounds = (BlockPos::new(8, 10, 8), BlockPos::new(80, 20, 144));
        let threads = cache_benchmark_threads();
        let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap();
        let mut records = Vec::new();
        for trial in 0..3 {
            let cache = Mutex::new(WireTableCache::new(32 * 1024 * 1024));
            let mut expected = None;
            let scenarios = if trial % 2 == 0 { ["uncached", "cold", "warm"] } else { ["cold", "warm", "uncached"] };
            for scenario in scenarios {
                let (hits_before, misses_before) = { let stats = cache.lock().unwrap(); (stats.hits, stats.misses) };
                let entry = ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() };
                let mut scratch = ScratchWorld::from_world_monitored(&world, bounds, entry, None).unwrap();
                let start = std::time::Instant::now();
                let graph = pool.install(|| WireGraph::compile_with_cache(&mut scratch, positions.clone(), &tokens, None,
                    (scenario != "uncached").then_some(&cache))).unwrap();
                let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                let node_elapsed_ns = graph.node_elapsed_ns;
                let work = graph.cache_work.clone();
                if let Some(expected) = &expected { assert_eq!(&graph.nodes, expected); } else { expected = Some(graph.nodes); }
                let stats = cache.lock().unwrap();
                records.push(serde_json::json!({"trial": trial + 1, "scenario":scenario, "elapsed_ms":elapsed_ms,
                    "node_elapsed_ns":node_elapsed_ns, "work":work, "hits":stats.hits-hits_before, "misses":stats.misses-misses_before,
                    "evictions_total":stats.evictions, "oversized_total":stats.oversized, "duplicates_total":stats.duplicates,
                    "retained_bytes":stats.retained(), "peak_retained_bytes":stats.peak, "entries":stats.len(), "semantic_match":true}));
            }
            for (scenario, edit) in [("distant_edit", BlockPos::new(10, 16, 10)), ("local_edit", positions[0].offset(BlockFace::Top))] {
                assert_eq!(world.get_block(edit), Block::Air {});
                world.set_block(edit, Block::Stone {});
                let (hits_before, misses_before) = { let stats = cache.lock().unwrap(); (stats.hits, stats.misses) };
                let mut results = Vec::new();
                for cached in [false, true] {
                    let entry = ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() };
                    let mut scratch = ScratchWorld::from_world_monitored(&world, bounds, entry, None).unwrap();
                    let start = std::time::Instant::now();
                    let graph = pool.install(|| WireGraph::compile_with_cache(&mut scratch, positions.clone(), &tokens, None,
                        cached.then_some(&cache))).unwrap();
                    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                    let stats = cache.lock().unwrap();
                    if cached && scenario == "distant_edit" { assert_eq!(stats.misses, misses_before); }
                    if cached && scenario == "local_edit" { assert!(stats.misses > misses_before); }
                    records.push(serde_json::json!({"trial":trial + 1,
                        "scenario":if cached { scenario.to_string() } else { format!("{scenario}_uncached") }, "elapsed_ms":elapsed_ms,
                        "node_elapsed_ns":graph.node_elapsed_ns, "work":graph.cache_work,
                        "hits":stats.hits-hits_before, "misses":stats.misses-misses_before, "evictions_total":stats.evictions,
                        "oversized_total":stats.oversized, "duplicates_total":stats.duplicates, "retained_bytes":stats.retained(),
                        "peak_retained_bytes":stats.peak, "entries":stats.len(), "semantic_match":true}));
                    results.push(graph.nodes);
                }
                assert_eq!(results[0], results[1]);
                world.set_block(edit, Block::Air {});
            }
        }
        let cache = Mutex::new(WireTableCache::new(32 * 1024 * 1024));
        let mut results = Vec::new();
        for threads in [1, 4] {
            let entry = ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() };
            let mut scratch = ScratchWorld::from_world_monitored(&world, bounds, entry, None).unwrap();
            let workers = rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap();
            results.push(workers.install(|| WireGraph::compile_with_cache(&mut scratch, positions.clone(), &tokens, None, Some(&cache)))
                .unwrap().nodes);
        }
        assert_eq!(results[0], results[1]);
        assert!(cache.lock().unwrap().hits > 0);
        println!("{}", serde_json::to_string(&serde_json::json!({"fixture":"synthetic", "wires":positions.len(),
            "threads":threads, "budget_bytes":32 * 1024 * 1024, "bypass_order":["entry", "context", "writes", "entities"],
            "work_times":"summed worker nanoseconds; overlap is possible", "records":records})).unwrap());
    }

    #[test]
    #[ignore = "real-fixture timing experiment"]
    fn benchmark_wire_table_cache_cpu() {
        use super::super::{compiler, cpu_trace::cpus};
        let name = std::env::var("MCHPRS_WIRE_CACHE_FIXTURE").unwrap_or_else(|_| "zcore".into());
        let cpu = match name.as_str() {
            "zcore" => cpus::CPUS[3],
            "pm1" => cpus::Cpu { schematic: "PM1_SORT_FIXED.schem",
                sha256: "ad68a0160990c72612c0105802fade337758de127dcdbd4cc1a195059ad2d893", ..cpus::CPUS[0] },
            _ => panic!("expected zcore or pm1"),
        };
        let mut world = cpus::load_cpu(cpu);
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data").join(cpu.schematic);
        let clip = crate::plot::worldedit::load_schematic(std::fs::File::open(path).unwrap()).unwrap();
        let bounds = (cpu.origin, cpu.origin + BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1));
        let entry = ScratchEntry { scheduled: world.scheduler().iter_entries().collect(), piston: world.piston_state().clone() };
        let actors = compiler::discover_piston_actors(&world, bounds, entry.clone()).unwrap();
        let tokens = compiler::discover_piston_tokens(&world, &actors).unwrap();
        let mut positions = Vec::new();
        crate::world::for_each_block_optimized(&world, bounds.0, bounds.1, |pos| {
            if matches!(world.get_block(pos), Block::RedstoneWire { .. }) { positions.push(pos); }
        });
        positions.sort_unstable_by_key(|pos| (pos.y, pos.z, pos.x));
        let protected: HashSet<_> = actors.iter().flat_map(|actor| [actor.base, actor.home, actor.far])
            .chain(tokens.iter().flat_map(|token| token.position.into_iter().chain(token.cells.iter().copied()))).collect();
        let token_cells = index_token_cells(&tokens);
        let in_bounds = |pos: BlockPos| pos.x >= bounds.0.x && pos.x <= bounds.1.x && pos.y >= bounds.0.y
            && pos.y <= bounds.1.y && pos.z >= bounds.0.z && pos.z <= bounds.1.z;
        let edits = positions.iter().find_map(|&wire| {
            if wire_token_axes(wire, &tokens, &token_cells).is_empty() { return None; }
            let local = wire.offset(BlockFace::Top);
            if !in_bounds(local) || protected.contains(&local) || world.get_block(local) != (Block::Air {}) { return None; }
            let chunk_x = wire.x.div_euclid(16) * 16;
            let chunk_z = wire.z.div_euclid(16) * 16;
            (chunk_x..chunk_x + 16).flat_map(|x| (chunk_z..chunk_z + 16).map(move |z| BlockPos::new(x, local.y, z)))
                .find(|&pos| in_bounds(pos) && pos.x.abs_diff(wire.x) + pos.y.abs_diff(wire.y) + pos.z.abs_diff(wire.z) > 2
                    && !protected.contains(&pos) && world.get_block(pos) == (Block::Air {}))
                .map(|distant| (wire, local, distant))
        }).expect("fixture must have an editable local and distant air cell");
        let threads = cache_benchmark_threads();
        let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap();
        let mut records = Vec::new();
        for trial in 0..3 {
            let cache = Mutex::new(WireTableCache::new(32 * 1024 * 1024));
            let mut expected = None;
            let scenarios = if trial % 2 == 0 { ["uncached", "cold", "warm"] } else { ["cold", "warm", "uncached"] };
            for scenario in scenarios {
                let (hits_before, misses_before) = { let stats = cache.lock().unwrap(); (stats.hits, stats.misses) };
                let mut scratch = ScratchWorld::from_world_monitored(&world, bounds, entry.clone(), None).unwrap();
                let start = std::time::Instant::now();
                let graph = pool.install(|| WireGraph::compile_with_cache(&mut scratch, positions.clone(), &tokens, None,
                    (scenario != "uncached").then_some(&cache))).unwrap();
                let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                let node_elapsed_ns = graph.node_elapsed_ns;
                let work = graph.cache_work.clone();
                if let Some(expected) = &expected { assert_eq!(&graph.nodes, expected); } else { expected = Some(graph.nodes); }
                let stats = cache.lock().unwrap();
                records.push(serde_json::json!({"trial":trial + 1, "scenario":scenario, "elapsed_ms":elapsed_ms,
                    "node_elapsed_ns":node_elapsed_ns, "work":work, "hits":stats.hits-hits_before, "misses":stats.misses-misses_before,
                    "evictions_total":stats.evictions, "oversized_total":stats.oversized, "duplicates_total":stats.duplicates,
                    "retained_bytes":stats.retained(), "peak_retained_bytes":stats.peak, "entries":stats.len(), "semantic_match":true}));
            }
            for (scenario, edit) in [("distant_edit", edits.2), ("local_edit", edits.1)] {
                world.set_block(edit, Block::Stone {});
                let (hits_before, misses_before) = { let stats = cache.lock().unwrap(); (stats.hits, stats.misses) };
                let mut results = Vec::new();
                for cached in [false, true] {
                    let mut scratch = ScratchWorld::from_world_monitored(&world, bounds, entry.clone(), None).unwrap();
                    let start = std::time::Instant::now();
                    let graph = pool.install(|| WireGraph::compile_with_cache(&mut scratch, positions.clone(), &tokens, None,
                        cached.then_some(&cache))).unwrap();
                    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                    let stats = cache.lock().unwrap();
                    if cached && scenario == "local_edit" && stats.evictions == 0 && stats.len() > 0 {
                        assert!(stats.misses > misses_before, "local edit did not invalidate a retained table");
                    }
                    records.push(serde_json::json!({"trial":trial + 1,
                        "scenario":if cached { scenario.to_string() } else { format!("{scenario}_uncached") }, "elapsed_ms":elapsed_ms,
                        "node_elapsed_ns":graph.node_elapsed_ns, "work":graph.cache_work,
                        "hits":stats.hits - hits_before, "misses":stats.misses - misses_before,
                        "evictions_total":stats.evictions, "oversized_total":stats.oversized, "duplicates_total":stats.duplicates,
                        "retained_bytes":stats.retained(), "peak_retained_bytes":stats.peak, "entries":stats.len(), "semantic_match":true}));
                    results.push(graph.nodes);
                }
                assert_eq!(results[0], results[1]);
                world.set_block(edit, Block::Air {});
            }
        }
        println!("{}", serde_json::to_string(&serde_json::json!({"fixture":cpu.schematic, "sha256":cpu.sha256,
            "wires":positions.len(), "tokens":tokens.len(), "pending_ticks":entry.scheduled.len(),
            "edit_wire":edits.0, "local_edit":edits.1, "distant_edit":edits.2,
            "threads":threads, "budget_bytes":32 * 1024 * 1024, "bypass_order":["entry", "context", "writes", "entities"],
            "work_times":"summed worker nanoseconds; overlap is possible", "records":records})).unwrap());
    }

    #[test]
    fn parallel_topology_and_arrangements_match_serial_discovery() {
        let discover = |threads| rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap().install(|| {
            let mut world = super::super::tests::host();
            let mut wires = Vec::new();
            let mut tokens = Vec::new();
            for index in 0..512 {
                let wire = BlockPos::new(12 + (index % 16) * 4, 15, 12 + (index / 16) * 4);
                world.set_block(wire.offset(BlockFace::Bottom), Block::Stone {});
                world.set_block(wire, Block::RedstoneWire { wire: RedstoneWire::default() });
                let cell = wire.offset(BlockFace::West);
                world.set_block(cell, Block::RedstoneBlock);
                tokens.push(PistonToken { block: Block::RedstoneBlock, position: Some(cell), cells: vec![cell] });
                wires.push(wire);
            }
            let mut program = Program::new(wires.clone(), wires.iter().map(|&pos| world.get_block(pos)).collect(),
                vec![], vec![[vec![], vec![]]; wires.len()], vec![], vec![], vec![]).unwrap();
            program.tokens = tokens;
            let components = wires.iter().enumerate().map(|(id, &pos)| (pos, id)).collect();
            let indexes = ProbeIndexes::new(&components, &program.positions, &program.initial, &program.tokens, None).unwrap();
            let entry = ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() };
            let bounds = (BlockPos::new(8, 10, 8), BlockPos::new(80, 20, 144));
            let discovery = super::super::group_probe::run(&world, bounds, &entry, &program, &indexes, None).unwrap();
            assert_eq!(discovery.groups.len(), wires.len());
            discovery.groups.into_iter().map(|group| {
                assert_eq!(group.tokens, vec![group.target]);
                assert_eq!(group.conditional_sources, vec![ArrangementSource::Token(group.target)]);
                (group.target, group.side, group.tokens, group.inputs)
            }).collect::<Vec<_>>()
        });
        assert_eq!(discover(1), discover(4));
    }

    #[test]
    fn routed_search_enforces_wire_visit_limit_and_can_restart() {
        let root = BlockPos::new(0, 0, 0);
        let next = BlockPos::new(1, 0, 0);
        let graph = WireGraph {
            positions: vec![root, next], index: HashMap::from([(root, 0), (next, 1)]), token_cells: HashMap::new(),
            nodes: vec![Local { axes: vec![], ports: vec![Some(Ports { wires: vec![1], sources: vec![] })] },
                Local { axes: vec![], ports: vec![Some(Ports { wires: vec![], sources: vec![] })] }],
            source_ids: HashMap::new(), readers: vec![], by_component: vec![], by_token: vec![],
            cache_work: None, node_elapsed_ns: 0,
        };
        let mut search = Search { remaining: Some(1), ..Default::default() };
        let error = graph.sources(root, |_| 0, &mut search).unwrap_err();
        assert_eq!(error.reason, "runtime semantic operation limit exceeded");
        assert_eq!(error.position, Some(next));
        assert_eq!(search.spent, 1);
        search.remaining = Some(2);
        search.spent = 0;
        assert!(graph.sources(root, |_| 0, &mut search).unwrap().is_empty());
        assert_eq!(search.spent, 2);
        assert_eq!(search.remaining, Some(0));
    }


    #[test]
    fn spatial_candidate_indexes_match_brute_force_with_overlapping_tokens() {
        use mchprs_blocks::{BlockFacing, blocks::RedstonePiston};

        let plain_pos = BlockPos::new(0, 20, 0);
        let piston_pos = BlockPos::new(30, 20, 0);
        let piston = Block::Piston { piston: RedstonePiston { facing: BlockFacing::East, sticky: false, extended: false } };
        let piston_cells: HashSet<_> = BlockFace::values().into_iter()
            .filter(|&face| face != BlockFace::East).map(|face| piston_pos.offset(face)).collect();
        let above = piston_pos.offset(BlockFace::Top);
        let mut piston_cells = piston_cells;
        piston_cells.extend(BlockFace::values().into_iter().map(|face| above.offset(face)));

        let overlap = BlockPos::new(2, 20, 0);
        let adjacent_to_piston_wire = piston_pos.offset(BlockFace::West).offset(BlockFace::North);
        let excluded_piston_front = piston_pos.offset(BlockFace::East);
        let tokens = vec![
            PistonToken { block: Block::Stone {}, position: None, cells: vec![overlap, piston_pos.offset(BlockFace::West)] },
            PistonToken { block: Block::RedstoneBlock, position: None, cells: vec![overlap, adjacent_to_piston_wire] },
            PistonToken { block: Block::Stone {}, position: None, cells: vec![plain_pos + BlockPos::new(3, 0, 0), excluded_piston_front] },
            PistonToken { block: Block::Stone {}, position: None, cells: vec![BlockPos::new(80, 80, 80)] },
        ];
        let token_cells = index_token_cells(&tokens);

        let mut wire_positions = manhattan_positions(plain_pos, 2);
        wire_positions.push(plain_pos + BlockPos::new(3, 0, 0));
        for &cell in &piston_cells {
            wire_positions.push(cell);
            wire_positions.extend(adjacent_positions(cell));
        }
        wire_positions.push(piston_pos + BlockPos::new(4, 0, 0));
        let wire_index: HashMap<_, _> = wire_positions.iter().enumerate().map(|(index, &pos)| (pos, index)).collect();

        for wire in [plain_pos, piston_pos, overlap] {
            let expected: Vec<_> = tokens.iter().enumerate().filter_map(|(token, record)|
                record.cells.iter().any(|&cell| touches(wire, cell)).then_some(Axis::Token(token, record.cells.len()))).collect();
            assert_eq!(wire_token_axes(wire, &tokens, &token_cells), expected, "wire at {wire:?}");
        }

        for (position, block) in [(plain_pos, Block::Stone {}), (piston_pos, piston)] {
            let old_piston_cells = if let Block::Piston { piston } = block {
                let mut cells: HashSet<_> = BlockFace::values().into_iter()
                    .filter(|&face| face != piston.facing.into()).map(|face| position.offset(face)).collect();
                let above = position.offset(BlockFace::Top);
                cells.extend(BlockFace::values().into_iter().map(|face| above.offset(face)));
                Some(cells)
            } else { None };
            let radius = if old_piston_cells.is_some() { 3 } else { 2 };
            let old_contacts: HashSet<_> = wire_positions.iter().copied().filter(|&at|
                old_piston_cells.as_ref().map_or_else(
                    || at.x.abs_diff(position.x) + at.y.abs_diff(position.y) + at.z.abs_diff(position.z) <= radius,
                    |cells| cells.contains(&at) || cells.iter().any(|&cell| touches(cell, at)))).collect();
            let indexed_contacts = reader_contact_positions(position, old_piston_cells.as_ref(), &wire_index);
            assert_eq!(indexed_contacts, old_contacts, "contacts at {position:?}");

            let expected_axes: Vec<_> = tokens.iter().enumerate().filter_map(|(token, record)| record.cells.iter().any(|&cell|
                old_piston_cells.as_ref().map_or_else(
                    || cell.x.abs_diff(position.x) + cell.y.abs_diff(position.y) + cell.z.abs_diff(position.z) <= 2,
                    |cells| cells.contains(&cell))
                || BlockFace::values().iter().any(|&face| old_contacts.contains(&cell.offset(face))))
                .then_some(Axis::Token(token, record.cells.len()))).collect();
            assert_eq!(reader_token_axes(position, &tokens, &token_cells, old_piston_cells.as_ref(), &indexed_contacts), expected_axes,
                "reader axes at {position:?}");
        }
    }

    #[test]
    fn piston_reader_ignores_unreachable_cells_and_keeps_joint_contacts() {
        use mchprs_blocks::{BlockFacing, blocks::RedstonePiston};
        let mut world = super::super::tests::host();
        let pos = BlockPos::new(15, 15, 15);
        let piston = Block::Piston { piston: RedstonePiston { facing: BlockFacing::East, sticky: true, extended: false } };
        world.set_block(pos, piston);
        let contacts = [pos.offset(BlockFace::West), pos.offset(BlockFace::Top).offset(BlockFace::East)];
        let mut tokens: Vec<_> = contacts.iter().map(|&cell| PistonToken { block: Block::RedstoneBlock, position: None, cells: vec![cell] }).collect();
        let direct: HashSet<_> = BlockFace::values().into_iter().filter(|&face| face != BlockFace::East).map(|face| pos.offset(face))
            .chain(BlockFace::values().into_iter().map(|face| pos.offset(BlockFace::Top).offset(face))).collect();
        for dx in -2i32..=2 { for dy in -2i32..=2 { for dz in -2i32..=2 {
            if dx.abs() + dy.abs() + dz.abs() != 2 { continue; }
            let cell = pos + BlockPos::new(dx, dy, dz);
            if !direct.contains(&cell) { tokens.push(PistonToken { block: Block::Stone {}, position: None, cells: vec![cell] }); }
        } } }
        assert!(tokens.len() > 12);
        let entry = ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() };
        let bounds = (BlockPos::new(10, 10, 10), BlockPos::new(20, 20, 20));
        let mut scratch = ScratchWorld::from_world_monitored(&world, bounds, entry, None).unwrap();
        let mut graph = WireGraph::compile(&mut scratch, vec![], &tokens, None).unwrap();
        let mut program = Program::new(vec![pos], vec![piston], vec![], vec![[vec![], vec![]]], vec![], vec![], vec![]).unwrap();
        program.tokens = tokens;
        let components = HashMap::from([(pos, 0)]);
        let indexes = ProbeIndexes::new(&components, &program.positions, &program.initial, &program.tokens, None).unwrap();
        graph.compile_readers(&mut scratch, &program, &[0], &indexes, None).unwrap();
        assert_eq!(graph.readers[0].table.ports.len(), 4);
        for state in 0..4 {
            for (token, &cell) in contacts.iter().enumerate() {
                world.set_block(cell, if state & (1 << token) != 0 { Block::RedstoneBlock } else { Block::Air });
            }
            let actual = graph.reader_sources(0, |Axis::Token(token, _)| usize::from(state & (1 << token) != 0), &mut Search::default()).unwrap();
            let expected = super::super::group_probe::inputs_of(&world, &program, 0, None, &indexes).unwrap()
                .into_iter().map(|(key, (attenuation, _))| (key, attenuation)).collect();
            assert_eq!(actual, expected, "state {state}");
        }
    }

    #[test]
    #[ignore = "ANPU local-connectivity census and tick-278 path check, without full compilation"]
    fn anpu_local_graph_reproduces_the_hidden_source() {
        use super::super::{compiler, cpu_trace::cpus};
        let cpu = cpus::CPUS[1];
        let mut world = cpus::load_cpu(cpu);
        let clip = crate::plot::worldedit::load_schematic(std::fs::File::open(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data").join(cpu.schematic)).unwrap()).unwrap();
        let bounds = (cpu.origin, cpu.origin + BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1));
        let entry = ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() };
        let actors = compiler::discover_piston_actors(&world, bounds, entry.clone()).unwrap();
        let tokens = compiler::discover_piston_tokens(&world, &actors).unwrap();
        let mut wires = Vec::new();
        let mut components = Vec::new();
        crate::world::for_each_block_optimized(&world, bounds.0, bounds.1, |pos| {
            let block = world.get_block(pos);
            if matches!(block, Block::RedstoneWire { .. }) { wires.push(pos); }
            if block.pressure_plate_powered().is_some() || matches!(block,
                Block::RedstoneWire { .. } | Block::RedstoneBlock | Block::Lever { .. } | Block::StoneButton { .. }
                | Block::RedstoneLamp { .. } | Block::Hopper { .. } | Block::NoteBlock { .. } | Block::Piston { .. }
                | Block::Observer { .. } | Block::RedstoneRepeater { .. } | Block::RedstoneComparator { .. }
                | Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. }) { components.push(pos); }
        });
        let mut scratch = ScratchWorld::from_world_monitored(&world, bounds, entry, None).unwrap();
        let started = std::time::Instant::now();
        let mut graph = WireGraph::compile(&mut scratch, wires, &tokens, None).unwrap();
        let build_ms = started.elapsed().as_secs_f64() * 1000.0;
        let outcomes: usize = graph.nodes.iter().map(|node| node.ports.len()).sum();
        let max_outcomes = graph.nodes.iter().map(|node| node.ports.len()).max().unwrap_or(0);
        let initial: Vec<_> = components.iter().map(|&pos| world.get_block(pos)).collect();
        let repeaters = initial.iter().enumerate().filter_map(|(component, block)| matches!(block, Block::RedstoneRepeater { .. })
            .then_some(super::super::program::RepeaterRecord { component, output_is_diode: false, side_sources: None })).collect();
        let component_index: HashMap<_, _> = components.iter().enumerate().map(|(node, &pos)| (pos, node)).collect();
        let mut program = Program::new(components, initial, vec![], vec![[vec![], vec![]]; component_index.len()], vec![], vec![], repeaters).unwrap();
        program.tokens = tokens.clone();
        let indexes = ProbeIndexes::new(&component_index, &program.positions, &program.initial, &program.tokens, None).unwrap();
        let mut kinds = HashSet::new();
        let mut targets = vec![component_index[&BlockPos::new(99, 49, 109)], component_index[&BlockPos::new(84, 36, 76)]];
        targets.extend(program.initial.iter().enumerate().filter_map(|(node, block)| kinds.insert(block.get_name()).then_some(node)));
        targets.sort_unstable(); targets.dedup();
        graph.compile_readers(&mut scratch, &program, &targets, &indexes, None).unwrap();
        let removed: HashSet<_> = tokens.iter().enumerate().filter_map(|(token, record)| record.cells.iter().any(|p|
            [99, 100].contains(&p.x) && p.y == 50 && [109, 111, 113, 115, 117].contains(&p.z)).then_some(token)).collect();
        assert_eq!(removed.len(), 5);
        let mut digits: Vec<_> = tokens.iter().map(|record| record.position.and_then(|pos| record.cells.iter().position(|&cell| cell == pos)).map_or(0, |cell| cell + 1)).collect();
        for &token in &removed {
            digits[token] = 0;
            if let Some(pos) = tokens[token].position { world.set_block(pos, Block::Air); }
        }
        let root = BlockPos::new(99, 49, 109);
        let actual = graph.sources(root, |Axis::Token(token, _)| digits[token], &mut Search::default()).unwrap();
        let expected: Vec<_> = discover_wire_sources(&world, &[root]).unwrap().into_iter().map(|s| (s.source, s.attenuation)).collect();
        assert_eq!(actual, expected);
        assert!(actual.contains(&(BlockPos::new(98, 48, 118), 10)));
        for (reader, descriptor) in graph.readers.iter().enumerate() {
            let actual = graph.reader_sources(reader, |Axis::Token(token, _)| digits[token], &mut Search::default()).unwrap();
            let expected = super::super::group_probe::inputs_of(&world, &program, descriptor.component, None, &indexes).unwrap();
            let expected: BTreeMap<_, _> = expected.into_iter().map(|(key, (attenuation, _))| (key, attenuation)).collect();
            assert_eq!(actual, expected, "reader at {:?}", descriptor.position);
        }
        eprintln!("ANPU local wire graph: {} wires, {} tokens, {outcomes} local outcomes, largest={max_outcomes}, build={build_ms:.1} ms; hidden torch attenuation=10", graph.nodes.len(), tokens.len());
    }

    #[test]
    fn local_graph_matches_extraction_without_a_seed_and_with_alternate_routes() {
        for detour in [false, true] {
            let mut world = super::super::tests::host();
            let mut wires: Vec<_> = (8..=16).map(|x| BlockPos::new(x, 10 + x % 2, 10)).collect();
            if detour {
                wires.extend((8..=16).map(|x| BlockPos::new(x, 10, 12)));
                wires.extend([BlockPos::new(8, 10, 11), BlockPos::new(16, 10, 11)]);
            }
            for &wire in &wires {
                world.set_block(wire.offset(BlockFace::Bottom), Block::Stone {});
                world.set_block(wire, Block::RedstoneWire { wire: RedstoneWire::default() });
            }
            let source = BlockPos::new(17, 10, 10);
            world.set_block(source, Block::RedstoneBlock);
            let tokens: Vec<_> = [10, 14].into_iter().map(|x| {
                let cell = BlockPos::new(x, 11, 10);
                world.set_block(cell, Block::Stone {});
                PistonToken { block: Block::Stone {}, position: Some(cell), cells: vec![cell] }
            }).collect();
            let entry = ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() };
            let mut scratch = ScratchWorld::from_world_monitored(&world, (BlockPos::new(5, 5, 5), BlockPos::new(22, 15, 15)), entry, None).unwrap();
            let graph = WireGraph::compile(&mut scratch, wires.clone(), &tokens, None).unwrap();
            let mut search = Search::default();
            for state in 0..4 {
                for (token, record) in tokens.iter().enumerate() {
                    world.set_block(record.cells[0], if state & (1 << token) == 0 { Block::Air } else { record.block });
                }
                for &root in &wires {
                    let expected: Vec<_> = discover_wire_sources(&world, &[root]).unwrap().into_iter().map(|s| (s.source, s.attenuation)).collect();
                    let actual = graph.sources(root, |axis| match axis { Axis::Token(token, _) => usize::from(state & (1 << token) != 0) }, &mut search).unwrap();
                    assert_eq!(actual, expected, "detour={detour}, state={state}, root={root:?}");
                }
                let root = wires[0];
                let sources = graph.sources(root, |axis| match axis { Axis::Token(token, _) => usize::from(state & (1 << token) != 0) }, &mut search).unwrap();
                assert_eq!(sources.first().map(|s| s.1), if state == 0 { Some(8) } else if detour { Some(12) } else { None });
            }
        }
    }
}
