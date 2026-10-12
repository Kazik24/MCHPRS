//! Read-only admission and contact extraction for the stationary piston subset.
#![allow(dead_code)] // The electrical parser consumes this draft in the next step.
use super::{AdmissionIssue, AnalysisError, AnalysisLimits, AnalysisReport, analyze};
use crate::interaction;
use crate::redpiler::TaskMonitor;
use crate::redpiler::program_graph::{
    CircuitNodeDraft, CircuitNodeKind, ComponentState, ContactId, NodeId, PistonAvailability,
    PistonId, PistonLane, PistonState, Profile, ProfileElectrical, ProfileId, ResourceId,
    ResourceSpec, ResourceState,
};
use crate::world::World;
use mchprs_blocks::block_entities::{BlockEntity, ContainerType};
use mchprs_blocks::blocks::{Block, RedstonePiston};
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_world::TickEntry;
use rustc_hash::{FxHashMap, FxHashSet};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GeometryIssue {
    MalformedHead {
        actor: BlockPos,
        position: BlockPos,
        found: u32,
    },
    MissingMotionMetadata {
        position: BlockPos,
    },
    UnsupportedMaterial {
        actor: BlockPos,
        position: BlockPos,
        block: u32,
    },
    OrdinaryPush {
        actor: BlockPos,
        position: BlockPos,
        block: u32,
    },
    PossibleLongerPush {
        actor: BlockPos,
        position: BlockPos,
        block: u32,
    },
    ChangingBlocker {
        actor: BlockPos,
        position: BlockPos,
    },
    OutsideFootprint {
        actor: BlockPos,
        position: BlockPos,
    },
    UnknownExternalOccupancy {
        actor: BlockPos,
        position: BlockPos,
    },
    SupportLoss {
        actor: BlockPos,
        changed: BlockPos,
        dependent: BlockPos,
        block: u32,
    },
    MultipleResources {
        actor: BlockPos,
        first: BlockPos,
        second: BlockPos,
    },
    ContactLimit,
}

impl fmt::Display for GeometryIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedHead {
                actor,
                position,
                found,
            } => write!(
                f,
                "piston at {actor:?} has malformed head at {position:?} (state {found})"
            ),
            Self::MissingMotionMetadata { position } => {
                write!(
                    f,
                    "moving piston at {position:?} has missing motion metadata"
                )
            }
            Self::UnsupportedMaterial {
                actor,
                position,
                block,
            } => write!(
                f,
                "piston at {actor:?} may transport unsupported {} at {position:?}",
                Block::from_id(*block).get_name()
            ),
            Self::OrdinaryPush {
                actor,
                position,
                block,
            } => write!(
                f,
                "ordinary piston at {actor:?} may push {} at {position:?}",
                Block::from_id(*block).get_name()
            ),
            Self::PossibleLongerPush {
                actor,
                position,
                block,
            } => write!(
                f,
                "piston at {actor:?} may make an unsupported longer push through {} at {position:?}",
                Block::from_id(*block).get_name()
            ),
            Self::ChangingBlocker { actor, position } => write!(
                f,
                "piston at {actor:?} has a possible payload line blocked by changing head at {position:?}"
            ),
            Self::OutsideFootprint { actor, position } => write!(
                f,
                "piston at {actor:?} may move a payload outside the compiled footprint at {position:?}"
            ),
            Self::UnknownExternalOccupancy { actor, position } => write!(
                f,
                "piston at {actor:?} needs unknown external occupancy at {position:?}"
            ),
            Self::SupportLoss {
                actor,
                changed,
                dependent,
                block,
            } => write!(
                f,
                "piston at {actor:?} can remove support at {changed:?} from {} at {dependent:?}",
                Block::from_id(*block).get_name()
            ),
            Self::MultipleResources {
                actor,
                first,
                second,
            } => write!(
                f,
                "piston at {actor:?} can connect independent payloads at {first:?} and {second:?}"
            ),
            Self::ContactLimit => f.write_str("piston geometry contact budget exceeded"),
        }
    }
}

