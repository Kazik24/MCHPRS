use super::program::{ArrangementSource, InputArrangement, PISTON_HEAD_NOTIFICATION};
use super::{Program, ScratchEntry, ScratchError, ScratchWorld};
use crate::redpilator::TaskMonitor;
use crate::world::World;
use mchprs_blocks::{BlockFace, BlockPos, blocks::Block};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

type InputKey = (ArrangementSource, bool);
type InputValue = (u8, bool);
type Inputs = BTreeMap<InputKey, InputValue>;

/// Return keys whose values were added, removed, or changed, in sorted order.
pub(super) fn find_changed_links<K: Ord + Copy, V: PartialEq>(
    before: &std::collections::BTreeMap<K, V>,
    after: &std::collections::BTreeMap<K, V>,
) -> Vec<K> {
    let all_sources: std::collections::BTreeSet<_> = before.keys().chain(after.keys()).copied().collect();
    all_sources.into_iter().filter(|source| before.get(source) != after.get(source)).collect()
}

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
    head_owners: HashMap<BlockPos, Vec<usize>>,
}

impl<'a> ProbeIndexes<'a> {
    pub(super) fn notification(&self, event: &crate::world::RedstoneUpdateObservation) -> Option<usize> {
        match event.block {
            Block::PistonHead { head } if event.input_on.is_some() => self
                .components
                .get(&event.pos.offset(head.facing.opposite().into()))
                .map(|&base| base | PISTON_HEAD_NOTIFICATION),
            Block::PistonHead { .. } => None,
            _ => self.components.get(&event.pos).copied(),
        }
    }

    pub(super) fn notifications_at(&self, position: BlockPos) -> impl Iterator<Item = usize> + '_ {
        self.components.get(&position).copied().into_iter().chain(
            self.head_owners
                .get(&position)
                .into_iter()
                .flatten()
                .map(|&base| base | PISTON_HEAD_NOTIFICATION),
        )
    }

    pub(super) fn source_at(&self, pos: BlockPos) -> Result<ArrangementSource, ScratchError> {
        if let Some(&token) = self.redstone_tokens.get(&pos) {
            Ok(ArrangementSource::Token(token))
        } else if let Some(&node) = self.components.get(&pos) {
            Ok(ArrangementSource::Component(node))
        } else {
            Err(ScratchError {
                position: Some(pos),
                reason: "unrepresented arrangement source",
            })
        }
    }
    /// Index component positions and movement-sensitive notification routes.
    ///
    /// Inputs: The component-position map, initial blocks, token records and optional monitor.
    ///
    /// Index wires and movable redstone sources, associate piston-head cells with their bases, and check
    /// cancellation while building the lookups.
    pub(super) fn new(
        components: &'a HashMap<BlockPos, usize>,
        positions: &[BlockPos],
        initial: &[Block],
        tokens: &[super::pistons::PistonToken],
        monitor: Option<&TaskMonitor>,
    ) -> Result<ProbeIndexes<'a>, ScratchError> {
        let mut redstone_tokens = HashMap::new();
        let mut wire_at = HashMap::new();
        let mut head_owners: HashMap<BlockPos, Vec<usize>> = HashMap::new();
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
            if let Block::Piston { piston } = block {
                head_owners.entry(pos.offset(piston.facing.into())).or_default().push(index);
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
            head_owners,
        })
    }
}

