//! Discover electrical sources and the wire callbacks that observe them.
use super::ScratchError;
use crate::redpilator::TaskMonitor;
use crate::world::World;
use mchprs_blocks::{BlockFace, BlockPos, blocks::Block};
use std::collections::{HashMap, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WireWatcher {
    /// location of the wire being watched.
    pub(crate) wire: BlockPos,
    /// location of the piston whose motion can affect the wire.
    pub(crate) piston: BlockPos,
    pub(crate) head: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WireSource {
    /// location of the target wire receiving this dependency.
    pub(crate) wire: BlockPos,
    /// location of the redstone source supplying power.
    pub(crate) source: BlockPos,
    /// signal strength reduction between source and wire, in levels 0–15.
    pub(crate) attenuation: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WirePorts {
    pub(crate) wires: Vec<BlockPos>,
    pub(crate) sources: Vec<BlockPos>,
}

/// One dust vertex, without flattening paths through other dust vertices.
///
/// Inputs: The world and wire position.
///
/// Follow native horizontal, climbing and descending dust rules, collect strong/weak power contacts, then
/// sort and deduplicate the resulting ports.
pub(crate) fn discover_wire_ports(world: &impl World, pos: BlockPos) -> Result<WirePorts, ScratchError> {
    let mut ports = WirePorts {
        wires: Vec::new(),
        sources: Vec::new(),
    };
    for face in BlockFace::values() {
        let at = pos.offset(face);
        let block = world.get_block(at);
        if matches!(block, Block::RedstoneWire { .. }) {
            if !matches!(face, BlockFace::Top | BlockFace::Bottom) {
                ports.wires.push(at);
            }
        } else if is_supported_dynamic_source(block) || block == Block::RedstoneBlock {
            if source_emits_to(block, world, at, face) {
                ports.sources.push(at);
            }
        } else if block.is_solid() {
            for side in BlockFace::values() {
                let source = at.offset(side);
                let state = world.get_block(source);
                if !crate::redstone::power::emits_strong_power(state, world, source, side, false) {
                    continue;
                }
                if is_supported_dynamic_source(state) || state == Block::RedstoneBlock {
                    ports.sources.push(source);
                } else if !matches!(state, Block::RedstoneWire { .. }) {
                    return Err(ScratchError {
                        position: Some(source),
                        reason: "unsupported watched-wire source dependency",
                    });
                }
            }
        } else if source_emits_to(block, world, at, face) {
            return Err(ScratchError {
                position: Some(at),
                reason: "unsupported watched-wire source dependency",
            });
        }
    }
    let above_solid = world.get_block(pos.offset(BlockFace::Top)).is_solid();
    for face in [BlockFace::North, BlockFace::South, BlockFace::East, BlockFace::West] {
        let adjacent = pos.offset(face);
        let block = world.get_block(adjacent);
        let diagonal = if !block.is_solid() {
            Some(adjacent.offset(BlockFace::Bottom))
        } else if !above_solid && !block.is_transparent() {
            Some(adjacent.offset(BlockFace::Top))
        } else {
            None
        };
        if let Some(at) = diagonal
            && matches!(world.get_block(at), Block::RedstoneWire { .. })
        {
            ports.wires.push(at);
        }
    }
    ports.wires.sort_unstable_by_key(|p| (p.x, p.y, p.z));
    ports.wires.dedup();
    ports.sources.sort_unstable_by_key(|p| (p.x, p.y, p.z));
    ports.sources.dedup();
    Ok(ports)
}

#[derive(Debug, PartialEq, Eq)]
#[cfg(test)]
pub(crate) struct WireDiscovery {
    /// candidate wire/piston pairs that need movement notifications.
    pub(crate) watchers: Vec<WireWatcher>,
    /// source-to-wire electrical dependencies discovered in the region.
    pub(crate) sources: Vec<WireSource>,
}
#[cfg(test)]
pub(crate) fn discovered_wire_strength(world: &impl World, discovery: &WireDiscovery, wire: BlockPos) -> u8 {
    // Multiple connected sources contribute by their strongest attenuated level.
    discovery
        .sources
        .iter()
        .filter(|dependency| dependency.wire == wire)
        .map(|dependency| {
            crate::redstone::source_strength(world.get_block(dependency.source), world, dependency.source).saturating_sub(dependency.attenuation)
        })
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
pub(crate) fn wire_strength_changed(before: u8, after: u8) -> bool {
    before != after
}

#[cfg(test)]
pub(crate) fn changed_wire_watchers(discovery: &WireDiscovery, before: &[(BlockPos, u8)], after: &[(BlockPos, u8)]) -> Vec<WireWatcher> {
    let after: HashMap<_, _> = after.iter().copied().collect();
    discovery
        .watchers
        .iter()
        .copied()
        .filter(|watch| {
            before
                .iter()
                .find(|(wire, _)| *wire == watch.wire)
                .is_some_and(|(_, power)| after.get(&watch.wire).is_some_and(|next| *power != *next))
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn max_attenuated_strength(contributions: impl IntoIterator<Item = (u8, u8)>) -> u8 {
    contributions
        .into_iter()
        .map(|(strength, attenuation)| strength.saturating_sub(attenuation))
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
pub(crate) fn discover_watched_wire_sources(world: &impl World, bounds: (BlockPos, BlockPos)) -> Result<WireDiscovery, ScratchError> {
    let (min, max) = bounds;
    let mut wires = Vec::new();
    let mut pistons = Vec::new();
    for y in min.y..=max.y {
        for z in min.z..=max.z {
            for x in min.x..=max.x {
                let pos = BlockPos::new(x, y, z);
                match world.get_block(pos) {
                    Block::RedstoneWire { .. } => wires.push(pos),
                    Block::Piston { .. } => pistons.push(pos),
                    _ => {}
                }
            }
        }
    }
    let watchers = discover_wire_watchers(world, &wires, &pistons);

    let sources = discover_wire_sources(world, &wires)?;
    Ok(WireDiscovery { watchers, sources })
}

/// Find wire changes that must notify piston bases or heads.
///
/// Inputs: The world, captured wire positions and piston base positions.
///
/// Index each base/head position, visit each wire's nearby notification footprint, and emit sorted
/// deduplicated wire-to-piston watcher records.
pub(crate) fn discover_wire_watchers(world: &impl World, wires: &[BlockPos], pistons: &[BlockPos]) -> Vec<WireWatcher> {
    let mut piston_index: HashMap<BlockPos, Vec<(usize, bool)>> = HashMap::new();
    for (index, &pos) in pistons.iter().enumerate() {
        piston_index.entry(pos).or_default().push((index, false));
        if let Block::Piston { piston } = world.get_block(pos) {
            piston_index.entry(pos.offset(piston.facing.into())).or_default().push((index, true));
        }
    }
    let mut watchers = Vec::new();
    for &wire in wires {
        let mut nearby = Vec::new();
        for dx in -2i32..=2 {
            for dy in -2i32..=2 {
                for dz in -2i32..=2 {
                    if dx.abs() + dy.abs() + dz.abs() <= 2
                        && let Some(indices) = piston_index.get(&BlockPos::new(wire.x + dx, wire.y + dy, wire.z + dz))
                    {
                        nearby.extend(indices.iter().copied());
                    }
                }
            }
        }
        nearby.sort_unstable();
        nearby.dedup();
        watchers.extend(nearby.into_iter().map(|(index, head)| WireWatcher {
            wire,
            piston: pistons[index],
            head,
        }));
    }
    watchers
}

/// Electrical extraction alone; watcher construction and executable admission are separate.
pub(crate) fn discover_wire_sources(world: &impl World, wires: &[BlockPos]) -> Result<Vec<WireSource>, ScratchError> {
    discover_wire_sources_inner(world, wires, false, None)
}

pub(crate) fn discover_wire_sources_monitored(
    world: &impl World,
    wires: &[BlockPos],
    monitor: &TaskMonitor,
) -> Result<Vec<WireSource>, ScratchError> {
    discover_wire_sources_inner(world, wires, false, Some(monitor))
}

#[cfg(test)]
pub(crate) fn trace_wire_sources(world: &impl World, wires: &[BlockPos]) -> Result<Vec<WireSource>, ScratchError> {
    discover_wire_sources_inner(world, wires, true, None)
}

/// Discover upstream sources and attenuation for each requested wire.
///
/// Inputs: The world, wire positions, trace flag and optional cancellation monitor.
///
/// Run a breadth-first search from every target wire, merge sources by their shortest distance, stop
/// beyond usable signal range, and report progress.
fn discover_wire_sources_inner(
    world: &impl World,
    wires: &[BlockPos],
    trace: bool,
    monitor: Option<&TaskMonitor>,
) -> Result<Vec<WireSource>, ScratchError> {
    // Dust attenuation follows the shortest represented wire path; dynamic source
    // states are included even when currently off (e.g. an unpowered lever).
    let mut sources = HashMap::new();
    if let Some(monitor) = monitor {
        monitor.set_progress(0, wires.len(), "wires checked");
    }
    for (index, &wire) in wires.iter().enumerate() {
        if monitor.is_some_and(TaskMonitor::cancelled) {
            return Err(ScratchError {
                position: None,
                reason: "compilation cancelled",
            });
        }
        let mut distances = HashMap::from([(wire, 0u8)]);
        let mut queue = VecDeque::from([wire]);
        while let Some(current) = queue.pop_front() {
            if monitor.is_some_and(TaskMonitor::cancelled) {
                return Err(ScratchError {
                    position: None,
                    reason: "compilation cancelled",
                });
            }
            let distance = distances[&current];
            if trace {
                eprintln!("wire traversal target={wire:?} current={current:?} distance={distance}");
            }
            let ports = discover_wire_ports(world, current)?;
            for source in ports.sources {
                sources
                    .entry((wire, source))
                    .and_modify(|old: &mut u8| *old = (*old).min(distance))
                    .or_insert(distance);
            }
            if distance < 14 {
                for neighbor in ports.wires {
                    let next = distance + 1;
                    if distances.get(&neighbor).is_none_or(|old| next < *old) {
                        distances.insert(neighbor, next);
                        queue.push_back(neighbor);
                    }
                }
            }
        }
        if let Some(monitor) = monitor {
            monitor.set_progress(index + 1, wires.len(), "wires checked");
        }
    }
    let mut sources: Vec<_> = sources
        .into_iter()
        .map(|((wire, source), attenuation)| WireSource { wire, source, attenuation })
        .collect();
    sources.sort_by_key(|source| {
        (
            source.wire.x,
            source.wire.y,
            source.wire.z,
            source.source.x,
            source.source.y,
            source.source.z,
        )
    });
    Ok(sources)
}

/// Collect a lamp's weak and one-hop conducted inputs, expanding each dust
/// contact through the shared wire graph.
pub(crate) fn discover_lamp_sources(world: &impl World, lamp: BlockPos) -> Result<Vec<WireSource>, ScratchError> {
    discover_signal_contacts(world, lamp, BlockFace::values(), true)
}

pub(crate) fn discover_lamp_ports(world: &impl World, lamp: BlockPos) -> Result<Vec<WireSource>, ScratchError> {
    discover_signal_contacts(world, lamp, BlockFace::values(), false)
}

pub(crate) fn discover_torch_sources(world: &impl World, pos: BlockPos, block: Block) -> Result<Vec<WireSource>, ScratchError> {
    discover_torch_contacts(world, pos, block, true)
}

pub(crate) fn discover_torch_ports(world: &impl World, pos: BlockPos, block: Block) -> Result<Vec<WireSource>, ScratchError> {
    discover_torch_contacts(world, pos, block, false)
}

fn discover_torch_contacts(world: &impl World, pos: BlockPos, block: Block, expand: bool) -> Result<Vec<WireSource>, ScratchError> {
    let face = match block {
        Block::RedstoneTorch { .. } => BlockFace::Bottom,
        Block::RedstoneWallTorch { facing, .. } => facing.opposite().block_face(),
        _ => {
            return Err(ScratchError {
                position: Some(pos),
                reason: "torch input requested for non-torch",
            });
        }
    };
    discover_signal_contacts(world, pos, [face], expand)
}

fn wire_contact(world: &impl World, pos: BlockPos, expand: bool) -> Result<Vec<WireSource>, ScratchError> {
    if expand {
        discover_wire_sources(world, &[pos])
    } else {
        Ok(vec![WireSource {
            wire: pos,
            source: pos,
            attenuation: 0,
        }])
    }
}

/// Discover the electrical contacts powering a signal-reading block.
///
/// Inputs: The world and the receiver-specific positions/options supplied by its caller.
///
/// Inspect adjacent power sources and solid-block conduction using native power rules, optionally expand
/// wire contacts, and merge repeated sources at minimum attenuation.
fn discover_signal_contacts(
    world: &impl World,
    lamp: BlockPos,
    faces: impl IntoIterator<Item = BlockFace>,
    expand: bool,
) -> Result<Vec<WireSource>, ScratchError> {
    let mut sources = HashMap::new();
    let mut add = |source: BlockPos, attenuation: u8| {
        sources
            .entry(source)
            .and_modify(|old: &mut u8| *old = (*old).min(attenuation))
            .or_insert(attenuation);
    };
    let supported_source =
        |block: Block| is_supported_dynamic_source(block) || block == Block::RedstoneBlock || block.pressure_plate_powered().is_some();
    for face in faces {
        let neighbor = lamp.offset(face);
        let block = world.get_block(neighbor);
        if matches!(block, Block::RedstoneWire { .. }) {
            if crate::redstone::power::emits_weak_power(block, world, neighbor, face, true) {
                for source in wire_contact(world, neighbor, expand)? {
                    add(source.source, source.attenuation);
                }
            }
        } else if block.is_solid() {
            for source_face in BlockFace::values() {
                let source_pos = neighbor.offset(source_face);
                let source_block = world.get_block(source_pos);
                if !crate::redstone::power::emits_strong_power(source_block, world, source_pos, source_face, true) {
                    continue;
                }
                if matches!(source_block, Block::RedstoneWire { .. }) {
                    for source in wire_contact(world, source_pos, expand)? {
                        add(source.source, source.attenuation);
                    }
                } else if supported_source(source_block) {
                    add(source_pos, 0);
                } else {
                    return Err(ScratchError {
                        position: Some(source_pos),
                        reason: "unsupported lamp input source",
                    });
                }
            }
        } else if crate::redstone::power::emits_weak_power(block, world, neighbor, face, true) {
            if supported_source(block) {
                add(neighbor, 0);
            } else {
                return Err(ScratchError {
                    position: Some(neighbor),
                    reason: "unsupported lamp input source",
                });
            }
        }
    }
    let mut sources: Vec<_> = sources
        .into_iter()
        .map(|(source, attenuation)| WireSource {
            wire: lamp,
            source,
            attenuation,
        })
        .collect();
    sources.sort_by_key(|source| (source.source.x, source.source.y, source.source.z));
    Ok(sources)
}

/// Discover the direct and quasi-connected inputs sampled by a piston.
pub(crate) fn discover_piston_sources(
    world: &impl World,
    pos: BlockPos,
    facing: mchprs_blocks::BlockFacing,
) -> Result<Vec<WireSource>, ScratchError> {
    discover_piston_contacts(world, pos, facing, true)
}

pub(crate) fn discover_piston_ports(world: &impl World, pos: BlockPos, facing: mchprs_blocks::BlockFacing) -> Result<Vec<WireSource>, ScratchError> {
    discover_piston_contacts(world, pos, facing, false)
}

/// Discover direct and quasi-connectivity power contacts for a piston.
///
/// Inputs: The world, piston base, facing direction and source-expansion mode.
///
/// Inspect the base and upper power neighborhoods while excluding the facing contact as required, resolve
/// wire/conductor sources, and merge their attenuation.
fn discover_piston_contacts(
    world: &impl World,
    pos: BlockPos,
    facing: mchprs_blocks::BlockFacing,
    expand: bool,
) -> Result<Vec<WireSource>, ScratchError> {
    let mut contacts: Vec<_> = BlockFace::values()
        .into_iter()
        .filter(|&face| face != facing.into())
        .map(|face| (pos, face))
        .collect();
    let above = pos.offset(BlockFace::Top);
    contacts.extend(BlockFace::values().into_iter().map(|face| (above, face)));
    let mut sources = HashMap::new();
    for (origin, face) in contacts {
        let at = origin.offset(face);
        let block = world.get_block(at);
        let mut add = |source: BlockPos, attenuation: u8| {
            sources
                .entry(source)
                .and_modify(|old: &mut u8| *old = (*old).min(attenuation))
                .or_insert(attenuation);
        };
        if matches!(block, Block::RedstoneWire { .. }) {
            if crate::redstone::power::emits_weak_power(block, world, at, face, true) {
                for source in wire_contact(world, at, expand)? {
                    add(source.source, source.attenuation);
                }
            }
        } else if block.is_solid() {
            for side in BlockFace::values() {
                let source_pos = at.offset(side);
                let source = world.get_block(source_pos);
                if crate::redstone::power::emits_strong_power(source, world, source_pos, side, true) {
                    if matches!(source, Block::RedstoneWire { .. }) {
                        for source in wire_contact(world, source_pos, expand)? {
                            add(source.source, source.attenuation);
                        }
                    } else if is_supported_dynamic_source(source) || source == Block::RedstoneBlock {
                        add(source_pos, 0);
                    } else {
                        return Err(ScratchError {
                            position: Some(source_pos),
                            reason: "unsupported piston input source",
                        });
                    }
                }
            }
        } else if crate::redstone::power::emits_weak_power(block, world, at, face, true) {
            if is_supported_dynamic_source(block) || block == Block::RedstoneBlock {
                add(at, 0);
            } else {
                return Err(ScratchError {
                    position: Some(at),
                    reason: "unsupported piston input source",
                });
            }
        }
    }
    let mut result: Vec<_> = sources
        .into_iter()
        .map(|(source, attenuation)| WireSource {
            wire: pos,
            source,
            attenuation,
        })
        .collect();
    result.sort_by_key(|source| (source.source.x, source.source.y, source.source.z));
    Ok(result)
}

fn is_supported_dynamic_source(block: Block) -> bool {
    block.pressure_plate_powered().is_some()
        || matches!(
            block,
            Block::Lever { .. }
                | Block::StoneButton { .. }
                | Block::RedstoneTorch { .. }
                | Block::RedstoneWallTorch { .. }
                | Block::RedstoneRepeater { .. }
                | Block::RedstoneComparator { .. }
                | Block::Observer { .. }
        )
}

fn source_emits_to(block: Block, world: &impl World, pos: BlockPos, face: BlockFace) -> bool {
    crate::redstone::power::emits_weak_power(block, world, pos, face, true)
}