#[derive(Debug)]
pub(crate) enum GeometryError {
    Analysis(AnalysisError),
    Entry(AdmissionIssue),
    Rejected(GeometryIssue),
}

impl fmt::Display for GeometryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Analysis(error) => error.fmt(f),
            Self::Entry(issue) => issue.fmt(f),
            Self::Rejected(issue) => issue.fmt(f),
        }
    }
}

impl std::error::Error for GeometryError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GeometryView {
    Fixed,
    Air,
    PistonBase { piston: PistonId, extended: bool },
    MovingPistonBase(PistonId),
    MovingPistonHead(PistonId),
    PistonHead(PistonId),
    Resource(ResourceId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GeometryVariant {
    pub(crate) view: GeometryView,
    pub(crate) profile: ProfileId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GeometryContact {
    pub(crate) node: NodeId,
    pub(crate) position: BlockPos,
    pub(crate) variants: Vec<GeometryVariant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GeometryPiston {
    pub(crate) id: PistonId,
    pub(crate) node: NodeId,
    pub(crate) position: BlockPos,
    pub(crate) facing: BlockFace,
    pub(crate) sticky: bool,
    pub(crate) base: ContactId,
    pub(crate) near: ContactId,
    pub(crate) far: ContactId,
    pub(crate) lane: PistonLane,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResourceOperation {
    Push,
    Pull,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResourceWitness {
    pub(crate) actor: PistonId,
    pub(crate) position: BlockPos,
    pub(crate) operation: ResourceOperation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GeometryResource {
    pub(crate) source: BlockPos,
    pub(crate) spec: ResourceSpec,
    pub(crate) initial: ResourceState,
    pub(crate) witnesses: Vec<ResourceWitness>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GeometryObservation {
    pub(crate) position: BlockPos,
    pub(crate) profile: ProfileId,
    pub(crate) contact: Option<ContactId>,
}

#[derive(Debug, Clone)]
pub(crate) struct GeometryDraft {
    pub(crate) remaining_cells: usize,
    pub(crate) profiles: Vec<Profile>,
    pub(crate) nodes: Vec<CircuitNodeDraft>,
    pub(crate) contacts: Vec<GeometryContact>,
    pub(crate) pistons: Vec<GeometryPiston>,
    pub(crate) resources: Vec<GeometryResource>,
    pub(crate) initial_pistons: Vec<PistonState>,
    pub(crate) initial_resources: Vec<ResourceState>,
    pub(crate) observations: Vec<GeometryObservation>,
}

#[derive(Debug, Clone)]
pub(crate) struct GeometryResult {
    /// Retains runtime-unavailable markers for callers; only this parser handles them.
    pub(crate) report: AnalysisReport,
    pub(crate) draft: GeometryDraft,
}

#[derive(Debug, Clone, Copy)]
struct Actor {
    id: PistonId,
    node: NodeId,
    position: BlockPos,
    piston: RedstonePiston,
    facing: BlockFace,
    near: BlockPos,
    far: BlockPos,
}

#[derive(Debug, Clone, Copy)]
struct WitnessSeed {
    actor: PistonId,
    position: BlockPos,
    operation: ResourceOperation,
}

#[derive(Debug)]
enum LineEnd {
    Air(BlockPos),
    Container,
    Head(BlockPos),
    WorldEdge,
}

#[derive(Debug)]
struct PayloadLine {
    blocks: Vec<(BlockPos, Block)>,
    end: LineEnd,
}

struct Probe<'a, W> {
    world: &'a W,
    monitor: &'a TaskMonitor,
    bounds: (BlockPos, BlockPos),
    budget: usize,
    reads: usize,
    blocks: FxHashMap<BlockPos, Block>,
}

impl<W: World> Probe<'_, W> {
    fn block(&mut self, actor: BlockPos, position: BlockPos) -> Result<Block, GeometryError> {
        if self.monitor.cancelled() {
            return Err(GeometryError::Analysis(AnalysisError::Cancelled));
        }
        if let Some(block) = self.blocks.get(&position) {
            return Ok(*block);
        }
        if self.reads >= self.budget {
            return Err(GeometryError::Rejected(GeometryIssue::ContactLimit));
        }
        self.reads += 1;
        if !(0..crate::plot::PLOT_BLOCK_HEIGHT).contains(&position.y) {
            return Ok(Block::Air);
        }
        if self
            .world
            .get_chunk(position.x.div_euclid(16), position.z.div_euclid(16))
            .is_none()
        {
            return Err(GeometryError::Rejected(
                GeometryIssue::UnknownExternalOccupancy { actor, position },
            ));
        }
        let block = self.world.get_block(position);
        self.blocks.insert(position, block);
        Ok(block)
    }

    fn in_bounds(&self, position: BlockPos) -> bool {
        self.bounds.0.x <= position.x
            && position.x <= self.bounds.1.x
            && self.bounds.0.y <= position.y
            && position.y <= self.bounds.1.y
            && self.bounds.0.z <= position.z
            && position.z <= self.bounds.1.z
    }
}

/// Prepare a finite, read-only model of stationary piston contacts and payload identities.
pub(crate) fn prepare_geometry(
    world: &impl World,
    bounds: (BlockPos, BlockPos),
    ticks: &[TickEntry],
    monitor: &TaskMonitor,
    limits: AnalysisLimits,
) -> Result<GeometryResult, GeometryError> {
    let report = analyze(world, bounds, ticks, monitor, limits).map_err(GeometryError::Analysis)?;
    validate_entry(world, &report)?;

    let mut probe = Probe {
        world,
        monitor,
        bounds: report.bounds,
        budget: limits.max_cells.saturating_sub(report.inspected_cells),
        reads: 0,
        blocks: FxHashMap::default(),
    };
    let actors = inventory_actors(&mut probe, &report)?;
    let (resources, resource_at) = resolve_resources(&mut probe, &actors)?;
    validate_footprint(&mut probe, &actors, &resources, &resource_at)?;
    validate_support(&mut probe, &actors, &resources)?;
    let draft = build_draft(&mut probe, &actors, resources)?;
    if monitor.cancelled() {
        return Err(GeometryError::Analysis(AnalysisError::Cancelled));
    }
    Ok(GeometryResult { report, draft })
}

fn validate_entry(world: &impl World, report: &AnalysisReport) -> Result<(), GeometryError> {
    for issue in &report.issues {
        match issue {
            AdmissionIssue::PistonRuntimeUnavailable { .. } => {}
            AdmissionIssue::MovingPiston { pos } => {
                validate_moving_metadata(world, *pos)?;
                return Err(GeometryError::Entry(issue.clone()));
            }
            _ => return Err(GeometryError::Entry(issue.clone())),
        }
    }
    Ok(())
}

fn validate_moving_metadata(world: &impl World, position: BlockPos) -> Result<(), GeometryError> {
    if !matches!(
        world.get_block_entity(position),
        Some(BlockEntity::MovingPiston(_))
    ) || world.piston_motion_index(position, None).is_none()
    {
        return Err(GeometryError::Rejected(
            GeometryIssue::MissingMotionMetadata { position },
        ));
    }
    Ok(())
}

fn inventory_actors<W: World>(
    probe: &mut Probe<'_, W>,
    report: &AnalysisReport,
) -> Result<Vec<Actor>, GeometryError> {
    let mut actors = Vec::with_capacity(report.pistons.len());
    for (index, &position) in report.pistons.iter().enumerate() {
        let Block::Piston { piston } = probe.block(position, position)? else {
            unreachable!("analysis recorded a piston at {position:?}");
        };
        let facing = BlockFace::from(piston.facing);
        let near = position.offset(facing);
        let far = near.offset(facing);
        let near_block = probe.block(position, near)?;
        if piston.extended
            && !matches!(near_block, Block::PistonHead { head }
                if head.facing == piston.facing && head.sticky == piston.sticky && !head.short)
        {
            return Err(GeometryError::Rejected(GeometryIssue::MalformedHead {
                actor: position,
                position: near,
                found: near_block.get_id(),
            }));
        }
        if !probe.in_bounds(near) {
            return Err(GeometryError::Rejected(GeometryIssue::OutsideFootprint {
                actor: position,
                position: near,
            }));
        }
        if piston.sticky && !probe.in_bounds(far) {
            return Err(GeometryError::Rejected(GeometryIssue::OutsideFootprint {
                actor: position,
                position: far,
            }));
        }
        let node = NodeId::new(index as u32);
        actors.push(Actor {
            id: PistonId::new(index as u32),
            node,
            position,
            piston,
            facing,
            near,
            far,
        });
    }
    Ok(actors)
}

fn resolve_resources<W: World>(
    probe: &mut Probe<'_, W>,
    actors: &[Actor],
) -> Result<
    (
        Vec<(BlockPos, Vec<BlockPos>, Vec<ResourceWitness>)>,
        FxHashMap<BlockPos, ResourceId>,
    ),
    GeometryError,
> {
    let mut seeds: FxHashMap<BlockPos, Vec<WitnessSeed>> = FxHashMap::default();
    let mut edges = Vec::new();

    for actor in actors {
        let line = if actor.piston.extended {
            None
        } else {
            Some(payload_line(
                probe,
                actor.position,
                actor.near,
                actor.facing,
                None,
            )?)
        };
        if let Some(line) = &line {
            if let LineEnd::Air(end) = line.end {
                check_line_destination(probe, actor, line, end)?;
                if line.blocks.len() > 1 {
                    let (position, block) = line.blocks[1];
                    return Err(GeometryError::Rejected(GeometryIssue::PossibleLongerPush {
                        actor: actor.position,
                        position,
                        block: block.get_id(),
                    }));
                }
                if let Some(&(position, block)) = line.blocks.first() {
                    if !actor.piston.sticky {
                        return Err(GeometryError::Rejected(GeometryIssue::OrdinaryPush {
                            actor: actor.position,
                            position,
                            block: block.get_id(),
                        }));
                    }
                    classify_material(actor.position, position, block, probe.world)?;
                    seeds.entry(position).or_default().push(WitnessSeed {
                        actor: actor.id,
                        position,
                        operation: ResourceOperation::Push,
                    });
                }
            }
            if let LineEnd::Head(position) = line.end {
                if !line.blocks.is_empty() {
                    return Err(GeometryError::Rejected(GeometryIssue::ChangingBlocker {
                        actor: actor.position,
                        position,
                    }));
                }
            }
        }

        if !actor.piston.sticky {
            continue;
        }
        let extension_can_settle = actor.piston.extended
            || line.as_ref().is_some_and(|line| {
                matches!(line.end, LineEnd::Air(_))
                    || matches!(line.end, LineEnd::Head(_)) && line.blocks.is_empty()
            });
        let far_block = probe.block(actor.position, actor.far)?;
        if extension_can_settle && far_block != Block::Air && !is_container(far_block) {
            classify_material(actor.position, actor.far, far_block, probe.world)?;
            seeds.entry(actor.far).or_default().push(WitnessSeed {
                actor: actor.id,
                position: actor.far,
                operation: ResourceOperation::Pull,
            });
        }
        if extension_can_settle && !is_container(far_block) {
            edges.push((actor.near, actor.far));
        }
    }

    let mut adjacency: FxHashMap<BlockPos, Vec<BlockPos>> = FxHashMap::default();
    for &(near, far) in &edges {
        adjacency.entry(near).or_default().push(far);
        adjacency.entry(far).or_default().push(near);
    }

    let mut resource_at = FxHashMap::default();
    let mut resources = Vec::new();
    let mut seen = FxHashSet::default();
    let mut seed_positions: Vec<_> = seeds.keys().copied().collect();
    seed_positions.sort_by_key(|p| (p.y, p.z, p.x));
    for source in seed_positions {
        if !seen.insert(source) {
            continue;
        }
        let mut contacts = Vec::new();
        let mut stack = vec![source];
        while let Some(position) = stack.pop() {
            contacts.push(position);
            for &next in adjacency.get(&position).into_iter().flatten() {
                if seen.insert(next) {
                    stack.push(next);
                }
            }
        }
        contacts.sort_by_key(|p| (p.y, p.z, p.x));
        let payloads: Vec<_> = contacts
            .iter()
            .filter(|position| seeds.contains_key(position))
            .copied()
            .collect();
        if payloads.len() > 1 {
            let actor = seeds[&payloads[1]][0].actor;
            return Err(GeometryError::Rejected(GeometryIssue::MultipleResources {
                actor: actors[actor.index()].position,
                first: payloads[0],
                second: payloads[1],
            }));
        }
        let id = ResourceId::new(resources.len() as u32);
        for &position in &contacts {
            resource_at.insert(position, id);
        }
        let witnesses = seeds[&source]
            .iter()
            .map(|seed| ResourceWitness {
                actor: seed.actor,
                position: seed.position,
                operation: seed.operation,
            })
            .collect();
        resources.push((source, contacts, witnesses));
    }
    Ok((resources, resource_at))
}

fn payload_line<W: World>(
    probe: &mut Probe<'_, W>,
    actor: BlockPos,
    start: BlockPos,
    facing: BlockFace,
    empty_resource_contacts: Option<(&FxHashMap<BlockPos, ResourceId>, ResourceId)>,
) -> Result<PayloadLine, GeometryError> {
    let mut blocks = Vec::new();
    let mut position = start;
    loop {
        if empty_resource_contacts
            .is_some_and(|(contacts, resource)| contacts.get(&position) == Some(&resource))
        {
            return Ok(PayloadLine {
                blocks,
                end: LineEnd::Air(position),
            });
        }
        if !(0..crate::plot::PLOT_BLOCK_HEIGHT).contains(&position.y) {
            return Ok(PayloadLine {
                blocks,
                end: LineEnd::WorldEdge,
            });
        }
        let block = probe.block(actor, position)?;
        if block == Block::Air {
            return Ok(PayloadLine {
                blocks,
                end: LineEnd::Air(position),
            });
        }
        if is_container(block) {
            return Ok(PayloadLine {
                blocks,
                end: LineEnd::Container,
            });
        }
        match block {
            Block::MovingPiston { .. } => {
                validate_moving_metadata(probe.world, position)?;
                return Err(GeometryError::Entry(AdmissionIssue::MovingPiston {
                    pos: position,
                }));
            }
            Block::PistonHead { .. } => {
                return Ok(PayloadLine {
                    blocks,
                    end: LineEnd::Head(position),
                });
            }
            _ => blocks.push((position, block)),
        }
        position = position.offset(facing);
    }
}

fn check_line_destination<W: World>(
    probe: &Probe<'_, W>,
    actor: &Actor,
    line: &PayloadLine,
    air: BlockPos,
) -> Result<(), GeometryError> {
    if !probe.in_bounds(air) {
        return Err(GeometryError::Rejected(GeometryIssue::OutsideFootprint {
            actor: actor.position,
            position: air,
        }));
    }
    for &(position, _) in &line.blocks {
        if !probe.in_bounds(position) {
            return Err(GeometryError::Rejected(GeometryIssue::OutsideFootprint {
                actor: actor.position,
                position,
            }));
        }
    }
    Ok(())
}

fn validate_footprint<W: World>(
    probe: &mut Probe<'_, W>,
    actors: &[Actor],
    resources: &[(BlockPos, Vec<BlockPos>, Vec<ResourceWitness>)],
    resource_at: &FxHashMap<BlockPos, ResourceId>,
) -> Result<(), GeometryError> {
    for actor in actors {
        let Some(&resource) = resource_at.get(&actor.near) else {
            continue;
        };
        let source = resources[resource.index()].0;
        let block = probe.block(actor.position, source)?;
        let line = payload_line(
            probe,
            actor.position,
            actor.far,
            actor.facing,
            Some((resource_at, resource)),
        )?;
        if let LineEnd::Air(end) = line.end {
            check_line_destination(probe, actor, &line, end)?;
            if let Some(&(position, block)) = line.blocks.first() {
                return Err(GeometryError::Rejected(GeometryIssue::PossibleLongerPush {
                    actor: actor.position,
                    position,
                    block: block.get_id(),
                }));
            }
            if !actor.piston.sticky {
                return Err(GeometryError::Rejected(GeometryIssue::OrdinaryPush {
                    actor: actor.position,
                    position: actor.near,
                    block: block.get_id(),
                }));
            }
        } else if let LineEnd::Head(position) = line.end {
            return Err(GeometryError::Rejected(GeometryIssue::ChangingBlocker {
                actor: actor.position,
                position,
            }));
        }
    }
    Ok(())
}

fn validate_support<W: World>(
    probe: &mut Probe<'_, W>,
    actors: &[Actor],
    resources: &[(BlockPos, Vec<BlockPos>, Vec<ResourceWitness>)],
) -> Result<(), GeometryError> {
    let mut changing: FxHashMap<BlockPos, Vec<(PistonId, Block)>> = FxHashMap::default();
    for actor in actors {
        let base = probe.block(actor.position, actor.position)?;
        changing.entry(actor.position).or_default().extend([
            (actor.id, base),
            (
                actor.id,
                Block::Piston {
                    piston: actor.piston.extend(!actor.piston.extended),
                },
            ),
            (
                actor.id,
                Block::MovingPiston {
                    moving: actor.piston.into(),
                },
            ),
        ]);
        changing.entry(actor.near).or_default().extend([
            (actor.id, Block::Air),
            (
                actor.id,
                Block::PistonHead {
                    head: actor.piston.into(),
                },
            ),
            (
                actor.id,
                Block::MovingPiston {
                    moving: actor.piston.into(),
                },
            ),
        ]);
    }
    for (source, contacts, witnesses) in resources {
        let block = probe.block(witnesses[0].position, *source)?;
        for &position in contacts {
            changing.entry(position).or_default().extend([
                (witnesses[0].actor, block),
                (witnesses[0].actor, Block::Air),
                (
                    witnesses[0].actor,
                    Block::MovingPiston {
                        moving: actors[witnesses[0].actor.index()].piston.into(),
                    },
                ),
            ]);
        }
    }
    for (&changed, alternatives) in &changing {
        for face in BlockFace::values() {
            let dependent = changed.offset(face);
            let actor = actors[alternatives[0].0.index()].position;
            let block = probe.block(actor, dependent)?;
            let Some((_, support_face)) = interaction::attachment_support(block, dependent) else {
                continue;
            };
            if let Some((support, _)) = interaction::attachment_support(block, dependent)
                && support != changed
            {
                // A changing neighbor's shape callback removes a dependent
                // that is already unsupported at entry.
                if interaction::supports_attachment(probe.block(actor, support)?, support_face) {
                    continue;
                }
                return Err(GeometryError::Rejected(GeometryIssue::SupportLoss {
                    actor,
                    changed: support,
                    dependent,
                    block: block.get_id(),
                }));
            }
            // Like interaction::supports_dust, a retracting downward base keeps
            // its upper face; its moving-base view only arises from retraction.
            let keeps_dust = |id: PistonId, support: Block| {
                let owner = &actors[id.index()];
                matches!(block, Block::RedstoneWire { .. })
                    && matches!(support, Block::MovingPiston { .. })
                    && changed == owner.position
                    && face == BlockFace::Top
                    && owner.facing == BlockFace::Bottom
            };
            if alternatives.iter().any(|&(id, support)| {
                !interaction::supports_attachment(support, support_face) && !keeps_dust(id, support)
            }) {
                return Err(GeometryError::Rejected(GeometryIssue::SupportLoss {
                    actor,
                    changed,
                    dependent,
                    block: block.get_id(),
                }));
            }
        }
    }
    Ok(())
}

fn build_draft<W: World>(
    probe: &mut Probe<'_, W>,
    actors: &[Actor],
    resources: Vec<(BlockPos, Vec<BlockPos>, Vec<ResourceWitness>)>,
) -> Result<GeometryDraft, GeometryError> {
    let mut positions = Vec::new();
    let mut position_set = FxHashSet::default();
    for actor in actors {
        for position in [actor.position, actor.near, actor.far] {
            if position_set.insert(position) {
                positions.push(position);
            }
        }
    }
    for (_, contacts, _) in &resources {
        for &position in contacts {
            if position_set.insert(position) {
                positions.push(position);
            }
        }
    }

    let mut profiles = Vec::new();
    let mut profile_ids = FxHashMap::default();
    let mut profile = |block: Block| -> ProfileId {
        let raw_state = block.get_id();
        *profile_ids.entry(raw_state).or_insert_with(|| {
            let id = ProfileId::new(profiles.len() as u32);
            profiles.push(Profile {
                raw_state,
                registry_type: if matches!(block, Block::Unknown { .. }) {
                    u32::MAX
                } else {
                    block.registry_id()
                },
                solid: block.is_solid(),
                transparent: block.is_transparent(),
                cube: block.is_cube(),
                electrical: if matches!(block, Block::RedstoneBlock {}) {
                    ProfileElectrical::RedstoneSource
                } else {
                    ProfileElectrical::None
                },
            });
            id
        })
    };

    let mut nodes = Vec::with_capacity(actors.len() + positions.len());
    for actor in actors {
        nodes.push(CircuitNodeDraft {
            position: Some(actor.position),
            raw_state: None,
            kind: CircuitNodeKind::Piston(actor.id),
            state: ComponentState::Specialized,
            output_route: None,
        });
    }
    for (index, &position) in positions.iter().enumerate() {
        nodes.push(CircuitNodeDraft {
            position: Some(position),
            raw_state: None,
            kind: CircuitNodeKind::Contact(ContactId::new(index as u32)),
            state: ComponentState::Specialized,
            output_route: None,
        });
    }
    let contact_ids: FxHashMap<_, _> = positions
        .iter()
        .enumerate()
        .map(|(i, &pos)| (pos, ContactId::new(i as u32)))
        .collect();
    let mut variants: FxHashMap<BlockPos, Vec<GeometryVariant>> = FxHashMap::default();
    for actor in actors {
        add_variant(
            &mut variants,
            actor.position,
            GeometryView::PistonBase {
                piston: actor.id,
                extended: false,
            },
            profile(Block::Piston {
                piston: actor.piston.extend(false),
            }),
        );
        add_variant(
            &mut variants,
            actor.position,
            GeometryView::PistonBase {
                piston: actor.id,
                extended: true,
            },
            profile(Block::Piston {
                piston: actor.piston.extend(true),
            }),
        );
        let moving = Block::MovingPiston {
            moving: actor.piston.into(),
        };
        add_variant(
            &mut variants,
            actor.position,
            GeometryView::MovingPistonBase(actor.id),
            profile(moving),
        );
        let head = Block::PistonHead {
            head: actor.piston.into(),
        };
        add_variant(
            &mut variants,
            actor.near,
            GeometryView::MovingPistonHead(actor.id),
            profile(moving),
        );
        add_variant(
            &mut variants,
            actor.near,
            GeometryView::PistonHead(actor.id),
            profile(head),
        );
        add_variant(
            &mut variants,
            actor.near,
            GeometryView::Air,
            profile(Block::Air),
        );
    }
    for (index, (source, contacts, _)) in resources.iter().enumerate() {
        let resource = ResourceId::new(index as u32);
        let block = probe.block(actors[0].position, *source)?;
        for &position in contacts {
            add_variant(
                &mut variants,
                position,
                GeometryView::Resource(resource),
                profile(block),
            );
            add_variant(
                &mut variants,
                position,
                GeometryView::Air,
                profile(Block::Air),
            );
        }
    }
    for &position in &positions {
        let block = probe.block(actors.first().map_or(position, |a| a.position), position)?;
        variants.entry(position).or_default();
        if !variants[&position]
            .iter()
            .any(|variant| variant.profile == profile(block))
        {
            add_variant(&mut variants, position, GeometryView::Fixed, profile(block));
        }
    }
    let contacts = positions
        .iter()
        .enumerate()
        .map(|(index, &position)| GeometryContact {
            node: NodeId::new(actors.len() as u32 + index as u32),
            position,
            variants: variants.remove(&position).unwrap_or_default(),
        })
        .collect::<Vec<_>>();
    let pistons = actors
        .iter()
        .map(|actor| GeometryPiston {
            id: actor.id,
            node: actor.node,
            position: actor.position,
            facing: actor.facing,
            sticky: actor.piston.sticky,
            base: contact_ids[&actor.position],
            near: contact_ids[&actor.near],
            far: contact_ids[&actor.far],
            lane: if actor.piston.sticky {
                PistonLane::SingleResource {
                    far: contact_ids[&actor.far],
                }
            } else {
                PistonLane::Empty
            },
        })
        .collect();
    let initial_pistons = actors
        .iter()
        .map(|actor| PistonState {
            extended: actor.piston.extended,
            availability: PistonAvailability::Ready,
            revision: 0,
        })
        .collect();
    let geometry_resources = resources
        .into_iter()
        .map(|(source, positions, witnesses)| {
            let material = probe.block(witnesses[0].position, source)?;
            let contacts = positions
                .iter()
                .map(|position| contact_ids[position])
                .collect();
            let initial = ResourceState {
                at: contact_ids[&source],
                enabled: true,
                revision: 0,
            };
            Ok(GeometryResource {
                source,
                spec: ResourceSpec {
                    material: profile(material),
                    contacts,
                },
                initial,
                witnesses,
            })
        })
        .collect::<Result<Vec<_>, GeometryError>>()?;
    let initial_resources = geometry_resources
        .iter()
        .map(|resource| resource.initial)
        .collect();
    let mut observations = FxHashMap::default();
    for &position in &positions {
        for observed in
            std::iter::once(position).chain(BlockFace::values().map(|face| position.offset(face)))
        {
            let block = probe.block(position, observed)?;
            observations.entry(observed).or_insert(GeometryObservation {
                position: observed,
                profile: profile(block),
                contact: contact_ids.get(&observed).copied(),
            });
            if let Block::Observer { observer } = block {
                let watched = observed.offset(BlockFace::from(observer.facing));
                let watched_block = probe.block(position, watched)?;
                observations.entry(watched).or_insert(GeometryObservation {
                    position: watched,
                    profile: profile(watched_block),
                    contact: contact_ids.get(&watched).copied(),
                });
            }
        }
    }
    let mut observations: Vec<_> = observations.into_values().collect();
    observations.sort_by_key(|site| (site.position.y, site.position.z, site.position.x));
    Ok(GeometryDraft {
        remaining_cells: probe.budget - probe.reads,
        profiles,
        nodes,
        contacts,
        pistons,
        resources: geometry_resources,
        initial_pistons,
        initial_resources,
        observations,
    })
}

fn add_variant(
    variants: &mut FxHashMap<BlockPos, Vec<GeometryVariant>>,
    position: BlockPos,
    view: GeometryView,
    profile: ProfileId,
) {
    let entries = variants.entry(position).or_default();
    let candidate = GeometryVariant { view, profile };
    if !entries.contains(&candidate) {
        entries.push(candidate);
    }
}

fn classify_material<W: World>(
    actor: BlockPos,
    position: BlockPos,
    block: Block,
    world: &W,
) -> Result<(), GeometryError> {
    let name = block.get_name();
    let entity_free = !block.has_block_entity() && world.get_block_entity(position).is_none();
    // Registry wool/concrete/quartz states are opaque cubes without handlers;
    // modeled blocks qualify by the same facts. Anything with a callback,
    // comparator override, transparency, or entity keeps its own rejection.
    let modeled_inert = !matches!(block, Block::Unknown { .. })
        && block.is_solid()
        && !block.is_transparent()
        && !crate::redstone::has_neighbor_update(block)
        && !crate::redstone::comparator::has_override(block)
        && !matches!(block, Block::RedstoneBlock {});
    let inert_cube = block.is_cube()
        && (modeled_inert
            || name.ends_with("_wool")
            || name.ends_with("_concrete")
            || name.contains("quartz"));
    if entity_free && (matches!(block, Block::RedstoneBlock {}) || inert_cube) {
        Ok(())
    } else {
        Err(GeometryError::Rejected(
            GeometryIssue::UnsupportedMaterial {
                actor,
                position,
                block: block.get_id(),
            },
        ))
    }
}

fn is_container(block: Block) -> bool {
    ContainerType::from_block(block).is_some()
}
