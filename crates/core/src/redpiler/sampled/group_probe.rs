use super::compiler::find_changed_links;
use super::program::{ArrangementSource, InputArrangement};
use super::{Program, ScratchEntry, ScratchError, ScratchWorld};
use crate::world::World;
use crate::redpiler::TaskMonitor;
use mchprs_blocks::{BlockFace, BlockPos, blocks::Block};
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

type InputKey = (ArrangementSource, bool);
type InputValue = (u8, bool);
type Inputs = BTreeMap<InputKey, InputValue>;
type Dependency = (usize, ArrangementSource, bool);

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
        let identity = if let Some(index) = program
            .tokens
            .iter()
            .rposition(|token| matches!(token.block, Block::RedstoneBlock) && token.cells.contains(&source.source))
        {
            ArrangementSource::Token(index)
        } else if let Some(index) = program.positions.iter().position(|&p| p == source.source) {
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
    world: &impl World,
    scratch: &mut ScratchWorld,
    program: &Program,
    placements: &[(usize, usize)],
    members: &[usize],
    monitor: Option<&TaskMonitor>,
) -> Result<Vec<Inputs>, ScratchError> {
    if monitor.is_some_and(TaskMonitor::cancelled) {
        return Err(ScratchError {
            position: None,
            reason: "compilation cancelled",
        });
    }
    for token in &program.tokens {
        for &cell in &token.cells {
            scratch.world_mut().set_block(cell, world.get_block(cell));
        }
    }
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
    for &member in members {
        if monitor.is_some_and(TaskMonitor::cancelled) {
            return Err(ScratchError {
                position: None,
                reason: "compilation cancelled",
            });
        }
        result.push(inputs_of(scratch.world(), program, member, monitor)?);
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
    monitor: Option<&TaskMonitor>,
) -> Result<Vec<InputArrangement>, ScratchError> {
    let mut scratch = ScratchWorld::from_world(world, bounds, entry.clone())?;
    let wires: Vec<_> = program
        .initial
        .iter()
        .enumerate()
        .filter_map(|(i, b)| matches!(b, Block::RedstoneWire { .. }).then_some(i))
        .collect();
    let readers: Vec<_> = program
        .initial
        .iter()
        .enumerate()
        .filter_map(|(i, b)| {
            matches!(
                b,
                Block::Piston { .. }
                    | Block::RedstoneLamp { .. }
                    | Block::RedstoneRepeater { .. }
                    | Block::RedstoneComparator { .. }
                    | Block::RedstoneTorch { .. }
                    | Block::RedstoneWallTorch { .. }
            )
            .then_some(i)
        })
        .collect();
    let mut affects = Vec::new();
    for token in &program.tokens {
        let mut touched = HashSet::new();
        let mut queue = VecDeque::new();
        for &wire in &wires {
            if token.cells.iter().any(|&cell| near(cell, program.positions[wire], 1)) {
                touched.insert(wire);
                queue.push_back((wire, 0));
            }
        }
        while let Some((wire, depth)) = queue.pop_front() {
            if depth == 15 {
                continue;
            }
            let pos = program.positions[wire];
            for &next in &wires {
                let at = program.positions[next];
                if pos.x.abs_diff(at.x) + pos.z.abs_diff(at.z) == 1 && pos.y.abs_diff(at.y) <= 1 && touched.insert(next) {
                    queue.push_back((next, depth + 1));
                }
            }
        }
        let mut members: BTreeSet<_> = touched.iter().copied().collect();
        for &reader in &readers {
            if token.cells.iter().any(|&cell| near(cell, program.positions[reader], 2))
                || touched.iter().any(|&wire| near(program.positions[wire], program.positions[reader], 2))
            {
                members.insert(reader);
            }
        }
        affects.push(members);
    }
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
        let baseline = sample(world, &mut scratch, program, &[], &members, monitor)?;
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
            let changed = sample(world, &mut scratch, program, &[(token, digit)], &members, monitor)?;
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
            let candidates: Vec<_> = (0..program.tokens.len())
                .filter(|candidate| {
                    !tokens.contains(candidate)
                        && members.iter().any(|member| affects[*candidate].contains(member))
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
                let baseline = sample(world, &mut scratch, program, &base, &members, monitor)?;
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
                        let changed = sample(world, &mut scratch, program, &joint, &members, monitor)?;
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
            let map = sample(world, &mut scratch, program, &placements(program, &tokens, state), &[target], monitor)?
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
    Ok(result)
}
