//! Publish exact ordered programs replacing closed-corridor interior propagation.
use super::{
    PassContext,
    planning::{PlanBuilder, PlanOutput},
};
use crate::redpiler::program_graph::{CompileGraph, infer_signal_channels};

pub(crate) struct CompileSignalChannels;

impl PlanBuilder for CompileSignalChannels {
    fn name(&self) -> &'static str {
        "compile-signal-channels"
    }

    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        let plan = infer_signal_channels(graph, context.monitor)?;
        context
            .monitor
            .record_preparation_tables(vec![crate::redpiler::PreparationTableStatistics {
                stage: "signal channels",
                table: format!(
                    "ordered transitions ({} nets, {} wires; declines {:?})",
                    plan.stats.nets, plan.stats.wires, plan.stats.declines
                ),
                entries: plan.stats.transitions,
                capacity: plan.stats.transitions,
                estimated_bytes: plan.stats.operations * std::mem::size_of::<crate::redpiler::program_graph::SignalOp>(),
            }]);
        Ok(PlanOutput::SignalChannels(plan))
    }
}
