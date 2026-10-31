use super::compiler::find_changed_links;
use super::program::{ArrangementSource, InputArrangement};
use super::{Program, ScratchEntry, ScratchError, ScratchWorld};
use crate::world::World;
use crate::redpiler::TaskMonitor;
use mchprs_blocks::{BlockFace, BlockPos, blocks::Block};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

type InputKey = (ArrangementSource, bool);
type InputValue = (u8, bool);
type Inputs = BTreeMap<InputKey, InputValue>;
type Dependency = (usize, ArrangementSource, bool);

pub(super) struct ProbeIndexes<'a> {
    pub(super) components: &'a HashMap<BlockPos, usize>,
    redstone_tokens: HashMap<BlockPos, usize>,
    wire_at: HashMap<BlockPos, usize>,
    wire_neighbors: Vec<Vec<usize>>,
    readers: HashMap<BlockPos, usize>,
}

impl ProbeIndexes<'_> {
    pub(super) fn new<'a>(
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
            if matches!(
                block,
                Block::Piston { .. }
                    | Block::RedstoneLamp { .. }
                    | Block::RedstoneRepeater { .. }
                    | Block::RedstoneComparator { .. }
                    | Block::RedstoneTorch { .. }
                    | Block::RedstoneWallTorch { .. }
            ) {
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
    samples: usize,
    member_reads: usize,
    affected_members: usize,
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
            queue.extend(indexes.wire_neighbors[wire].iter().copied().filter_map(|neighbor| touched.insert(neighbor).then_some((neighbor, depth + 1))));
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

fn wire_sources(world: &impl World, wires: &[BlockPos], monitor: Option<&TaskMonitor>) -> Result<Vec<super::super::scratch::WireSource>, ScratchError> {
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
    match block {
        Block::RedstoneWire { .. } => found = wire_sources(world, &[pos], monitor)?,
        Block::Piston { piston } => found = crate::redpiler::scratch::discover_piston_sources(world, pos, piston.facing)?,
        Block::RedstoneLamp { .. } => found = crate::redpiler::scratch::discover_lamp_sources(world, pos)?,
        Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. } => found = crate::redpiler::scratch::discover_torch_sources(world, pos, block)?,
        Block::RedstoneRepeater { .. } | Block::RedstoneComparator { .. } => {
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
            } else if block.is_solid() || block.is_copper_bulb() {
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
        _ => return Ok(Inputs::new()),
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
    scratch.revert();
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
    scratch.finish_probe()?;
    Ok(result)
}

fn count(program: &Program, tokens: &[usize], pos: BlockPos) -> Result<usize, ScratchError> {
    let n = tokens
        .iter()
        .try_fold(1usize, |n, &t| n.checked_mul(program.tokens[t].cells.len() + 1))
        .unwrap_or(usize::MAX);
    if n > 4096 {
        Err(ScratchError {
            position: Some(pos),
            reason: "too many piston states near dependency",
        })
    } else {
        Ok(n)
    }
}

fn placements(program: &Program, tokens: &[usize], mut arrangement: usize) -> Vec<(usize, usize)> {
    tokens
        .iter()
        .map(|&token| {
            let radix = program.tokens[token].cells.len() + 1;
            let digit = arrangement % radix;
            arrangement /= radix;
            (token, digit)
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
    let mut affected_started = std::time::Instant::now();
    let mut affects = Vec::with_capacity(program.tokens.len());
    for token in &program.tokens {
        affects.push(affected_members(&token.cells, &program.positions, indexes, monitor)?);
    }
    if std::env::var_os("MCHPRS_REDPILER_PHASE_TIMINGS").is_some() {
        eprintln!("Redpiler arrangement affected-region construction: {} ms", affected_started.elapsed().as_millis());
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
    let mut single_started = std::time::Instant::now();
    let mut depends: BTreeMap<Dependency, BTreeSet<usize>> = BTreeMap::new();
    let mut grown = BTreeSet::new();
    for token in 0..program.tokens.len() {
        if monitor.is_some_and(TaskMonitor::cancelled) {
            return Err(ScratchError {
                position: None,
                reason: "compilation cancelled",
            });
        }
        let members: Vec<_> = affects[token].iter().copied().collect();
        if members.is_empty() {
            continue;
        }
        let baseline = sample(&mut scratch, program, &[], &members, monitor, indexes, &mut metrics)?;
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
            let changed = sample(&mut scratch, program, &[(token, digit)], &members, monitor, indexes, &mut metrics)?;
            for (i, &member) in members.iter().enumerate() {
                for (source, side) in find_changed_links(&baseline[i], &changed[i]) {
                    let dep = (member, source, side);
                    if depends.entry(dep).or_default().insert(token) {
                        grown.insert(dep);
                    }
                }
            }
        }
    }
    if std::env::var_os("MCHPRS_REDPILER_PHASE_TIMINGS").is_some() {
        eprintln!("Redpiler arrangement individual-token probes: {} ms", single_started.elapsed().as_millis());
    }
    let mut joint_started = std::time::Instant::now();
    let mut first = true;
    loop {
        let mut grouped: BTreeMap<Vec<usize>, Vec<Dependency>> = BTreeMap::new();
        for (&dep, tokens) in &depends {
            grouped.entry(tokens.iter().copied().collect()).or_default().push(dep);
        }
        let mut discovered = Vec::new();
        for (tokens, deps) in grouped {
            if monitor.is_some_and(TaskMonitor::cancelled) {
                return Err(ScratchError {
                    position: None,
                    reason: "compilation cancelled",
                });
            }
            if !first && !deps.iter().any(|dep| grown.contains(dep)) {
                continue;
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
                continue;
            }
            let size = count(program, &tokens, program.positions[members[0]])?;
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
                let base = placements(program, &tokens, state);
                if base.iter().map(|pair| pair.1).eq(initial.iter().copied()) {
                    continue;
                }
                let baseline = sample(&mut scratch, program, &base, &members, monitor, indexes, &mut metrics)?;
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
                    for digit in 0..=program.tokens[candidate].cells.len() {
                        if monitor.is_some_and(TaskMonitor::cancelled) {
                            return Err(ScratchError {
                                position: None,
                                reason: "compilation cancelled",
                            });
                        }
                        if digit == initial_candidate {
                            continue;
                        }
                        let mut joint = base.clone();
                        joint.push((candidate, digit));
                        let changed = sample(&mut scratch, program, &joint, &members, monitor, indexes, &mut metrics)?;
                        for (i, &member) in members.iter().enumerate() {
                            if !affects[candidate].contains(&member) {
                                continue;
                            }
                            for (source, side) in find_changed_links(&baseline[i], &changed[i]) {
                                let dep = (member, source, side);
                                if deps.contains(&dep) {
                                    discovered.push((dep, candidate));
                                }
                            }
                        }
                    }
                }
            }
        }
        first = false;
        grown.clear();
        for (dep, token) in discovered {
            if depends.entry(dep).or_default().insert(token) {
                grown.insert(dep);
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
    let mut result = Vec::new();
    for ((tokens, target, side), sources) in grouped {
        if monitor.is_some_and(TaskMonitor::cancelled) {
            return Err(ScratchError {
                position: None,
                reason: "compilation cancelled",
            });
        }
        let size = count(program, &tokens, program.positions[target])?;
        let mut inputs = Vec::with_capacity(size);
        for state in 0..size {
            let map = sample(
                &mut scratch,
                program,
                &placements(program, &tokens, state),
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
        result.push(InputArrangement {
            target,
            side,
            silent: matches!(program.initial[target], Block::Piston { .. }),
            tokens,
            conditional_sources: sources,
            inputs,
        });
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
