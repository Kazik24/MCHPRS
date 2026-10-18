//! Lower graph-derived stable-input recurrences into immutable Boolean phase programs.
use super::PassContext;
use super::planning::{PlanBuilder, PlanOutput};
use crate::redpiler::program_graph::CompileGraph;

pub(crate) struct CompileSymbolicKernels;

impl PlanBuilder for CompileSymbolicKernels {
    fn name(&self) -> &'static str {
        "compile-symbolic-kernels"
    }

    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        if !context.options.stable_instant_inputs || !context.entry.pending_ticks.is_empty() || !graph.plans.instant.regions.is_empty() || !graph.plans.classic_nets.is_empty() {
            return Ok(PlanOutput::SymbolicKernels(Vec::new()));
        }
        let kernels = crate::redpiler::analysis::instant_regions::compiler::aot::compile(graph, context.monitor)?;
        Ok(PlanOutput::SymbolicKernels(kernels))
    }
}
