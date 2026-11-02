use super::compiler::find_changed_links;
use super::program::{ArrangementSource, InputArrangement};
use super::{Program, ScratchEntry, ScratchError, ScratchWorld};
use crate::redpiler::TaskMonitor;
use crate::world::World;
use mchprs_blocks::{BlockFace, BlockPos, blocks::Block};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

type InputKey = (ArrangementSource, bool);
type InputValue = (u8, bool);
type Inputs = BTreeMap<InputKey, InputValue>;

#[derive(Clone, Copy)]
enum ReaderKind {
    Wire,
    Piston,
    Lamp,
    Torch,
    Diode,
}

fn reader_kind(block: Block) -> Option<ReaderKind> {
    match block {
        Block::RedstoneWire { .. } => Some(ReaderKind::Wire),
        Block::Piston { .. } => Some(ReaderKind::Piston),
        Block::RedstoneLamp { .. } | Block::Hopper { .. } | Block::IronTrapdoor { .. } | Block::NoteBlock { .. } => Some(ReaderKind::Lamp),
        block if block.is_copper_bulb() || block.is_command_block() => Some(ReaderKind::Lamp),
        Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. } => Some(ReaderKind::Torch),
        Block::RedstoneRepeater { .. } | Block::RedstoneComparator { .. } => Some(ReaderKind::Diode),
        _ => None,
    }
}

pub(super) struct ProbeIndexes<'a> {
    /// borrowed world-position to component-index map.
    pub(super) components: &'a HashMap<BlockPos, usize>,
    /// positions where movable redstone-block tokens may supply power, mapped to token IDs.
    redstone_tokens: HashMap<BlockPos, usize>,
    /// wire world positions mapped to component IDs.
    wire_at: HashMap<BlockPos, usize>,
}

impl<'a> ProbeIndexes<'a> {
    pub(super) fn source_at(&self, pos: BlockPos) -> Result<ArrangementSource, ScratchError> {
        if let Some(&token) = self.redstone_tokens.get(&pos) { Ok(ArrangementSource::Token(token)) }
        else if let Some(&node) = self.components.get(&pos) { Ok(ArrangementSource::Component(node)) }
        else { Err(ScratchError { position: Some(pos), reason: "unrepresented arrangement source" }) }
    }
    pub(super) fn new(
        components: &'a HashMap<BlockPos, usize>,
        positions: &[BlockPos],
        initial: &[Block],
        tokens: &[super::compiler::PistonToken],
        monitor: Option<&TaskMonitor>,
    ) -> Result<ProbeIndexes<'a>, ScratchError> {
        let mut redstone_tokens = HashMap::new();
        let mut wire_at = HashMap::new();
        for (&pos, &block) in positions.iter().zip(initial) {
            if monitor.is_some_and(TaskMonitor::cancelled) {
                return Err(ScratchError {
                    position: None,
                    reason: "compilation cancelled",
                });
            }
            let index = components[&pos];
            if matches!(block, Block::RedstoneWire { .. }) {
                wire_at.insert(pos, index);
            }
        }
        for (token, record) in tokens.iter().enumerate() {
            if matches!(record.block, Block::RedstoneBlock) {
                for &cell in &record.cells {
                    redstone_tokens.insert(cell, token);
                }
            }
        }
        Ok(Self {
            components,
            redstone_tokens,
            wire_at,
        })
    }
}

fn wire_sources(
    world: &impl World,
    wires: &[BlockPos],
    monitor: Option<&TaskMonitor>,
    expand: bool,
) -> Result<Vec<super::super::scratch::WireSource>, ScratchError> {
    if !expand {
        return Ok(wires.iter().map(|&pos| super::super::scratch::WireSource { wire: pos, source: pos, attenuation: 0 }).collect());
    }
    if let Some(monitor) = monitor {
        crate::redpiler::scratch::discover_wire_sources_monitored(world, wires, monitor)
    } else {
        crate::redpiler::scratch::discover_wire_sources(world, wires)
    }
}

fn near(a: BlockPos, b: BlockPos, distance: u32) -> bool {
    a.x.abs_diff(b.x) <= distance && a.y.abs_diff(b.y) <= distance && a.z.abs_diff(b.z) <= distance
}

pub(super) fn inputs_of(
    world: &impl World,
    program: &Program,
    member: usize,
    monitor: Option<&TaskMonitor>,
    indexes: &ProbeIndexes<'_>,
) -> Result<Inputs, ScratchError> {
    discover_inputs(world, program, member, monitor, indexes, true)
}

