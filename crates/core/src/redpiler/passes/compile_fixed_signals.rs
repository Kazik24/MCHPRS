//! Compile fixed directional/conductor reads into dense weighted live-state queries.
//! Also infer dust-free main/side equations, like historical input_search. Those
//! settled equations are inspection/proof input, not permission to erase Turbo.
//! Contacts, state-dependent guards and unproved wire direction stay general.
//! Exact live leaves retain original wire state, source delivery and event ordering.
//! Validation re-infers the plan; rewrites invalidate it with other derived plans.
//! Cost is bounded backward traversal within the four-bit attenuation radius.
use super::{PassContext, planning::{PlanBuilder, PlanOutput}};
use crate::redpiler::{PreparationTableStatistics, program_graph::infer_fixed_signals};

pub(crate) struct CompileFixedSignals;

impl PlanBuilder for CompileFixedSignals {
    fn name(&self) -> &'static str { "compile-fixed-signals" }
    fn build(&self, graph: &crate::redpiler::program_graph::CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        let plans = infer_fixed_signals(graph, context.monitor)?;
        let mut counts = vec![("compiled live queries".to_owned(), plans.live.len()),
            ("general query fallbacks".to_owned(), plans.fallback.len()),
            ("traced receiver channels".to_owned(), plans.traced.len()),
            ("traced weighted edges".to_owned(), plans.traced.iter().map(|query| query.inputs.len()).sum())];
        counts.extend(plans.declines.iter().map(|(reason, count)| (format!("declined: {reason}"), *count)));
        context.monitor.record_preparation_tables(counts.into_iter().map(|(table, entries)| PreparationTableStatistics {
            stage: "fixed signals", table, entries, capacity: entries, estimated_bytes: 0,
        }).collect());
        Ok(PlanOutput::FixedSignals(plans))
    }
}
