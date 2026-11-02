//! Local dust connectivity for inputs that do not fit a dense arrangement table.
use super::{Program, ScratchError, ScratchWorld, compiler::PistonToken, group_probe::ProbeIndexes, program::ArrangementSource};
use crate::redpiler::{TaskMonitor, scratch::discover_wire_ports};
use crate::world::World;
use mchprs_blocks::{BlockPos, blocks::Block};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

#[derive(Clone, Copy, Debug)]
pub(super) enum Axis {
    Token(usize, usize),
}

impl Axis {
    fn radix(self) -> usize {
        match self { Self::Token(_, cells) => cells + 1 }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Ports {
    wires: Vec<usize>,
    sources: Vec<BlockPos>,
}

#[derive(Clone, Debug)]
struct Local<T> {
    axes: Vec<Axis>,
    ports: Vec<Option<T>>,
}

type Node = Local<Ports>;

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReaderPorts {
    roots: Vec<(usize, bool)>,
    direct: Vec<(ArrangementSource, bool)>,
}

#[derive(Clone, Debug)]
struct Reader {
    component: usize,
    position: BlockPos,
    table: Local<ReaderPorts>,
    dependencies: Vec<usize>,
    links: Vec<usize>,
}

impl<T> Local<T> {
    fn select(&self, digit: impl Fn(Axis) -> usize) -> Option<&T> {
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

fn compile_local<T: PartialEq>(scratch: &mut ScratchWorld, tokens: &[PistonToken], mut axes: Vec<Axis>, position: BlockPos,
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
    nodes: Vec<Node>,
    source_ids: HashMap<BlockPos, ArrangementSource>,
    readers: Vec<Reader>,
    by_component: Vec<Option<usize>>,
    by_token: Vec<Vec<usize>>,
}

#[derive(Default)]
pub(super) struct Search {
    distances: Vec<u8>,
    visited: Vec<usize>,
    queue: VecDeque<usize>,
    pub(super) remaining: Option<usize>,
    pub(super) spent: usize,
}

fn touches(a: BlockPos, b: BlockPos) -> bool {
    a.x.abs_diff(b.x) + a.y.abs_diff(b.y) + a.z.abs_diff(b.z) == 1
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
        let mut nodes = Vec::with_capacity(positions.len());
        let mut rows = 0usize;
        for &pos in &positions {
            if monitor.is_some_and(TaskMonitor::cancelled) {
                return Err(ScratchError { position: None, reason: "compilation cancelled" });
            }
            // Non-wire payloads cannot alter a distance-two strong-source read.
            let axes: Vec<_> = tokens.iter().enumerate().filter_map(|(token, record)|
                record.cells.iter().any(|&cell| touches(pos, cell)).then_some(Axis::Token(token, record.cells.len()))).collect();
            let node = compile_local(scratch, tokens, axes, pos, monitor, |world| {
                let found = discover_wire_ports(world, pos)?;
                let wires = found.wires.into_iter().map(|at| index.get(&at).copied().ok_or(ScratchError {
                    position: Some(at), reason: "unrepresented routed wire",
                })).collect::<Result<_, _>>()?;
                Ok(Ports { wires, sources: found.sources })
            })?;
            rows += node.ports.len();
            if rows > 1_048_576 { return Err(ScratchError { position: Some(pos), reason: "local routing data exceeds size bound" }); }
            nodes.push(node);
        }
        scratch.revert();
        Ok(Self { positions, index, nodes, source_ids: HashMap::new(), readers: Vec::new(), by_component: Vec::new(), by_token: Vec::new() })
    }

    pub(super) fn compile_readers(&mut self, scratch: &mut ScratchWorld, program: &Program, targets: &[usize], indexes: &ProbeIndexes<'_>, monitor: Option<&TaskMonitor>) -> Result<(), ScratchError> {
        let mut rows: usize = self.nodes.iter().map(|node| node.ports.len()).sum();
        for node in &self.nodes {
            for ports in node.ports.iter().flatten() {
                for &pos in &ports.sources { self.source_ids.insert(pos, indexes.source_at(pos)?); }
            }
        }
        for &component in targets {
            if monitor.is_some_and(TaskMonitor::cancelled) { return Err(ScratchError { position: None, reason: "compilation cancelled" }); }
            let position = program.positions[component];
            let radius = if matches!(program.initial[component], Block::Piston { .. }) { 3 } else { 2 };
            let contacts: HashSet<_> = self.positions.iter().copied().filter(|at|
                at.x.abs_diff(position.x) + at.y.abs_diff(position.y) + at.z.abs_diff(position.z) <= radius).collect();
            let axes = if matches!(program.initial[component], Block::RedstoneWire { .. }) { Vec::new() } else {
                program.tokens.iter().enumerate().filter_map(|(token, record)| record.cells.iter().any(|&cell|
                    cell.x.abs_diff(position.x) + cell.y.abs_diff(position.y) + cell.z.abs_diff(position.z) <= 2
                    || mchprs_blocks::BlockFace::values().iter().any(|&face| contacts.contains(&cell.offset(face))))
                    .then_some(Axis::Token(token, record.cells.len()))).collect()
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
            rows += table.ports.len();
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
            self.readers.push(Reader { component, position, table, dependencies: dependencies.into_keys().collect(), links: Vec::new() });
        }
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
    use crate::redpiler::scratch::{ScratchEntry, discover_wire_sources};
    use mchprs_blocks::{BlockFace, blocks::RedstoneWire};

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
