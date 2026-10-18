//! Resolve fixed Turbo callback admission into dense masks. Dynamic slots retain
//! the original ordered guard selection; captured receiver state stays live.
use super::{
    PassContext,
    planning::{PlanBuilder, PlanOutput},
};
use crate::redpiler::{
    PreparationTableStatistics,
    program_graph::{CompileGraph, WireCallbackSelection},
};

pub(crate) struct CompileWireCallbacks;
impl PlanBuilder for CompileWireCallbacks {
    fn name(&self) -> &'static str {
        "compile-wire-callbacks"
    }
    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        let mut plans = Vec::with_capacity(graph.wires.len());
        for index in 0..graph.wires.len() {
            if context.monitor.cancelled() {
                return Err("cancelled while compiling wire callbacks".into());
            }
            plans.push(WireCallbackSelection::infer(graph, index));
        }
        let known = plans.iter().flatten().map(|plan| plan.known.count_ones() as usize).sum();
        let admitted = plans.iter().flatten().map(|plan| plan.admitted.count_ones() as usize).sum();
        let fixed = plans.iter().flatten().filter(|plan| plan.known == WireCallbackSelection::ALL).count();
        let table = |name: &str, entries| PreparationTableStatistics {
            stage: "compiled wire callbacks",
            table: name.into(),
            entries,
            capacity: entries,
            estimated_bytes: 0,
        };
        context.monitor.record_preparation_tables(vec![
            table("fully compiled wires", fixed),
            table("decided slots", known),
            table("admitted slots", admitted),
            table("dynamic slots", graph.wires.len() * 24 - known),
        ]);
        Ok(PlanOutput::WireCallbacks(plans))
    }
}
