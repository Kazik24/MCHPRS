//! Replaces recognized observer-reset execution with one shared built-in pulse clock.
//!
//! Only an empty output route may be absorbed: resetting can affect power reads,
//! but cannot synchronously notify another component. The existing node/state ID,
//! ingress guards, directions, captures and scheduler positions remain unchanged.
//! A qualifying notification arms the same deduplicated tick. That tick toggles
//! reset power and, on the rising edge, schedules its falling edge in the same
//! queue position as the observer. Thus arbitrary interruptions and imported
//! ticks retain their original behavior; reset is not replaced by a guessed timer.
//! Shared reset drivers remain one clock, even when several actors use them.
//! Unsupported output circuits remain ordinary observers. Publication is atomic
//! after cancellation, and discovery is linear in the indexed graph tables.
//!
//! This first lowering bypasses ordinary observer dispatch/output routing; it
//! retains ingress notifications and scheduled transitions, not a whole-region
//! contraction. Existing wire certificates remain valid because every observable
//! state, notification, power value and scheduled obligation is preserved.

use super::{GraphPass, PassContext};
use crate::redpiler::PreparationTableStatistics;
use crate::redpiler::program_graph::{CircuitNodeKind, CompileGraph, PowerPort, SiteSource};
use mchprs_blocks::{BlockFace, blocks::Block};
use rustc_hash::FxHashSet;

pub(crate) struct CompileBuiltinResets;

impl GraphPass for CompileBuiltinResets {
    fn name(&self) -> &'static str {
        "compile-builtin-resets"
    }

    fn run(&self, graph: &mut CompileGraph, context: &PassContext<'_>) -> Result<(), String> {
        let observers: FxHashSet<_> = super::utils::instant_mechanisms::discover_mechanisms(graph, context.monitor)?
            .iter()
            .map(|mechanism| mechanism.observer)
            .collect();
        let mut replacements = Vec::new();
        for observer in observers {
            if context.monitor.cancelled() {
                return Err("cancelled while compiling built-in resets".into());
            }
            let node = &graph.nodes[observer.index()];
            let CircuitNodeKind::Observer { watched, facing } = node.kind else {
                continue;
            };
            if !matches!(node.raw_state.map(Block::from_id), Some(Block::Observer { observer }) if BlockFace::from(observer.facing) == facing) {
                continue;
            }
            if node.output_route.is_some_and(|route| !graph.routes[route.index()].deliveries.is_empty()) {
                continue;
            }
            replacements.push((observer, CircuitNodeKind::BuiltinReset { watched, facing }));
        }
        let selected: FxHashSet<_> = replacements.iter().map(|(node, _)| *node).collect();
        let mut direct_outputs = Vec::new();
        for (index, link) in graph.power_links.iter().enumerate() {
            if context.monitor.cancelled() {
                return Err("cancelled while compiling reset outputs".into());
            }
            let SiteSource::Component(node) = graph.sites[link.from.index()].source else {
                continue;
            };
            if !selected.contains(&node) {
                continue;
            }
            let CircuitNodeKind::Observer { facing, .. } = graph.nodes[node.index()].kind else {
                continue;
            };
            if matches!(link.from_port, PowerPort::Weak(face) | PowerPort::Strong(face) if face == facing) {
                direct_outputs.push(index);
            }
        }
        if context.monitor.cancelled() {
            return Err("cancelled before publishing built-in resets".into());
        }
        let count = replacements.len();
        for (observer, kind) in replacements {
            graph.nodes[observer.index()].kind = kind;
            graph.nodes[observer.index()].output_route = None;
        }
        for index in direct_outputs {
            graph.power_links[index].from_port = PowerPort::Output;
        }
        context.monitor.record_preparation_tables(vec![PreparationTableStatistics {
            stage: "built-in reset compilation",
            table: "compiled reset clocks".into(),
            entries: count,
            capacity: count,
            estimated_bytes: 0,
        }]);
        Ok(())
    }
}
