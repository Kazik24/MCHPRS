//! Opt-in hypothesis test: settle closed instant-only nets and erase interior callbacks.
//!
//! Closure checks every possible receiver and conducted consumer, including shared
//! memory cells. It does NOT prove that instant request order can be discarded.
//! The existing fixed-point experiment executes the reduction; normal tiers never
//! select it. Native counterexamples are evidence against the proposed boundary.
//! Original state, geometry and reset clocks remain available for projection.
use super::{
    PassContext,
    planning::{PlanBuilder, PlanOutput},
};
use crate::redpiler::program_graph::CompileGraph;

pub(crate) struct AbstractInstantNets;

impl PlanBuilder for AbstractInstantNets {
    fn name(&self) -> &'static str {
        "unsafe-abstract-instant-nets"
    }
    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        let masks = crate::redpiler::program_graph::infer_abstract_instant_nets(graph, context.monitor)?;
        context
            .monitor
            .record_preparation_tables(vec![crate::redpiler::PreparationTableStatistics {
                stage: "abstract instant nets hypothesis",
                table: "closed multi-wire members (not FIFO certified)".into(),
                entries: masks.iter().filter(|&&enabled| enabled).count(),
                capacity: masks.len(),
                estimated_bytes: masks.len(),
            }]);
        Ok(PlanOutput::AbstractInstantNets(masks))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absent_boundary_catalog_keeps_all_wires_general() {
        let (world, bounds) = super::super::simplify_instant_resets::tests::fixture();
        let monitor = crate::redpiler::TaskMonitor::default();
        let prepared = super::super::rich::prepare(&world, bounds, vec![], &monitor, crate::redpiler::analysis::AnalysisLimits::default()).unwrap();
        let options = Default::default();
        let context = PassContext::new(&monitor, &options, &prepared.entry);
        let PlanOutput::AbstractInstantNets(masks) = AbstractInstantNets.build(&prepared.graph, &context).unwrap() else {
            panic!("wrong plan owner")
        };
        assert!(masks.iter().all(|enabled| !*enabled));
        monitor.cancel();
        assert!(AbstractInstantNets.build(&prepared.graph, &context).is_err());
    }
}
