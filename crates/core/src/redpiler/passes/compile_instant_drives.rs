//! Build typed, exact-u8 expressions for wire no-dust samples.
use super::{
    PassContext,
    planning::{PlanBuilder, PlanOutput},
};
use crate::redpiler::{PreparationTableStatistics, program_graph::infer_instant_drives};

pub(crate) struct CompileInstantDrives;

impl PlanBuilder for CompileInstantDrives {
    fn name(&self) -> &'static str {
        "compile-instant-drives"
    }

    fn build(&self, graph: &crate::redpiler::program_graph::CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        let plan = infer_instant_drives(graph, context.monitor)?;
        let stats = plan.stats.clone();
        context.monitor.record_preparation_tables(vec![
            PreparationTableStatistics {
                stage: "instant drives",
                table: "wire queries".into(),
                entries: plan.queries.len(),
                capacity: plan.queries.len(),
                estimated_bytes: plan.queries.len() * std::mem::size_of::<crate::redpiler::program_graph::DriveExpr>(),
            },
            PreparationTableStatistics {
                stage: "instant drives",
                table: "drive terms".into(),
                entries: plan.terms.len(),
                capacity: plan.terms.len(),
                estimated_bytes: plan.terms.len() * std::mem::size_of::<crate::redpiler::program_graph::DriveTerm>(),
            },
            PreparationTableStatistics {
                stage: "instant drives",
                table: "conditions".into(),
                entries: plan.conditions.len(),
                capacity: plan.conditions.len(),
                estimated_bytes: 0,
            },
        ]);
        tracing::info!(
            compiled = stats.compiled,
            fallback = stats.fallback,
            work = stats.work,
            "compiled instant drive expressions"
        );
        Ok(PlanOutput::InstantDrives(plan))
    }
}
