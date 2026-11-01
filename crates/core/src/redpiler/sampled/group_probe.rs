use super::compiler::find_changed_links;
use super::program::{ArrangementSource, InputArrangement};
use super::{Program, ScratchEntry, ScratchError, ScratchWorld};
use crate::redpiler::TaskMonitor;
use crate::world::World;
use mchprs_blocks::{BlockFace, BlockPos, blocks::Block};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

type InputKey = (ArrangementSource, bool);
type InputValue = (u8, bool);
type Inputs = BTreeMap<InputKey, InputValue>;
type Dependency = (usize, ArrangementSource, bool);

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
    /// per wire component, neighboring wire component IDs.
    wire_neighbors: Vec<Vec<usize>>,
    /// non-wire reader positions mapped to component IDs.
    readers: HashMap<BlockPos, usize>,
}

impl<'a> ProbeIndexes<'a> {
    pub(super) fn new(
        components: &'a HashMap<BlockPos, usize>,
        positions: &[BlockPos],
        initial: &[Block],
        tokens: &[super::compiler::PistonToken],
        monitor: Option<&TaskMonitor>,
    ) -> Result<ProbeIndexes<'a>, ScratchError> {
        let mut redstone_tokens = HashMap::new();
        let mut wire_at = HashMap::new();
        let mut readers = HashMap::new();
        for (&pos, &block) in positions.iter().zip(initial) {
            if monitor.is_some_and(TaskMonitor::cancelled) {
                return Err(ScratchError {
                    position: None,
                    reason: "compilation cancelled",
                });
            }
            let index = components[&pos];
            if reader_kind(block).is_some_and(|kind| !matches!(kind, ReaderKind::Wire)) {
                readers.insert(pos, index);
            }
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
        let mut wire_neighbors = vec![Vec::new(); positions.len()];
        for (&pos, &wire) in &wire_at {
            if monitor.is_some_and(TaskMonitor::cancelled) {
                return Err(ScratchError {
                    position: None,
                    reason: "compilation cancelled",
                });
            }
            let neighbors = &mut wire_neighbors[wire];
            for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                for dy in -1..=1 {
                    if let Some(&neighbor) = wire_at.get(&BlockPos::new(pos.x + dx, pos.y + dy, pos.z + dz)) {
                        neighbors.push(neighbor);
                    }
                }
            }
            neighbors.sort_unstable();
        }
        Ok(Self {
            components,
            redstone_tokens,
            wire_at,
            wire_neighbors,
            readers,
        })
    }
}

#[derive(Default)]
struct ProbeMetrics {
    /// number of arrangement samples evaluated.
    samples: usize,
    /// number of component reads performed while sampling.
    member_reads: usize,
    /// number of affected arrangement members visited.
    affected_members: usize,
    #[cfg(test)]
    sample_times: [std::time::Duration; 4],
}

impl ProbeMetrics {
    fn add(&mut self, other: Self) {
        self.samples += other.samples;
        self.member_reads += other.member_reads;
        self.affected_members += other.affected_members;
    }
}

fn affected_members(
    token_cells: &[BlockPos],
    positions: &[BlockPos],
    indexes: &ProbeIndexes<'_>,
    monitor: Option<&TaskMonitor>,
) -> Result<BTreeSet<usize>, ScratchError> {
    let mut touched = HashSet::new();
    let mut queue = VecDeque::new();
    for &cell in token_cells {
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(&wire) = indexes.wire_at.get(&BlockPos::new(cell.x + dx, cell.y + dy, cell.z + dz))
                        && touched.insert(wire)
                    {
                        queue.push_back((wire, 0));
                    }
                }
            }
        }
    }
    while let Some((wire, depth)) = queue.pop_front() {
        if monitor.is_some_and(TaskMonitor::cancelled) {
            return Err(ScratchError {
                position: None,
                reason: "compilation cancelled",
            });
        }
        if depth < 15 {
            queue.extend(
                indexes.wire_neighbors[wire]
                    .iter()
                    .copied()
                    .filter_map(|neighbor| touched.insert(neighbor).then_some((neighbor, depth + 1))),
            );
        }
    }

    let mut members: BTreeSet<_> = touched.iter().copied().collect();
    for center in token_cells.iter().copied().chain(touched.iter().map(|&wire| positions[wire])) {
        if monitor.is_some_and(TaskMonitor::cancelled) {
            return Err(ScratchError {
                position: None,
                reason: "compilation cancelled",
            });
        }
        for dx in -2..=2 {
            for dy in -2..=2 {
                for dz in -2..=2 {
                    if let Some(&reader) = indexes.readers.get(&BlockPos::new(center.x + dx, center.y + dy, center.z + dz)) {
                        members.insert(reader);
                    }
                }
            }
        }
    }
    Ok(members)
}

