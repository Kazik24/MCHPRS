//! Compile graph-bound actor/reset event programs without enumerating input assignments.
//! Effect application shares Graph's resource, callback and scheduler primitives.
use super::PassContext;
use super::planning::{PlanBuilder, PlanOutput};
use crate::redpiler::program_graph::CompileGraph;

pub(crate) struct CompileSymbolicActors;

impl PlanBuilder for CompileSymbolicActors {
    fn name(&self) -> &'static str {
        "compile-symbolic-actors"
    }

    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        if !context.options.stable_instant_inputs {
            return Ok(PlanOutput::SymbolicActors(Vec::new()));
        }
        let actors = crate::redpiler::analysis::instant_regions::symbolic_actor::compile(graph, context.monitor)?;
        Ok(PlanOutput::SymbolicActors(actors))
    }
}
