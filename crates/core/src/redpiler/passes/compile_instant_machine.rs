//! O4 compiles bounded closed instant selections into validated phase/expression kernels.
//! Full-state entry checks and cold reconstruction preserve exact fallback and continuation;
//! hot ticks evaluate only compiled outputs and effects, without private propagation.
use super::PassContext;
use super::planning::{PlanBuilder, PlanOutput};
use crate::redpiler::program_graph::CompileGraph;

pub(crate) struct CompileInstantMachine;

impl PlanBuilder for CompileInstantMachine {
    fn name(&self) -> &'static str {
        "compile-instant-machine"
    }
    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        if context.monitor.cancelled() {
            return Err("cancelled while compiling instant machine".into());
        }
        if !context.options.stable_instant_inputs || !context.entry.pending_ticks.is_empty() || !graph.plans.instant.regions.is_empty() || !graph.plans.classic_nets.is_empty() {
            return Ok(PlanOutput::Machine(None));
        }
        let machine = crate::redpiler::analysis::instant_regions::machine::compile(graph, context.monitor)?;
        Ok(PlanOutput::Machine(machine))
    }
}