pub(super) fn ports_of(world: &impl World, program: &Program, member: usize, monitor: Option<&TaskMonitor>, indexes: &ProbeIndexes<'_>) -> Result<Inputs, ScratchError> {
    discover_inputs(world, program, member, monitor, indexes, false)
}

fn discover_inputs(world: &impl World, program: &Program, member: usize, monitor: Option<&TaskMonitor>, indexes: &ProbeIndexes<'_>, expand: bool) -> Result<Inputs, ScratchError> {
    let pos = program.positions[member];
    let block = program.initial[member];
    let mut found = Vec::new();
    let mut side = Vec::new();
    match reader_kind(block) {
        Some(ReaderKind::Wire) => found = wire_sources(world, &[pos], monitor, expand)?,
        Some(ReaderKind::Piston) => {
            let Block::Piston { piston } = block else { unreachable!() };
            found = if expand { crate::redpiler::scratch::discover_piston_sources(world, pos, piston.facing)? }
                else { crate::redpiler::scratch::discover_piston_ports(world, pos, piston.facing)? };
        }
        Some(ReaderKind::Lamp) => found = if expand { crate::redpiler::scratch::discover_lamp_sources(world, pos)? }
            else { crate::redpiler::scratch::discover_lamp_ports(world, pos)? },
        Some(ReaderKind::Torch) => found = if expand { crate::redpiler::scratch::discover_torch_sources(world, pos, block)? }
            else { crate::redpiler::scratch::discover_torch_ports(world, pos, block)? },
        Some(ReaderKind::Diode) => {
            let (facing, comparator) = match block {
                Block::RedstoneRepeater { repeater } => (repeater.facing, false),
                Block::RedstoneComparator { comparator } => (comparator.facing, true),
                _ => unreachable!(),
            };
            let face = facing.block_face();
            let rear = pos.offset(face);
            let block = world.get_block(rear);
            if matches!(block, Block::RedstoneWire { .. }) {
                found = wire_sources(world, &[rear], monitor, expand)?;
            } else if block.is_solid() {
                for direction in BlockFace::values() {
                    let at = rear.offset(direction);
                    if crate::redstone::power::emits_strong_power(world.get_block(at), world, at, direction, true) {
                        if matches!(world.get_block(at), Block::RedstoneWire { .. }) {
                            found.extend(wire_sources(world, &[at], monitor, expand)?);
                        } else {
                            found.push(crate::redpiler::scratch::WireSource {
                                wire: pos,
                                source: at,
                                attenuation: 0,
                            });
                        }
                    }
                }
            } else if crate::redstone::power::emits_weak_power(block, world, rear, face, true) {
                found.push(crate::redpiler::scratch::WireSource {
                    wire: pos,
                    source: rear,
                    attenuation: 0,
                });
            }
            for direction in [facing.rotate(), facing.rotate_ccw()] {
                let face = direction.block_face();
                let at = pos.offset(face);
                let block = world.get_block(at);
                if comparator && matches!(block, Block::RedstoneWire { .. }) {
                    side.extend(wire_sources(world, &[at], monitor, expand)?);
                } else if (comparator && matches!(block, Block::RedstoneBlock))
                    || ((comparator || crate::redstone::is_diode(block)) && crate::redstone::power::emits_strong_power(block, world, at, face, false))
                {
                    side.push(crate::redpiler::scratch::WireSource {
                        wire: pos,
                        source: at,
                        attenuation: 0,
                    });
                }
            }
        }
        None => return Ok(Inputs::new()),
    }
    let mut result = Inputs::new();
    for (is_side, source) in found.into_iter().map(|s| (false, s)).chain(side.into_iter().map(|s| (true, s))) {
        let identity = indexes.source_at(source.source)?;
        let silent = matches!(block, Block::Piston { .. });
        result
            .entry((identity, is_side))
            .and_modify(|value| {
                value.0 = value.0.min(source.attenuation);
                value.1 &= silent;
            })
            .or_insert((source.attenuation, silent));
    }
    Ok(result)
}

fn electrical_states(program: &Program, indexes: &ProbeIndexes<'_>, tokens: &[usize], target: usize) -> Vec<Vec<usize>> {
    tokens.iter().map(|&token| {
        let record = &program.tokens[token];
        let fold = matches!(record.block, Block::Concrete { .. });
        let mut next = 0;
        std::iter::once(0).chain(record.cells.iter().map(|&cell| {
            // Other readers also query local conductors, including piston quasi-power.
            let outside_reader = matches!(program.initial[target], Block::RedstoneWire { .. }) || !near(cell, program.positions[target], 2);
            if fold && outside_reader && !BlockFace::values().iter().any(|&face| indexes.wire_at.contains_key(&cell.offset(face))) {
                0
            } else {
                next += 1;
                next
            }
        })).collect()
    }).collect()
}

