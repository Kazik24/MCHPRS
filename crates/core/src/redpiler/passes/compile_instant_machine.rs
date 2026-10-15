//! Replace bounded closed instant selections with checked, pre-evaluated state transitions.
//! Compilation runs ordered Graph semantics; execution applies only the resulting writes and
//! future obligations. Exact full-state lookup prevents input-only caching and stale reuse.
//! Unknown states/operations keep general execution before effects; no table is learned at runtime.
use super::{GraphPass, PassContext};
use crate::redpiler::program_graph::CompileGraph;

pub(crate) struct CompileInstantMachine;

impl GraphPass for CompileInstantMachine {
    fn name(&self) -> &'static str {
        "compile-instant-machine"
    }
    fn run(&self, graph: &mut CompileGraph, context: &PassContext<'_>) -> Result<(), String> {
        if context.monitor.cancelled() {
            return Err("cancelled while compiling instant machine".into());
        }
        if !context.options.stable_instant_inputs || !context.entry.pending_ticks.is_empty() {
            graph.instant_machine = None;
            return Ok(());
        }
        let machine = crate::redpiler::analysis::instant_regions::machine::compile(graph, context.monitor)?;
        graph.instant_machine = machine;
        Ok(())
    }
    fn preserves_instant_machine(&self) -> bool {
        true
    }
    fn preserves_instant_regions(&self) -> bool {
        true
    }
    fn preserves_wire_hops(&self) -> bool {
        true
    }
    fn preserves_dust_paths(&self) -> bool {
        true
    }
    fn preserves_turbo_callback_indices(&self) -> bool {
        true
    }
    fn preserves_wire_query_facts(&self) -> bool {
        true
    }
    fn preserves_instant_resets(&self) -> bool {
        true
    }
}
