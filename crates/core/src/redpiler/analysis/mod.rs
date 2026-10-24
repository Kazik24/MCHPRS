//! Inspect the selection and check whether it can be compiled.
//!
//! Geometry, electrical discovery, and callback construction build the graph.
//! [`graph::prepare_candidate_graph`] also runs the selected passes, without
//! starting the circuit. See the [redpiler overview](crate::redpiler).
use super::TaskMonitor;
use crate::plot::PLOT_BLOCK_HEIGHT;
use crate::world::World;
use mchprs_blocks::blocks::Block;
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_world::{AdvancePhase, TickEntry};
use rustc_hash::FxHashSet;
use serde::Serialize;
use std::fmt;

pub(crate) mod callbacks;
pub(crate) mod electrical;
#[cfg(test)]
pub(crate) mod export_json;
pub(crate) mod geometry;
pub mod graph;
mod guard_table;
pub(crate) mod instant;
pub(crate) mod instant_regions;
pub(crate) mod instant_resets;

// shortcut: excludes maps and allocator overhead, upgrade if measured RSS exceeds the table estimate.
pub(crate) const MAX_GRAPH_PREPARATION_BYTES: usize = 1024 * 1024 * 1024;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy)]
/// Limits the world scan and piston expansion performed during analysis.
pub struct AnalysisLimits {
    /// Bound inspected cells (including air) and subsequent graph preparation work.
    pub max_cells: usize,
    pub max_pistons: usize,
}

impl Default for AnalysisLimits {
    fn default() -> Self {
        Self {
            max_cells: 16 * 1024 * 1024,
            max_pistons: 65_536,
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
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// A failure that prevents bounded analysis or graph preparation from completing.
pub enum AnalysisError {
    Cancelled,
    InvalidBounds,
    UnloadedChunk {
        x: i32,
        z: i32,
    },
    CellLimit,
    PistonLimit,
    PreparationMemoryLimit {
        stage: &'static str,
        estimated_bytes: usize,
        limit_bytes: usize,
    },
}

impl fmt::Display for AnalysisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str("analysis cancelled"),
            Self::InvalidBounds => f.write_str("analysis bounds exceed world height or coordinate range"),
            Self::UnloadedChunk { x, z } => write!(f, "analysis requires loaded chunk {x}, {z}"),
            Self::CellLimit => f.write_str("analysis cell budget exceeded"),
            Self::PistonLimit => f.write_str("analysis piston budget exceeded"),
            Self::PreparationMemoryLimit {
                stage,
                estimated_bytes,
                limit_bytes,
            } => write!(
                f,
                "Graph {stage} table estimate ({estimated_bytes} bytes) exceeds the {limit_bytes}-byte preparation limit"
            ),
        }
    }
}

impl std::error::Error for AnalysisError {}

/// One work budget follows preparation across reads and graph expansion.
pub(crate) struct PreparationBudget<'a> {
    monitor: &'a TaskMonitor,
    pub(crate) remaining: usize,
}

impl<'a> PreparationBudget<'a> {
    pub(crate) fn new(monitor: &'a TaskMonitor, remaining: usize) -> Self {
        Self { monitor, remaining }
    }

    pub(crate) fn check(&self) -> Result<(), AnalysisError> {
        if self.monitor.cancelled() {
            return Err(AnalysisError::Cancelled);
        }
        Ok(())
    }

    pub(crate) fn consume(&mut self, work: usize) -> Result<(), AnalysisError> {
        self.check()?;
        self.remaining = self.remaining.checked_sub(work).ok_or(AnalysisError::CellLimit)?;
        Ok(())
    }