/// Resolve either direct wire contacts or their upstream sources.
///
/// Inputs: The world, wire positions, monitor and expand flag.
fn wire_sources(
    world: &impl World,
    wires: &[BlockPos],
    monitor: Option<&TaskMonitor>,
    expand: bool,
) -> Result<Vec<super::super::scratch::WireSource>, ScratchError> {
    if !expand {
        return Ok(wires
            .iter()
            .map(|&pos| super::super::scratch::WireSource {
                wire: pos,
                source: pos,
                attenuation: 0,
            })
            .collect());
    }
    if let Some(monitor) = monitor {
        crate::redpilator::scratch::discover_wire_sources_monitored(world, wires, monitor)
    } else {
        crate::redpilator::scratch::discover_wire_sources(world, wires)
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

pub(super) fn ports_of(
    world: &impl World,
    program: &Program,
    member: usize,
    monitor: Option<&TaskMonitor>,
    indexes: &ProbeIndexes<'_>,
) -> Result<Inputs, ScratchError> {
    discover_inputs(world, program, member, monitor, indexes, false)
}

/// Discover the main and side sources feeding a component.
///
/// Inputs: The world, program, component ID, monitor, position indexes and expansion mode.
///
/// Apply the receiver's native power rules, optionally traverse connected wires, then merge source
/// identities using minimum attenuation and the required notification behavior.
fn discover_inputs(
    world: &impl World,
    program: &Program,
    member: usize,
    monitor: Option<&TaskMonitor>,
    indexes: &ProbeIndexes<'_>,
    expand: bool,
) -> Result<Inputs, ScratchError> {
    let pos = program.positions[member];
    let block = program.initial[member];
    let mut found = Vec::new();
    let mut side = Vec::new();
    match reader_kind(block) {
        Some(ReaderKind::Wire) => found = wire_sources(world, &[pos], monitor, expand)?,
        Some(ReaderKind::Piston) => {
            let Block::Piston { piston } = block else { unreachable!() };
            found = if expand {
                crate::redpilator::scratch::discover_piston_sources(world, pos, piston.facing)?
            } else {
                crate::redpilator::scratch::discover_piston_ports(world, pos, piston.facing)?
            };
        }
        Some(ReaderKind::Lamp) => {
            found = if expand {
                crate::redpilator::scratch::discover_lamp_sources(world, pos)?
            } else {
                crate::redpilator::scratch::discover_lamp_ports(world, pos)?
            }
        }
        Some(ReaderKind::Torch) => {
            found = if expand {
                crate::redpilator::scratch::discover_torch_sources(world, pos, block)?
            } else {
                crate::redpilator::scratch::discover_torch_ports(world, pos, block)?
            }
        }
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
                            found.push(crate::redpilator::scratch::WireSource {
                                wire: pos,
                                source: at,
                                attenuation: 0,
                            });
                        }
                    }
                }
            } else if crate::redstone::power::emits_weak_power(block, world, rear, face, true) {
                found.push(crate::redpilator::scratch::WireSource {
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
                    side.push(crate::redpilator::scratch::WireSource {
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

/// Fold token positions that are electrically equivalent for one reader.
///
/// Inputs: The program, wire indexes, relevant token IDs and target component.
///
/// Keep absence as state zero; fold eligible concrete placements without reader/wire contacts into that
/// state, and number the remaining distinct positions.
fn electrical_states(program: &Program, indexes: &ProbeIndexes<'_>, tokens: &[usize], target: usize) -> Vec<Vec<usize>> {
    tokens
        .iter()
        .map(|&token| {
            let record = &program.tokens[token];
            let fold = matches!(record.block, Block::Concrete { .. });
            let mut next = 0;
            std::iter::once(0)
                .chain(record.cells.iter().map(|&cell| {
                    // Other readers also query local conductors, including piston quasi-power.
                    let outside_reader = matches!(program.initial[target], Block::RedstoneWire { .. }) || !near(cell, program.positions[target], 2);
                    if fold && outside_reader && !BlockFace::values().iter().any(|&face| indexes.wire_at.contains_key(&cell.offset(face))) {
                        0
                    } else {
                        next += 1;
                        next
                    }
                }))
                .collect()
        })
        .collect()
}

fn placements(tokens: &[usize], axes: &[Vec<usize>], mut arrangement: usize) -> Vec<(usize, usize)> {
    // Decode the flat arrangement index into one selected position for each token axis.
    tokens
        .iter()
        .zip(axes)
        .map(|(&token, states)| {
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

/// Fold axis digits only when every combination of the other axes has identical inputs.
fn compact_arrangement(group: &mut InputArrangement) {
    let mut stride = 1;
    for axis in 0..group.tokens.len() {
        let radix = group.token_states[axis].iter().copied().max().unwrap_or(0) + 1;
        let mut representatives = Vec::new();
        let mut digits = vec![0; radix];
        for digit in 0..radix {
            let equivalent = representatives.iter().position(|&other| {
                (0..group.inputs.len())
                    .filter(|state| state / stride % radix == 0)
                    .all(|state| group.inputs[state + digit * stride] == group.inputs[state + other * stride])
            });
            digits[digit] = equivalent.unwrap_or_else(|| {
                representatives.push(digit);
                representatives.len() - 1
            });
        }
        if representatives.len() != radix {
            let mut inputs = Vec::with_capacity(group.inputs.len() / radix * representatives.len());
            for block in (0..group.inputs.len()).step_by(stride * radix) {
                for &digit in &representatives {
                    inputs.extend((0..stride).map(|offset| std::mem::take(&mut group.inputs[block + digit * stride + offset])));
                }
            }
            group.inputs = inputs;
            for digit in &mut group.token_states[axis] {
                *digit = digits[*digit];
            }
        }
        // Keep token axes, including constant ones, to preserve movement-group ordering.
        stride *= representatives.len();
    }
    group.tokens.shrink_to_fit();
    group.token_states.shrink_to_fit();
    for states in &mut group.token_states {
        states.shrink_to_fit();
    }
    group.conditional_sources.shrink_to_fit();
    group.inputs.shrink_to_fit();
    for inputs in &mut group.inputs {
        inputs.shrink_to_fit();
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct DenseRank {
    reader: usize,
    work: usize,
    bytes: usize,
    states: usize,
}

impl Ord for DenseRank {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let left = (self.work as u128) * (other.bytes as u128) * (other.states as u128);
        let right = (other.work as u128) * (self.bytes as u128) * (self.states as u128);
        left.cmp(&right)
            .then_with(|| other.reader.cmp(&self.reader))
            .then_with(|| self.work.cmp(&other.work))
            .then_with(|| self.bytes.cmp(&other.bytes))
            .then_with(|| self.states.cmp(&other.states))
    }
}

impl PartialOrd for DenseRank {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn admit_dense(rank: DenseRank, ranks: &mut BTreeSet<DenseRank>, bytes: &mut usize, limit: usize) -> Option<Vec<usize>> {
    if rank.bytes > limit {
        return None;
    }
    let needed = rank.bytes.saturating_sub(limit - *bytes);
    let mut recovered = 0usize;
    let mut count = 0;
    for old in ranks.iter().take_while(|&&old| old < rank) {
        if recovered >= needed {
            break;
        }
        recovered += old.bytes;
        count += 1;
    }
    if recovered < needed {
        return None;
    }
    let mut removed = Vec::with_capacity(count);
    for _ in 0..count {
        let old = ranks.pop_first().unwrap();
        *bytes -= old.bytes;
        removed.push(old.reader);
    }
    *bytes += rank.bytes;
    ranks.insert(rank);
    Some(removed)
}

fn compact_selected(selected: &mut [Option<Vec<InputArrangement>>], ranks: &mut BTreeSet<DenseRank>, bytes: &mut usize) -> usize {
    let mut removed_rows = 0;
    *bytes = 0;
    for mut rank in std::mem::take(ranks) {
        let groups = selected[rank.reader].as_mut().unwrap();
        for group in groups.iter_mut() {
            let before = group.inputs.len();
            compact_arrangement(group);
            removed_rows += before - group.inputs.len();
        }
        rank.bytes = groups.iter().fold(0usize, |total, group| total.saturating_add(dense_table_bytes(group)));
        *bytes += rank.bytes;
        ranks.insert(rank);
    }
    removed_rows
}

/// Estimate retained memory for a dense arrangement table.
///
/// Inputs: The input arrangement's state mappings, source lists and allocated capacities.
///
/// Count unique connections and departure/arrival differences, then use saturating arithmetic to include
/// input rows, movement axes, transitions, masks and spare flip storage.
// Estimate retained logical payload without assuming every input flips at every transition.
fn dense_table_bytes(group: &InputArrangement) -> usize {
    use super::movement::{ArrangementFlip, MovementAxis, MovementGroup, MovementTransition};
    use super::program::ArrangementLink;
    if group.token_states.len() != group.tokens.len() {
        return usize::MAX;
    }
    let states = group.inputs.len();
    let conditions: HashSet<_> = group.inputs.iter().flatten().copied().collect();
    let mut flips = 0usize;
    let mut stride = 1usize;
    for mapping in &group.token_states {
        let radix = mapping.iter().copied().max().unwrap_or(0) + 1;
        for state in 0..states {
            let digit = state / stride % radix;
            if digit == 0 {
                continue;
            }
            let empty = state - digit * stride;
            let a = &group.inputs[state];
            let b = &group.inputs[empty];
            let changed = a
                .iter()
                .filter(|entry| !b.contains(entry))
                .count()
                .saturating_add(b.iter().filter(|entry| !a.contains(entry)).count());
            // Each change has a departure and an arrival flip.
            flips = flips.saturating_add(changed.saturating_mul(2));
        }
        stride = stride.saturating_mul(radix);
    }
    let size = std::mem::size_of::<usize>();
    let axes = group.tokens.len();
    // Axis ranges are retained twice; allow for spare capacity in the flattened copy.
    let ranges = states
        .saturating_mul(axes)
        .saturating_mul(6)
        .saturating_mul(std::mem::size_of::<std::ops::Range<usize>>());
    let transitions = group
        .token_states
        .iter()
        .map(|map| map.len().saturating_sub(1))
        .fold(0usize, usize::saturating_add)
        .saturating_mul(2 * std::mem::size_of::<MovementTransition>() + 2 * std::mem::size_of::<std::ops::Range<usize>>());
    let rows = group
        .inputs
        .capacity()
        .saturating_mul(std::mem::size_of::<Vec<(ArrangementSource, u8)>>())
        .saturating_add(
            group
                .inputs
                .iter()
                .map(|row| row.capacity().saturating_mul(std::mem::size_of::<(ArrangementSource, u8)>()))
                .fold(0usize, usize::saturating_add),
        );
    let input_metadata = (2 * std::mem::size_of::<InputArrangement>())
        .saturating_add(group.tokens.capacity().saturating_mul(size))
        .saturating_add(group.token_states.capacity().saturating_mul(std::mem::size_of::<Vec<usize>>()))
        .saturating_add(
            group
                .token_states
                .iter()
                .map(|map| map.capacity().saturating_mul(size))
                .fold(0usize, usize::saturating_add),
        )
        .saturating_add(
            group
                .conditional_sources
                .capacity()
                .saturating_mul(std::mem::size_of::<ArrangementSource>()),
        );
    let movement_metadata = (2 * std::mem::size_of::<MovementGroup>())
        .saturating_add(axes.saturating_mul(2 * std::mem::size_of::<MovementAxis>() + 8 * size))
        .saturating_add(
            group
                .token_states
                .iter()
                .map(|map| map.len().saturating_mul(size))
                .fold(0usize, usize::saturating_add),
        );
    rows.saturating_add(input_metadata)
        .saturating_add(movement_metadata)
        .saturating_add(ranges)
        // Allow spare capacity in the flattened flip vector.
        .saturating_add(transitions)
        .saturating_add(flips.saturating_mul(2 * std::mem::size_of::<ArrangementFlip>()))
        .saturating_add(conditions.len().saturating_mul(states + 2 * std::mem::size_of::<ArrangementLink>()))
}

pub(super) fn run(
    world: &impl World,
    bounds: (BlockPos, BlockPos),
    entry: &ScratchEntry,
    program: &Program,
    indexes: &ProbeIndexes<'_>,
    monitor: Option<&TaskMonitor>,
) -> Result<Discovery, ScratchError> {
    // This bounds estimated retained tables, not compilation or process memory.
    const MAX_DENSE_TABLE_BYTES: usize = 768 * 1024 * 1024;
    run_with_dense_budget(world, bounds, entry, program, indexes, monitor, MAX_DENSE_TABLE_BYTES)
}

/// Choose dense arrangement tables or routing for movement-dependent readers.
///
/// Inputs: The captured world/entry, program, indexes, monitor and retained-table byte budget.
///
/// Build local wire/reader graphs, enumerate bounded non-overlapping token states, retain affordable dense
/// tables and keep graph routing for the remaining readers.
fn run_with_dense_budget(
    world: &impl World,
    bounds: (BlockPos, BlockPos),
    entry: &ScratchEntry,
    program: &Program,
    indexes: &ProbeIndexes<'_>,
    monitor: Option<&TaskMonitor>,
    max_dense_bytes: usize,
) -> Result<Discovery, ScratchError> {
    use super::routing::{Axis, Search, WireGraph};
    if program.tokens.is_empty() {
        return Ok(Discovery {
            groups: Vec::new(),
            routing: None,
        });
    }
    let started = std::time::Instant::now();
    let mut scratch = ScratchWorld::from_world_monitored(world, bounds, entry.clone(), monitor)?;
    let wires = program
        .positions
        .iter()
        .zip(&program.initial)
        .filter_map(|(&pos, block)| matches!(block, Block::RedstoneWire { .. }).then_some(pos))
        .collect();
    let mut graph = WireGraph::compile(&mut scratch, wires, &program.tokens, monitor)?;
    let targets: Vec<_> = program
        .initial
        .iter()
        .enumerate()
        .filter_map(|(node, &block)| reader_kind(block).is_some().then_some(node))
        .collect();
    graph.compile_readers(&mut scratch, program, &targets, indexes, monitor)?;
    let initial: Vec<_> = program
        .tokens
        .iter()
        .map(|record| {
            record
                .position
                .and_then(|pos| record.cells.iter().position(|&cell| cell == pos))
                .map_or(0, |cell| cell + 1)
        })
        .collect();
    const MAX_TEMP_SLOTS: usize = 65_536;
    let mut jobs = Vec::new();
    let mut fallback = BTreeSet::new();
    let mut rejected_overlap = 0usize;
    let mut rejected_states = 0usize;
    let mut rejected_slots = 0usize;
    for reader in 0..graph.reader_count() {
        let tokens = graph.reader_dependencies(reader);
        if tokens.is_empty() {
            continue;
        }
        let target = graph.reader_component(reader);
        let axes = electrical_states(program, indexes, tokens, target);
        let states = axes
            .iter()
            .try_fold(1usize, |count, axis| count.checked_mul(axis.iter().copied().max().unwrap_or(0) + 1));
        let mut occupied = HashSet::new();
        let overlaps = tokens
            .iter()
            .flat_map(|&token| &program.tokens[token].cells)
            .any(|&cell| !occupied.insert(cell));
        let sources = graph.potential_sources(reader).len();
        let slots = states.map_or(usize::MAX, |states| states.saturating_mul(sources.max(1)));
        if !overlaps && states.is_some_and(|states| states <= super::movement::MAX_ARRANGEMENT_STATES) && slots <= MAX_TEMP_SLOTS {
            jobs.push((reader, axes, states.unwrap(), slots));
        } else {
            if overlaps {
                rejected_overlap += 1;
            } else if states.is_none_or(|states| states > super::movement::MAX_ARRANGEMENT_STATES) {
                rejected_states += 1;
            } else {
                rejected_slots += 1;
            }
            fallback.insert(reader);
        }
    }
    let mut groups = Vec::new();
    let mut routed = BTreeSet::new();
    let mut dense_bytes = 0usize;
    let mut selected = vec![None; graph.reader_count()];
    let mut ranks = BTreeSet::new();
    let mut sampled_states = 0usize;
    let mut compact_states = 0usize;
    let mut compact = false;
    let mut start = 0;
    if let Some(monitor) = monitor {
        monitor.set_progress(0, jobs.len(), "arrangements");
    }
    while start < jobs.len() {
        let mut end = start;
        let mut slots = 0usize;
        while end < jobs.len() && slots + jobs[end].3 <= MAX_TEMP_SLOTS {
            slots += jobs[end].3;
            end += 1;
        }
        let lowered = jobs[start..end]
            .par_iter()
            .map(|&(reader, ref axes, states, _)| {
                let target = graph.reader_component(reader);
                let tokens = graph.reader_dependencies(reader);
                let mut search = Search::default();
                let basis = graph.reader_sources(reader, |Axis::Token(token, _)| initial[token], &mut search)?;
                search.spent = 0;
                let mut maps = Vec::with_capacity(states);
                let mut conditional = BTreeSet::new();
                for state in 0..states {
                    if monitor.is_some_and(TaskMonitor::cancelled) {
                        return Err(ScratchError {
                            position: None,
                            reason: "compilation cancelled",
                        });
                    }
                    let positions = placements(tokens, &axes, state);
                    let inputs = graph.reader_sources(
                        reader,
                        |Axis::Token(token, _)| tokens.binary_search(&token).map_or(initial[token], |axis| positions[axis].1),
                        &mut search,
                    )?;
                    conditional.extend(find_changed_links(&basis, &inputs));
                    maps.push(inputs);
                }
                let mut groups = Vec::new();
                for side in [false, true] {
                    let sources: Vec<_> = conditional
                        .iter()
                        .filter_map(|&(source, port)| (port == side).then_some(source))
                        .collect();
                    if sources.is_empty() {
                        continue;
                    }
                    let inputs = maps
                        .iter()
                        .map(|map| {
                            map.iter()
                                .filter_map(|(&(source, port), &attenuation)| {
                                    (port == side && conditional.contains(&(source, port))).then_some((source, attenuation))
                                })
                                .collect()
                        })
                        .collect();
                    let group = InputArrangement {
                        target,
                        side,
                        silent: matches!(program.initial[target], Block::Piston { .. }),
                        tokens: tokens.to_vec(),
                        token_states: axes.clone(),
                        conditional_sources: sources,
                        inputs,
                    };
                    groups.push(group);
                }
                if let Some(monitor) = monitor {
                    monitor.advance_progress();
                }
                // Assume uniform token movement: visits per query times dependency count.
                let work = search.spent.saturating_add(states).saturating_mul(tokens.len());
                Ok((reader, groups, work, states))
            })
            .collect::<Result<Vec<_>, ScratchError>>()?;
        for (reader, mut lowered, work, states) in lowered {
            sampled_states += lowered.len() * states;
            let mut estimate = lowered.iter().fold(0usize, |total, group| total.saturating_add(dense_table_bytes(group)));
            // Keep shared mappings intact unless table memory pressure requires quotienting.
            if !compact && dense_bytes.saturating_add(estimate) > max_dense_bytes {
                compact = true;
                compact_states -= compact_selected(&mut selected, &mut ranks, &mut dense_bytes);
            }
            if compact {
                for group in &mut lowered {
                    compact_arrangement(group);
                }
                estimate = lowered.iter().fold(0usize, |total, group| total.saturating_add(dense_table_bytes(group)));
            }
            compact_states += lowered.iter().map(|group| group.inputs.len()).sum::<usize>();
            let rank = DenseRank {
                reader,
                work,
                bytes: estimate,
                states,
            };
            if let Some(removed) = admit_dense(rank, &mut ranks, &mut dense_bytes, max_dense_bytes) {
                for reader in removed {
                    selected[reader] = None;
                    fallback.insert(reader);
                }
                selected[reader] = Some(lowered);
            } else {
                fallback.insert(reader);
            }
        }
        start = end;
    }
    // Admission order must not reorder the retained input/movement programs.
    for lowered in selected.into_iter().flatten() {
        groups.extend(lowered);
    }
    let rejected_budget = fallback.len() - rejected_overlap - rejected_states - rejected_slots;
    for reader in fallback {
        graph.reader_sources(reader, |Axis::Token(token, _)| initial[token], &mut Search::default())?;
        let target = graph.reader_component(reader);
        routed.insert(target);
        let potential = graph.potential_sources(reader);
        for side in [false, true] {
            let sources: Vec<_> = potential.keys().filter_map(|&(source, port)| (port == side).then_some(source)).collect();
            if sources.is_empty() {
                continue;
            }
            groups.push(InputArrangement {
                target,
                side,
                silent: matches!(program.initial[target], Block::Piston { .. }),
                tokens: Vec::new(),
                token_states: Vec::new(),
                conditional_sources: sources.clone(),
                inputs: vec![sources.into_iter().map(|source| (source, 0)).collect()],
            });
        }
    }
    graph.retain_readers(&routed);
    if std::env::var_os("MCHPRS_REDPILATOR_PHASE_TIMINGS").is_some() {
        eprintln!(
            "Redpilator topology lowering: targets={} arrangement_groups={} routed_targets={} dense_table_estimated_bytes={} elapsed_ms={}",
            targets.len(),
            groups.len(),
            routed.len(),
            dense_bytes,
            started.elapsed().as_millis()
        );
        eprintln!("Redpilator routing fallback: overlap={rejected_overlap} states={rejected_states} slots={rejected_slots} budget={rejected_budget}");
        eprintln!(
            "Redpilator arrangement compression: sampled_rows={sampled_states} compact_rows={compact_states} budget_triggered={compact}; admission=uniform-token-visits-per-byte"
        );
    }
    Ok(Discovery {
        groups,
        routing: (!routed.is_empty()).then_some(graph),
    })
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dense_budget_preserves_shared_mappings_until_compaction_is_needed() {
        use mchprs_blocks::{
            BlockDirection,
            blocks::{Lever, LeverFace},
        };
        let cell = BlockPos::new(15, 15, 15);
        let source = cell.offset(BlockFace::West);
        let lamp = cell.offset(BlockFace::East);
        let lever = Block::Lever {
            lever: Lever::new(LeverFace::Wall, BlockDirection::West, true),
        };
        let mut world = super::super::tests::host();
        world.set_block(cell, Block::Stone {});
        world.set_block(source, lever);
        world.set_block(lamp, Block::RedstoneLamp { lit: true });
        let mut program = Program::new(
            vec![source, lamp],
            vec![lever, Block::RedstoneLamp { lit: true }],
            vec![0],
            vec![[vec![], vec![]]; 2],
            vec![],
            vec![],
            vec![],
        )
        .unwrap();
        program.tokens.push(super::super::pistons::PistonToken {
            block: Block::Stone {},
            position: Some(cell),
            cells: vec![cell, BlockPos::new(20, 20, 20)],
        });
        let components = program.positions.iter().enumerate().map(|(index, &pos)| (pos, index)).collect();
        let indexes = ProbeIndexes::new(&components, &program.positions, &program.initial, &program.tokens, None).unwrap();
        let entry = super::super::ScratchEntry {
            scheduled: vec![],
            piston: world.piston_state().clone(),
        };
        let bounds = (BlockPos::new(10, 10, 10), BlockPos::new(22, 21, 22));
        let full = run_with_dense_budget(&world, bounds, &entry, &program, &indexes, None, usize::MAX).unwrap();
        assert_eq!(full.groups.len(), 1);
        assert_eq!(full.groups[0].token_states, vec![vec![0, 1, 2]]);
        let mut compact = full.groups[0].clone();
        compact_arrangement(&mut compact);
        let limit = dense_table_bytes(&compact);
        assert!(limit < dense_table_bytes(&full.groups[0]));
        let pressured = run_with_dense_budget(&world, bounds, &entry, &program, &indexes, None, limit).unwrap();
        assert!(pressured.routing.is_none());
        assert_eq!(pressured.groups[0].token_states, vec![vec![0, 1, 0]]);
        for physical in 0..3 {
            let before = full.groups[0].token_states[0][physical];
            let after = pressured.groups[0].token_states[0][physical];
            assert_eq!(full.groups[0].inputs[before], pressured.groups[0].inputs[after]);
        }
    }

    #[test]
    fn compact_axes_preserve_every_physical_arrangement_and_axis_order() {
        let a = vec![(ArrangementSource::Component(3), 0)];
        let b = vec![(ArrangementSource::Component(3), 2)];
        let mut group = InputArrangement {
            target: 0,
            side: false,
            silent: false,
            tokens: vec![7, 9],
            token_states: vec![vec![0, 1, 2, 1], vec![0, 1]],
            conditional_sources: vec![ArrangementSource::Component(3)],
            inputs: vec![a.clone(), b.clone(), a.clone(), a.clone(), b.clone(), a],
        };
        let before = group.clone();
        let rank = DenseRank {
            reader: 0,
            work: 60,
            bytes: dense_table_bytes(&before),
            states: 6,
        };
        let mut selected = vec![Some(vec![before.clone()])];
        let mut ranks = BTreeSet::from([rank]);
        let mut bytes = rank.bytes;
        assert_eq!(compact_selected(&mut selected, &mut ranks, &mut bytes), 4);
        assert_eq!(bytes, dense_table_bytes(&selected[0].as_ref().unwrap()[0]));
        let updated = ranks.first().unwrap();
        assert_eq!((updated.work, updated.states), (rank.work, rank.states));
        assert_eq!(updated.bytes, bytes);
        assert!(bytes < rank.bytes);
        compact_arrangement(&mut group);
        assert_eq!(group.tokens, before.tokens);
        assert_eq!(group.token_states, vec![vec![0, 1, 0, 1], vec![0, 0]]);
        assert_eq!(group.inputs.len(), 2);
        assert!(dense_table_bytes(&group) < dense_table_bytes(&before));
        for x in 0..4 {
            for y in 0..2 {
                let old = before.token_states[0][x] + 3 * before.token_states[1][y];
                let new = group.token_states[0][x] + 2 * group.token_states[1][y];
                assert_eq!(before.inputs[old], group.inputs[new]);
            }
        }
        let once = group.clone();
        compact_arrangement(&mut group);
        assert_eq!(group.inputs, once.inputs);
        assert_eq!(group.token_states, once.token_states);
    }

    #[test]
    fn dense_admission_prefers_work_per_byte_and_keeps_budget_and_ties_stable() {
        let rank = |reader, work, bytes| DenseRank {
            reader,
            work,
            bytes,
            states: 1,
        };
        let mut ranks = BTreeSet::new();
        let mut bytes = 0;
        assert_eq!(admit_dense(rank(2, 100, 100), &mut ranks, &mut bytes, 100), Some(vec![]));
        assert_eq!(admit_dense(rank(1, 100, 100), &mut ranks, &mut bytes, 100), Some(vec![2]));
        assert_eq!(admit_dense(rank(3, 100, 100), &mut ranks, &mut bytes, 100), None);
        assert_eq!(admit_dense(rank(4, 200, 100), &mut ranks, &mut bytes, 100), Some(vec![1]));
        assert_eq!(admit_dense(rank(5, 1000, 101), &mut ranks, &mut bytes, 100), None);
        assert_eq!(admit_dense(rank(6, 1, 0), &mut ranks, &mut bytes, 100), Some(vec![]));
        assert_eq!(bytes, 100);
        assert_eq!(ranks.iter().map(|rank| rank.reader).collect::<BTreeSet<_>>(), BTreeSet::from([4, 6]));
        assert_eq!(bytes, ranks.iter().map(|rank| rank.bytes).sum::<usize>());
    }

    #[test]
    fn dense_budget_counts_sparse_and_attenuation_transitions() {
        use super::super::movement::{ArrangementFlip, MovementAxis, MovementGroup, MovementTransition};
        use super::super::program::ArrangementLink;
        let source = ArrangementSource::Component(0);
        let token = ArrangementSource::Token(0);
        let make = |inputs| InputArrangement {
            target: 1,
            side: false,
            silent: false,
            tokens: vec![0, 1],
            token_states: vec![vec![0, 1, 1], vec![0, 1]],
            conditional_sources: vec![source, token],
            inputs,
        };
        let sparse = make(vec![
            vec![(source, 0)],
            vec![(source, 0)],
            vec![(source, 0), (token, 2)],
            vec![(source, 1), (token, 2)],
        ]);
        let unchanged = make(vec![vec![(source, 0)]; 4]);
        assert!(dense_table_bytes(&sparse) > dense_table_bytes(&unchanged));

        let mut program = Program::new(
            vec![BlockPos::new(0, 0, 0), BlockPos::new(1, 0, 0)],
            vec![Block::RedstoneBlock, Block::RedstoneLamp { lit: false }],
            vec![],
            vec![[vec![], vec![]]; 2],
            vec![],
            vec![],
            vec![],
        )
        .unwrap();
        for x in 0..2 {
            program.tokens.push(super::super::pistons::PistonToken {
                block: Block::Stone {},
                position: None,
                cells: if x == 0 {
                    vec![BlockPos::new(x, 1, 0), BlockPos::new(x, 2, 0)]
                } else {
                    vec![BlockPos::new(x, 1, 0)]
                },
            });
        }
        let mut other_side = sparse.clone();
        other_side.side = true;
        let estimate = dense_table_bytes(&sparse) + dense_table_bytes(&other_side);
        program.input_arrangements.extend([sparse, other_side]);
        program.compile_movement_flips().unwrap();
        assert_eq!(program.movement_groups.len(), 1, "identical axes should be merged");
        let retained = program.input_arrangements.capacity() * std::mem::size_of::<InputArrangement>()
            + program
                .input_arrangements
                .iter()
                .map(|group| {
                    group.tokens.capacity() * std::mem::size_of::<usize>()
                        + group.token_states.capacity() * std::mem::size_of::<Vec<usize>>()
                        + group
                            .token_states
                            .iter()
                            .map(|row| row.capacity() * std::mem::size_of::<usize>())
                            .sum::<usize>()
                        + group.conditional_sources.capacity() * std::mem::size_of::<ArrangementSource>()
                        + group.inputs.capacity() * std::mem::size_of::<Vec<(ArrangementSource, u8)>>()
                        + group
                            .inputs
                            .iter()
                            .map(|row| row.capacity() * std::mem::size_of::<(ArrangementSource, u8)>())
                            .sum::<usize>()
                })
                .sum::<usize>()
            + program.movement_groups.capacity() * std::mem::size_of::<MovementGroup>()
            + program
                .movement_groups
                .iter()
                .map(|group| {
                    group.axes.capacity() * std::mem::size_of::<MovementAxis>()
                        + group
                            .axes
                            .iter()
                            .map(|axis| {
                                axis.states.capacity() * std::mem::size_of::<usize>()
                                    + (axis.depart.capacity() + axis.arrive.capacity()) * std::mem::size_of::<std::ops::Range<usize>>()
                            })
                            .sum::<usize>()
                })
                .sum::<usize>()
            + program.movement_flips.capacity() * std::mem::size_of::<ArrangementFlip>()
            + program.arrangement_links.capacity() * std::mem::size_of::<ArrangementLink>()
            + program.arrangement_links.iter().map(|link| link.active.capacity()).sum::<usize>()
            + program.movement_ranges.capacity() * std::mem::size_of::<std::ops::Range<usize>>()
            + program.movement_transitions.capacity() * std::mem::size_of::<MovementTransition>()
            + program.movement_cells.capacity() * std::mem::size_of::<Vec<std::ops::Range<usize>>>()
            + program
                .movement_cells
                .iter()
                .map(|row| row.capacity() * std::mem::size_of::<std::ops::Range<usize>>())
                .sum::<usize>()
            + program.token_movement_groups.capacity() * std::mem::size_of::<Vec<(usize, usize)>>()
            + program
                .token_movement_groups
                .iter()
                .map(|row| row.capacity() * std::mem::size_of::<(usize, usize)>())
                .sum::<usize>();
        assert!(estimate >= retained, "estimate={estimate} retained={retained}");
    }

    #[test]
    fn dense_and_routed_inputs_match_during_movement_and_source_changes() {
        use mchprs_blocks::{
            BlockDirection,
            blocks::{Lever, LeverFace},
        };
        let cell = BlockPos::new(15, 15, 15);
        let source = cell.offset(BlockFace::West);
        let lamp = cell.offset(BlockFace::East);
        let lever = Block::Lever {
            lever: Lever::new(LeverFace::Wall, BlockDirection::West, true),
        };
        let mut world = super::super::tests::host();
        world.set_block(cell, Block::Stone {});
        world.set_block(source, lever);
        world.set_block(lamp, Block::RedstoneLamp { lit: true });
        let mut program = Program::new(
            vec![source, lamp],
            vec![lever, Block::RedstoneLamp { lit: true }],
            vec![0],
            vec![[vec![], vec![]]; 2],
            vec![],
            vec![],
            vec![],
        )
        .unwrap();
        program.tokens.push(super::super::pistons::PistonToken {
            block: Block::Stone {},
            position: Some(cell),
            cells: vec![cell],
        });
        let components = program.positions.iter().enumerate().map(|(index, &pos)| (pos, index)).collect();
        let indexes = ProbeIndexes::new(&components, &program.positions, &program.initial, &program.tokens, None).unwrap();
        let entry = super::super::ScratchEntry {
            scheduled: vec![],
            piston: world.piston_state().clone(),
        };
        let bounds = (BlockPos::new(10, 10, 10), BlockPos::new(22, 21, 22));
        let runtime = |budget| {
            let mut discovery = run_with_dense_budget(&world, bounds, &entry, &program, &indexes, None, budget).unwrap();
            assert_eq!(discovery.routing.is_some(), budget == 0);
            let mut program = program.clone();
            program.input_arrangements = discovery.groups;
            program.compile_movement_flips().unwrap();
            if let Some(graph) = &mut discovery.routing {
                graph.bind_links(&program.arrangement_links);
            }
            program.routing = discovery.routing.map(std::sync::Arc::new);
            let links: Vec<_> = program
                .arrangement_links
                .iter()
                .enumerate()
                .filter_map(|(index, link)| {
                    (link.source == ArrangementSource::Component(0)).then_some(super::super::program::Operation::ArrangementContribution(index))
                })
                .collect();
            for powered in [false, true] {
                program.set_output_program(0, powered, links.clone());
            }
            program.start()
        };
        let mut dense = runtime(usize::MAX);
        let mut routed = runtime(0);
        for (position, powered) in [(Some(cell), true), (None, true), (None, false), (Some(cell), false), (Some(cell), true)] {
            dense.set_token_pos(0, position);
            routed.set_token_pos(0, position);
            dense.set_input(0, powered).unwrap();
            routed.set_input(0, powered).unwrap();
            assert_eq!(dense.input_strength(1), if position.is_some() && powered { 15 } else { 0 });
            assert_eq!(routed.input_strength(1), dense.input_strength(1));
            assert_eq!(routed.pending_entries(), dense.pending_entries());
        }
    }

    #[test]
    fn mixed_budget_preserves_independent_recipients_and_routing_first_leftovers() {
        use mchprs_blocks::{
            BlockDirection,
            blocks::{Lever, LeverFace},
        };
        let cell = BlockPos::new(15, 15, 15);
        let source = cell.offset(BlockFace::West);
        let lamp = cell.offset(BlockFace::East);
        let other_lamp = cell.offset(BlockFace::South);
        let lever = Block::Lever {
            lever: Lever::new(LeverFace::Wall, BlockDirection::West, true),
        };
        let mut world = super::super::tests::host();
        world.set_block(cell, Block::Stone {});
        world.set_block(source, lever);
        world.set_block(lamp, Block::RedstoneLamp { lit: true });
        world.set_block(other_lamp, Block::RedstoneLamp { lit: true });
        let mut program = Program::new(
            vec![source, lamp, other_lamp],
            vec![lever, Block::RedstoneLamp { lit: true }, Block::RedstoneLamp { lit: true }],
            vec![0],
            vec![[vec![], vec![]]; 3],
            vec![],
            vec![],
            vec![],
        )
        .unwrap();
        program.tokens.push(super::super::pistons::PistonToken {
            block: Block::Stone {},
            position: Some(cell),
            cells: vec![cell],
        });
        let components = program.positions.iter().enumerate().map(|(index, &pos)| (pos, index)).collect();
        let indexes = ProbeIndexes::new(&components, &program.positions, &program.initial, &program.tokens, None).unwrap();
        let entry = super::super::ScratchEntry {
            scheduled: vec![],
            piston: world.piston_state().clone(),
        };
        let bounds = (BlockPos::new(10, 10, 10), BlockPos::new(22, 21, 22));
        let full = run_with_dense_budget(&world, bounds, &entry, &program, &indexes, None, usize::MAX).unwrap();
        let mixed_budget = full.groups.iter().map(dense_table_bytes).min().unwrap();
        let runtime = |budget| {
            let mut discovery = run_with_dense_budget(&world, bounds, &entry, &program, &indexes, None, budget).unwrap();
            assert_eq!(discovery.routing.is_some(), budget != usize::MAX);
            if budget == mixed_budget {
                assert!(discovery.groups.iter().any(|group| !group.tokens.is_empty()));
                assert!(discovery.groups.iter().any(|group| group.tokens.is_empty()));
            }
            let mut program = program.clone();
            program.input_arrangements = discovery.groups;
            program.compile_movement_flips().unwrap();
            if let Some(graph) = &mut discovery.routing {
                graph.bind_links(&program.arrangement_links);
            }
            program.routing = discovery.routing.map(std::sync::Arc::new);
            let links: Vec<_> = program
                .arrangement_links
                .iter()
                .enumerate()
                .filter_map(|(index, link)| {
                    (link.source == ArrangementSource::Component(0)).then_some(super::super::program::Operation::ArrangementContribution(index))
                })
                .collect();
            for powered in [false, true] {
                program.set_output_program(0, powered, links.clone());
            }
            program.start()
        };
        let mut dense = runtime(usize::MAX);
        let mut routed = runtime(0);
        let mut mixed = runtime(mixed_budget);
        for (position, powered) in [(Some(cell), true), (None, true), (None, false), (Some(cell), false), (Some(cell), true)] {
            dense.set_token_pos(0, position);
            routed.set_token_pos(0, position);
            mixed.set_token_pos(0, position);
            dense.set_input(0, powered).unwrap();
            routed.set_input(0, powered).unwrap();
            mixed.set_input(0, powered).unwrap();
            assert_eq!(dense.input_strength(1), if position.is_some() && powered { 15 } else { 0 });
            assert_eq!(routed.input_strength(1), dense.input_strength(1));
            // Leftover callbacks are routing-first in the existing executor, so budgets can change global order.
            for recipient in [lamp, other_lamp] {
                let expected: Vec<_> = dense.pending_entries().into_iter().filter(|entry| entry.pos == recipient).collect();
                assert_eq!(
                    routed
                        .pending_entries()
                        .into_iter()
                        .filter(|entry| entry.pos == recipient)
                        .collect::<Vec<_>>(),
                    expected
                );
                assert_eq!(
                    mixed
                        .pending_entries()
                        .into_iter()
                        .filter(|entry| entry.pos == recipient)
                        .collect::<Vec<_>>(),
                    expected
                );
            }
            if position.is_none() && powered {
                assert_eq!(
                    mixed.pending_entries().iter().map(|entry| entry.pos).collect::<Vec<_>>(),
                    vec![other_lamp, lamp]
                );
                assert_eq!(
                    dense.pending_entries().iter().map(|entry| entry.pos).collect::<Vec<_>>(),
                    vec![lamp, other_lamp]
                );
            }
            assert_eq!(mixed.input_strength(1), dense.input_strength(1));
            assert_eq!(mixed.input_strength(2), dense.input_strength(2));
            assert_eq!(routed.input_strength(2), dense.input_strength(2));
        }
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn llvm_routed_movement_and_handoff_match_rust() {
        if std::env::var_os("MCHPRS_LLVM_DLL").is_none()
            && !std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/native_engine.dll")
                .exists()
        {
            return;
        }
        use super::super::program::Operation;
        use mchprs_blocks::{BlockFacing, blocks::RedstonePiston};
        let mut world = super::super::tests::host();
        let lever = BlockPos::new(15, 15, 15);
        let base = lever.offset(BlockFace::East);
        let home = base.offset(BlockFace::East);
        let wire = home.offset(BlockFace::East).offset(BlockFace::East);
        let lamp = wire.offset(BlockFace::East);
        world.set_block(lever, Block::from_name("lever").unwrap());
        world.set_block(lever.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(
            base,
            Block::Piston {
                piston: RedstonePiston {
                    facing: BlockFacing::East,
                    sticky: true,
                    extended: false,
                },
            },
        );
        world.set_block(home, Block::RedstoneBlock);
        world.set_block(wire.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(wire, Block::from_name("redstone_wire").unwrap());
        world.set_block(
            wire,
            Block::RedstoneWire {
                wire: crate::redstone::wire::get_state_for_placement(&world, wire),
            },
        );
        world.set_block(lamp, Block::RedstoneLamp { lit: false });
        let entry = super::super::ScratchEntry {
            scheduled: vec![],
            piston: world.piston_state().clone(),
        };
        let bounds = (BlockPos::new(10, 10, 10), BlockPos::new(23, 20, 20));
        let mut program = super::super::compile_program(&world, bounds, entry.clone(), None).unwrap();
        let components = program.positions.iter().enumerate().map(|(index, &pos)| (pos, index)).collect();
        let indexes = ProbeIndexes::new(&components, &program.positions, &program.initial, &program.tokens, None).unwrap();
        let mut discovery = run_with_dense_budget(&world, bounds, &entry, &program, &indexes, None, 0).unwrap();
        assert!(discovery.routing.is_some());
        let old_operations: Vec<_> = (0..program.initial.len())
            .map(|source| {
                [false, true].map(|powered| {
                    program
                        .output_program(source, powered)
                        .iter()
                        .copied()
                        .filter(|op| !matches!(op, Operation::ArrangementContribution(_)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        program.input_arrangements = discovery.groups;
        program.compile_movement_flips().unwrap();
        discovery.routing.as_mut().unwrap().bind_links(&program.arrangement_links);
        program.routing = discovery.routing.map(std::sync::Arc::new);
        for (source, pair) in old_operations.into_iter().enumerate() {
            let links: Vec<_> = program
                .arrangement_links
                .iter()
                .enumerate()
                .filter_map(|(index, link)| {
                    (link.source == ArrangementSource::Component(source)).then_some(Operation::ArrangementContribution(index))
                })
                .collect();
            for (powered, old) in pair.into_iter().enumerate() {
                program.set_output_program(source, powered != 0, links.iter().copied().chain(old).collect());
            }
        }
        program.prepare_projection();
        let wire_id = program.positions.iter().position(|&pos| pos == wire).unwrap();
        let mut rust = program.clone().into_runtime_prepared();
        let mut projection = program.clone().into_runtime_prepared();
        let mut native = super::super::llvm::LlvmRuntime::new(&program, &[]).unwrap();
        let mut handoff: Option<super::super::Runtime> = None;
        let mut moved = false;
        for tick in 0..40 {
            if [0, 7, 15, 23, 32].contains(&tick) {
                rust.use_control(lever).unwrap();
                native.use_control(lever).unwrap();
                if let Some(runtime) = &mut handoff {
                    runtime.use_control(lever).unwrap();
                }
            }
            rust.tick().unwrap();
            native.tick().unwrap();
            if let Some(runtime) = &mut handoff {
                runtime.tick().unwrap();
            }
            native.sync_projection(&mut projection).unwrap();
            moved |= rust.token_positions[0] != Some(home);
            assert_eq!(projection.projected_blocks(), rust.projected_blocks(), "tick {tick}");
            assert_eq!(projection.token_positions, rust.token_positions, "tick {tick}");
            assert_eq!(projection.arrangement_contributions, rust.arrangement_contributions, "tick {tick}");
            assert_eq!(projection.routing_attenuations, rust.routing_attenuations, "tick {tick}");
            assert_eq!(projection.input_strength(wire_id), rust.input_strength(wire_id), "wire input tick {tick}");
            assert_eq!(projection.pending_entries(), rust.pending_entries(), "tick {tick}");
            if handoff.is_none() && rust.input_strength(wire_id) > 0 {
                let mut runtime = rust.program.clone().as_ref().clone().into_runtime_prepared();
                native.sync_projection(&mut runtime).unwrap();
                handoff = Some(runtime);
            }
            if let Some(runtime) = &handoff {
                assert_eq!(runtime.projected_blocks(), rust.projected_blocks(), "handoff tick {tick}");
                assert_eq!(runtime.arrangement_contributions, rust.arrangement_contributions, "handoff tick {tick}");
                assert_eq!(runtime.routing_attenuations, rust.routing_attenuations, "handoff tick {tick}");
                assert_eq!(
                    runtime.input_strength(wire_id),
                    rust.input_strength(wire_id),
                    "handoff wire input tick {tick}"
                );
                assert_eq!(runtime.pending_entries(), rust.pending_entries(), "handoff tick {tick}");
            }
        }
        assert!(moved, "fixture must exercise piston movement");
        assert!(handoff.is_some(), "fixture must hand off while the routed wire is powered");
    }

    #[test]
    fn concrete_wire_axes_fold_only_cells_without_wire_contacts() {
        let wire = BlockPos::new(10, 10, 10);
        let lamp = BlockPos::new(10, 13, 10);
        let mut program = Program::new(
            vec![wire, lamp],
            vec![Block::from_name("redstone_wire").unwrap(), Block::RedstoneLamp { lit: false }],
            vec![],
            vec![[vec![], vec![]]; 2],
            vec![],
            vec![],
            vec![],
        )
        .unwrap();
        for block in [Block::from_name("black_concrete").unwrap(), Block::RedstoneBlock] {
            program.tokens.push(super::super::pistons::PistonToken {
                block,
                position: Some(BlockPos::new(10, 12, 10)),
                cells: vec![BlockPos::new(10, 12, 10), BlockPos::new(10, 11, 10)],
            });
        }
        let components = program.positions.iter().enumerate().map(|(node, &pos)| (pos, node)).collect();
        let indexes = ProbeIndexes::new(&components, &program.positions, &program.initial, &program.tokens, None).unwrap();
        assert_eq!(electrical_states(&program, &indexes, &[0, 1], 0), vec![vec![0, 0, 1], vec![0, 1, 2]]);
        assert_eq!(electrical_states(&program, &indexes, &[0], 1), vec![vec![0, 1, 2]]);
    }
}
