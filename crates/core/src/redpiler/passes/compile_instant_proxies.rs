//! Compiles certified instant-region routes into ordered proxy instructions.
//!
//! Default compilation admits private two-contact resources with a built-in self-reset.
//! The stable-input option also admits composed groups with fully recognized reset actors.
//! Internal reset notifications become direct clock arms; self notifications become
//! direct samples. All outward callbacks, guards, ordering, live queries, FIFO actions,
//! deferred publications and imported clock work remain. This is not an atomic gate.
//! Each instruction is rederived and validated against the original graph before lowering.
//! Unsupported groups retain general execution; only explicitly opted-in regions carry a protocol promise.
use super::{GraphPass, PassContext};
use crate::redpiler::program_graph::CompileGraph;

pub(crate) struct CompileInstantProxies;

impl GraphPass for CompileInstantProxies {
    fn name(&self) -> &'static str {
        "compile-instant-proxies"
    }
    fn run(&self, graph: &mut CompileGraph, context: &PassContext<'_>) -> Result<(), String> {
        let proxies = crate::redpiler::analysis::instant_regions::infer_proxies(graph, &graph.instant_regions, context.monitor)?;
        graph.instant_proxies = proxies;
        Ok(())
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
