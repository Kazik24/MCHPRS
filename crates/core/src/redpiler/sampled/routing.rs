//! Local dust connectivity for inputs that do not fit a dense arrangement table.
use super::{ScratchError, ScratchWorld, compiler::PistonToken};
use crate::redpiler::{TaskMonitor, scratch::discover_wire_ports};
use crate::world::World;
use mchprs_blocks::{BlockPos, blocks::Block};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

#[derive(Clone, Copy, Debug)]
pub(super) enum Axis {
    Token(usize, usize),
}

impl Axis {
    fn radix(self) -> usize {
        match self { Self::Token(_, cells) => cells + 1 }
    }
}

#[derive(Clone, Debug)]
struct Ports {
    wires: Vec<usize>,
    sources: Vec<BlockPos>,
}

#[derive(Clone, Debug)]
struct Node {
    axes: Vec<Axis>,
    ports: Vec<Option<Ports>>,
}

#[derive(Clone, Debug)]
pub(super) struct WireGraph {
    positions: Vec<BlockPos>,
    index: HashMap<BlockPos, usize>,
    nodes: Vec<Node>,
}

#[derive(Default)]
pub(super) struct Search {
    distances: Vec<u8>,
    visited: Vec<usize>,
    queue: VecDeque<usize>,
}

fn touches(a: BlockPos, b: BlockPos) -> bool {
    a.x.abs_diff(b.x) + a.y.abs_diff(b.y) + a.z.abs_diff(b.z) == 1
}

impl WireGraph {
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
        for &pos in &positions {
            if monitor.is_some_and(TaskMonitor::cancelled) {
                return Err(ScratchError { position: None, reason: "compilation cancelled" });
            }
            // Non-wire payloads cannot alter a distance-two strong-source read.
            let axes: Vec<_> = tokens.iter().enumerate().filter_map(|(token, record)|
                record.cells.iter().any(|&cell| touches(pos, cell)).then_some(Axis::Token(token, record.cells.len()))).collect();
            let states = axes.iter().try_fold(1usize, |count, axis| count.checked_mul(axis.radix()));
            let Some(states) = states.filter(|&count| count <= 4096) else {
                return Err(ScratchError { position: Some(pos), reason: "local wire connectivity exceeds state bound" });
            };
            let mut ports = Vec::with_capacity(states);
            for state in 0..states {
                if monitor.is_some_and(TaskMonitor::cancelled) {
                    return Err(ScratchError { position: None, reason: "compilation cancelled" });
                }
                scratch.revert();
                for axis in &axes {
                    if let Axis::Token(token, _) = *axis && let Some(initial) = tokens[token].position {
                        scratch.world_mut().set_block(initial, Block::Air);
                    }
                }
                let mut digits = state;
                let mut occupied = HashSet::new();
                let mut valid = true;
                for &axis in &axes {
                    let digit = digits % axis.radix();
                    digits /= axis.radix();
                    match axis {
                        Axis::Token(token, _) if digit != 0 => {
                            let cell = tokens[token].cells[digit - 1];
                            valid &= occupied.insert(cell);
                            scratch.world_mut().set_block(cell, tokens[token].block);
                        }
                        _ => {}
                    }
                }
                if !valid { scratch.finish_probe()?; ports.push(None); continue; }
                let found = discover_wire_ports(scratch.world(), pos)?;
                let wires = found.wires.into_iter().map(|at| index.get(&at).copied().ok_or(ScratchError {
                    position: Some(at), reason: "unrepresented routed wire",
                })).collect::<Result<_, _>>()?;
                ports.push(Some(Ports { wires, sources: found.sources }));
                scratch.finish_probe()?;
            }
            nodes.push(Node { axes, ports });
        }
        scratch.revert();
        Ok(Self { positions, index, nodes })
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
        crate::world::for_each_block_optimized(&world, bounds.0, bounds.1, |pos| {
            if matches!(world.get_block(pos), Block::RedstoneWire { .. }) { wires.push(pos); }
        });
        let mut scratch = ScratchWorld::from_world_monitored(&world, bounds, entry, None).unwrap();
        let started = std::time::Instant::now();
        let graph = WireGraph::compile(&mut scratch, wires, &tokens, None).unwrap();
        let build_ms = started.elapsed().as_secs_f64() * 1000.0;
        let outcomes: usize = graph.nodes.iter().map(|node| node.ports.len()).sum();
        let max_outcomes = graph.nodes.iter().map(|node| node.ports.len()).max().unwrap_or(0);
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
