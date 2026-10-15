//! Experimental replacement of ordered Turbo walks with final-state settling.
//!
//! Keep boundary routes but disable specializations requiring ordered walks.
//! This is not an equivalent transformation and must never enter normal tiers.

use super::{GraphPass, PassContext};
#[cfg(test)]
use crate::redpiler::passes::Pass;
use crate::redpiler::program_graph::CompileGraph;

pub(crate) struct SettleDustNetworks;

impl GraphPass for SettleDustNetworks {
    fn name(&self) -> &'static str {
        "unsafe-settle-dust-networks"
    }

    fn run(&self, graph: &mut CompileGraph, context: &PassContext<'_>) -> Result<(), String> {
        if context.monitor.cancelled() {
            return Err("cancelled before replacing Turbo propagation".into());
        }
        for wire in &mut graph.wires {
            wire.execution.instant_reset = None;
            wire.execution.query_facts = None;
        }
        graph.plans.turbo_callback_indices.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::analysis::AnalysisLimits;
    use crate::redpiler::passes::{PassEntry, PassPipeline, rich, run_graph_pipeline};
    use crate::redpiler::{CompilerOptions, TaskMonitor};

    #[test]
    fn settling_retains_boundary_routes_and_entry_state() {
        let (world, bounds) = crate::redpiler::passes::simplify_instant_resets::tests::fixture();
        let monitor = TaskMonitor::default();
        let mut prepared = rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::default()).unwrap();
        let nodes = prepared.graph.nodes.clone();
        assert!(prepared.graph.wires.iter().any(|wire| wire.turbo_route.is_some()));
        let pipeline = PassPipeline::new(vec![PassEntry {
            pass: Pass::rewrite(SettleDustNetworks),
            enabled: true,
        }]);
        run_graph_pipeline(&mut prepared.graph, &CompilerOptions::default(), &prepared.entry, &pipeline, &monitor).unwrap();
        assert!(prepared.graph.wires.iter().all(|wire| wire.turbo_route.is_some()));
        assert_eq!(prepared.graph.nodes, nodes);
        prepared.graph.lower().unwrap();
    }
}
