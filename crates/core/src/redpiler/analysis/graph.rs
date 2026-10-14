//! Read-only inspection of the same prepared graph artifact used by execution.
use super::{AdmissionIssue, AnalysisError, AnalysisReport};
use crate::redpiler::passes::{self, rich::PreparationError};
use crate::redpiler::program_graph::CompileGraph;
use crate::redpiler::{CompilerOptions, TaskMonitor};
use crate::world::World;
use mchprs_blocks::BlockPos;
use mchprs_world::TickEntry;
use serde::Serialize;
use std::fmt;
use std::sync::Arc;

#[derive(Debug)]
/// Why a candidate graph could not be prepared for inspection.
pub enum GraphPreparationError {
    Analysis(AnalysisError),
    Entry(AdmissionIssue),
    Graph(String),
}

impl fmt::Display for GraphPreparationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Analysis(error) => error.fmt(f),
            Self::Entry(issue) => issue.fmt(f),
            Self::Graph(error) => f.write_str(error),
        }
    }
}
impl std::error::Error for GraphPreparationError {}

#[derive(Debug, Serialize)]
/// Counts of nodes and links in a prepared graph.
pub struct GraphSummary {
    pub nodes: usize,
    pub links: usize,
}

#[derive(Debug)]
/// Prepared graph and admission report, available for inspection before execution.
pub struct CandidateGraph {
    pub(crate) graph: CompileGraph,
    pub report: AnalysisReport,
}

impl CandidateGraph {
    pub fn summary(&self) -> GraphSummary {
        GraphSummary {
            nodes: self.graph.nodes.len(),
            links: self.graph.links.len() + self.graph.power_links.len() + self.graph.wire_relations.len(),
        }
    }
}

/// Rebuild the live selection so inspection cannot reuse stale parser identities.
pub fn prepare_candidate_graph(
    world: &impl World,
    bounds: (BlockPos, BlockPos),
    ticks: &[TickEntry],
    options: &CompilerOptions,
    monitor: Arc<TaskMonitor>,
) -> Result<CandidateGraph, GraphPreparationError> {
    monitor.set_budget_multiplier(options.budget_multiplier);
    let mut prepared = passes::rich::prepare(
        world,
        bounds,
        ticks.to_vec(),
        &monitor,
        super::AnalysisLimits::for_budget(monitor.budget_multiplier()),
    )
    .map_err(map_preparation_error)?;
    let pipeline = passes::default_graph_pipeline(options);
    passes::run_graph_pipeline(&mut prepared.graph, options, &prepared.entry, &pipeline, &monitor)
        .map_err(|error| GraphPreparationError::Graph(error.to_string()))?;
    prepared.finish_pipeline_statistics(&monitor);
    Ok(CandidateGraph {
        graph: prepared.graph,
        report: prepared.report,
    })
}

fn map_preparation_error(error: PreparationError) -> GraphPreparationError {
    match error {
        PreparationError::Analysis(error) => GraphPreparationError::Analysis(error),
        PreparationError::Geometry(error) => match error {
            crate::redpiler::analysis::geometry::GeometryError::Entry(issue) => GraphPreparationError::Entry(issue),
            error => GraphPreparationError::Graph(error.to_string()),
        },
        PreparationError::Electrical(error) => GraphPreparationError::Graph(error.to_string()),
        PreparationError::Callbacks(error) => GraphPreparationError::Graph(error.to_string()),
    }
}