fn wire_sources(
    world: &impl World,
    wires: &[BlockPos],
    monitor: Option<&TaskMonitor>,
) -> Result<Vec<super::super::scratch::WireSource>, ScratchError> {
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
    let pos = program.positions[member];
    let block = program.initial[member];
    let mut found = Vec::new();
    let mut side = Vec::new();
    match reader_kind(block) {
        Some(ReaderKind::Wire) => found = wire_sources(world, &[pos], monitor)?,
        Some(ReaderKind::Piston) => {
            let Block::Piston { piston } = block else { unreachable!() };
            found = crate::redpiler::scratch::discover_piston_sources(world, pos, piston.facing)?;
        }
        Some(ReaderKind::Lamp) => found = crate::redpiler::scratch::discover_lamp_sources(world, pos)?,
        Some(ReaderKind::Torch) => found = crate::redpiler::scratch::discover_torch_sources(world, pos, block)?,
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
                found = wire_sources(world, &[rear], monitor)?;
            } else if block.is_solid() {
                for direction in BlockFace::values() {
                    let at = rear.offset(direction);
                    if crate::redstone::power::emits_strong_power(world.get_block(at), world, at, direction, true) {
                        if matches!(world.get_block(at), Block::RedstoneWire { .. }) {
                            found.extend(wire_sources(world, &[at], monitor)?);
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
                    side.extend(wire_sources(world, &[at], monitor)?);
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
        let identity = if let Some(&index) = indexes.redstone_tokens.get(&source.source) {
            ArrangementSource::Token(index)
        } else if let Some(&index) = indexes.components.get(&source.source) {
            ArrangementSource::Component(index)
        } else {
            return Err(ScratchError {
                position: Some(source.source),
                reason: "unrepresented arrangement source",
            });
        };
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

fn sample(
    scratch: &mut ScratchWorld,
    program: &Program,
    placements: &[(usize, usize)],
    members: &[usize],
    monitor: Option<&TaskMonitor>,
    indexes: &ProbeIndexes<'_>,
    metrics: &mut ProbeMetrics,
) -> Result<Vec<Inputs>, ScratchError> {
    if monitor.is_some_and(TaskMonitor::cancelled) {
        return Err(ScratchError {
            position: None,
            reason: "compilation cancelled",
        });
    }
    #[cfg(test)]
    let started = std::time::Instant::now();
    scratch.revert();
    #[cfg(test)]
    let placed = { metrics.sample_times[0] += started.elapsed(); std::time::Instant::now() };
    for &(token, digit) in placements {
        let record = &program.tokens[token];
        let next = digit.checked_sub(1).map(|cell| record.cells[cell]);
        if record.position != next {
            if let Some(initial) = record.position {
                scratch.world_mut().set_block(initial, Block::Air);
            }
        }
        if let Some(next) = next {
            scratch.world_mut().set_block(next, record.block);
        }
    }
    #[cfg(test)]
    let queried = { metrics.sample_times[1] += placed.elapsed(); std::time::Instant::now() };
    let mut result = Vec::with_capacity(members.len());
    metrics.samples += 1;
    metrics.member_reads += members.len();
    for &member in members {
        if monitor.is_some_and(TaskMonitor::cancelled) {
            return Err(ScratchError {
                position: None,
                reason: "compilation cancelled",
            });
        }
        result.push(inputs_of(scratch.world(), program, member, monitor, indexes)?);
    }
    #[cfg(test)]
    let checked = { metrics.sample_times[2] += queried.elapsed(); std::time::Instant::now() };
    scratch.finish_probe()?;
    #[cfg(test)]
    { metrics.sample_times[3] += checked.elapsed(); }
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

fn count(program: &Program, tokens: &[usize], axes: &[Vec<usize>], pos: BlockPos) -> Result<usize, ScratchError> {
    let n = tokens
        .iter()
        .zip(axes).try_fold(1usize, |n, (_, states)| n.checked_mul(states.iter().copied().max().unwrap_or(0) + 1))
        .unwrap_or(usize::MAX);
    if n > super::program::MAX_ARRANGEMENT_STATES {
        if std::env::var_os("MCHPRS_REDPILER_BOUNDARY_TRACE").is_some() {
            eprintln!("Arrangement limit at {pos:?}: states={n}, tokens={:?}", tokens.iter().map(|&token| (token, &program.tokens[token])).collect::<Vec<_>>());
        }
        Err(ScratchError {
            position: Some(pos),
            reason: "too many piston states near dependency",
        })
    } else {
        Ok(n)
    }
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

pub(super) fn run(
    world: &impl World,
    bounds: (BlockPos, BlockPos),
    entry: &ScratchEntry,
    program: &Program,
    indexes: &ProbeIndexes<'_>,
    monitor: Option<&TaskMonitor>,
) -> Result<Vec<InputArrangement>, ScratchError> {
    let mut scratch = ScratchWorld::from_world_monitored(world, bounds, entry.clone(), monitor)?;
    let affected_started = std::time::Instant::now();
    let mut affects = Vec::with_capacity(program.tokens.len());
    for token in &program.tokens {
        affects.push(affected_members(&token.cells, &program.positions, indexes, monitor)?);
    }
    if std::env::var_os("MCHPRS_REDPILER_PHASE_TIMINGS").is_some() {
        eprintln!(
            "Redpiler arrangement affected-region construction: {} ms",
            affected_started.elapsed().as_millis()
        );
    }
    let mut metrics = ProbeMetrics {
        affected_members: affects.iter().map(BTreeSet::len).sum(),
        ..ProbeMetrics::default()
    };
    let mut tokens_near = vec![Vec::new(); program.positions.len()];
    for (token, members) in affects.iter().enumerate() {
        for &member in members {
            tokens_near[member].push(token);
        }
    }
    let single_started = std::time::Instant::now();
    let mut depends: BTreeMap<Dependency, BTreeSet<usize>> = BTreeMap::new();
    let mut grown = BTreeSet::new();
    let tokens: Vec<_> = (0..program.tokens.len()).collect();
    let singles = scratch.probe_jobs(&tokens, monitor, |scratch, &token| {
        let mut metrics = ProbeMetrics::default();
        let mut dependencies = Vec::new();
        if monitor.is_some_and(TaskMonitor::cancelled) {
            return Err(ScratchError {
                position: None,
                reason: "compilation cancelled",
            });
        }
        let members: Vec<_> = affects[token].iter().copied().collect();
        if members.is_empty() {
            return Ok((dependencies, metrics));
        }
        let baseline = sample(scratch, program, &[], &members, monitor, indexes, &mut metrics)?;
        let initial = program.tokens[token]
            .position
            .and_then(|p| program.tokens[token].cells.iter().position(|&c| c == p))
            .map_or(0, |i| i + 1);
        for digit in 0..=program.tokens[token].cells.len() {
            if monitor.is_some_and(TaskMonitor::cancelled) {
                return Err(ScratchError {
                    position: None,
                    reason: "compilation cancelled",
                });
            }
            if digit == initial {
                continue;
            }
            let changed = sample(scratch, program, &[(token, digit)], &members, monitor, indexes, &mut metrics)?;
            for (i, &member) in members.iter().enumerate() {
                for (source, side) in find_changed_links(&baseline[i], &changed[i]) {
                    dependencies.push((member, source, side));
                }
            }
        }
        Ok((dependencies, metrics))
    })?;
    for (token, (dependencies, work)) in singles.into_iter().enumerate() {
        metrics.add(work);
        for dep in dependencies {
            if depends.entry(dep).or_default().insert(token) {
                grown.insert(dep);
            }
        }
    }
    if std::env::var_os("MCHPRS_REDPILER_PHASE_TIMINGS").is_some() {
        eprintln!(
            "Redpiler arrangement individual-token probes: {} ms",
            single_started.elapsed().as_millis()
        );
    }
    let joint_started = std::time::Instant::now();
    let mut first = true;
    loop {
        let mut grouped: BTreeMap<(Vec<usize>, usize), Vec<Dependency>> = BTreeMap::new();
        for (&dep, tokens) in &depends {
            grouped.entry((tokens.iter().copied().collect(), dep.0)).or_default().push(dep);
        }
        let grouped: Vec<_> = grouped.into_iter().collect();
        let batches = scratch.probe_jobs(&grouped, monitor, |scratch, ((tokens, target), deps)| {
            let mut metrics = ProbeMetrics::default();
            let mut discovered = BTreeSet::new();
            if monitor.is_some_and(TaskMonitor::cancelled) {
                return Err(ScratchError {
                    position: None,
                    reason: "compilation cancelled",
                });
            }
            if !first && !deps.iter().any(|dep| grown.contains(dep)) {
                return Ok((discovered, metrics));
            }
            let members: Vec<_> = deps.iter().map(|dep| dep.0).collect::<BTreeSet<_>>().into_iter().collect();
            let nearby_tokens: BTreeSet<_> = members.iter().flat_map(|&member| tokens_near[member].iter().copied()).collect();
            let candidates: Vec<_> = nearby_tokens
                .into_iter()
                .filter(|candidate| {
                    !tokens.contains(candidate)
                        && tokens.iter().any(|&own| {
                            program.tokens[own]
                                .cells
                                .iter()
                                .any(|&a| program.tokens[*candidate].cells.iter().any(|&b| near(a, b, 5)))
                        })
                })
                .collect();
            if candidates.is_empty() {
                return Ok((discovered, metrics));
            }
            let axes = electrical_states(program, indexes, tokens, *target);
            let size = count(program, tokens, &axes, program.positions[*target])?;
            let initial: Vec<_> = tokens
                .iter()
                .map(|&token| {
                    program.tokens[token]
                        .position
                        .and_then(|p| program.tokens[token].cells.iter().position(|&c| c == p))
                        .map_or(0, |i| i + 1)
                })
                .collect();
            for state in 0..size {
                if monitor.is_some_and(TaskMonitor::cancelled) {
                    return Err(ScratchError {
                        position: None,
                        reason: "compilation cancelled",
                    });
                }
                let base = placements(tokens, &axes, state);
                if base.iter().map(|pair| pair.1).eq(initial.iter().copied()) {
                    continue;
                }
                let baseline = sample(scratch, program, &base, &members, monitor, indexes, &mut metrics)?;
                for &candidate in &candidates {
                    if monitor.is_some_and(TaskMonitor::cancelled) {
                        return Err(ScratchError {
                            position: None,
                            reason: "compilation cancelled",
                        });
                    }
                    let initial_candidate = program.tokens[candidate]
                        .position
                        .and_then(|p| program.tokens[candidate].cells.iter().position(|&c| c == p))
                        .map_or(0, |i| i + 1);
                    let candidate_axes = electrical_states(program, indexes, &[candidate], *target);
                    for (digit, &electrical) in candidate_axes[0].iter().enumerate() {
                        if monitor.is_some_and(TaskMonitor::cancelled) {
                            return Err(ScratchError {
                                position: None,
                                reason: "compilation cancelled",
                            });
                        }
                        if electrical == candidate_axes[0][initial_candidate]
                            || candidate_axes[0][..digit].contains(&electrical) {
                            continue;
                        }
                        let mut joint = base.clone();
                        joint.push((candidate, digit));
                        let changed = sample(scratch, program, &joint, &members, monitor, indexes, &mut metrics)?;
                        let mut new_links = Vec::new();
                        for (i, &member) in members.iter().enumerate() {
                            if !affects[candidate].contains(&member) {
                                continue;
                            }
                            for (source, side) in find_changed_links(&baseline[i], &changed[i]) {
                                let dep = (member, source, side);
                                if deps.contains(&dep) {
                                    discovered.insert((dep, candidate));
                                } else {
                                    // A jointly opened path can expose a source absent from every single-token probe.
                                    discovered.insert((dep, candidate));
                                    if tokens.iter().any(|&token| !depends.get(&dep).is_some_and(|known| known.contains(&token))
                                        && !discovered.contains(&(dep, token))) {
                                        new_links.push((i, dep));
                                    }
                                }
                            }
                        }
                        if !new_links.is_empty() {
                            for (own, &initial_digit) in initial.iter().enumerate() {
                                if joint[own].1 == initial_digit { continue; }
                                if new_links.iter().all(|&(_, dep)| depends.get(&dep).is_some_and(|known| known.contains(&tokens[own]))
                                    || discovered.contains(&(dep, tokens[own]))) { continue; }
                                let mut restored = joint.clone();
                                restored[own].1 = initial_digit;
                                let without = sample(scratch, program, &restored, &members, monitor, indexes, &mut metrics)?;
                                for &(i, dep) in &new_links {
                                    if !depends.get(&dep).is_some_and(|known| known.contains(&tokens[own]))
                                        && !discovered.contains(&(dep, tokens[own]))
                                        && without[i].get(&(dep.1, dep.2)) != changed[i].get(&(dep.1, dep.2)) {
                                        discovered.insert((dep, tokens[own]));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Ok((discovered, metrics))
        })?;
        first = false;
        grown.clear();
        for (discovered, work) in batches {
            metrics.add(work);
            for (dep, token) in discovered {
                if depends.entry(dep).or_default().insert(token) {
                    grown.insert(dep);
                }
            }
        }
        if grown.is_empty() {
            break;
        }
    }
    let mut grouped: BTreeMap<(Vec<usize>, usize, bool), Vec<ArrangementSource>> = BTreeMap::new();
    for ((member, source, side), tokens) in depends {
        grouped.entry((tokens.into_iter().collect(), member, side)).or_default().push(source);
    }
    let grouped: Vec<_> = grouped.into_iter().collect();
    let batches = scratch.probe_jobs(&grouped, monitor, |scratch, ((tokens, target, side), sources)| {
        let (target, side) = (*target, *side);
        let mut metrics = ProbeMetrics::default();
        if monitor.is_some_and(TaskMonitor::cancelled) {
            return Err(ScratchError {
                position: None,
                reason: "compilation cancelled",
            });
        }
        let axes = electrical_states(program, indexes, tokens, target);
        let size = count(program, tokens, &axes, program.positions[target])?;
        let mut inputs = Vec::with_capacity(size);
        for state in 0..size {
            let map = sample(
                scratch,
                program,
                &placements(tokens, &axes, state),
                &[target],
                monitor,
                indexes,
                &mut metrics,
            )?
            .pop()
            .unwrap();
            inputs.push(
                map.into_iter()
                    .filter_map(|((source, port), (distance, _))| (port == side && sources.contains(&source)).then_some((source, distance)))
                    .collect(),
            );
        }
        Ok((
            InputArrangement {
                target,
                side,
                silent: matches!(program.initial[target], Block::Piston { .. }),
                tokens: tokens.clone(),
                token_states: axes,
                conditional_sources: sources.clone(),
                inputs,
            },
            metrics,
        ))
    })?;
    let mut result = Vec::with_capacity(batches.len());
    for (group, work) in batches {
        metrics.add(work);
        result.push(group);
    }
    if std::env::var_os("MCHPRS_REDPILER_PHASE_TIMINGS").is_some() {
        eprintln!("Redpiler arrangement joint probes: {} ms", joint_started.elapsed().as_millis());
        eprintln!(
            "Redpiler arrangement work: tokens={} affected_members={} samples={} member_reads={} input_groups={}",
            program.tokens.len(),
            metrics.affected_members,
            metrics.samples,
            metrics.member_reads,
            result.len()
        );
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "isolated ANPU geometry sampling; measures one target, not a full compile"]
    fn isolated_anpu_joint_probe_cost() {
        use super::super::cpu_trace::cpus;
        let cpu = cpus::CPUS[1];
        let world = cpus::load_cpu(cpu);
        let clip = crate::plot::worldedit::load_schematic(std::fs::File::open(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data").join(cpu.schematic)).unwrap()).unwrap();
        let bounds = (cpu.origin, cpu.origin + BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1));
        let mut positions = Vec::new();
        crate::world::for_each_block_optimized(&world, bounds.0, bounds.1, |pos| {
            let block = world.get_block(pos);
            if reader_kind(block).is_some() || block.pressure_plate_powered().is_some()
                || matches!(block, Block::RedstoneBlock | Block::Lever { .. } | Block::StoneButton { .. } | Block::Observer { .. }) {
                positions.push(pos);
            }
        });
        let components: HashMap<_, _> = positions.iter().enumerate().map(|(node, &pos)| (pos, node)).collect();
        let target = components[&BlockPos::new(84, 37, 72)];
        let initial: Vec<_> = positions.iter().map(|&pos| world.get_block(pos)).collect();
        let repeaters = initial.iter().enumerate().filter_map(|(component, block)| matches!(block, Block::RedstoneRepeater { .. })
            .then_some(super::super::program::RepeaterRecord { component, output_is_diode: false, side_sources: None })).collect();
        let mut program = Program::new(positions, initial, vec![], vec![[vec![], vec![]]; components.len()], vec![], vec![], repeaters).unwrap();
        for z in (61..=75).step_by(2) {
            for x in [84, 86] {
                let lower = BlockPos::new(x, 38, z);
                let block = world.get_block(lower);
                assert!(matches!(block, Block::Concrete { .. }));
                program.tokens.push(super::super::compiler::PistonToken {
                    block, position: Some(lower), cells: vec![BlockPos::new(x, 39, z), lower],
                });
            }
        }
        let indexes = ProbeIndexes::new(&components, &program.positions, &program.initial, &program.tokens, None).unwrap();
        let entry = ScratchEntry { scheduled: vec![], piston: world.piston_state().clone() };
        let mut scratch = ScratchWorld::from_world_monitored(&world, bounds, entry, None).unwrap();
        let max_axes = std::env::var("MCHPRS_PROBE_AXES").ok().map_or(10, |s| s.parse::<usize>().unwrap());
        assert!((2..=14).contains(&max_axes));
        let mut rows = Vec::new();
        for axes_count in (2..=max_axes).step_by(2) {
            let tokens: Vec<_> = (0..axes_count).collect();
            let axes = electrical_states(&program, &indexes, &tokens, target);
            let states = count(&program, &tokens, &axes, program.positions[target]).unwrap();
            let mut metrics = ProbeMetrics::default();
            let mut changed = 0;
            let started = std::time::Instant::now();
            for state in 0..states {
                let mut base = placements(&tokens, &axes, state);
                let before = sample(&mut scratch, &program, &base, &[target], None, &indexes, &mut metrics).unwrap();
                base.push((14, 0));
                let after = sample(&mut scratch, &program, &base, &[target], None, &indexes, &mut metrics).unwrap();
                changed += usize::from(before != after);
            }
            assert_eq!(metrics.samples, states * 2);
            let row = serde_json::json!({"axes":axes_count,"states":states,"baseline_samples":states,"candidate_samples":states,
                "restoration_samples":0,"changed_inputs":changed,"wall_ms":started.elapsed().as_secs_f64()*1000.0,
                "revert_ms":metrics.sample_times[0].as_secs_f64()*1000.0,"placement_ms":metrics.sample_times[1].as_secs_f64()*1000.0,
                "input_discovery_ms":metrics.sample_times[2].as_secs_f64()*1000.0,"boundary_check_ms":metrics.sample_times[3].as_secs_f64()*1000.0});
            eprintln!("isolated ANPU sweep: {row}");
            rows.push(row);
        }
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/anpu-isolated-probe-cost.json");
        std::fs::write(path, serde_json::to_vec_pretty(&rows).unwrap()).unwrap();
    }

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

    #[test]
    fn indexed_affected_regions_match_full_scan() {
        let mut positions = Vec::new();
        let mut wire_at = HashMap::new();
        for x in 20..=42 {
            let pos = BlockPos::new(x, 10 + (x % 3 == 0) as i32, 30);
            wire_at.insert(pos, positions.len());
            positions.push(pos);
        }
        let reader_positions = [
            BlockPos::new(20, 12, 32),
            BlockPos::new(28, 12, 32),
            BlockPos::new(42, 10, 30),
            BlockPos::new(50, 10, 30),
        ];
        let readers: HashMap<_, _> = reader_positions
            .into_iter()
            .map(|pos| {
                let index = positions.len();
                positions.push(pos);
                (pos, index)
            })
            .collect();
        let wire_neighbors = positions
            .iter()
            .enumerate()
            .map(|(wire, &pos)| {
                if wire >= wire_at.len() {
                    return Vec::new();
                }
                let mut neighbors = Vec::new();
                for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    for dy in -1..=1 {
                        if let Some(&next) = wire_at.get(&BlockPos::new(pos.x + dx, pos.y + dy, pos.z + dz)) {
                            neighbors.push(next);
                        }
                    }
                }
                neighbors.sort_unstable();
                neighbors
            })
            .collect();
        let components: HashMap<_, _> = positions.iter().enumerate().map(|(i, &pos)| (pos, i)).collect();
        let indexes = ProbeIndexes {
            components: &components,
            redstone_tokens: HashMap::new(),
            wire_at: wire_at.clone(),
            wire_neighbors,
            readers: readers.clone(),
        };
        let mut wire_list: Vec<_> = wire_at.values().copied().collect();
        wire_list.sort_unstable();
        let token_cells = [BlockPos::new(19, 10, 30), BlockPos::new(30, 11, 31)];
        let mut expected = BTreeSet::new();
        let mut touched = HashSet::new();
        let mut queue = VecDeque::new();
        for &wire in &wire_list {
            if token_cells.iter().any(|&cell| near(cell, positions[wire], 1)) {
                touched.insert(wire);
                queue.push_back((wire, 0));
            }
        }
        while let Some((wire, depth)) = queue.pop_front() {
            if depth == 15 {
                continue;
            }
            let pos = positions[wire];
            for &next in &wire_list {
                let at = positions[next];
                if pos.x.abs_diff(at.x) + pos.z.abs_diff(at.z) == 1 && pos.y.abs_diff(at.y) <= 1 && touched.insert(next) {
                    queue.push_back((next, depth + 1));
                }
            }
        }
        expected.extend(touched.iter().copied());
        for (&pos, &reader) in &readers {
            if token_cells.iter().any(|&cell| near(cell, pos, 2)) || touched.iter().any(|&wire| near(positions[wire], pos, 2)) {
                expected.insert(reader);
            }
        }

        assert_eq!(affected_members(&token_cells, &positions, &indexes, None).unwrap(), expected);
    }
}
