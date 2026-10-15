//! Compile one boundary-driven event program per owned instant region.
//! Actor and reset transitions reuse Graph's state owners and ordered scheduler primitives.
use super::PassContext;
use super::planning::{PlanBuilder, PlanOutput};
use crate::redpiler::program_graph::CompileGraph;

pub(crate) struct CompileSymbolicRegions;

impl PlanBuilder for CompileSymbolicRegions {
    fn name(&self) -> &'static str {
        "compile-symbolic-regions"
    }

    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        if !context.options.stable_instant_inputs {
            return Ok(PlanOutput::SymbolicRegions(Default::default()));
        }
        let compilation = crate::redpiler::analysis::instant_regions::symbolic_region::compile(graph, context.monitor)?;
        Ok(PlanOutput::SymbolicRegions(compilation))
    }
}
