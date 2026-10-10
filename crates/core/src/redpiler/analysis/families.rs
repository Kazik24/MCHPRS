//! Structural reset recognition. A matched path is not a runtime certificate:
//! ports, waveform, synchronization and materialization are separate gates.
use super::topology::{PowerDependencies, SourceKind, Topology};
use super::{AnalysisError, PayloadGroup, PistonDescriptor};
use crate::redstone;
use crate::world::World;
use mchprs_blocks::block_entities::{BlockEntity, ContainerType};
use mchprs_blocks::blocks::Block;
use mchprs_blocks::{BlockFace, BlockFacing, BlockPos};
use mchprs_world::TickEntry;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ResetFamily {
    ObserverAbove,
    Torch,
    DustBelowHead,
    LateralDust,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ResetSupply {
    Independent,
    Payload {
        group: usize,
        positions: Vec<BlockPos>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct ResetPath {
    pub family: ResetFamily,
    pub source: BlockPos,
    pub support: BlockPos,
    pub supply: ResetSupply,
    pub wires: Vec<BlockPos>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum RecognitionFailure {
    NonSticky,
    RetractedEntry,
    UnsupportedDirection,
    MismatchedHead,
    UnsupportedPayload {
        pos: BlockPos,
        block: &'static str,
    },
    BlockEntity {
        pos: BlockPos,
    },
    /// A valid BUD may have this state. It needs a storage protocol rather
    /// than admission as a ready instant.
    UnsampledEntry,
    OutsideBounds {
        pos: BlockPos,
    },
    NonconductingSupport {
        pos: BlockPos,
    },
    ObserverFacing {
        source: BlockPos,
        actual: BlockFace,
    },
    MovableSupport {
        pos: BlockPos,
    },
    ActiveReset {
        pos: BlockPos,
    },
    PendingReset {
        pos: BlockPos,
    },
    AdditionalResetWriter {
        pos: BlockPos,
    },
    ForcedPower {
        source: BlockPos,
    },
    NoResetPath,
    NoReturnPath {
        source: BlockPos,
    },
    NoPayloadSupply {
        source: BlockPos,
    },
    UnverifiedTorchControl {
        source: BlockPos,
    },
    AmbiguousReset,
}

impl fmt::Display for RecognitionFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonSticky => f.write_str("ordinary pistons do not have a supported instant mechanism"),
            Self::RetractedEntry => f.write_str("the current matcher requires a ready, extended piston"),
            Self::UnsupportedDirection => f.write_str("upward-facing instant mechanisms are not supported yet"),
            Self::MismatchedHead => f.write_str("the extended piston has a missing or incompatible stationary head"),
            Self::UnsupportedPayload { pos, block } => write!(f, "payload minecraft:{block} at {pos:?} is not a supported redstone emitter or fixed conductor"),
            Self::BlockEntity { pos } => write!(f, "moving or reset context at {pos:?} contains an unsupported block entity"),
            Self::UnsampledEntry => f.write_str("present power differs from the sampled piston state; this needs a storage protocol"),
            Self::OutsideBounds { pos } => write!(f, "required context at {pos:?} is outside the selection"),
            Self::NonconductingSupport { pos } => write!(f, "reset support at {pos:?} does not conduct power"),
            Self::ObserverFacing { source, actual } => write!(f, "reset observer at {source:?} faces {actual:?}; the supported construction faces down"),
            Self::MovableSupport { pos } => write!(f, "reset support at {pos:?} is a possible payload position"),
            Self::ActiveReset { pos } => write!(f, "reset source at {pos:?} is already active"),
            Self::PendingReset { pos } => write!(f, "reset source at {pos:?} has pending work"),
            Self::AdditionalResetWriter { pos } => write!(f, "reset circuitry has an additional power writer at {pos:?}"),
            Self::ForcedPower { source } => write!(f, "a fixed source at {source:?} supplies piston power"),
            Self::NoResetPath => f.write_str("no supported reset path was found"),
            Self::NoReturnPath { source } => write!(f, "reset source at {source:?} has no power path back to the piston"),
            Self::NoPayloadSupply { source } => write!(f, "reset source at {source:?} is not supplied by the retracted payload"),
            Self::UnverifiedTorchControl { source } => write!(f, "reset torch at {source:?} has no verified control path for all admitted strengths"),
            Self::AmbiguousReset => f.write_str("multiple reset paths match; joint reset behavior is not supported yet"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PistonRecognition {
    pub piston: usize,
    pub inputs: PowerDependencies,
    pub resets: Vec<ResetPath>,
    pub failures: Vec<RecognitionFailure>,
}

impl PistonRecognition {
    pub fn is_matched(&self) -> bool {
        self.failures.is_empty() && !self.resets.is_empty()
    }
}

pub(crate) fn reset_positions(piston: &PistonRecognition) -> impl Iterator<Item = BlockPos> + '_ {
    piston.resets.iter().flat_map(|reset| {
        std::iter::once(reset.source).chain(reset.wires.iter().copied().filter(move |_| {
            matches!(
                reset.family,
                ResetFamily::DustBelowHead | ResetFamily::LateralDust
            )
        }))
    })
}

pub(crate) fn reset_internals(recognition: &[PistonRecognition]) -> FxHashSet<BlockPos> {
    recognition.iter().flat_map(reset_positions).collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum GroupFailure {
    PayloadCount {
        count: usize,
    },
    UnsupportedMember {
        piston: usize,
    },
    /// A shared payload can move to another owner's near position. A reset
    /// powered only at this owner's position cannot certify every outcome.
    ResetSupplyLost {
        piston: usize,
        owner: usize,
        position: BlockPos,
    },
}

impl fmt::Display for GroupFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PayloadCount { count } => write!(f, "expected one supported payload, found {count}"),
            Self::UnsupportedMember { piston } => write!(f, "member piston {piston} has no supported ready mechanism"),
            Self::ResetSupplyLost { piston, owner, position } => write!(f, "member piston {piston} loses reset supply when owner {owner} moves the payload to {position:?}"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupRecognition {
    pub group: usize,
    pub payloads: Vec<BlockPos>,
    pub failures: Vec<GroupFailure>,
}

impl GroupRecognition {
    pub fn has_reset_closure(&self) -> bool {
        self.failures.is_empty()
    }
}

pub(super) fn recognize<W: World>(
    topology: &mut Topology<'_, W>,
    pistons: &[PistonDescriptor],
    groups: &[PayloadGroup],
    ticks: &[TickEntry],
) -> Result<(Vec<PistonRecognition>, Vec<GroupRecognition>), AnalysisError> {
    let mut actor_groups = vec![0; pistons.len()];
    for (group, descriptor) in groups.iter().enumerate() {
        for &actor in &descriptor.members {
            actor_groups[actor] = group;
        }
    }
    let movement: FxHashSet<_> = groups
        .iter()
        .flat_map(|g| g.positions.iter().copied())
        .collect();
    let pending: FxHashSet<_> = ticks
        .iter()
        .filter(|t| {
            t.block_type
                .is_none_or(|id| topology.world.get_block(t.pos).registry_id() == id)
        })
        .map(|t| t.pos)
        .collect();
    let mut matches = Vec::with_capacity(pistons.len());
    for (index, p) in pistons.iter().enumerate() {
        let inputs = topology.piston_inputs(p)?;
        let mut result = PistonRecognition {
            piston: index,
            inputs,
            resets: Vec::new(),
            failures: Vec::new(),
        };
        if !p.piston.sticky {
            result.failures.push(RecognitionFailure::NonSticky);
        }
        if !p.piston.extended {
            result.failures.push(RecognitionFailure::RetractedEntry);
        }
        if p.piston.facing == BlockFacing::Up {
            result
                .failures
                .push(RecognitionFailure::UnsupportedDirection);
        }
        if p.piston.extended {
            match topology.read(p.head)? {
                None => result
                    .failures
                    .push(RecognitionFailure::OutsideBounds { pos: p.head }),
                Some(Block::PistonHead { head })
                    if head.facing == p.piston.facing && head.sticky && !head.short => {}
                Some(_) => result.failures.push(RecognitionFailure::MismatchedHead),
            }
        }
        match topology.read(p.payload)? {
            None => result
                .failures
                .push(RecognitionFailure::OutsideBounds { pos: p.payload }),
            Some(block) if crate::redpiler::analysis::supported_payload(block) => {}
            Some(block) => result
                .failures
                .push(RecognitionFailure::UnsupportedPayload {
                    pos: p.payload,
                    block: block.get_name(),
                }),
        }
        for pos in [p.pos, p.head, p.payload] {
            if topology.world.get_block_entity(pos).is_some() {
                result
                    .failures
                    .push(RecognitionFailure::BlockEntity { pos });
            }
        }
        if p.powered != p.piston.extended {
            result.failures.push(RecognitionFailure::UnsampledEntry);
        }
        for &pos in &result.inputs.outside_bounds {
            result
                .failures
                .push(RecognitionFailure::OutsideBounds { pos });
        }
        for d in &result.inputs.sources {
            if d.kind == SourceKind::Constant {
                result
                    .failures
                    .push(RecognitionFailure::ForcedPower { source: d.source });
            }
        }
        observer(topology, p, &movement, &pending, &mut result)?;
        torches(topology, p, &movement, &pending, &mut result)?;
        dust(
            topology,
            p,
            actor_groups[index],
            &movement,
            &pending,
            &mut result,
        )?;
        if result.resets.is_empty() {
            result.failures.push(RecognitionFailure::NoResetPath);
        }
        if result.resets.len() > 1 {
            result.failures.push(RecognitionFailure::AmbiguousReset);
        }
        matches.push(result);
    }
    let mut group_matches = Vec::with_capacity(groups.len());
    for (index, group) in groups.iter().enumerate() {
        let mut result = GroupRecognition {
            group: index,
            payloads: Vec::new(),
            failures: Vec::new(),
        };
        for &pos in &group.positions {
            if matches!(
                topology.read(pos)?,
                Some(block) if crate::redpiler::analysis::supported_payload(block)
            ) {
                result.payloads.push(pos);
            }
        }
        if result.payloads.len() != 1 {
            result.failures.push(GroupFailure::PayloadCount {
                count: result.payloads.len(),
            });
        }
        for &member in &group.members {
            if !matches[member].is_matched() {
                result
                    .failures
                    .push(GroupFailure::UnsupportedMember { piston: member });
                continue;
            }
            if result.payloads.len() != 1 {
                continue;
            }
            for &owner in &group.members {
                topology.step()?;
                let pos = pistons[owner].head;
                let supplied = matches[member].resets.iter().any(|r| match &r.supply {
                    ResetSupply::Independent => true,
                    ResetSupply::Payload {
                        group: id,
                        positions,
                    } => *id == index && positions.contains(&pos),
                });
                if !supplied {
                    result.failures.push(GroupFailure::ResetSupplyLost {
                        piston: member,
                        owner,
                        position: pos,
                    });
                }
            }
        }
        group_matches.push(result);
    }
    Ok((matches, group_matches))
}

pub(crate) fn fixed_container(world: &impl World, pos: BlockPos) -> bool {
    let block = world.get_block(pos);
    block.is_solid()
        && !redstone::has_neighbor_update(block)
        && matches!(
            world.get_block_entity(pos),
            Some(BlockEntity::Container { ty, .. })
                if ContainerType::from_block(block) == Some(*ty)
        )
}

fn support<W: World>(
    topology: &mut Topology<'_, W>,
    pos: BlockPos,
    movement: &FxHashSet<BlockPos>,
    r: &mut PistonRecognition,
) -> Result<bool, AnalysisError> {
    let Some(block) = topology.read(pos)? else {
        r.failures.push(RecognitionFailure::OutsideBounds { pos });
        return Ok(false);
    };
    if !block.is_solid() {
        r.failures
            .push(RecognitionFailure::NonconductingSupport { pos });
        return Ok(false);
    }
    if movement.contains(&pos) {
        r.failures.push(RecognitionFailure::MovableSupport { pos });
        return Ok(false);
    }
    // Fixed inventories supply an analog override without changing reset conduction.
    if topology.world.get_block_entity(pos).is_some() && !fixed_container(topology.world, pos) {
        r.failures.push(RecognitionFailure::BlockEntity { pos });
        return Ok(false);
    }
    Ok(true)
}

fn ready(
    pos: BlockPos,
    active: bool,
    pending: &FxHashSet<BlockPos>,
    r: &mut PistonRecognition,
) -> bool {
    if active {
        r.failures.push(RecognitionFailure::ActiveReset { pos });
    }
    if pending.contains(&pos) {
        r.failures.push(RecognitionFailure::PendingReset { pos });
    }
    !active && !pending.contains(&pos)
}

fn observer<W: World>(
    topology: &mut Topology<'_, W>,
    p: &PistonDescriptor,
    movement: &FxHashSet<BlockPos>,
    pending: &FxHashSet<BlockPos>,
    r: &mut PistonRecognition,
) -> Result<(), AnalysisError> {
    let source = p.pos.offset(BlockFace::Top);
    let Some(Block::Observer { observer }) = topology.read(source)? else {
        return Ok(());
    };
    if observer.facing != BlockFacing::Down {
        r.failures.push(RecognitionFailure::ObserverFacing {
            source,
            actual: observer.facing.into(),
        });
        return Ok(());
    }
    let cap = source.offset(BlockFace::Top);
    if !support(topology, cap, movement, r)? {
        return Ok(());
    }
    if !ready(source, observer.powered, pending, r) {
        return Ok(());
    }
    let supply = topology.signal_inputs(cap, BlockFace::Top)?;
    for pos in supply.outside_bounds {
        r.failures.push(RecognitionFailure::OutsideBounds { pos });
    }
    if let Some(writer) = supply.sources.iter().find(|d| {
        if d.source == source { return false; }
        // A stable lever on the cap is a prepared inhibit input, not an
        // independent reset writer: firing already requires its power to fall.
        // Keep delayed, mobile and indirect writers outside this certificate.
        let prepared_inhibit = d.kind == SourceKind::Ordinary
            && d.source == cap.offset(BlockFace::Top)
            && matches!(topology.world.get_block(d.source), Block::Lever { lever } if lever.face == mchprs_blocks::blocks::LeverFace::Floor)
            && r.inputs.sources.iter().any(|input| input.source == d.source && input.attenuation <= d.attenuation);
        !prepared_inhibit
    }) {
        r.failures
            .push(RecognitionFailure::AdditionalResetWriter { pos: writer.source });
        return Ok(());
    }
    if !r.inputs.sources.iter().any(|d| d.source == source) {
        r.failures.push(RecognitionFailure::NoReturnPath { source });
        return Ok(());
    }
    r.resets.push(ResetPath {
        family: ResetFamily::ObserverAbove,
        source,
        support: cap,
        supply: ResetSupply::Independent,
        wires: Vec::new(),
    });
    Ok(())
}

fn torches<W: World>(
    topology: &mut Topology<'_, W>,
    p: &PistonDescriptor,
    movement: &FxHashSet<BlockPos>,
    pending: &FxHashSet<BlockPos>,
    r: &mut PistonRecognition,
) -> Result<(), AnalysisError> {
    for dy in 0..=2 {
        for dz in -1..=1 {
            for dx in -1..=1 {
                let source = p.pos + BlockPos::new(dx, dy, dz);
                let (support_pos, face) = match topology.read(source)? {
                    Some(Block::RedstoneTorch { lit: false }) => {
                        (source.offset(BlockFace::Bottom), BlockFace::Top)
                    }
                    Some(Block::RedstoneWallTorch { lit: false, facing }) => (
                        source.offset(facing.opposite().block_face()),
                        facing.opposite().block_face(),
                    ),
                    _ => continue,
                };
                // An unrelated unlit torch is not a reset candidate.
                if !r.inputs.sources.iter().any(|d| d.source == source) {
                    continue;
                }
                if !support(topology, support_pos, movement, r)?
                    || !ready(source, false, pending, r)
                {
                    continue;
                }
                let control = topology.signal_inputs(support_pos, face)?;
                for &pos in &control.outside_bounds {
                    r.failures.push(RecognitionFailure::OutsideBounds { pos });
                }
                // If base power is gone, every controlling source must also be
                // below the torch's threshold. A shorter control wire could
                // stay powered after an analog fall has already released the
                // piston, preventing its reset.
                let known_control = !control.sources.is_empty()
                    && control.sources.iter().all(|d| {
                        d.kind == SourceKind::Ordinary
                            && r.inputs.sources.iter().any(|input| {
                                input.source == d.source && input.attenuation <= d.attenuation
                            })
                    });
                let off = redstone::get_redstone_power(
                    topology.world.get_block(support_pos),
                    topology.world,
                    support_pos,
                    face,
                ) > 0;
                if !known_control
                    || !off
                    || control
                        .sources
                        .iter()
                        .any(|d| d.kind != SourceKind::Ordinary)
                {
                    r.failures
                        .push(RecognitionFailure::UnverifiedTorchControl { source });
                    continue;
                }
                r.resets.push(ResetPath {
                    family: ResetFamily::Torch,
                    source,
                    support: support_pos,
                    supply: ResetSupply::Independent,
                    wires: control.wires,
                });
            }
        }
    }
    Ok(())
}

fn dust<W: World>(
    topology: &mut Topology<'_, W>,
    p: &PistonDescriptor,
    group: usize,
    movement: &FxHashSet<BlockPos>,
    pending: &FxHashSet<BlockPos>,
    r: &mut PistonRecognition,
) -> Result<(), AnalysisError> {
    let under = p.head.offset(BlockFace::Bottom);
    let mut candidates = vec![(under, ResetFamily::DustBelowHead)];
    for face in [
        BlockFace::North,
        BlockFace::South,
        BlockFace::East,
        BlockFace::West,
    ] {
        let source = p.head.offset(face);
        if !movement.contains(&source) {
            candidates.push((source, ResetFamily::LateralDust));
        }
    }
    for (source, family) in candidates {
        let Some(Block::RedstoneWire { wire }) = topology.read(source)? else {
            continue;
        };
        let conductor = family == ResetFamily::DustBelowHead
            && conductor_reset(topology, p, source, wire, movement, pending)?;
        if !r.inputs.wires.contains(&source) && !conductor {
            // Under-head dust is a named construction even if its support no
            // longer returns power. Unrelated lateral output dust is ignored.
            if family == ResetFamily::DustBelowHead {
                r.failures.push(RecognitionFailure::NoReturnPath { source });
            }
            continue;
        }
        let support_pos = source.offset(BlockFace::Bottom);
        if !support(topology, support_pos, movement, r)?
            || !ready(source, wire.power > 0, pending, r)
        {
            continue;
        }
        let supply = topology.wire_inputs(source)?;
        for &pos in &supply.outside_bounds {
            r.failures.push(RecognitionFailure::OutsideBounds { pos });
        }
        let mut positions: Vec<_> = supply
            .sources
            .iter()
            .filter(|d| d.kind == SourceKind::MobilePayload { group })
            .map(|d| d.source)
            .collect();
        if !positions.contains(&p.head) && conductor {
            positions.push(p.head);
        }
        if !positions.contains(&p.head) {
            r.failures
                .push(RecognitionFailure::NoPayloadSupply { source });
            continue;
        }
        if let Some(writer) = supply
            .sources
            .iter()
            .find(|d| d.kind != SourceKind::MobilePayload { group })
        {
            r.failures
                .push(RecognitionFailure::AdditionalResetWriter { pos: writer.source });
            continue;
        }
        r.resets.push(ResetPath {
            family,
            source,
            support: support_pos,
            supply: ResetSupply::Payload { group, positions },
            wires: supply.wires,
        });
    }
    Ok(())
}

/// A near conductor can close the reset path without emitting power itself.
/// Its fixed repeater supply must remain on for every admitted input value.
fn conductor_reset<W: World>(
    topology: &mut Topology<'_, W>,
    p: &PistonDescriptor,
    source: BlockPos,
    wire: mchprs_blocks::blocks::RedstoneWire,
    movement: &FxHashSet<BlockPos>,
    pending: &FxHashSet<BlockPos>,
) -> Result<bool, AnalysisError> {
    let Some(payload) = topology.read(p.payload)? else {
        return Ok(false);
    };
    if !BlockFace::from(p.piston.facing).is_horizontal()
        || payload == Block::RedstoneBlock
        || !crate::redpiler::analysis::supported_payload(payload)
        || !topology
            .read(p.pos.offset(BlockFace::Bottom))?
            .is_some_and(|block| block.is_solid())
    {
        return Ok(false);
    }
    // Shape reads stay within the adjacent columns, including their top/bottom cells.
    let mut context = FxHashMap::default();
    for x in -1..=1 {
        for y in -1..=1 {
            for z in -1..=1 {
                let pos = source + BlockPos::new(x, y, z);
                let Some(block) = topology.read(pos)? else {
                    return Ok(false);
                };
                if pos != p.head
                    && pos != p.payload
                    && pos != p.pos
                    && (movement.contains(&pos) || matches!(block, Block::Piston { .. }))
                {
                    return Ok(false);
                }
                context.insert(pos, block);
            }
        }
    }
    context.insert(p.head, payload);
    context.insert(p.payload, Block::Air);
    context.insert(
        p.pos,
        Block::Piston {
            piston: p.piston.extend(false),
        },
    );
    let shape = redstone::wire::get_regulated_sides_from(wire, source, |pos| context[&pos]);
    if redstone::wire::get_current_side(
        shape,
        BlockFace::from(p.piston.facing)
            .opposite()
            .unwrap_direction(),
    )
    .is_none()
    {
        return Ok(false);
    }
    for side in BlockFace::values() {
        let pos = p.head.offset(side);
        if movement.contains(&pos) || pending.contains(&pos) {
            continue;
        }
        let Some(Block::RedstoneRepeater { repeater }) = topology.read(pos)? else {
            continue;
        };
        if !repeater.powered || repeater.locked || repeater.facing.block_face() != side {
            continue;
        }
        let rear = pos.offset(repeater.facing.block_face());
        if movement.contains(&rear) || topology.read(rear)? != Some(Block::RedstoneBlock) {
            continue;
        }
        let mut lockable = false;
        for facing in [repeater.facing.rotate(), repeater.facing.rotate_ccw()] {
            let adjacent = pos.offset(facing.block_face());
            let Some(block) = topology.read(adjacent)? else {
                lockable = true;
                break;
            };
            lockable |= movement.contains(&adjacent)
                || (redstone::is_diode(block)
                    && redstone::power::emits_weak_power(
                        block,
                        topology.world,
                        adjacent,
                        facing.block_face(),
                        false,
                    ));
        }
        if !lockable {
            return Ok(true);
        }
    }
    Ok(false)
}
