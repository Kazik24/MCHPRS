//! Pack validated event kernels into flat live-read and Boolean mask programs.
//!
//! Replaces pointer-rich predicate/power traversal with contiguous read recipes
//! and parallel required/forbidden masks. Only pure reads within one original
//! sampling cut are reordered; FIFO actions, captures, callbacks and deferred
//! revisions remain untouched. Contact variants retain first-match priority;
//! delivered wire strengths are read live, never expanded to equilibrium.
//! Bounded DNF compilation (64 reads/clauses) keeps exact input-query fallbacks;
//! unsupported acceptance predicates retain the original event entry. Rewrites
//! clear the plan and validation independently recompiles it from the kernels
//! and source graph. Runtime uses word operations, with AVX2 clause comparisons
//! selected once when supported by CPU/OS, and a portable scalar implementation.
//! Work is bounded per entry; all runtime tables are contiguous immutable arrays
//! without a second mutable state owner. See CPU_KERNEL_EXECUTION.md for evidence.
use super::{
    PassContext,
    planning::{PlanBuilder, PlanOutput},
};
use crate::redpiler::program_graph::{CompileGraph, CpuKernelPlans};

pub(crate) struct OptimizeInstantKernelsCpu;

impl PlanBuilder for OptimizeInstantKernelsCpu {
    fn name(&self) -> &'static str {
        "optimize-instant-kernels-cpu"
    }
    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        let plans = CpuKernelPlans::compile(graph, context.monitor)?;
        Ok(PlanOutput::CpuKernels(plans))
    }
}
