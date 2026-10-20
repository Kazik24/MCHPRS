//! Compile the source-side certificates consumed by the exact dense Turbo executor.
//!
//! The pass runs at O5 after boundary, route, callback and competing-owner plans
//! have been published. It admits only multi-wire corridors whose wires have a
//! Turbo route with no conditional transfer inputs, whose changing direct wire
//! inputs are all corridor members, and whose six no-dust power queries cannot
//! depend on a member wire. It also rejects selectable geometry that can become
//! dust and receivers that could synchronously change electrical/resource state.
//! These checks make cached non-dust drive values stable for one walk; piston,
//! observer, reset and contact callbacks remain queue-only and are sampled live
//! at their original capture cuts. Callback admission is not precomputed from
//! current power or resource state.
//!
//! The resulting rows preserve each wire state, the 24 canonical neighbor slots,
//! shared first-discovery sites, and callback routes. The backend executes them
//! with the original three rotating FIFO queues, retaining event order and
//! intermediate observations rather than solving a settled Boolean network.
//! Plans are independently rederived on publication and invalidated by graph
//! rewrites. Compilation indexes power links once, then checks each wire's
//! bounded 24-slot neighborhood plus its dependency closure; memory is capped by
//! the graph preparation budget. Runtime work is proportional to visited
//! corridor wires and captured slots, avoiding full physical-neighborhood walks
//! for admitted corridors. See `program_graph::instant_transport`,
//! `backend::graph::transport`, and `backend::graph::tests::instant_transport`
//! for the certificate, executor, and equivalence coverage.
use super::{
    PassContext,
    planning::{PlanBuilder, PlanOutput},
};
use crate::redpiler::program_graph::CompileGraph;

pub(crate) struct CompileInstantTransport;

impl PlanBuilder for CompileInstantTransport {
    fn name(&self) -> &'static str {
        "compile-instant-transport"
    }

    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        let plan = crate::redpiler::program_graph::infer_instant_transport(graph, context.monitor)?;
        let covered_wires = plan.nets.iter().map(|net| net.wires.len()).sum::<usize>();
        let candidates = graph
            .plans
            .instant
            .boundaries
            .as_ref()
            .map_or(0, |catalog| catalog.corridors.iter().map(|corridor| corridor.wires.len()).sum());
        let bytes = plan
            .nets
            .iter()
            .map(|net| {
                net.wires.len() * std::mem::size_of::<crate::redpiler::program_graph::TransportWire>()
                    + net.sites.len() * (std::mem::size_of::<crate::redpiler::program_graph::SiteId>() + std::mem::size_of::<u32>())
            })
            .sum();
        let stats = plan.stats;
        context
            .monitor
            .record_preparation_tables(vec![crate::redpiler::PreparationTableStatistics {
                stage: "instant transport",
                table: format!(
                    "dense wire corridors ({covered_wires}/{candidates} wires; {}/{} corridors admitted, too_small={}, owner_overlap={}, no_route_or_facts={}, conditional_input={}, no_dust_dependency={}, unsafe_receiver={}, unsafe_input={}, unsupported={})",
                    stats.admitted,
                    stats.corridors,
                    stats.too_small,
                    stats.owner_overlap,
                    stats.no_route_or_facts,
                    stats.conditional_input,
                    stats.no_dust_dependency,
                    stats.unsafe_receiver,
                    stats.unsafe_input,
                    stats.unsupported
                ),
                entries: plan.nets.len(),
            capacity: plan.nets.capacity(),
                estimated_bytes: bytes,
            }]);
        Ok(PlanOutput::InstantTransport(plan))
    }
}
