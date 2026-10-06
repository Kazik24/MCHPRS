//! Read-only graph preparation for recognized geometry. The result is a
//! candidate artifact, not authorization to execute a piston region.
use super::families::{GroupFailure, RecognitionFailure};
use super::{AdmissionIssue, AnalysisError, AnalysisReport};
use crate::redpiler::compile_graph::{CompileGraph, GraphError, NodeType};
use crate::redpiler::instant::boundary::Boundaries;
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
    Piston {
        pos: BlockPos,
        failures: Vec<RecognitionFailure>,
    },
    PayloadGroup {
        group: usize,
        failures: Vec<GroupFailure>,
    },
    Entry(AdmissionIssue),
    ExposedReset {
        source: BlockPos,
        consumer: BlockPos,
    },
    PortOutsideBounds {
        consumer: BlockPos,
        pos: BlockPos,
    },
    UnsupportedExport,
    Graph(GraphError),
}

impl fmt::Display for GraphPreparationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Analysis(error) => error.fmt(f),
            Self::Piston { pos, failures } => {
                write!(f, "piston at {pos:?} cannot enter the candidate graph: ")?;
                match failures.first() {
                    Some(reason) => reason.fmt(f)?,
                    None => f.write_str("no supported reset mechanism")?,
                }
                if failures.len() > 1 {
                    write!(f, " ({} failed checks)", failures.len())?;
                }
                Ok(())
            }
            Self::PayloadGroup { group, failures } => {
                write!(f, "payload group {group} lacks reset closure: ")?;
                match failures.first() {
                    Some(reason) => reason.fmt(f)?,
                    None => f.write_str("no verified reset supply")?,
                }
                if failures.len() > 1 {
                    write!(f, " ({} failed checks)", failures.len())?;
                }
                Ok(())
            }
            Self::Entry(issue) => issue.fmt(f),
            Self::ExposedReset { source, consumer } => write!(
                f,
                "reset signal at {source:?} feeds context outside its payload group at {consumer:?}"
            ),
            Self::PortOutsideBounds { consumer, pos } => write!(
                f,
                "consumer {consumer:?} needs context outside the selection at {pos:?}"
            ),
            Self::UnsupportedExport => {
                f.write_str("instant candidate graph export is not implemented")
            }
            Self::Graph(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for GraphPreparationError {}

#[derive(Debug, Serialize)]
pub struct GraphSummary {
    pub ordinary_nodes: usize,
    pub instant_inputs: usize,
    pub mobile_sources: usize,
    pub electrical_links: usize,
}

#[derive(Debug)]
pub struct CandidateGraph {
    pub(crate) graph: CompileGraph,
    pub report: AnalysisReport,
}

impl CandidateGraph {
    pub fn summary(&self) -> GraphSummary {
        let mut result = GraphSummary {
            ordinary_nodes: 0,
            instant_inputs: 0,
            mobile_sources: 0,
            electrical_links: self.graph.edge_count(),
        };
        for node in self.graph.node_weights() {
            match node.ty {
                NodeType::InstantInput { .. } => result.instant_inputs += 1,
                NodeType::MobileSource { .. } => result.mobile_sources += 1,
                _ => result.ordinary_nodes += 1,
            }
        }
        result
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
    let report = super::analyze(world, bounds, ticks, &monitor, Default::default())
        .map_err(GraphPreparationError::Analysis)?;
    for issue in &report.issues {
        if !matches!(
            issue,
            AdmissionIssue::PistonRuntimeUnavailable { .. }
                | AdmissionIssue::ObserverRuntimeUnavailable { .. }
        ) {
            return Err(GraphPreparationError::Entry(issue.clone()));
        }
    }
    for r in &report.recognition {
        if !r.is_matched() {
            return Err(GraphPreparationError::Piston {
                pos: report.pistons[r.piston].pos,
                failures: r.failures.clone(),
            });
        }
    }
    for r in &report.group_recognition {
        if !r.has_reset_closure() {
            return Err(GraphPreparationError::PayloadGroup {
                group: r.group,
                failures: r.failures.clone(),
            });
        }
    }
    let boundaries = Boundaries::new(&report);
    for port in &report.ports.pistons {
        if let Some(&pos) = port.outside_bounds.first() {
            return Err(GraphPreparationError::Piston {
                pos: report.pistons[port.piston].pos,
                failures: vec![RecognitionFailure::OutsideBounds { pos }],
            });
        }
    }
    if let Some(exposure) = report.ports.reset_exposures.first() {
        return Err(GraphPreparationError::ExposedReset {
            source: exposure.source,
            consumer: exposure.consumer,
        });
    }
    for output in &report.ports.outputs {
        if let Some(&pos) = output.dependencies.outside_bounds.first() {
            return Err(GraphPreparationError::PortOutsideBounds {
                consumer: output.consumer,
                pos,
            });
        }
    }
    // Standalone observers have no region owner, even if every piston matches.
    for &pos in &report.observers {
        if !boundaries.is_internal(pos) {
            return Err(GraphPreparationError::Entry(
                AdmissionIssue::ObserverRuntimeUnavailable { pos },
            ));
        }
    }
    let input = CompilerInput {
        world,
        bounds: report.bounds,
        ticks,
        boundaries: Some(&boundaries),
    };
    let graph = crate::redpiler::passes::make_default_pass_manager()
        .run_passes(options, &input, monitor.clone())
        .map_err(GraphPreparationError::Graph)?;
    if monitor.cancelled() {
        return Err(GraphPreparationError::Analysis(AnalysisError::Cancelled));
    }
    Ok(CandidateGraph { graph, report })
}
