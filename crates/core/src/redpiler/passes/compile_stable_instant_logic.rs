//! Compile certified stable-input wire corridors into max-weighted source expressions.
use super::{
    PassContext,
    planning::{PlanBuilder, PlanOutput},
};
use crate::redpiler::program_graph::{CompileGraph, infer_stable_instant_logic};

pub(crate) struct CompileStableInstantLogic;

impl PlanBuilder for CompileStableInstantLogic {
    fn name(&self) -> &'static str {
        "compile-stable-instant-logic"
    }

    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        if !context.options.assumes_stable_instant_inputs() {
            return Ok(PlanOutput::StableInstantLogic(Default::default()));
        }
        let plan = infer_stable_instant_logic(graph, context.monitor)?;
        let stats = plan.stats;
        context
            .monitor
            .record_preparation_tables(vec![crate::redpiler::PreparationTableStatistics {
                stage: "stable instant logic",
                table: format!(
                    "max-weighted wire nets ({}/{} admitted; {} wires, {} shared inputs, {} terms, {} external output sites)",
                    stats.admitted, stats.nets, stats.wires, stats.inputs, stats.terms, stats.outputs
                ),
                entries: stats.nets,
                capacity: plan.nets.capacity(),
                estimated_bytes: stats.bytes,
            }]);
        tracing::info!(
            nets = stats.nets,
            admitted = stats.admitted,
            declined = stats.declined,
            wires = stats.wires,
            shared_inputs = stats.inputs,
            terms = stats.terms,
            work = stats.work,
            bytes = stats.bytes,
            "compiled stable instant logic"
        );
        Ok(PlanOutput::StableInstantLogic(plan))
    }
}
