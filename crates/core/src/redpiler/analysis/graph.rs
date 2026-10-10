//! Read-only graph preparation for ordinary, piston-free selections.
use super::{AdmissionIssue, AnalysisError, AnalysisReport};
use crate::redpiler::compile_graph::{CompileGraph, GraphError};
use crate::redpiler::{CompilerInput, CompilerOptions, TaskMonitor};
use crate::world::World;
use mchprs_blocks::BlockPos;
use mchprs_world::TickEntry;
use serde::Serialize;
use std::fmt;
use std::sync::Arc;

#[derive(Debug)]
pub enum GraphPreparationError {
    Analysis(AnalysisError),
    Entry(AdmissionIssue),
    UnsupportedExport,
    Graph(GraphError),
}

impl fmt::Display for GraphPreparationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Analysis(error) => error.fmt(f),
            Self::Entry(issue) => issue.fmt(f),
            Self::UnsupportedExport => {
                f.write_str("export flags are unavailable during read-only graph analysis")
            }
            Self::Graph(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for GraphPreparationError {}

#[derive(Debug, Serialize)]
pub struct GraphSummary {
    pub ordinary_nodes: usize,
    pub electrical_links: usize,
}

#[derive(Debug)]
pub struct CandidateGraph {
    pub(crate) graph: CompileGraph,
    pub report: AnalysisReport,
}

impl CandidateGraph {
    pub fn summary(&self) -> GraphSummary {
        GraphSummary {
            ordinary_nodes: self.graph.node_count(),
            electrical_links: self.graph.edge_count(),
        }
    }
}

/// Analyze again from the live world so an artifact cannot borrow stale report
/// identities after an edit. Nothing is transferred to the compiled backend.
pub fn prepare_candidate_graph(
    world: &impl World,
    bounds: (BlockPos, BlockPos),
    ticks: &[TickEntry],
    options: &CompilerOptions,
    monitor: Arc<TaskMonitor>,
) -> Result<CandidateGraph, GraphPreparationError> {
    if options.export {
        return Err(GraphPreparationError::UnsupportedExport);
    }
    monitor.set_budget_multiplier(options.budget_multiplier);
    let report = super::analyze(
        world,
        bounds,
        ticks,
        &monitor,
        super::AnalysisLimits::for_budget(monitor.budget_multiplier()),
    )
    .map_err(GraphPreparationError::Analysis)?;
    if let Some(issue) = report.issues.first() {
        return Err(GraphPreparationError::Entry(issue.clone()));
    }
    let input = CompilerInput {
        world,
        bounds: report.bounds,
        ticks,
    };
    let graph = crate::redpiler::passes::run_passes(options, &input, &monitor)
        .map_err(GraphPreparationError::Graph)?;
    if monitor.cancelled() {
        return Err(GraphPreparationError::Analysis(AnalysisError::Cancelled));
    }
    Ok(CandidateGraph { graph, report })
}
