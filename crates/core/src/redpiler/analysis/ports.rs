//! Interface discovery before logical lowering. Power dependencies never imply
//! sampling: a BUD's qualifying update has a separate physical channel.
use super::families::PistonRecognition;
use super::topology::{PowerDependencies, PowerRoute, SourceKind, Topology};
use super::{AnalysisError, PayloadGroup, PistonDescriptor};
use crate::world::World;
use mchprs_blocks::blocks::Block;
use mchprs_blocks::{BlockFace, BlockPos};
use rustc_hash::FxHashMap;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum UpdateKind {
    WireNotification,
    AdjacentHeadChange,
    /// A potential native callback route; movement timing is not certified here.
    PistonBaseChange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateDependency {
    pub source: BlockPos,
    pub kind: UpdateKind,
    /// The callback source is separate from this piston's recorded electrical
    /// dependencies. This is a physical channel, not a Boolean edge or a
    /// sampling certificate.
    pub independent_of_power: bool,
    /// A callback to the receiver's head forwards only while that head exists.
    pub requires_extended: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PistonPorts {
    pub piston: usize,
    pub updates: Vec<UpdateDependency>,
    pub outside_bounds: Vec<BlockPos>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ConsumerKind {
    Repeater { delay: u8, locked: bool },
    Comparator,
    Torch,
    Lamp,
    CopperBulb,
    Trapdoor,
    NoteBlock,
    CommandBlock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ConsumerInput {
    Main,
    ComparatorSide,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutputInterface {
    pub group: usize,
    pub consumer: BlockPos,
    pub kind: ConsumerKind,
    pub input: ConsumerInput,
    pub dependencies: PowerDependencies,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegionConnection {
    pub from_group: usize,
    pub to_piston: usize,
    pub source_alias: BlockPos,
    pub attenuation: u8,
    pub route: PowerRoute,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResetExposure {
    pub source: BlockPos,
    pub consumer: BlockPos,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PortReport {
    pub pistons: Vec<PistonPorts>,
    pub outputs: Vec<OutputInterface>,
    pub connections: Vec<RegionConnection>,
    pub reset_exposures: Vec<ResetExposure>,
}

pub(crate) fn is_consumer(block: Block) -> bool {
    block.is_command_block()
        || block.is_copper_bulb()
        || matches!(
            block,
            Block::RedstoneRepeater { .. }
                | Block::RedstoneComparator { .. }
                | Block::RedstoneTorch { .. }
                | Block::RedstoneWallTorch { .. }
                | Block::RedstoneLamp { .. }
                | Block::IronTrapdoor { .. }
                | Block::NoteBlock { .. }
        )
}

/// Receiving cells and faces, shared by static and conditional admission.
pub(crate) fn consumer_roots(
    block: Block,
    pos: BlockPos,
) -> Vec<(BlockPos, BlockFace, ConsumerInput)> {
    let main = |face: BlockFace| (pos.offset(face), face, ConsumerInput::Main);
    match block {
        Block::RedstoneRepeater { repeater } => vec![main(repeater.facing.block_face())],
        Block::RedstoneComparator { comparator } => vec![
            main(comparator.facing.block_face()),
            (
                pos.offset(comparator.facing.rotate().block_face()),
                comparator.facing.rotate().block_face(),
                ConsumerInput::ComparatorSide,
            ),
            (
                pos.offset(comparator.facing.rotate_ccw().block_face()),
                comparator.facing.rotate_ccw().block_face(),
                ConsumerInput::ComparatorSide,
            ),
        ],
        Block::RedstoneTorch { .. } => vec![(
            pos.offset(BlockFace::Bottom),
            BlockFace::Top,
            ConsumerInput::Main,
        )],
        Block::RedstoneWallTorch { facing, .. } => vec![main(facing.opposite().block_face())],
        _ if is_consumer(block) => BlockFace::values().into_iter().map(main).collect(),
        _ => Vec::new(),
    }
}

/// Discover electrical consumers and physical callback routes separately.
/// Shared reset exposure prevents one region from taking another region's updates.
pub(super) fn discover<W: World>(
    topology: &mut Topology<'_, W>,
    pistons: &[PistonDescriptor],
    recognition: &[PistonRecognition],
    groups: &[PayloadGroup],
    consumers: &[(BlockPos, Block)],
) -> Result<PortReport, AnalysisError> {
    let mut report = PortReport::default();
    let piston_bases: FxHashMap<_, _> = pistons
        .iter()
        .enumerate()
        .map(|(actor, piston)| (piston.pos, actor))
        .collect();
    let piston_heads: FxHashMap<_, _> = pistons
        .iter()
        .enumerate()
        .map(|(actor, piston)| (piston.head, actor))
        .collect();
    let reset_internals = super::families::reset_internals(recognition);
    let mut piston_groups = vec![0; pistons.len()];
    let mut reset_owners: FxHashMap<BlockPos, Vec<usize>> = FxHashMap::default();
    for (group, payload) in groups.iter().enumerate() {
        for &member in &payload.members {
            piston_groups[member] = group;
            for pos in super::families::reset_positions(&recognition[member]) {
                let owners = reset_owners.entry(pos).or_default();
                if owners.last() != Some(&group) {
                    owners.push(group);
                }
            }
        }
    }
    for (index, p) in pistons.iter().enumerate() {
        let inputs = &recognition[index].inputs;
        // Removing a reset from the ordinary graph is safe only when its
        // electrical consumers belong to that same payload protocol. A reset
        // shared between distinct groups needs a joint protocol certificate.
        for source in inputs
            .sources
            .iter()
            .map(|d| d.source)
            .chain(inputs.wires.iter().copied())
        {
            if reset_owners
                .get(&source)
                .is_some_and(|owners| owners.as_slice() != [piston_groups[index]])
            {
                report.reset_exposures.push(ResetExposure {
                    source,
                    consumer: p.pos,
                });
            }
        }
        let mut ports = PistonPorts {
            piston: index,
            updates: Vec::new(),
            outside_bounds: Vec::new(),
        };
        // update_wire_neighbors rechecks every one- and two-face neighbor,
        // independent of whether the wire electrically powers the piston.
        for dy in -2i32..=2 {
            for dz in -2i32..=2 {
                for dx in -2i32..=2 {
                    let distance = dx.abs() + dy.abs() + dz.abs();
                    if distance == 0 || distance > 2 {
                        continue;
                    }
                    let pos = p.pos + BlockPos::new(dx, dy, dz);
                    let Some(block) = topology.read(pos)? else {
                        ports.outside_bounds.push(pos);
                        continue;
                    };
                    let kind = match block {
                        Block::RedstoneWire { .. } => UpdateKind::WireNotification,
                        Block::PistonHead { .. } if distance == 1 => UpdateKind::AdjacentHeadChange,
                        _ => continue,
                    };
                    let provides_power = match kind {
                        UpdateKind::WireNotification => inputs.wires.contains(&pos),
                        UpdateKind::AdjacentHeadChange => {
                            inputs.sources.iter().any(|d| d.source == pos)
                        }
                        UpdateKind::PistonBaseChange => unreachable!(),
                    };
                    ports.updates.push(UpdateDependency {
                        source: pos,
                        kind,
                        independent_of_power: !provides_power,
                        requires_extended: false,
                    });
                }
            }
        }
        // Native base changes can recheck another base or its still-present head.
        for (receiver, requires_extended) in [(p.pos, false), (p.head, true)] {
            for face in BlockFace::values() {
                let source = receiver.offset(face);
                for (actors, kind) in [
                    (&piston_bases, UpdateKind::PistonBaseChange),
                    (&piston_heads, UpdateKind::AdjacentHeadChange),
                ] {
                    if actors.get(&source).is_some_and(|&actor| actor != index) {
                        ports.updates.push(UpdateDependency {
                            source,
                            kind,
                            independent_of_power: !inputs
                                .sources
                                .iter()
                                .any(|d| d.source == source),
                            requires_extended,
                        });
                    }
                }
            }
        }
        ports.updates.sort_by_key(|u| {
            (
                u.source.y,
                u.source.z,
                u.source.x,
                u.kind as u8,
                u.requires_extended,
            )
        });
        ports.updates.dedup();
        for d in &inputs.sources {
            if let SourceKind::MobilePayload { group } = d.kind {
                report.connections.push(RegionConnection {
                    from_group: group,
                    to_piston: index,
                    source_alias: d.source,
                    attenuation: d.attenuation,
                    route: d.route,
                });
            }
        }
        report.pistons.push(ports);
    }
    // Query receiving faces, not signs or the block directly below a label.
    // Stop at ordinary timed nodes: a downstream lamp belongs to the repeater's
    // graph, rather than acquiring a second instant output alias.
    for &(pos, block) in consumers {
        if reset_internals.contains(&pos) {
            continue;
        }
        let kind = match block {
            Block::RedstoneRepeater { repeater } => ConsumerKind::Repeater {
                delay: repeater.delay,
                locked: repeater.locked,
            },
            Block::RedstoneComparator { .. } => ConsumerKind::Comparator,
            Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. } => ConsumerKind::Torch,
            Block::RedstoneLamp { .. } => ConsumerKind::Lamp,
            block if block.is_copper_bulb() => ConsumerKind::CopperBulb,
            Block::IronTrapdoor { .. } => ConsumerKind::Trapdoor,
            Block::NoteBlock { .. } => ConsumerKind::NoteBlock,
            block if block.is_command_block() => ConsumerKind::CommandBlock,
            _ => continue,
        };
        let roots = consumer_roots(block, pos);
        for (root, face, input) in roots {
            // A direct, stationary inventory override replaces rear power.
            // Observer power through that support is not a reset exposure;
            // the ordinary graph owns the unchanged analog value.
            if matches!(block, Block::RedstoneComparator { .. })
                && input == ConsumerInput::Main
                && topology.mobile_group(root).is_none()
                && topology
                    .read(root)?
                    .is_some_and(crate::redstone::comparator::has_override)
            {
                continue;
            }
            let dependencies = match input {
                ConsumerInput::Main => topology.signal_inputs(root, face)?,
                ConsumerInput::ComparatorSide => topology.comparator_side_inputs(root, face)?,
            };
            for source in dependencies
                .sources
                .iter()
                .map(|d| d.source)
                .chain(dependencies.wires.iter().copied())
            {
                if reset_internals.contains(&source) {
                    report.reset_exposures.push(ResetExposure {
                        source,
                        consumer: pos,
                    });
                }
            }
            let mut groups: Vec<_> = dependencies
                .sources
                .iter()
                .filter_map(|d| {
                    if let SourceKind::MobilePayload { group } = d.kind {
                        Some(group)
                    } else {
                        None
                    }
                })
                .collect();
            groups.sort_unstable();
            groups.dedup();
            for group in groups {
                report.outputs.push(OutputInterface {
                    group,
                    consumer: pos,
                    kind,
                    input,
                    dependencies: dependencies.clone(),
                });
            }
        }
    }
    report.outputs.sort_by_key(|o| {
        (
            o.consumer.y,
            o.consumer.z,
            o.consumer.x,
            o.group,
            o.input as u8,
        )
    });
    let mut merged: Vec<OutputInterface> = Vec::with_capacity(report.outputs.len());
    for output in report.outputs {
        if let Some(previous) = merged.last_mut().filter(|p| {
            p.consumer == output.consumer && p.group == output.group && p.input == output.input
        }) {
            previous
                .dependencies
                .sources
                .extend(output.dependencies.sources);
            previous
                .dependencies
                .wires
                .extend(output.dependencies.wires);
            previous
                .dependencies
                .outside_bounds
                .extend(output.dependencies.outside_bounds);
        } else {
            merged.push(output);
        }
    }
    for output in &mut merged {
        output.dependencies.normalize();
    }
    report.outputs = merged;
    report.reset_exposures.sort_by_key(|e| {
        (
            e.consumer.y,
            e.consumer.z,
            e.consumer.x,
            e.source.y,
            e.source.z,
            e.source.x,
        )
    });
    report.reset_exposures.dedup();
    Ok(report)
}
