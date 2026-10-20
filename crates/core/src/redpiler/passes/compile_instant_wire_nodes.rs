//! O4 local signal nodes for isolated dust serving only recognized instant cells.
//!
//! The builder reads the graph and publishes a receiver-slot mask. Lowering keeps the
//! original wire state and routes; execution replaces discovery/capture with ordered
//! signal deliveries. All possible contact occupants and conducted power consumers
//! must stay inside the instant boundary. Lamps, classic gates, ordinary pistons,
//! other dust, and overlapping execution owners decline. Reset clocks retain their
//! original scheduler obligations. Cost is linear in graph edges plus local closure.
//! Selected by normal O4/O5; every admitted boundary is certified without a Cargo feature.
use super::{
    PassContext,
    planning::{PlanBuilder, PlanOutput},
};
use crate::redpiler::program_graph::CompileGraph;

pub(crate) struct CompileInstantWireNodes;

impl PlanBuilder for CompileInstantWireNodes {
    fn name(&self) -> &'static str {
        "experimental-compile-instant-wire-nodes"
    }

    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        let nodes = crate::redpiler::program_graph::infer_instant_wire_nodes(graph, context.monitor)?;
        let accepted = nodes.iter().filter(|node| node.is_some()).count();
        context
            .monitor
            .record_preparation_tables(vec![crate::redpiler::PreparationTableStatistics {
                stage: "experimental instant wire nodes",
                table: "closed single-wire nets".into(),
                entries: accepted,
                capacity: nodes.len(),
                estimated_bytes: nodes.len() * std::mem::size_of::<Option<crate::redpiler::program_graph::InstantWireNode>>(),
            }]);
        Ok(PlanOutput::InstantWireNodes(nodes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancelled_builder_does_not_publish() {
        let (world, bounds) = super::super::simplify_instant_resets::tests::fixture();
        let monitor = crate::redpiler::TaskMonitor::default();
        let prepared = super::super::rich::prepare(&world, bounds, vec![], &monitor, crate::redpiler::analysis::AnalysisLimits::default()).unwrap();
        let graph = prepared.graph;
        let before = graph.clone();
        let options = Default::default();
        let context = PassContext::new(&monitor, &options, &prepared.entry);
        monitor.cancel();
        assert!(CompileInstantWireNodes.build(&graph, &context).is_err());
        assert_eq!(graph, before);
    }
}
