//! Compile private BUD memory read ports into live payload-presence tests.
//!
//! The upper sticky storage owns a private redstone resource, while a lower
//! admitted instant actor has exactly one Quasi contribution. Its first selected
//! contact profile emits fifteen iff ResourceAt(Far) && ResourceEnabled; matching
//! the complete guard proves an exact zero/fifteen read for every live state.
//! Other inputs, foreign/shared resources and variable extra guards decline.
//! Only reads change: storage updates, request sampling, acceptance, clock/FIFO,
//! revision checks and deferred publication remain on Graph. No memory pose or
//! update signal is inferred from missing observer provenance alone. The typed
//! pair and exact electrical witness are retained and independently validated.
//! Discovery indexes power/ownership once, then visits read candidates and guard
//! trees with bounded depth. State remains in the original piston/resource slots.
use super::{
    PassContext,
    planning::{PlanBuilder, PlanOutput},
};
use crate::redpiler::program_graph::CompileGraph;

pub(crate) struct CompileBudMemoryReads;
impl PlanBuilder for CompileBudMemoryReads {
    fn name(&self) -> &'static str {
        "experimental-compile-bud-memory-reads"
    }
    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        let reads = crate::redpiler::program_graph::infer_bud_memory_reads(graph, context.monitor)?;
        context
            .monitor
            .record_preparation_tables(vec![crate::redpiler::PreparationTableStatistics {
                stage: "compiled BUD memory reads",
                table: "live storage/read pairs".into(),
                entries: reads.len(),
                capacity: reads.capacity(),
                estimated_bytes: std::mem::size_of_val(reads.as_slice()),
            }]);
        Ok(PlanOutput::BudMemoryReads(reads))
    }
}
