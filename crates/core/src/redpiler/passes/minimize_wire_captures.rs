//! Remove immutable passive sites from the ordered wire machine's mutable state.
//!
//! Constant passive observations have no receiver and can never become a visited
//! wire. Their heading bias and queue-layer fields therefore have no observable
//! use. Keep their original site IDs and profiles as constant operands, but omit
//! discovery, scratch insertion, bias writes and enqueue tests. Mutable contacts,
//! wires, controls and all active receivers retain canonical discovery order and
//! the original FIFO. This is partial evaluation, not settled-value substitution.
//! Requires compiled captures and hop roots; independent validation reconstructs
//! each 24-bit mask. Discovery is O(wires * 24), retaining an eight-byte optional mask per wire.
use super::{
    PassContext,
    planning::{PlanBuilder, PlanOutput},
};
use crate::redpiler::program_graph::{CompileGraph, ObservedProperties, WireCapture};

pub(crate) struct MinimizeWireCaptures;

impl PlanBuilder for MinimizeWireCaptures {
    fn name(&self) -> &'static str {
        "experimental-minimize-wire-captures"
    }

    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        if !graph.wires.is_empty() && graph.plans.wire_captures.len() != graph.sites.len() {
            return Err("wire minimization requires compiled captures".into());
        }
        let mut masks = Vec::with_capacity(graph.wires.len());
        for wire in &graph.wires {
            if context.monitor.cancelled() {
                return Err("cancelled while minimizing wire captures".into());
            }
            if wire.execution.hop_root.is_none() {
                return Err("wire minimization requires compiled hop roots".into());
            }
            let mut mask = 0;
            for (slot, site) in wire.canonical_neighbors.iter().enumerate() {
                if !matches!(&graph.plans.wire_captures[site.index()], WireCapture::Fixed(observed)
                    if observed.receiver.is_none() && observed.properties == ObservedProperties::Passive)
                {
                    mask |= 1 << slot;
                }
            }
            masks.push(mask);
        }
        Ok(PlanOutput::WireCaptureMasks(masks))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::{
        TaskMonitor,
        analysis::AnalysisLimits,
        passes,
        program_graph::{EntryState, GraphBuildError},
    };

    #[test]
    fn minimized_capture_masks_reject_mutable_omissions_and_cancel_without_publication() {
        let (world, bounds) = passes::simplify_instant_resets::tests::fixture();
        let monitor = TaskMonitor::default();
        let mut prepared = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::default()).unwrap();
        let options = Default::default();
        let entry = EntryState {
            logical_tick: 0,
            pending_ticks: vec![],
        };
        let context = PassContext::new(&monitor, &options, &entry);
        assert!(MinimizeWireCaptures.run(&mut prepared.graph, &context).is_err());
        passes::compile_wire_captures::CompileWireCaptures
            .run(&mut prepared.graph, &context)
            .unwrap();
        passes::compile_wire_hops::CompileWireHops.run(&mut prepared.graph, &context).unwrap();
        MinimizeWireCaptures.run(&mut prepared.graph, &context).unwrap();
        prepared.graph.validate().unwrap();
        let saved = prepared.graph.clone();
        MinimizeWireCaptures.run(&mut prepared.graph, &context).unwrap();
        assert_eq!(prepared.graph, saved);
        let index = prepared
            .graph
            .wires
            .iter()
            .position(|wire| wire.execution.mutable_captures.unwrap() != 0)
            .unwrap();
        let mask = prepared.graph.wires[index].execution.mutable_captures.unwrap();
        for forged in [mask & (mask - 1), mask | (1 << 24)] {
            prepared.graph.wires[index].execution.mutable_captures = Some(forged);
            assert!(matches!(prepared.graph.validate(), Err(GraphBuildError::InvalidWireCaptureMask { wire }) if wire == index));
        }
        prepared.graph = saved.clone();
        monitor.cancel();
        assert!(MinimizeWireCaptures.run(&mut prepared.graph, &context).is_err());
        assert_eq!(prepared.graph, saved);
        prepared.graph.clear_execution_plans();
        assert!(prepared.graph.wires.iter().all(|wire| wire.execution.mutable_captures.is_none()));
    }
}
