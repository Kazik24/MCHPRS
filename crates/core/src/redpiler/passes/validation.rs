//! Required validation stages, independent of optimization flags and developer pass ordering.
//!
//! Rewrites can change the circuit and require a full check. Builders borrow the
//! circuit immutably and publish only derived plans, so their output needs all plan
//! checks against the already validated structure. Lowering keeps its full check.
use super::{Pass, PassPipelineError};
use crate::redpiler::TaskMonitor;
use crate::redpiler::program_graph::{CompileGraph, GraphBuildError};

#[cfg(test)]
mod tests;

pub(super) fn input(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<(), PassPipelineError> {
    graph.validate_with_monitor(monitor).map_err(PassPipelineError::InvalidInput)
}

pub(super) fn after_pass(pass: &Pass, graph: &CompileGraph, monitor: &TaskMonitor) -> Result<(), GraphBuildError> {
    match pass {
        Pass::Rewrite(_) => graph.validate_with_monitor(monitor),
        Pass::Build(_) => graph.validate_execution_plans_with_monitor(monitor),
    }
}
