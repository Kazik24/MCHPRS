//! Local dust connectivity for inputs that do not fit a dense arrangement table.
use super::{Program, ScratchError, ScratchWorld, group_probe::ProbeIndexes, pistons::PistonToken, program::ArrangementSource};
use crate::redpilator::{TaskMonitor, scratch::discover_wire_ports};
use crate::world::World;
use mchprs_blocks::{BlockPos, blocks::Block};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Axis {
    Token(usize, usize),
}

impl Axis {
    fn radix(self) -> usize {
        match self {
            Self::Token(_, cells) => cells + 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Ports {
    pub(super) wires: Vec<usize>,
    pub(super) sources: Vec<BlockPos>,
}

#[derive(Clone, Debug)]
pub(super) struct Local<T> {
    pub(super) axes: Vec<Axis>,
    pub(super) ports: Vec<Option<T>>,
}

type Node = Local<Ports>;
type TokenCells = HashMap<BlockPos, Vec<usize>>;

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
            if value >= axis.radix() {
                return None;
            }
            state += value * stride;
            stride *= axis.radix();
        }
        self.ports.get(state)?.as_ref()
    }
}

/// Sample local connectivity for every relevant token placement.
///
/// Inputs: A reusable scratch world, token records, token axes, position, monitor and evaluator callback.
///
/// Bound the product of axis sizes, restore scratch before each arrangement, reject cell collisions,
/// evaluate valid placements and fold a constant table to one row.
pub(super) fn compile_local<T: PartialEq>(
    scratch: &mut ScratchWorld,
    tokens: &[PistonToken],
    mut axes: Vec<Axis>,
    position: BlockPos,
    monitor: Option<&TaskMonitor>,
    evaluate: impl Fn(&crate::plot::PlotWorld) -> Result<T, ScratchError>,
) -> Result<Local<T>, ScratchError> {
    let states = axes.iter().try_fold(1usize, |count, axis| count.checked_mul(axis.radix()));
    let Some(states) = states.filter(|&count| count <= 4096) else {
        return Err(ScratchError {
            position: Some(position),
            reason: "local connectivity exceeds state bound",
        });
    };
    let mut ports = Vec::with_capacity(states);
    for state in 0..states {
        if monitor.is_some_and(TaskMonitor::cancelled) {
            return Err(ScratchError {
                position: None,
                reason: "compilation cancelled",
            });
        }
        scratch.revert();
        for &Axis::Token(token, _) in &axes {
            if let Some(initial) = tokens[token].position {
                scratch.world_mut().set_block(initial, Block::Air);
            }
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
        if valid {
            ports.push(Some(evaluate(scratch.world())?));
        } else {
            ports.push(None);
        }
        scratch.finish_probe()?;
    }
    if ports.iter().all(|entry| entry == &ports[0]) {
        axes.clear();
        ports.truncate(1);
    }
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
}

#[derive(Default)]
pub(super) struct Search {
    distances: Vec<u8>,
    visited: Vec<usize>,
    queue: VecDeque<usize>,
    pub(super) updates: Vec<(usize, u8)>,
    pub(super) remaining: Option<usize>,
    pub(super) spent: usize,
}

#[cfg(test)]
fn touches(a: BlockPos, b: BlockPos) -> bool {
    a.x.abs_diff(b.x) + a.y.abs_diff(b.y) + a.z.abs_diff(b.z) == 1
}

fn checked_offset(position: BlockPos, dx: i32, dy: i32, dz: i32) -> Option<BlockPos> {
    Some(BlockPos::new(
        position.x.checked_add(dx)?,
        position.y.checked_add(dy)?,
        position.z.checked_add(dz)?,
    ))
}

fn adjacent_positions(position: BlockPos) -> impl Iterator<Item = BlockPos> {
    [
        checked_offset(position, 0, -1, 0),
        checked_offset(position, 0, 1, 0),
        checked_offset(position, 0, 0, -1),
        checked_offset(position, 0, 0, 1),
        checked_offset(position, -1, 0, 0),
        checked_offset(position, 1, 0, 0),
    ]
    .into_iter()
    .flatten()
}

/// Enumerate nearby positions within a taxi-cab distance.
///
/// Inputs: The center position and distance limit.
///
/// Visit offset triples whose absolute coordinate sum stays within the limit, using checked coordinate
/// offsets to avoid overflow.
fn manhattan_positions(center: BlockPos, radius: i32) -> Vec<BlockPos> {
    let mut positions = Vec::with_capacity(match radius {
        2 => 25,
        3 => 63,
        _ => 1,
    });
    for dx in -radius..=radius {
        for dy in -radius..=radius {
            for dz in -radius..=radius {
                if dx.abs() + dy.abs() + dz.abs() <= radius {
                    if let Some(position) = checked_offset(center, dx, dy, dz) {
                        positions.push(position);
                    }
                }
            }
        }
    }
    positions
}

fn index_token_cells(tokens: &[PistonToken]) -> TokenCells {
    let mut cells: TokenCells = HashMap::new();
    for (token, record) in tokens.iter().enumerate() {
        for &cell in &record.cells {
            cells.entry(cell).or_default().push(token);
        }
    }
    cells
}

fn append_tokens_at(index: &TokenCells, position: BlockPos, into: &mut Vec<usize>) {
    if let Some(tokens) = index.get(&position) {
        into.extend_from_slice(tokens);
    }
}

fn axes_for_tokens(tokens: &[PistonToken], mut candidates: Vec<usize>) -> Vec<Axis> {
    candidates.sort_unstable();
    candidates.dedup();
    candidates
        .into_iter()
        .map(|token| Axis::Token(token, tokens[token].cells.len()))
        .collect()
}

fn wire_token_axes(position: BlockPos, tokens: &[PistonToken], token_cells: &TokenCells) -> Vec<Axis> {
    let mut candidates = Vec::new();
    for cell in adjacent_positions(position) {
        append_tokens_at(token_cells, cell, &mut candidates);
    }
    axes_for_tokens(tokens, candidates)
}

fn piston_contact_cells(position: BlockPos, block: Block) -> Option<HashSet<BlockPos>> {
    let Block::Piston { piston } = block else {
        return None;
    };
    let mut cells: HashSet<_> = mchprs_blocks::BlockFace::values()
        .into_iter()
        .filter(|&face| face != piston.facing.into())
        .map(|face| position.offset(face))
        .collect();
    let above = position.offset(mchprs_blocks::BlockFace::Top);
    cells.extend(mchprs_blocks::BlockFace::values().into_iter().map(|face| above.offset(face)));
    Some(cells)
}

/// Collect the local cells that can conduct into a reader.
///
/// Inputs: The reader position, optional piston contact cells and fixed-wire index.
///
/// Enumerate the appropriate nearby/contact positions, retaining the cells needed to discover wire and
/// conductor dependencies.
fn reader_contact_positions(
    position: BlockPos,
    piston_cells: Option<&HashSet<BlockPos>>,
    wire_index: &HashMap<BlockPos, usize>,
) -> HashSet<BlockPos> {
    let mut contacts = HashSet::new();
    if let Some(cells) = piston_cells {
        for &cell in cells {
            if wire_index.contains_key(&cell) {
                contacts.insert(cell);
            }
            for adjacent in adjacent_positions(cell) {
                if wire_index.contains_key(&adjacent) {
                    contacts.insert(adjacent);
                }
            }
        }
    } else {
        for at in manhattan_positions(position, 2) {
            if wire_index.contains_key(&at) {
                contacts.insert(at);
            }
        }
    }
    contacts
}

/// Choose the tokens that can change a reader's direct connectivity.
///
/// Inputs: The reader position, token records, cell index and reader contact positions.
///
/// Collect tokens touching relevant contacts or piston power cells and turn their legal placement counts
/// into arrangement axes.
fn reader_token_axes(
    position: BlockPos,
    tokens: &[PistonToken],
    token_cells: &TokenCells,
    piston_cells: Option<&HashSet<BlockPos>>,
    contacts: &HashSet<BlockPos>,
) -> Vec<Axis> {
    let mut candidates = Vec::new();
    if let Some(cells) = piston_cells {
        for &cell in cells {
            append_tokens_at(token_cells, cell, &mut candidates);
        }
    } else {
        for cell in manhattan_positions(position, 2) {
            append_tokens_at(token_cells, cell, &mut candidates);
        }
    }
    for &contact in contacts {
        for cell in adjacent_positions(contact) {
            append_tokens_at(token_cells, cell, &mut candidates);
        }
    }
    axes_for_tokens(tokens, candidates)
}

impl WireGraph {
    pub(super) fn reader_count(&self) -> usize {
        self.readers.len()
    }
    pub(super) fn reader_component(&self, reader: usize) -> usize {
        self.readers[reader].component
    }
    pub(super) fn reader_dependencies(&self, reader: usize) -> &[usize] {
        &self.readers[reader].dependencies
    }
    pub(super) fn retain_readers(&mut self, targets: &BTreeSet<usize>) {
        self.readers.retain(|reader| targets.contains(&reader.component));
        self.reindex();
    }
    /// Rebuild lookup lists after the routing reader list changes.
    ///
    /// Inputs: This graph's readers, their component IDs and token dependencies.
    ///
    /// Clear the reverse indexes, then record each reader under its component and every token that can
    /// change its connectivity.
    fn reindex(&mut self) {
        self.by_component.fill(None);
        for readers in &mut self.by_token {
            readers.clear();
        }
        for (index, reader) in self.readers.iter().enumerate() {
            self.by_component[reader.component] = Some(index);
            for &token in &reader.dependencies {
                self.by_token[token].push(index);
            }
        }
    }
    pub(super) fn reader_for_component(&self, component: usize) -> Option<usize> {
        self.by_component[component]
    }
    pub(super) fn readers_for_token(&self, token: usize) -> &[usize] {
        &self.by_token[token]
    }
    pub(super) fn links(&self, reader: usize) -> &[usize] {
        &self.readers[reader].links
    }
    /// Attach lowered arrangement-link IDs to their routing readers.
    ///
    /// Inputs: The arrangement links and this graph's component-to-reader index.
    ///
    /// Clear existing bindings and assign each link to the reader responsible for its target component.
    pub(super) fn bind_links(&mut self, links: &[super::program::ArrangementLink]) {
        for reader in &mut self.readers {
            reader.links.clear();
        }
        for (index, link) in links.iter().enumerate() {
            if let Some(reader) = self.reader_for_component(link.target) {
                self.readers[reader].links.push(index);
            }
        }
    }

    /// Find every source a reader could reach in any sampled arrangement.
    ///
    /// Inputs: The reader index and this graph's alternative local port tables.
    ///
    /// Start from all direct sources and wire roots, traverse the union of possible wire contacts up to
    /// signal range, and merge the possible source/side identities.
    pub(super) fn potential_sources(&self, reader: usize) -> BTreeMap<(ArrangementSource, bool), u8> {
        let mut found = BTreeMap::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        for ports in self.readers[reader].table.ports.iter().flatten() {
            for &source in &ports.direct {
                found.insert(source, 0);
            }
            for &(root, side) in &ports.roots {
                if visited.insert((root, side)) {
                    queue.push_back((root, side, 0u8));
                }
            }
        }
        while let Some((wire, side, distance)) = queue.pop_front() {
            for ports in self.nodes[wire].ports.iter().flatten() {
                for pos in &ports.sources {
                    found
                        .entry((self.source_ids[pos], side))
                        .and_modify(|old| *old = (*old).min(distance))
                        .or_insert(distance);
                }
                if distance == 14 {
                    continue;
                }
                for &next in &ports.wires {
                    if visited.insert((next, side)) {
                        queue.push_back((next, side, distance + 1));
                    }
                }
            }
        }
        found
    }

    /// Build movement-sensitive wire connectivity tables.
    ///
    /// Inputs: The scratch world, fixed wire positions, movable tokens and optional monitor.
    ///
    /// Reject unsupported payloads/overlaps, sample each wire's local token arrangements in isolated jobs,
    /// and enforce the retained local-table row limit.
    pub(super) fn compile(
        scratch: &mut ScratchWorld,
        positions: Vec<BlockPos>,
        tokens: &[PistonToken],
        monitor: Option<&TaskMonitor>,
    ) -> Result<Self, ScratchError> {
        let index: HashMap<_, _> = positions.iter().enumerate().map(|(node, &pos)| (pos, node)).collect();
        for token in tokens {
            if token.block != Block::RedstoneBlock
                && token.block.slab_type().is_none()
                && (token.block.pressure_plate_powered().is_some() || !super::pistons::is_passive_geometry(token.block))
            {
                return Err(ScratchError {
                    position: token.position,
                    reason: "unsupported routed payload",
                });
            }
            let at = token.position.or_else(|| token.cells.first().copied()).ok_or(ScratchError {
                position: None,
                reason: "routed payload has no cells",
            })?;
            if mchprs_blocks::BlockFace::values()
                .iter()
                .any(|&face| crate::redstone::power::emits_strong_power(token.block, scratch.world(), at, face, false))
            {
                return Err(ScratchError {
                    position: Some(at),
                    reason: "routed payload emits strong power",
                });
            }
        }
        if tokens.iter().flat_map(|token| &token.cells).any(|cell| index.contains_key(cell)) {
            return Err(ScratchError {
                position: None,
                reason: "movable payload overlaps a fixed wire",
            });
        }
        let token_cells = index_token_cells(tokens);
        let rows = AtomicUsize::new(0);
        let nodes = scratch.probe_jobs(&positions, monitor, |scratch, &pos| {
            // Non-wire payloads cannot alter a distance-two strong-source read.
            let axes = wire_token_axes(pos, tokens, &token_cells);
            let node = compile_local(scratch, tokens, axes, pos, monitor, |world| {
                let found = discover_wire_ports(world, pos)?;
                let wires = found
                    .wires
                    .into_iter()
                    .map(|at| {
                        index.get(&at).copied().ok_or(ScratchError {
                            position: Some(at),
                            reason: "unrepresented routed wire",
                        })
                    })
                    .collect::<Result<_, _>>()?;
                Ok(Ports {
                    wires,
                    sources: found.sources,
                })
            })?;
            let rows = rows.fetch_add(node.ports.len(), Ordering::Relaxed) + node.ports.len();
            if rows > 1_048_576 {
                return Err(ScratchError {
                    position: Some(pos),
                    reason: "local routing data exceeds size bound",
                });
            }
            Ok(node)
        })?;
        scratch.revert();
        Ok(Self {
            positions,
            index,
            token_cells,
            nodes,
            source_ids: HashMap::new(),
            readers: Vec::new(),
            by_component: Vec::new(),
            by_token: Vec::new(),
        })
    }

    /// Add reader contact tables and their token dependencies to the wire graph.
    ///
    /// Inputs: The scratch world, program, target component IDs, probe indexes and monitor.
    ///
    /// Sample each reader's direct/wire contacts, traverse all reachable wire alternatives to collect
    /// token dependencies, then rebuild reverse indexes.
    pub(super) fn compile_readers(
        &mut self,
        scratch: &mut ScratchWorld,
        program: &Program,
        targets: &[usize],
        indexes: &ProbeIndexes<'_>,
        monitor: Option<&TaskMonitor>,
    ) -> Result<(), ScratchError> {
        let token_cells = std::mem::take(&mut self.token_cells);
        let rows = AtomicUsize::new(self.nodes.iter().map(|node| node.ports.len()).sum());
        for node in &self.nodes {
            for ports in node.ports.iter().flatten() {
                for &pos in &ports.sources {
                    self.source_ids.insert(pos, indexes.source_at(pos)?);
                }
            }
        }
        let readers = scratch.probe_jobs(targets, monitor, |scratch, &component| {
            let position = program.positions[component];
            let piston_contacts = piston_contact_cells(position, program.initial[component]);
            let contacts = reader_contact_positions(position, piston_contacts.as_ref(), &self.index);
            let axes = if matches!(program.initial[component], Block::RedstoneWire { .. }) {
                Vec::new()
            } else {
                reader_token_axes(position, &program.tokens, &token_cells, piston_contacts.as_ref(), &contacts)
            };
            let table = compile_local(scratch, &program.tokens, axes, position, monitor, |world| {
                let inputs = super::group_probe::ports_of(world, program, component, monitor, indexes)?;
                let mut ports = ReaderPorts {
                    roots: Vec::new(),
                    direct: Vec::new(),
                };
                for ((source, side), _) in inputs {
                    if let ArrangementSource::Component(node) = source
                        && matches!(program.initial[node], Block::RedstoneWire { .. })
                    {
                        let root = self.index.get(&program.positions[node]).copied().ok_or(ScratchError {
                            position: Some(program.positions[node]),
                            reason: "unrepresented routed reader contact",
                        })?;
                        ports.roots.push((root, side));
                    } else {
                        ports.direct.push((source, side));
                    }
                }
                Ok(ports)
            })?;
            let rows = rows.fetch_add(table.ports.len(), Ordering::Relaxed) + table.ports.len();
            if rows > 1_048_576 {
                return Err(ScratchError {
                    position: Some(position),
                    reason: "local routing data exceeds size bound",
                });
            }
            let mut dependencies: BTreeMap<usize, ()> = table.axes.iter().map(|&Axis::Token(token, _)| (token, ())).collect();
            let mut reached = HashSet::new();
            let mut queue = VecDeque::new();
            for &(root, _) in table.ports.iter().flatten().flat_map(|ports| &ports.roots) {
                if reached.insert(root) {
                    queue.push_back((root, 0));
                }
            }
            while let Some((wire, distance)) = queue.pop_front() {
                for &Axis::Token(token, _) in &self.nodes[wire].axes {
                    dependencies.insert(token, ());
                }
                if distance == 14 {
                    continue;
                }
                for &next in self.nodes[wire].ports.iter().flatten().flat_map(|ports| &ports.wires) {
                    if reached.insert(next) {
                        queue.push_back((next, distance + 1));
                    }
                }
            }
            Ok(Reader {
                component,
                position,
                table,
                dependencies: dependencies.into_keys().collect(),
                links: Vec::new(),
            })
        })?;
        self.readers.extend(readers);
        self.by_component = vec![None; program.initial.len()];
        self.by_token = vec![Vec::new(); program.tokens.len()];
        self.reindex();
        scratch.revert();
        Ok(())
    }

    /// Resolve the currently connected sources of one routed reader.
    ///
    /// Inputs: The reader index, a callback selecting each token digit and reusable search state.
    ///
    /// Select its current contact row, seed direct sources, traverse each wire root and merge main/side
    /// results using minimum attenuation.
    pub(super) fn reader_sources(
        &self,
        reader: usize,
        digit: impl Fn(Axis) -> usize + Copy,
        search: &mut Search,
    ) -> Result<BTreeMap<(ArrangementSource, bool), u8>, ScratchError> {
        let reader = &self.readers[reader];
        let ports = reader.table.select(digit).ok_or(ScratchError {
            position: Some(reader.position),
            reason: "invalid routed reader state",
        })?;
        let mut sources: BTreeMap<_, u8> = ports.direct.iter().map(|&key| (key, 0)).collect();
        for &(root, side) in &ports.roots {
            for (pos, attenuation) in self.sources(self.positions[root], digit, search)? {
                let source = self.source_ids[&pos];
                sources
                    .entry((source, side))
                    .and_modify(|old| *old = (*old).min(attenuation))
                    .or_insert(attenuation);
            }
        }
        Ok(sources)
    }

    /// Append link-ordered attenuation updates without constructing per-reader maps.
    pub(super) fn reader_attenuations(
        &self,
        reader: usize,
        links: &[super::program::ArrangementLink],
        digit: impl Fn(Axis) -> usize + Copy,
        search: &mut Search,
        updates: &mut Vec<(usize, u8)>,
    ) -> Result<(), ScratchError> {
        let reader = &self.readers[reader];
        let ports = reader.table.select(digit).ok_or(ScratchError {
            position: Some(reader.position),
            reason: "invalid routed reader state",
        })?;
        let start = updates.len();
        updates.extend(reader.links.iter().map(|&link| (link, u8::MAX)));
        let values = &mut updates[start..];
        let mut record = |source, side, distance| {
            for (index, attenuation) in values.iter_mut() {
                if links[*index].source == source && links[*index].side == side {
                    *attenuation = (*attenuation).min(distance);
                }
            }
        };
        for &(source, side) in &ports.direct {
            record(source, side, 0);
        }
        for &(root, side) in &ports.roots {
            self.visit_sources(root, digit, search, |source, distance| record(self.source_ids[&source], side, distance))?;
        }
        Ok(())
    }

    /// Find power sources connected to a wire in the current arrangement.
    ///
    /// Inputs: The root world position, token-digit selector and reusable search/budget state.
    ///
    /// Run breadth-first wire traversal through the selected local port rows, stop after attenuation 14,
    /// and return each source at its shortest discovered distance.
    pub(super) fn sources(&self, root: BlockPos, digit: impl Fn(Axis) -> usize, search: &mut Search) -> Result<Vec<(BlockPos, u8)>, ScratchError> {
        let root = *self.index.get(&root).ok_or(ScratchError {
            position: Some(root),
            reason: "unrepresented routed wire root",
        })?;
        let mut found = BTreeMap::new();
        self.visit_sources(root, digit, search, |source, distance| {
            found
                .entry((source.x, source.y, source.z))
                .and_modify(|old: &mut u8| *old = (*old).min(distance))
                .or_insert(distance);
        })?;
        Ok(found
            .into_iter()
            .map(|((x, y, z), attenuation)| (BlockPos::new(x, y, z), attenuation))
            .collect())
    }

    fn visit_sources(
        &self,
        root: usize,
        digit: impl Fn(Axis) -> usize,
        search: &mut Search,
        mut emit: impl FnMut(BlockPos, u8),
    ) -> Result<(), ScratchError> {
        for wire in search.visited.drain(..) {
            search.distances[wire] = u8::MAX;
        }
        search.distances.resize(self.nodes.len(), u8::MAX);
        search.queue.clear();
        search.distances[root] = 0;
        search.visited.push(root);
        search.queue.push_back(root);
        while let Some(wire) = search.queue.pop_front() {
            if let Some(remaining) = &mut search.remaining {
                if *remaining == 0 {
                    return Err(ScratchError {
                        position: Some(self.positions[wire]),
                        reason: "runtime semantic operation limit exceeded",
                    });
                }
                *remaining -= 1;
            }
            search.spent += 1;
            let node = &self.nodes[wire];
            let mut state = 0;
            let mut stride = 1;
            for &axis in &node.axes {
                let value = digit(axis);
                if value >= axis.radix() {
                    return Err(ScratchError {
                        position: Some(self.positions[wire]),
                        reason: "invalid routing axis state",
                    });
                }
                state += value * stride;
                stride *= axis.radix();
            }
            let ports = node.ports[state].as_ref().ok_or(ScratchError {
                position: Some(self.positions[wire]),
                reason: "conflicting routed token positions",
            })?;
            let distance = search.distances[wire];
            for &source in &ports.sources {
                emit(source, distance);
            }
            if distance == 14 {
                continue;
            }
            for &next in &ports.wires {
                if search.distances[next] != u8::MAX {
                    continue;
                }
                search.distances[next] = distance + 1;
                search.visited.push(next);
                search.queue.push_back(next);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpilator::scratch::{ScratchEntry, discover_wire_sources};
    use mchprs_blocks::{BlockFace, blocks::RedstoneWire};

    #[test]
    fn direct_attenuations_match_maps_and_every_budget_failure() {
        let positions = [0, 1, 2].map(|x| BlockPos::new(x, 10, 0));
        let a = BlockPos::new(0, 11, 0);
        let b = BlockPos::new(2, 11, 0);
        let source_a = ArrangementSource::Component(5);
        let source_b = ArrangementSource::Token(0);
        let keys = [
            (source_a, false),
            (source_b, false),
            (source_a, true),
            (source_b, true),
            (ArrangementSource::Component(6), false),
        ];
        let links: Vec<_> = keys
            .iter()
            .map(|&(source, side)| super::super::program::ArrangementLink {
                target: 0,
                source,
                side,
                silent: false,
                attenuation: 0,
                group: 0,
                active: vec![true],
            })
            .collect();
        let mut graph = WireGraph {
            positions: positions.to_vec(),
            index: positions.into_iter().enumerate().map(|(i, pos)| (pos, i)).collect(),
            token_cells: HashMap::new(),
            nodes: (0..3)
                .map(|i| Local {
                    axes: vec![],
                    ports: vec![Some(Ports {
                        wires: if i < 2 { vec![i + 1] } else { vec![] },
                        sources: if i == 1 {
                            vec![a]
                        } else if i == 2 {
                            vec![b, a]
                        } else {
                            vec![]
                        },
                    })],
                })
                .collect(),
            source_ids: HashMap::from([(a, source_a), (b, source_b)]),
            readers: vec![Reader {
                component: 0,
                position: positions[0],
                dependencies: vec![],
                links: vec![3, 1, 4, 0, 2],
                table: Local {
                    axes: vec![],
                    ports: vec![Some(ReaderPorts {
                        roots: vec![(0, false), (2, false), (0, true), (0, false)],
                        direct: vec![(source_a, false)],
                    })],
                },
            }],
            by_component: vec![Some(0)],
            by_token: vec![],
        };
        for budget in 0..=12 {
            let mut old_search = Search {
                remaining: Some(budget),
                ..Default::default()
            };
            let mut new_search = Search {
                remaining: Some(budget),
                ..Default::default()
            };
            let expected = graph.reader_sources(0, |_| 0, &mut old_search);
            let mut updates = vec![(99, 7)];
            let actual = graph.reader_attenuations(0, &links, |_| 0, &mut new_search, &mut updates);
            assert_eq!(old_search.spent, new_search.spent);
            assert_eq!(old_search.remaining, new_search.remaining);
            assert_eq!(updates[0], (99, 7));
            match (expected, actual) {
                (Ok(expected), Ok(())) => assert_eq!(
                    updates[1..],
                    graph
                        .links(0)
                        .iter()
                        .map(|&index| { (index, expected.get(&(links[index].source, links[index].side)).copied().unwrap_or(u8::MAX)) })
                        .collect::<Vec<_>>()
                ),
                (Err(expected), Err(actual)) => {
                    assert_eq!(actual.position, expected.position);
                    assert_eq!(actual.reason, expected.reason);
                }
                _ => panic!("routing paths disagree at budget {budget}"),
            }
        }
        graph.nodes[1].axes.push(Axis::Token(0, 1));
        graph.nodes[1].ports.push(None);
        for digit in [1, 2] {
            let mut old_search = Search::default();
            let mut new_search = Search::default();
            let expected = graph.reader_sources(0, |_| digit, &mut old_search).unwrap_err();
            let actual = graph
                .reader_attenuations(0, &links, |_| digit, &mut new_search, &mut Vec::new())
                .unwrap_err();
            assert_eq!(actual.position, expected.position);
            assert_eq!(actual.reason, expected.reason);
            assert_eq!(new_search.spent, old_search.spent);
        }
    }

    #[test]
    fn parallel_topology_and_arrangements_match_serial_discovery() {
        let discover = |threads| {
            rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap().install(|| {
                let mut world = super::super::tests::host();
                let mut wires = Vec::new();
                let mut tokens = Vec::new();
                for index in 0..512 {
                    let wire = BlockPos::new(12 + (index % 16) * 4, 15, 12 + (index / 16) * 4);
                    world.set_block(wire.offset(BlockFace::Bottom), Block::Stone {});
                    world.set_block(
                        wire,
                        Block::RedstoneWire {
                            wire: RedstoneWire::default(),
                        },
                    );
                    let cell = wire.offset(BlockFace::West);
                    world.set_block(cell, Block::RedstoneBlock);
                    tokens.push(PistonToken {
                        block: Block::RedstoneBlock,
                        position: Some(cell),
                        cells: vec![cell],
                    });
                    wires.push(wire);
                }
                let mut program = Program::new(
                    wires.clone(),
                    wires.iter().map(|&pos| world.get_block(pos)).collect(),
                    vec![],
                    vec![[vec![], vec![]]; wires.len()],
                    vec![],
                    vec![],
                    vec![],
                )
                .unwrap();
                program.tokens = tokens;
                let components = wires.iter().enumerate().map(|(id, &pos)| (pos, id)).collect();
                let indexes = ProbeIndexes::new(&components, &program.positions, &program.initial, &program.tokens, None).unwrap();
                let entry = ScratchEntry {
                    scheduled: vec![],
                    piston: world.piston_state().clone(),
                };
                let bounds = (BlockPos::new(8, 10, 8), BlockPos::new(80, 20, 144));
                let discovery = super::super::group_probe::run(&world, bounds, &entry, &program, &indexes, None).unwrap();
                assert_eq!(discovery.groups.len(), wires.len());
                discovery
                    .groups
                    .into_iter()
                    .map(|group| {
                        assert_eq!(group.tokens, vec![group.target]);
                        assert_eq!(group.conditional_sources, vec![ArrangementSource::Token(group.target)]);
                        (group.target, group.side, group.tokens, group.inputs)
                    })
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(discover(1), discover(4));
    }

    #[test]
    fn routed_search_enforces_wire_visit_limit_and_can_restart() {
        let root = BlockPos::new(0, 0, 0);
        let next = BlockPos::new(1, 0, 0);
        let graph = WireGraph {
            positions: vec![root, next],
            index: HashMap::from([(root, 0), (next, 1)]),
            token_cells: HashMap::new(),
            nodes: vec![
                Local {
                    axes: vec![],
                    ports: vec![Some(Ports {
                        wires: vec![1],
                        sources: vec![],
                    })],
                },
                Local {
                    axes: vec![],
                    ports: vec![Some(Ports {
                        wires: vec![],
                        sources: vec![],
                    })],
                },
            ],
            source_ids: HashMap::new(),
            readers: vec![],
            by_component: vec![],
            by_token: vec![],
        };
        let mut search = Search {
            remaining: Some(1),
            ..Default::default()
        };
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
        let piston = Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::East,
                sticky: false,
                extended: false,
            },
        };
        let piston_cells: HashSet<_> = BlockFace::values()
            .into_iter()
            .filter(|&face| face != BlockFace::East)
            .map(|face| piston_pos.offset(face))
            .collect();
        let above = piston_pos.offset(BlockFace::Top);
        let mut piston_cells = piston_cells;
        piston_cells.extend(BlockFace::values().into_iter().map(|face| above.offset(face)));

        let overlap = BlockPos::new(2, 20, 0);
        let adjacent_to_piston_wire = piston_pos.offset(BlockFace::West).offset(BlockFace::North);
        let excluded_piston_front = piston_pos.offset(BlockFace::East);
        let tokens = vec![
            PistonToken {
                block: Block::Stone {},
                position: None,
                cells: vec![overlap, piston_pos.offset(BlockFace::West)],
            },
            PistonToken {
                block: Block::RedstoneBlock,
                position: None,
                cells: vec![overlap, adjacent_to_piston_wire],
            },
            PistonToken {
                block: Block::Stone {},
                position: None,
                cells: vec![plain_pos + BlockPos::new(3, 0, 0), excluded_piston_front],
            },
            PistonToken {
                block: Block::Stone {},
                position: None,
                cells: vec![BlockPos::new(80, 80, 80)],
            },
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
            let expected: Vec<_> = tokens
                .iter()
                .enumerate()
                .filter_map(|(token, record)| {
                    record
                        .cells
                        .iter()
                        .any(|&cell| touches(wire, cell))
                        .then_some(Axis::Token(token, record.cells.len()))
                })
                .collect();
            assert_eq!(wire_token_axes(wire, &tokens, &token_cells), expected, "wire at {wire:?}");
        }

        for (position, block) in [(plain_pos, Block::Stone {}), (piston_pos, piston)] {
            let old_piston_cells = if let Block::Piston { piston } = block {
                let mut cells: HashSet<_> = BlockFace::values()
                    .into_iter()
                    .filter(|&face| face != piston.facing.into())
                    .map(|face| position.offset(face))
                    .collect();
                let above = position.offset(BlockFace::Top);
                cells.extend(BlockFace::values().into_iter().map(|face| above.offset(face)));
                Some(cells)
            } else {
                None
            };
            let radius = if old_piston_cells.is_some() { 3 } else { 2 };
            let old_contacts: HashSet<_> = wire_positions
                .iter()
                .copied()
                .filter(|&at| {
                    old_piston_cells.as_ref().map_or_else(
                        || at.x.abs_diff(position.x) + at.y.abs_diff(position.y) + at.z.abs_diff(position.z) <= radius,
                        |cells| cells.contains(&at) || cells.iter().any(|&cell| touches(cell, at)),
                    )
                })
                .collect();
            let indexed_contacts = reader_contact_positions(position, old_piston_cells.as_ref(), &wire_index);
            assert_eq!(indexed_contacts, old_contacts, "contacts at {position:?}");

            let expected_axes: Vec<_> = tokens
                .iter()
                .enumerate()
                .filter_map(|(token, record)| {
                    record
                        .cells
                        .iter()
                        .any(|&cell| {
                            old_piston_cells.as_ref().map_or_else(
                                || cell.x.abs_diff(position.x) + cell.y.abs_diff(position.y) + cell.z.abs_diff(position.z) <= 2,
                                |cells| cells.contains(&cell),
                            ) || BlockFace::values().iter().any(|&face| old_contacts.contains(&cell.offset(face)))
                        })
                        .then_some(Axis::Token(token, record.cells.len()))
                })
                .collect();
            assert_eq!(
                reader_token_axes(position, &tokens, &token_cells, old_piston_cells.as_ref(), &indexed_contacts),
                expected_axes,
                "reader axes at {position:?}"
            );
        }
    }

    #[test]
    fn piston_reader_ignores_unreachable_cells_and_keeps_joint_contacts() {
        use mchprs_blocks::{BlockFacing, blocks::RedstonePiston};
        let mut world = super::super::tests::host();
        let pos = BlockPos::new(15, 15, 15);
        let piston = Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::East,
                sticky: true,
                extended: false,
            },
        };
        world.set_block(pos, piston);
        let contacts = [pos.offset(BlockFace::West), pos.offset(BlockFace::Top).offset(BlockFace::East)];
        let mut tokens: Vec<_> = contacts
            .iter()
            .map(|&cell| PistonToken {
                block: Block::RedstoneBlock,
                position: None,
                cells: vec![cell],
            })
            .collect();
        let direct: HashSet<_> = BlockFace::values()
            .into_iter()
            .filter(|&face| face != BlockFace::East)
            .map(|face| pos.offset(face))
            .chain(BlockFace::values().into_iter().map(|face| pos.offset(BlockFace::Top).offset(face)))
            .collect();
        for dx in -2i32..=2 {
            for dy in -2i32..=2 {
                for dz in -2i32..=2 {
                    if dx.abs() + dy.abs() + dz.abs() != 2 {
                        continue;
                    }
                    let cell = pos + BlockPos::new(dx, dy, dz);
                    if !direct.contains(&cell) {
                        tokens.push(PistonToken {
                            block: Block::Stone {},
                            position: None,
                            cells: vec![cell],
                        });
                    }
                }
            }
        }
        assert!(tokens.len() > 12);
        let entry = ScratchEntry {
            scheduled: vec![],
            piston: world.piston_state().clone(),
        };
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
            let actual = graph
                .reader_sources(0, |Axis::Token(token, _)| usize::from(state & (1 << token) != 0), &mut Search::default())
                .unwrap();
            let expected = super::super::group_probe::inputs_of(&world, &program, 0, None, &indexes)
                .unwrap()
                .into_iter()
                .map(|(key, (attenuation, _))| (key, attenuation))
                .collect();
            assert_eq!(actual, expected, "state {state}");
        }
    }

    #[test]
    #[ignore = "ANPU local-connectivity census and tick-278 path check, without full compilation"]
    fn anpu_local_graph_reproduces_the_hidden_source() {
        use super::super::{cpu_trace::cpus, pistons};
        let cpu = cpus::CPUS[1];
        let mut world = cpus::load_cpu(cpu);
        let clip = crate::plot::worldedit::load_schematic(
            std::fs::File::open(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../test_data")
                    .join(cpu.schematic),
            )
            .unwrap(),
        )
        .unwrap();
        let bounds = (
            cpu.origin,
            cpu.origin + BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1),
        );
        let entry = ScratchEntry {
            scheduled: vec![],
            piston: world.piston_state().clone(),
        };
        let actors = pistons::discover_piston_actors(&world, bounds, entry.clone()).unwrap();
        let tokens = pistons::discover_piston_tokens(&world, &actors).unwrap();
        let mut wires = Vec::new();
        let mut components = Vec::new();
        crate::world::for_each_block_optimized(&world, bounds.0, bounds.1, |pos| {
            let block = world.get_block(pos);
            if matches!(block, Block::RedstoneWire { .. }) {
                wires.push(pos);
            }
            if block.pressure_plate_powered().is_some()
                || matches!(
                    block,
                    Block::RedstoneWire { .. }
                        | Block::RedstoneBlock
                        | Block::Lever { .. }
                        | Block::StoneButton { .. }
                        | Block::RedstoneLamp { .. }
                        | Block::Hopper { .. }
                        | Block::NoteBlock { .. }
                        | Block::Piston { .. }
                        | Block::Observer { .. }
                        | Block::RedstoneRepeater { .. }
                        | Block::RedstoneComparator { .. }
                        | Block::RedstoneTorch { .. }
                        | Block::RedstoneWallTorch { .. }
                )
            {
                components.push(pos);
            }
        });
        let mut scratch = ScratchWorld::from_world_monitored(&world, bounds, entry, None).unwrap();
        let started = std::time::Instant::now();
        let mut graph = WireGraph::compile(&mut scratch, wires, &tokens, None).unwrap();
        let build_ms = started.elapsed().as_secs_f64() * 1000.0;
        let outcomes: usize = graph.nodes.iter().map(|node| node.ports.len()).sum();
        let max_outcomes = graph.nodes.iter().map(|node| node.ports.len()).max().unwrap_or(0);
        let initial: Vec<_> = components.iter().map(|&pos| world.get_block(pos)).collect();
        let repeaters = initial
            .iter()
            .enumerate()
            .filter_map(|(component, block)| {
                matches!(block, Block::RedstoneRepeater { .. }).then_some(super::super::program::RepeaterRecord {
                    component,
                    output_is_diode: false,
                    side_sources: None,
                })
            })
            .collect();
        let component_index: HashMap<_, _> = components.iter().enumerate().map(|(node, &pos)| (pos, node)).collect();
        let mut program = Program::new(
            components,
            initial,
            vec![],
            vec![[vec![], vec![]]; component_index.len()],
            vec![],
            vec![],
            repeaters,
        )
        .unwrap();
        program.tokens = tokens.clone();
        let indexes = ProbeIndexes::new(&component_index, &program.positions, &program.initial, &program.tokens, None).unwrap();
        let mut kinds = HashSet::new();
        let mut targets = vec![component_index[&BlockPos::new(99, 49, 109)], component_index[&BlockPos::new(84, 36, 76)]];
        targets.extend(
            program
                .initial
                .iter()
                .enumerate()
                .filter_map(|(node, block)| kinds.insert(block.get_name()).then_some(node)),
        );
        targets.sort_unstable();
        targets.dedup();
        graph.compile_readers(&mut scratch, &program, &targets, &indexes, None).unwrap();
        let removed: HashSet<_> = tokens
            .iter()
            .enumerate()
            .filter_map(|(token, record)| {
                record
                    .cells
                    .iter()
                    .any(|p| [99, 100].contains(&p.x) && p.y == 50 && [109, 111, 113, 115, 117].contains(&p.z))
                    .then_some(token)
            })
            .collect();
        assert_eq!(removed.len(), 5);
        let mut digits: Vec<_> = tokens
            .iter()
            .map(|record| {
                record
                    .position
                    .and_then(|pos| record.cells.iter().position(|&cell| cell == pos))
                    .map_or(0, |cell| cell + 1)
            })
            .collect();
        for &token in &removed {
            digits[token] = 0;
            if let Some(pos) = tokens[token].position {
                world.set_block(pos, Block::Air);
            }
        }
        let root = BlockPos::new(99, 49, 109);
        let actual = graph
            .sources(root, |Axis::Token(token, _)| digits[token], &mut Search::default())
            .unwrap();
        let expected: Vec<_> = discover_wire_sources(&world, &[root])
            .unwrap()
            .into_iter()
            .map(|s| (s.source, s.attenuation))
            .collect();
        assert_eq!(actual, expected);
        assert!(actual.contains(&(BlockPos::new(98, 48, 118), 10)));
        for (reader, descriptor) in graph.readers.iter().enumerate() {
            let actual = graph
                .reader_sources(reader, |Axis::Token(token, _)| digits[token], &mut Search::default())
                .unwrap();
            let expected = super::super::group_probe::inputs_of(&world, &program, descriptor.component, None, &indexes).unwrap();
            let expected: BTreeMap<_, _> = expected.into_iter().map(|(key, (attenuation, _))| (key, attenuation)).collect();
            assert_eq!(actual, expected, "reader at {:?}", descriptor.position);
        }
        eprintln!(
            "ANPU local wire graph: {} wires, {} tokens, {outcomes} local outcomes, largest={max_outcomes}, build={build_ms:.1} ms; hidden torch attenuation=10",
            graph.nodes.len(),
            tokens.len()
        );
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
                world.set_block(
                    wire,
                    Block::RedstoneWire {
                        wire: RedstoneWire::default(),
                    },
                );
            }
            let source = BlockPos::new(17, 10, 10);
            world.set_block(source, Block::RedstoneBlock);
            let tokens: Vec<_> = [10, 14]
                .into_iter()
                .map(|x| {
                    let cell = BlockPos::new(x, 11, 10);
                    world.set_block(cell, Block::Stone {});
                    PistonToken {
                        block: Block::Stone {},
                        position: Some(cell),
                        cells: vec![cell],
                    }
                })
                .collect();
            let entry = ScratchEntry {
                scheduled: vec![],
                piston: world.piston_state().clone(),
            };
            let mut scratch = ScratchWorld::from_world_monitored(&world, (BlockPos::new(5, 5, 5), BlockPos::new(22, 15, 15)), entry, None).unwrap();
            let graph = WireGraph::compile(&mut scratch, wires.clone(), &tokens, None).unwrap();
            let mut search = Search::default();
            for state in 0..4 {
                for (token, record) in tokens.iter().enumerate() {
                    world.set_block(record.cells[0], if state & (1 << token) == 0 { Block::Air } else { record.block });
                }
                for &root in &wires {
                    let expected: Vec<_> = discover_wire_sources(&world, &[root])
                        .unwrap()
                        .into_iter()
                        .map(|s| (s.source, s.attenuation))
                        .collect();
                    let actual = graph
                        .sources(
                            root,
                            |axis| match axis {
                                Axis::Token(token, _) => usize::from(state & (1 << token) != 0),
                            },
                            &mut search,
                        )
                        .unwrap();
                    assert_eq!(actual, expected, "detour={detour}, state={state}, root={root:?}");
                }
                let root = wires[0];
                let sources = graph
                    .sources(
                        root,
                        |axis| match axis {
                            Axis::Token(token, _) => usize::from(state & (1 << token) != 0),
                        },
                        &mut search,
                    )
                    .unwrap();
                assert_eq!(
                    sources.first().map(|s| s.1),
                    if state == 0 {
                        Some(8)
                    } else if detour {
                        Some(12)
                    } else {
                        None
                    }
                );
            }
        }
    }
}
