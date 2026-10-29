//! Compile property capture dispatch while retaining each walk's first discovery cut.
//!
//! On an already validated graph, classify immutable sites once and choose direct
//! mutable property readers. No saved mutable value, guard result or contact occupant
//! is frozen. General capture remains when the table is absent; wire-free graphs
//! retain no table. Rewrites clear the product and validation rederives every row.
//! Equality of ObservedSite at the unchanged discovery cut preserves all later
//! writes, requests, schedules and duplicate deliveries. The wire_hops tests cover
//! analog transitions, live contacts and nested walks. Build/validation are linear
//! in sites, with one recipe per site and no runtime allocation for capture itself.
//! See scraps/docs/research-2026-10-21/notes/redpiler-pistons-v4/WIRE_CAPTURE_COMPILATION.md for measured scope.
use super::{
    PassContext,
    planning::{PlanBuilder, PlanOutput},
};
use crate::redpiler::{
    PreparationTableStatistics,
    program_graph::{CompileGraph, WireCapture},
};

pub(crate) struct CompileWireCaptures;

impl PlanBuilder for CompileWireCaptures {
    fn name(&self) -> &'static str {
        "compile-wire-captures"
    }

    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        let mut captures = Vec::with_capacity(if graph.wires.is_empty() { 0 } else { graph.sites.len() });
        if !graph.wires.is_empty() {
            for site in &graph.sites {
                if context.monitor.cancelled() {
                    return Err("cancelled while compiling wire captures".into());
                }
                captures.push(WireCapture::infer(graph, site));
            }
        }
        if context.monitor.cancelled() {
            return Err("cancelled before publishing wire captures".into());
        }
        context.monitor.record_preparation_tables(vec![PreparationTableStatistics {
            stage: "compiled wire captures",
            table: "site capture operations".into(),
            entries: captures.len(),
            capacity: captures.capacity(),
            estimated_bytes: std::mem::size_of_val(captures.as_slice()),
        }]);
        Ok(PlanOutput::WireCaptures(captures))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::{
        TaskMonitor,
        analysis::AnalysisLimits,
        passes,
        program_graph::{GraphBuildError, ObservedProperties, ObservedSite},
    };

    #[test]
    fn capture_plans_reject_stale_bindings_and_cancel_before_publication() {
        let (world, bounds) = crate::redpiler::test_fixtures::piston_reset_fixture();
        let monitor = TaskMonitor::default();
        let mut prepared = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::default()).unwrap();
        let options = Default::default();
        let context = PassContext::new(&monitor, &options, &prepared.entry);
        let pass = CompileWireCaptures;
        pass.run(&mut prepared.graph, &context).unwrap();
        prepared.graph.validate().unwrap();
        let saved = prepared.graph.plans.wire_captures.clone();
        pass.run(&mut prepared.graph, &context).unwrap();
        assert_eq!(prepared.graph.plans.wire_captures, saved);
        let index = prepared
            .graph
            .plans
            .wire_captures
            .iter()
            .position(|op| !matches!(op, WireCapture::Fixed(_)))
            .unwrap();
        prepared.graph.plans.wire_captures[index] = WireCapture::Fixed(ObservedSite {
            profile: None,
            receiver: None,
            properties: ObservedProperties::Passive,
        });
        assert!(matches!(prepared.graph.validate(), Err(GraphBuildError::InvalidWireCapture { site }) if site == index));
        prepared.graph.plans.wire_captures = saved.clone();
        monitor.cancel();
        assert!(pass.run(&mut prepared.graph, &context).is_err());
        assert_eq!(prepared.graph.plans.wire_captures, saved);
        prepared.graph.clear_execution_plans();
        assert!(prepared.graph.plans.wire_captures.is_empty());
    }
}