fn placements(tokens: &[usize], axes: &[Vec<usize>], mut arrangement: usize) -> Vec<(usize, usize)> {
    tokens
        .iter()
        .zip(axes).map(|(&token, states)| {
            let radix = states.iter().copied().max().unwrap_or(0) + 1;
            let digit = arrangement % radix;
            arrangement /= radix;
            (token, states.iter().position(|&state| state == digit).unwrap())
        })
        .collect()
}

pub(super) struct Discovery {
    pub(super) groups: Vec<InputArrangement>,
    pub(super) routing: Option<super::routing::WireGraph>,
}

pub(super) fn run(
    world: &impl World,
    bounds: (BlockPos, BlockPos),
    entry: &ScratchEntry,
    program: &Program,
    indexes: &ProbeIndexes<'_>,
    monitor: Option<&TaskMonitor>,
) -> Result<Discovery, ScratchError> {
    use super::routing::{Axis, Search, WireGraph};
    if program.tokens.is_empty() { return Ok(Discovery { groups: Vec::new(), routing: None }); }
    let started = std::time::Instant::now();
    let mut scratch = ScratchWorld::from_world_monitored(world, bounds, entry.clone(), monitor)?;
    let wires = program.positions.iter().zip(&program.initial).filter_map(|(&pos, block)| matches!(block, Block::RedstoneWire { .. }).then_some(pos)).collect();
    let mut graph = WireGraph::compile(&mut scratch, wires, &program.tokens, monitor)?;
    let targets: Vec<_> = program.initial.iter().enumerate().filter_map(|(node, &block)| reader_kind(block).is_some().then_some(node)).collect();
    graph.compile_readers(&mut scratch, program, &targets, indexes, monitor)?;
    let initial: Vec<_> = program.tokens.iter().map(|record| record.position.and_then(|pos| record.cells.iter().position(|&cell| cell == pos)).map_or(0, |cell| cell + 1)).collect();
    const MAX_TEMP_SLOTS: usize = 65_536;
    const MAX_DENSE_BYTES: usize = 64 * 1024 * 1024;
    let mut jobs = Vec::new();
    let mut fallback = BTreeSet::new();
    for reader in 0..graph.reader_count() {
        let tokens = graph.reader_dependencies(reader);
        if tokens.is_empty() { continue; }
        let target = graph.reader_component(reader);
        let axes = electrical_states(program, indexes, tokens, target);
        let states = axes.iter().try_fold(1usize, |count, axis| count.checked_mul(axis.iter().copied().max().unwrap_or(0) + 1));
        let mut occupied = HashSet::new();
        let overlaps = tokens.iter().flat_map(|&token| &program.tokens[token].cells).any(|&cell| !occupied.insert(cell));
        let sources = graph.potential_sources(reader).len();
        let slots = states.map_or(usize::MAX, |states| states.saturating_mul(sources.max(1)));
        if !overlaps && states.is_some_and(|states| states <= super::program::MAX_ARRANGEMENT_STATES) && slots <= MAX_TEMP_SLOTS {
            jobs.push((reader, axes, states.unwrap(), slots));
        } else { fallback.insert(reader); }
    }
    let mut groups = Vec::new();
    let mut routed = BTreeSet::new();
    let mut dense_bytes = 0usize;
    let mut start = 0;
    while start < jobs.len() {
        let mut end = start;
        let mut slots = 0usize;
        while end < jobs.len() && slots + jobs[end].3 <= MAX_TEMP_SLOTS { slots += jobs[end].3; end += 1; }
        let lowered = scratch.probe_jobs(&jobs[start..end], monitor, |_, &(reader, ref axes, states, _)| {
            let target = graph.reader_component(reader);
            let tokens = graph.reader_dependencies(reader);
            let mut search = Search::default();
            let basis = graph.reader_sources(reader, |Axis::Token(token, _)| initial[token], &mut search)?;
        let mut maps = Vec::with_capacity(states);
        let mut conditional = BTreeSet::new();
        for state in 0..states {
            if monitor.is_some_and(TaskMonitor::cancelled) { return Err(ScratchError { position: None, reason: "compilation cancelled" }); }
            let positions = placements(tokens, &axes, state);
            let inputs = graph.reader_sources(reader, |Axis::Token(token, _)| {
                tokens.binary_search(&token).map_or(initial[token], |axis| positions[axis].1)
            }, &mut search)?;
            conditional.extend(find_changed_links(&basis, &inputs));
            maps.push(inputs);
        }
        let mut groups = Vec::new();
        for side in [false, true] {
            let sources: Vec<_> = conditional.iter().filter_map(|&(source, port)| (port == side).then_some(source)).collect();
            if sources.is_empty() { continue; }
            let inputs = maps.iter().map(|map| map.iter().filter_map(|(&(source, port), &attenuation)|
                (port == side && conditional.contains(&(source, port))).then_some((source, attenuation))).collect()).collect();
            groups.push(InputArrangement { target, side, silent: matches!(program.initial[target], Block::Piston { .. }),
                tokens: tokens.to_vec(), token_states: axes.clone(), conditional_sources: sources, inputs });
        }
            Ok((reader, groups))
        })?;
        for (reader, lowered) in lowered {
            let estimate = lowered.iter().fold(0usize, |total, group| {
                let conditions: HashSet<_> = group.inputs.iter().flatten().copied().collect();
                let ranges = group.inputs.len().saturating_mul(group.tokens.len()).saturating_mul(2);
                let row_bytes = group.inputs.capacity().saturating_mul(std::mem::size_of::<Vec<(ArrangementSource, u8)>>());
                let value_bytes = group.inputs.iter().map(|row| row.capacity().saturating_mul(std::mem::size_of::<(ArrangementSource, u8)>())).sum::<usize>();
                let plan_bytes = ranges.saturating_mul(2 * std::mem::size_of::<std::ops::Range<usize>>()
                    + 2 * conditions.len().saturating_mul(std::mem::size_of::<super::program::ArrangementFlip>()));
                let mask_bytes = conditions.len().saturating_mul(group.inputs.len() + std::mem::size_of::<super::program::ArrangementLink>());
                total.saturating_add(row_bytes).saturating_add(value_bytes).saturating_add(plan_bytes).saturating_add(mask_bytes)
            });
            if dense_bytes.saturating_add(estimate) <= MAX_DENSE_BYTES { dense_bytes += estimate; groups.extend(lowered); }
            else { fallback.insert(reader); }
        }
        start = end;
    }
    for reader in fallback {
        graph.reader_sources(reader, |Axis::Token(token, _)| initial[token], &mut Search::default())?;
        let target = graph.reader_component(reader);
        routed.insert(target);
        let potential = graph.potential_sources(reader);
        for side in [false, true] {
            let sources: Vec<_> = potential.keys().filter_map(|&(source, port)| (port == side).then_some(source)).collect();
            if sources.is_empty() { continue; }
            groups.push(InputArrangement { target, side, silent: matches!(program.initial[target], Block::Piston { .. }),
                tokens: Vec::new(), token_states: Vec::new(), conditional_sources: sources.clone(),
                inputs: vec![sources.into_iter().map(|source| (source, 0)).collect()] });
        }
    }
    graph.retain_readers(&routed);
    if std::env::var_os("MCHPRS_REDPILER_PHASE_TIMINGS").is_some() {
        eprintln!("Redpiler topology lowering: targets={} arrangement_groups={} routed_targets={} dense_table_estimated_bytes={} elapsed_ms={}", targets.len(), groups.len(), routed.len(), dense_bytes, started.elapsed().as_millis());
    }
    Ok(Discovery { groups, routing: (!routed.is_empty()).then_some(graph) })
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concrete_wire_axes_fold_only_cells_without_wire_contacts() {
        let wire = BlockPos::new(10, 10, 10);
        let lamp = BlockPos::new(10, 13, 10);
        let mut program = Program::new(vec![wire, lamp], vec![Block::from_name("redstone_wire").unwrap(), Block::RedstoneLamp { lit: false }],
            vec![], vec![[vec![], vec![]]; 2], vec![], vec![], vec![]).unwrap();
        for block in [Block::from_name("black_concrete").unwrap(), Block::RedstoneBlock] {
            program.tokens.push(super::super::compiler::PistonToken {
                block, position: Some(BlockPos::new(10, 12, 10)), cells: vec![BlockPos::new(10, 12, 10), BlockPos::new(10, 11, 10)],
            });
        }
        let components = program.positions.iter().enumerate().map(|(node, &pos)| (pos, node)).collect();
        let indexes = ProbeIndexes::new(&components, &program.positions, &program.initial, &program.tokens, None).unwrap();
        assert_eq!(electrical_states(&program, &indexes, &[0, 1], 0), vec![vec![0, 0, 1], vec![0, 1, 2]]);
        assert_eq!(electrical_states(&program, &indexes, &[0], 1), vec![vec![0, 1, 2]]);
    }

}
