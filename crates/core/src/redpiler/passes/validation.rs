//! Required validation stages, independent of optimization flags and developer pass ordering.
//!
//! Rewrites can change the circuit and require a full check. Builders borrow the
//! circuit immutably and publish typed products, so checks follow the actual changed
//! product and its dependents. Unreviewed ownership changes retain complete checks.
use super::{Pass, PassContext, PassPipelineError};
use crate::redpiler::TaskMonitor;
use crate::redpiler::program_graph::CompileGraph;
use std::time::{Duration, Instant};

#[cfg(test)]
mod tests;

pub(super) fn input(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<(), PassPipelineError> {
    graph.validate_for_lowering(monitor).map_err(PassPipelineError::InvalidInput)
}

pub(super) fn run_checked(pass: &Pass, graph: &mut CompileGraph, context: &PassContext<'_>) -> Result<Duration, PassPipelineError> {
    let name = pass.name();
    let failed = |message| PassPipelineError::Failed { pass: name, message };
    let malformed = |source| PassPipelineError::MalformedOutput { pass: name, source };
    let validation_start;
    match pass {
        Pass::Rewrite(rewrite) => {
            rewrite.run(graph, context).map_err(failed)?;
            validation_start = Instant::now();
            graph.validate_for_lowering(context.monitor).map_err(malformed)?;
        }
        Pass::Build(builder) => {
            let output = builder.build(graph, context).map_err(failed)?;
            if context.monitor.cancelled() {
                return Err(PassPipelineError::Cancelled);
            }
            let change = output.publish(graph).map_err(malformed)?;
            validation_start = Instant::now();
            graph.validate_plan_change(change, context.monitor).map_err(malformed)?;
        }
    }
    Ok(validation_start.elapsed())
}
