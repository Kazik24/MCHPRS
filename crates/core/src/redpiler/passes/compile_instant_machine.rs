//! O4 compiles bounded closed instant selections into validated phase/expression kernels.
//! Full-state entry checks and cold reconstruction preserve exact fallback and continuation;
//! hot ticks evaluate only compiled outputs and effects, without private propagation.
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
