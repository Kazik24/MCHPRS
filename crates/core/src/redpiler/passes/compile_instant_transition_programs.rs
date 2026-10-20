//! Certify transport nets eligible for exact ordered episode transcripts.
use super::{
    PassContext,
    planning::{PlanBuilder, PlanOutput},
};
use crate::redpiler::{PreparationTableStatistics, program_graph::infer_instant_transition_programs};

pub(crate) struct CompileInstantTransitionPrograms;

impl PlanBuilder for CompileInstantTransitionPrograms {
    fn name(&self) -> &'static str {
        "compile-instant-transition-programs"
    }

    fn build(&self, graph: &crate::redpiler::program_graph::CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        let plan = infer_instant_transition_programs(graph, &graph.plans.instant_transport, &graph.plans.instant_drives, context.monitor)?;
        let stats = &plan.stats;
        context.monitor.record_preparation_tables(vec![PreparationTableStatistics {
            stage: "instant transition programs",
            table: "ordered transport episode eligibility and wire queries".into(),
            entries: stats.eligible_wires,
            capacity: stats.eligible_wires,
            estimated_bytes: stats.bytes,
        }]);
        tracing::info!(
            eligible_nets = stats.eligible_nets,
            eligible_wires = stats.eligible_wires,
            declined_nets = stats.declined_nets,
            work = stats.work,
            bytes = stats.bytes,
            declines = ?stats.declines,
            "compiled exact instant transition-program eligibility"
        );
        Ok(PlanOutput::InstantTransitionPrograms(plan))
    }
}
