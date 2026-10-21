//! Compile ordinary single-owner redstone payload writers into typed event bindings.
use super::{
    PassContext,
    planning::{PlanBuilder, PlanOutput},
};
use crate::redpiler::program_graph::CompileGraph;

pub(crate) struct CompileLogicalBudOwners;

impl PlanBuilder for CompileLogicalBudOwners {
    fn name(&self) -> &'static str {
        "compile-logical-bud-owners"
    }

    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        let owners = crate::redpiler::program_graph::compile_logical_bud_owners(graph, context.monitor)?;
        context
            .monitor
            .record_preparation_tables(vec![crate::redpiler::PreparationTableStatistics {
                stage: "logical BUD owner bindings",
                table: "exclusive two-contact payload owners".into(),
                entries: owners.len(),
                capacity: owners.capacity(),
                estimated_bytes: crate::redpiler::program_graph::estimated_bytes(&owners),
            }]);
        Ok(PlanOutput::LogicalBudOwners(owners))
    }
}
