//! Compile exact live Boolean Main/Quasi predicates for piston power checks.
use super::{
    PassContext,
    planning::{PlanBuilder, PlanOutput},
};
use crate::redpiler::program_graph::{CompileGraph, CpuPowerQueryPlans};

pub(crate) struct CompileCpuPowerQueries;

impl PlanBuilder for CompileCpuPowerQueries {
    fn name(&self) -> &'static str {
        "compile-cpu-power-queries"
    }

    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        let plans = CpuPowerQueryPlans::compile(graph, context.monitor)?;
        context.monitor.record_preparation_tables(vec![
            crate::redpiler::PreparationTableStatistics {
                stage: "compiled piston power queries",
                table: "actor Main/Quasi roots".into(),
                entries: plans.entries.len(),
                capacity: plans.entries.capacity(),
                estimated_bytes: plans.entries.capacity() * std::mem::size_of::<crate::redpiler::program_graph::CpuPowerQueryEntry>(),
            },
            crate::redpiler::PreparationTableStatistics {
                stage: "compiled piston power queries",
                table: "exact input fallbacks".into(),
                entries: plans.input_fallback_entries(),
                capacity: plans.input_fallback_entries(),
                estimated_bytes: 0,
            },
        ]);
        Ok(PlanOutput::CpuPowerQueries(plans))
    }
}
