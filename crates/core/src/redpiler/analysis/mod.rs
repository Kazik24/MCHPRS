//! Read-only inventory of live piston geometry and compiler admission reasons.
//!
//! Reset seeds are candidates, never executable certificates. In particular,
//! geometric recognition does not establish synchronization, waveform, reset
//! closure or a safe route back to the interpreter.
use super::TaskMonitor;
use crate::plot::PLOT_BLOCK_HEIGHT;
use crate::redstone::piston::should_piston_extend;
use crate::world::World;
use mchprs_blocks::blocks::{Block, RedstonePiston};
use mchprs_blocks::{BlockFace, BlockFacing, BlockPos};
use mchprs_world::{AdvancePhase, TickEntry};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::Serialize;
use std::fmt;

pub mod families;
pub mod graph;
pub mod ports;
pub mod topology;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy)]
pub struct AnalysisLimits {
    /// Count inspected cells in occupied sections, including air. Empty sections
    /// are skipped without allocating a dense whole-plot snapshot.
    pub max_cells: usize,
    pub max_pistons: usize,
    /// Shared budget for reset guards, wire walks and consumer searches.
    pub max_dependency_steps: usize,
}

impl Default for AnalysisLimits {
    fn default() -> Self {
        Self {
            max_cells: 16 * 1024 * 1024,
            max_pistons: 65_536,
            max_dependency_steps: 4 * 1024 * 1024,
        }
    }
}

impl AnalysisLimits {
    pub fn for_budget(multiplier: usize) -> Self {
        let multiplier = multiplier.clamp(1, 8);
        let base = Self::default();
        Self {
            max_cells: base.max_cells * multiplier,
            max_pistons: base.max_pistons * multiplier,
            max_dependency_steps: base.max_dependency_steps * multiplier,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalysisError {
    Cancelled,
    InvalidBounds,
    UnloadedChunk { x: i32, z: i32 },
    CellLimit,
    PistonLimit,
    DependencyLimit,
}

impl fmt::Display for AnalysisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str("analysis cancelled"),
            Self::InvalidBounds => {
                f.write_str("analysis bounds exceed world height or coordinate range")
            }
            Self::UnloadedChunk { x, z } => write!(f, "analysis requires loaded chunk {x}, {z}"),
            Self::CellLimit => f.write_str("analysis cell budget exceeded"),
            Self::PistonLimit => f.write_str("analysis piston budget exceeded"),
            Self::DependencyLimit => f.write_str("analysis dependency budget exceeded"),
        }
    }
}

