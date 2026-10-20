//! Extend dense transport to live geometry and ordered delayed-device boundaries.
//!
//! Conditional edges sample the original first-discovery captures. Repeater,
//! torch and comparator callbacks retain their FIFO positions and pending work;
//! a runtime entry guard proves repeater emissions cannot change during a walk.
//! This replaces neighborhood discovery, not the boundary sampling schedule.
use super::{
    PassContext,
    planning::{PlanBuilder, PlanOutput},
};
use crate::redpiler::program_graph::CompileGraph;

pub(crate) struct CompileGuardedInstantTransport;

impl PlanBuilder for CompileGuardedInstantTransport {
    fn name(&self) -> &'static str {
        "compile-guarded-instant-transport"
    }

    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        let plan = crate::redpiler::program_graph::infer_guarded_instant_transport(graph, context.monitor)?;
        let wires = plan.nets.iter().map(|net| net.wires.len()).sum::<usize>();
        let conditional = plan
            .nets
            .iter()
            .flat_map(|net| net.wires.iter())
            .filter(|wire| wire.conditional_inputs != 0)
            .count();
        let bytes = plan
            .nets
            .iter()
            .map(|net| {
                net.wires.len() * std::mem::size_of::<crate::redpiler::program_graph::TransportWire>()
                    + net.sites.len() * (std::mem::size_of::<crate::redpiler::program_graph::SiteId>() + std::mem::size_of::<u32>())
                    + net.canonical_repeaters.len() * std::mem::size_of::<crate::redpiler::program_graph::NodeId>()
            })
            .sum();
        context
            .monitor
            .record_preparation_tables(vec![crate::redpiler::PreparationTableStatistics {
                stage: "guarded instant transport",
                table: format!(
                    "{wires} wires in {}/{} corridors; {conditional} conditional wire rows",
                    plan.nets.len(),
                    plan.stats.corridors
                ),
                entries: plan.nets.len(),
                capacity: plan.nets.capacity(),
                estimated_bytes: bytes,
            }]);
        Ok(PlanOutput::InstantTransport(plan))
    }
}
