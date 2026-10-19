//! Experimental stateful kernels: compile connected shared cells with FIFO-local entries.
//!
//! Each entry checks readiness and Main/Quasi power at the original acceptance
//! cut, then evaluates its reduced resource predicate. It consumes only the popped request; foreign requests, clocks,
//! deferred events, captured operands and wire walks remain authoritative Graph
//! work. Shared resources retain live first-accepted-owner arbitration. Reset,
//! extension, early-drop and non-ready requests keep the existing general path.
//! No signal is expanded through undelivered wiring and no whole-region idle or
//! queue claim is made. Structural connectivity groups entry points, not simultaneous
//! actions. Validation reconstructs the complete programs from the boundary catalog.
//! Inside a commit, own-actor updates are inert while its base is pending; inert
//! controls/contact callbacks can also be omitted. Other actors and clocks retain
//! their delivery positions. No interior wire propagation is elided by this pass.
//! Discovery is bounded by catalog ports and per-cell predicate slices; retained
//! programs are limited to one eighth of the graph preparation memory bound.
use super::{
    PassContext,
    planning::{PlanBuilder, PlanOutput},
};
use crate::redpiler::program_graph::CompileGraph;

pub(crate) struct CompileInstantEvents;

impl PlanBuilder for CompileInstantEvents {
    fn name(&self) -> &'static str {
        "experimental-compile-instant-events"
    }
    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        if graph.plans.instant.boundaries.is_none() {
            return Err("instant events require the boundary catalog".into());
        }
        let kernels = crate::redpiler::analysis::instant::event_kernels::infer(graph, context.monitor)?;
        Ok(PlanOutput::InstantEvents(kernels))
    }
}