    pub(crate) fn check_estimated_bytes(&self, stage: &'static str, estimated_bytes: usize) -> Result<(), AnalysisError> {
        self.check()?;
        if estimated_bytes > MAX_GRAPH_PREPARATION_BYTES {
            return Err(AnalysisError::PreparationMemoryLimit {
                stage,
                estimated_bytes,
                limit_bytes: MAX_GRAPH_PREPARATION_BYTES,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
/// A condition that makes the selected world state unsuitable for graph compilation.
pub enum AdmissionIssue {
    /// A block is recognized but its graph behavior is not implemented.
    UnsupportedComponent { pos: BlockPos, block: u32, reason: &'static str },
    /// An extended piston has no head at the expected position.
    MissingPistonHead { piston: BlockPos, head: BlockPos },
    /// A piston head in the selection does not belong to an extended piston.
    UnownedPistonHead { pos: BlockPos },
    /// A piston is mid-animation and must be handled by the interpreter.
    MovingPiston { pos: BlockPos },
    /// The selection was inspected while the world was not between ticks.
    EntryPhase { phase: AdvancePhase },
    /// Piston events are queued at the compilation boundary.
    PendingPistonEvents { count: usize },
    /// Piston motions are active at the compilation boundary.
    PendingPistonMotions { count: usize },
    /// Piston movement work remains unfinished at the compilation boundary.
    PendingMovementWork { count: usize },
}

impl fmt::Display for AdmissionIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedComponent { pos, block, reason } => {
                write!(f, "{} at {pos:?} is unsupported: {reason}", Block::from_id(*block).get_name())
            }
            Self::MissingPistonHead { piston, head } => write!(f, "extended piston at {piston:?} is missing its head at {head:?}"),
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

pub(crate) fn unsupported_component_reason(block: Block) -> Option<&'static str> {
    if block.get_name().ends_with("_trapdoor") {
        Some("trapdoor state and redstone behavior are not implemented in Graph")
    } else {
        None
    }
}

#[derive(Debug, Clone, Serialize)]
/// Results of scanning a selection for graph compilation support and limits.
pub struct AnalysisReport {
    /// Version of the serialized report format.
    pub schema_version: u32,
    pub bounds: (BlockPos, BlockPos),
    pub inspected_cells: usize,
    pub nonair_blocks: usize,
    pub pending_ticks: usize,
    pub pistons: Vec<BlockPos>,
    pub issues: Vec<AdmissionIssue>,
}

impl AnalysisReport {
    pub fn can_compile(&self) -> bool {
        self.issues.is_empty()
    }

    pub fn summary(&self) -> String {
        format!(
            "{} non-air blocks, {} pistons, {} pending ticks; {} admission issues",
            self.nonair_blocks,
            self.pistons.len(),
            self.pending_ticks,
            self.issues.len()
        )
    }
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
    monitor.set_message("Analyzing selection for compilation".into());
    let mut report = AnalysisReport {
        schema_version: 4,
        bounds,
        inspected_cells: 0,
        nonair_blocks: 0,
        pending_ticks: ticks.len(),
        pistons: Vec::new(),
        issues: Vec::new(),
    };
    let mut heads = Vec::new();
    let mut owned_heads = FxHashSet::default();
    for chunk_x in bounds.0.x.div_euclid(16)..=bounds.1.x.div_euclid(16) {
        for chunk_z in bounds.0.z.div_euclid(16)..=bounds.1.z.div_euclid(16) {
            if monitor.cancelled() {
                return Err(AnalysisError::Cancelled);
            }
            let chunk = world
                .get_chunk(chunk_x, chunk_z)
                .ok_or(AnalysisError::UnloadedChunk { x: chunk_x, z: chunk_z })?;
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
                            if let Some(reason) = unsupported_component_reason(block) {
                                report.issues.push(AdmissionIssue::UnsupportedComponent {
                                    pos,
                                    block: block.get_id(),
                                    reason,
                                });
                            }
                            match block {
                                Block::Air => continue,
                                Block::Piston { piston } => {
                                    if report.pistons.len() >= limits.max_pistons {
                                        return Err(AnalysisError::PistonLimit);
                                    }
                                    report.pistons.push(pos);
                                    if piston.extended {
                                        owned_heads.insert((
                                            pos.offset(BlockFace::from(piston.facing)),
                                            BlockFace::from(piston.facing),
                                            piston.sticky,
                                        ));
                                    }
                                }
                                Block::PistonHead { head } => heads.push((pos, head)),
                                Block::MovingPiston { .. } => report.issues.push(AdmissionIssue::MovingPiston { pos }),
                                _ => {}
                            }
                            report.nonair_blocks += 1;
                        }
                    }
                }
            }
        }
    }
    report.pistons.sort_by_key(|p| (p.y, p.z, p.x));
    for (pos, head) in heads {
        if !owned_heads.contains(&(pos, BlockFace::from(head.facing), head.sticky)) {
            report.issues.push(AdmissionIssue::UnownedPistonHead { pos });
        }
    }
    let state = world.piston_state();
    if state.phase != AdvancePhase::BetweenTicks {
        report.issues.push(AdmissionIssue::EntryPhase { phase: state.phase });
    }
    if !state.events.is_empty() {
        report.issues.push(AdmissionIssue::PendingPistonEvents { count: state.events.len() });
    }
    if !state.motions.is_empty() {
        report.issues.push(AdmissionIssue::PendingPistonMotions { count: state.motions.len() });
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