impl std::error::Error for AnalysisError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ResetSeed {
    ObserverAbove,
    TorchNearby,
    DustBelowHead,
    DustNearby,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum PistonDiagnostic {
    OrdinaryPiston,
    RetractedEntry,
    UpwardFacing,
    MissingOrMismatchedHead,
    UnsupportedPayload,
    PayloadEntity,
    EntryPowerMismatch,
    ContextOutsideBounds,
    NonconductingObserverCap,
    PoweredObserverCap,
    NoResetSeed,
}

#[derive(Debug, Clone, Serialize)]
pub struct PistonDescriptor {
    pub pos: BlockPos,
    #[serde(serialize_with = "serialize_piston")]
    pub piston: RedstonePiston,
    pub powered: bool,
    pub head: BlockPos,
    pub payload: BlockPos,
    pub reset_seeds: Vec<ResetSeed>,
    pub diagnostics: Vec<PistonDiagnostic>,
}

fn serialize_piston<S: serde::Serializer>(
    piston: &RedstonePiston,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    #[derive(Serialize)]
    struct Properties {
        facing: BlockFace,
        sticky: bool,
        extended: bool,
    }
    Properties {
        facing: piston.facing.into(),
        sticky: piston.sticky,
        extended: piston.extended,
    }
    .serialize(serializer)
}

/// Shared possible payload positions require joint ownership analysis, even
/// when only one piston currently holds the block. No actuator is certified on
/// its own merely because it has an observer.
#[derive(Debug, Clone, Serialize)]
pub struct PayloadGroup {
    pub members: Vec<usize>,
    pub positions: Vec<BlockPos>,
}

#[derive(Debug, Clone, Serialize)]
pub enum AdmissionIssue {
    PistonRuntimeUnavailable { pos: BlockPos },
    ObserverRuntimeUnavailable { pos: BlockPos },
    UnownedPistonHead { pos: BlockPos },
    MovingPiston { pos: BlockPos },
    EntryPhase { phase: AdvancePhase },
    PendingPistonEvents { count: usize },
    PendingPistonMotions { count: usize },
    PendingMovementWork { count: usize },
}

impl fmt::Display for AdmissionIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PistonRuntimeUnavailable { pos } => {
                write!(f, "piston at {pos:?} needs a validated instant runtime")
            }
            Self::ObserverRuntimeUnavailable { pos } => {
                write!(f, "observer at {pos:?} needs an execution owner")
            }
            Self::UnownedPistonHead { pos } => {
                write!(f, "piston head at {pos:?} has no matching extended base")
            }
            Self::MovingPiston { pos } => {
                write!(f, "moving piston at {pos:?} requires the interpreter")
            }
            Self::EntryPhase { phase } => write!(f, "entry phase {phase:?} is not between ticks"),
            Self::PendingPistonEvents { count } => write!(f, "{count} pending piston events"),
            Self::PendingPistonMotions { count } => write!(f, "{count} active piston motions"),
            Self::PendingMovementWork { count } => write!(f, "{count} pending movement work items"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalysisReport {
    pub schema_version: u32,
    pub bounds: (BlockPos, BlockPos),
    pub inspected_cells: usize,
    pub nonair_blocks: usize,
    pub pending_ticks: usize,
    pub pistons: Vec<PistonDescriptor>,
    pub payload_groups: Vec<PayloadGroup>,
    pub recognition: Vec<families::PistonRecognition>,
    pub group_recognition: Vec<families::GroupRecognition>,
    pub ports: ports::PortReport,
    pub dependency_steps: usize,
    pub observers: Vec<BlockPos>,
    pub issues: Vec<AdmissionIssue>,
}

impl AnalysisReport {
    pub fn can_compile(&self) -> bool {
        self.issues.is_empty()
    }

    pub fn summary(&self) -> String {
        let candidates = self
            .pistons
            .iter()
            .filter(|p| !p.reset_seeds.is_empty())
            .count();
        format!(
            "{} pistons, {} reset candidates, {} payload groups, {} observers; {} region validation requirements",
            self.pistons.len(), candidates, self.payload_groups.len(), self.observers.len(), self.issues.len()
        )
    }

    pub fn recognition_summary(&self) -> String {
        format!("{} matched reset mechanisms, {} groups with reset closure, {} ordinary consumer interfaces; joint-wave validation runs during compilation",
            self.recognition.iter().filter(|p| p.is_matched()).count(),
            self.group_recognition.iter().filter(|g| g.has_reset_closure()).count(),
            self.ports.outputs.len())
    }
}

fn contains(bounds: (BlockPos, BlockPos), pos: BlockPos) -> bool {
    pos.x >= bounds.0.x
        && pos.x <= bounds.1.x
        && pos.y >= bounds.0.y
        && pos.y <= bounds.1.y
        && pos.z >= bounds.0.z
        && pos.z <= bounds.1.z
}

/// Read-only, bounded and cancellable. Pending ticks are borrowed, not consumed.
pub fn analyze(
    world: &impl World,
    bounds: (BlockPos, BlockPos),
    ticks: &[TickEntry],
    monitor: &TaskMonitor,
    limits: AnalysisLimits,
) -> Result<AnalysisReport, AnalysisError> {
    let bounds = (bounds.0.min(bounds.1), bounds.0.max(bounds.1));
    if bounds.0.y < 0
        || bounds.1.y >= PLOT_BLOCK_HEIGHT
        || bounds.0.x <= i32::MIN + 16
        || bounds.1.x >= i32::MAX - 16
        || bounds.0.z <= i32::MIN + 16
        || bounds.1.z >= i32::MAX - 16
    {
        return Err(AnalysisError::InvalidBounds);
    }
    monitor.set_message("Analyzing live piston geometry".into());
    let mut report = AnalysisReport {
        schema_version: 3,
        bounds,
        inspected_cells: 0,
        nonair_blocks: 0,
        pending_ticks: ticks.len(),
        pistons: Vec::new(),
        payload_groups: Vec::new(),
        recognition: Vec::new(),
        group_recognition: Vec::new(),
        ports: Default::default(),
        dependency_steps: 0,
        observers: Vec::new(),
        issues: Vec::new(),
    };
    let mut heads = Vec::new();
    let mut consumers = Vec::new();
    // Iterate sections rather than allocating all cells or scanning unused
    // palette entries. Cancellation also applies to empty-section traversal.
    for chunk_x in bounds.0.x.div_euclid(16)..=bounds.1.x.div_euclid(16) {
        for chunk_z in bounds.0.z.div_euclid(16)..=bounds.1.z.div_euclid(16) {
            if monitor.cancelled() {
                return Err(AnalysisError::Cancelled);
            }
            let chunk = world
                .get_chunk(chunk_x, chunk_z)
                .ok_or(AnalysisError::UnloadedChunk {
                    x: chunk_x,
                    z: chunk_z,
                })?;
            for section_y in bounds.0.y / 16..=bounds.1.y / 16 {
                if chunk.sections[section_y as usize].block_count() == 0 {
                    continue;
                }
                let origin = BlockPos::new(chunk_x * 16, section_y * 16, chunk_z * 16);
                let first = bounds.0.max(origin);
                let last = bounds.1.min(origin + BlockPos::new(15, 15, 15));
                for y in first.y..=last.y {
                    if monitor.cancelled() {
                        return Err(AnalysisError::Cancelled);
                    }
                    for z in first.z..=last.z {
                        for x in first.x..=last.x {
                            if report.inspected_cells >= limits.max_cells {
                                return Err(AnalysisError::CellLimit);
                            }
                            report.inspected_cells += 1;
                            let pos = BlockPos::new(x, y, z);
                            let block = world.get_block(pos);
                            if block == Block::Air {
                                continue;
                            }
                            report.nonair_blocks += 1;
                            if ports::is_consumer(block) {
                                consumers.push((pos, block));
                            }
                            match block {
                                Block::Piston { piston } => {
                                    if report.pistons.len() >= limits.max_pistons {
                                        return Err(AnalysisError::PistonLimit);
                                    }
                                    report.pistons.push(describe(world, bounds, pos, piston));
                                    report
                                        .issues
                                        .push(AdmissionIssue::PistonRuntimeUnavailable { pos });
                                }
                                Block::PistonHead { head } => heads.push((pos, head)),
                                Block::MovingPiston { .. } => {
                                    report.issues.push(AdmissionIssue::MovingPiston { pos })
                                }
                                Block::Observer { .. } => {
                                    report.observers.push(pos);
                                    report
                                        .issues
                                        .push(AdmissionIssue::ObserverRuntimeUnavailable { pos });
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }
    report.pistons.sort_by_key(|p| (p.pos.y, p.pos.z, p.pos.x));
    let owned_heads: FxHashSet<_> = report
        .pistons
        .iter()
        .filter(|p| p.piston.extended)
        .map(|p| (p.head, BlockFace::from(p.piston.facing), p.piston.sticky))
        .collect();
    for (pos, head) in heads {
        if !owned_heads.contains(&(pos, BlockFace::from(head.facing), head.sticky)) {
            report
                .issues
                .push(AdmissionIssue::UnownedPistonHead { pos });
        }
    }
    report.payload_groups = payload_groups(&report.pistons);
    if !report.pistons.is_empty() {
        let mobile = report
            .payload_groups
            .iter()
            .enumerate()
            .filter(|(_, group)| {
                group
                    .positions
                    .iter()
                    .any(|&pos| world.get_block(pos) == Block::RedstoneBlock)
            })
            .flat_map(|(index, group)| group.positions.iter().map(move |&pos| (pos, index)))
            .collect();
        let mut topology =
            topology::Topology::new(world, bounds, monitor, limits.max_dependency_steps, mobile);
        (report.recognition, report.group_recognition) = families::recognize(
            &mut topology,
            &report.pistons,
            &report.payload_groups,
            ticks,
        )?;
        report.ports = ports::discover(
            &mut topology,
            &report.pistons,
            &report.recognition,
            &report.payload_groups,
            &consumers,
        )?;
        report.dependency_steps = topology.visited;
    }
    let state = world.piston_state();
    if state.phase != AdvancePhase::BetweenTicks {
        report
            .issues
            .push(AdmissionIssue::EntryPhase { phase: state.phase });
    }
    if !state.events.is_empty() {
        report.issues.push(AdmissionIssue::PendingPistonEvents {
            count: state.events.len(),
        });
    }
    if !state.motions.is_empty() {
        report.issues.push(AdmissionIssue::PendingPistonMotions {
            count: state.motions.len(),
        });
    }
    if state.movement_cursor < state.movement_work.len() {
        report.issues.push(AdmissionIssue::PendingMovementWork {
            count: state.movement_work.len() - state.movement_cursor,
        });
    }
    if monitor.cancelled() {
        return Err(AnalysisError::Cancelled);
    }
    Ok(report)
}

fn describe(
    world: &impl World,
    bounds: (BlockPos, BlockPos),
    pos: BlockPos,
    piston: RedstonePiston,
) -> PistonDescriptor {
    let face: BlockFace = piston.facing.into();
    let head = pos.offset(face);
    let payload = if piston.extended {
        head.offset(face)
    } else {
        head
    };
    let powered = should_piston_extend(world, piston.facing, pos);
    let mut p = PistonDescriptor {
        pos,
        piston,
        powered,
        head,
        payload,
        reset_seeds: Vec::new(),
        diagnostics: Vec::new(),
    };
    if !piston.sticky {
        p.diagnostics.push(PistonDiagnostic::OrdinaryPiston);
    }
    if !piston.extended {
        p.diagnostics.push(PistonDiagnostic::RetractedEntry);
    } else if !matches!(world.get_block(head), Block::PistonHead { head: h }
        if h.facing == piston.facing && h.sticky == piston.sticky && !h.short)
    {
        p.diagnostics
            .push(PistonDiagnostic::MissingOrMismatchedHead);
    }
    if piston.facing == BlockFacing::Up {
        p.diagnostics.push(PistonDiagnostic::UpwardFacing);
    }
    if !crate::redpiler::instant::outputs::supported_payload(world.get_block(payload)) {
        p.diagnostics.push(PistonDiagnostic::UnsupportedPayload);
    }
    if world.get_block_entity(payload).is_some() {
        p.diagnostics.push(PistonDiagnostic::PayloadEntity);
    }
    if powered != piston.extended {
        p.diagnostics.push(PistonDiagnostic::EntryPowerMismatch);
    }
    // The two-cell context is only a seed search. Later closure must follow
    // electrical and update influence beyond it; it cannot certify a family.
    if !contains(bounds, pos - BlockPos::new(2, 2, 2))
        || !contains(bounds, pos + BlockPos::new(2, 2, 2))
        || !contains(bounds, payload)
    {
        p.diagnostics.push(PistonDiagnostic::ContextOutsideBounds);
    }
    if piston.sticky {
        let above = pos.offset(BlockFace::Top);
        if matches!(world.get_block(above), Block::Observer { observer }
            if observer.facing == BlockFacing::Down)
        {
            let cap = world.get_block(above.offset(BlockFace::Top));
            if cap == Block::RedstoneBlock {
                p.diagnostics.push(PistonDiagnostic::PoweredObserverCap);
            }
            if !cap.is_solid() {
                p.diagnostics
                    .push(PistonDiagnostic::NonconductingObserverCap);
            } else {
                p.reset_seeds.push(ResetSeed::ObserverAbove);
            }
        }
        let under_head = head.offset(BlockFace::Bottom);
        if matches!(world.get_block(under_head), Block::RedstoneWire { .. })
            && world
                .get_block(under_head.offset(BlockFace::Bottom))
                .is_solid()
        {
            p.reset_seeds.push(ResetSeed::DustBelowHead);
        }
        let mut torch = false;
        let mut dust = false;
        for dy in 0..=2 {
            for dz in -1..=1 {
                for dx in -1..=1 {
                    let q = pos + BlockPos::new(dx, dy, dz);
                    match world.get_block(q) {
                        Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. } => {
                            torch = true
                        }
                        Block::RedstoneWire { .. } if dy > 0 => dust = true,
                        _ => {}
                    }
                }
            }
        }
        if torch {
            p.reset_seeds.push(ResetSeed::TorchNearby);
        }
        if dust {
            p.reset_seeds.push(ResetSeed::DustNearby);
        }
    }
    if p.reset_seeds.is_empty() {
        p.diagnostics.push(PistonDiagnostic::NoResetSeed);
    }
    p
}

fn payload_groups(pistons: &[PistonDescriptor]) -> Vec<PayloadGroup> {
    // Linear-size union-find over possible near/far payload positions. Avoid an
    // O(pistons^2) pairwise search on adders and long chains.
    let mut parent: Vec<_> = (0..pistons.len()).collect();
    let mut rank = vec![0u8; pistons.len()];
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let mut owners = FxHashMap::default();
    for (i, p) in pistons.iter().enumerate() {
        for q in [p.head, p.head.offset(p.piston.facing.into())] {
            if let Some(&j) = owners.get(&q) {
                let a = root(&mut parent, i);
                let b = root(&mut parent, j);
                if a != b {
                    if rank[a] < rank[b] {
                        parent[a] = b;
                    } else {
                        parent[b] = a;
                        if rank[a] == rank[b] {
                            rank[a] += 1;
                        }
                    }
                }
            } else {
                owners.insert(q, i);
            }
        }
    }
    let mut groups = Vec::<PayloadGroup>::new();
    let mut indexes = FxHashMap::default();
    for (i, p) in pistons.iter().enumerate() {
        let r = root(&mut parent, i);
        let group = *indexes.entry(r).or_insert_with(|| {
            groups.push(PayloadGroup {
                members: Vec::new(),
                positions: Vec::new(),
            });
            groups.len() - 1
        });
        groups[group].members.push(i);
        groups[group]
            .positions
            .extend([p.head, p.head.offset(p.piston.facing.into())]);
    }
    for group in &mut groups {
        group.positions.sort_by_key(|p| (p.y, p.z, p.x));
        group.positions.dedup();
    }
    groups
}
