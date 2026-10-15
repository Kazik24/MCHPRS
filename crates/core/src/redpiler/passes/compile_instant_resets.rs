//! Certifies private reset output walks after semantic rewriting, retaining their ordered effects.
use super::PassContext;
use super::planning::{PlanBuilder, PlanOutput};
use crate::redpiler::program_graph::CompileGraph;

/// Builds output-walk certificates after all power and reset-node rewrites.
pub(crate) struct CompileInstantResets;

impl PlanBuilder for CompileInstantResets {
    fn name(&self) -> &'static str {
        "compile-instant-resets"
    }

    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        let plans = crate::redpiler::analysis::instant_resets::infer_with_monitor(graph, context.monitor)?;
        context
            .monitor
            .record_preparation_tables(vec![crate::redpiler::PreparationTableStatistics {
                stage: "instant reset specialization",
                table: "certified output pairs".into(),
                entries: plans.len(),
                capacity: plans.len(),
                estimated_bytes: 0,
            }]);
        Ok(PlanOutput::InstantResets(plans))
    }
}
