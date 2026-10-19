//! Remove live geometry queries whose answers are invariant across every selectable variant.
//! Unlike whole-neighborhood facts, unrelated dynamic neighbors do not force a fallback.
//! Each relevant property is a conservative set over every contact alternative; Boolean
//! selection propagates these sets through the native side formula. Singleton sides prove
//! the raw shape. Enumerating at most 81 candidate shapes and both dot/non-dot inputs also
//! proves individual regulated output bits, even when a source changes its raw connection.
//! No guard or saved pose is frozen. Unsupported classes widen the sets; unresolved queries
//! retain the native formula with only unknown properties sampled live. Fixed output bits
//! read stored strength directly. Power, captures, writes, callbacks, FIFO and reset remain
//! on their existing paths. Validation rederives the certificate; exhaustive property-mask
//! checks and a selectable-contact runtime oracle test cover the formula and its binding.
//! Queries contain no callbacks or mutations, so dead property reads may be omitted.
//! Discovery uses bounded shape enumeration and property scans plus contact variants per
//! wire and one optional property-mask record per wire. It follows semantic rewrites and owns no state.
use super::{
    PassContext,
    planning::{PlanBuilder, PlanOutput},
};
use crate::redpiler::{
    PreparationTableStatistics,
    program_graph::{CompileGraph, WireShape},
};

pub(crate) struct CompileWireShapes;

impl PlanBuilder for CompileWireShapes {
    fn name(&self) -> &'static str {
        "compile-wire-shapes"
    }

    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        let mut shapes = Vec::with_capacity(graph.wires.len());
        for wire in &graph.wires {
            if context.monitor.cancelled() {
                return Err("cancelled while compiling wire shapes".into());
            }
            shapes.push(WireShape::infer(graph, wire));
        }
        let raw = shapes.iter().flatten().filter(|shape| shape.raw.is_some()).count();
        let outputs = shapes.iter().flatten().map(|shape| shape.emitted_known.count_ones() as usize).sum();
        context.monitor.record_preparation_tables(vec![
            PreparationTableStatistics {
                stage: "compiled wire shapes",
                table: "invariant raw shapes".into(),
                entries: raw,
                capacity: shapes.capacity(),
                estimated_bytes: std::mem::size_of_val(shapes.as_slice()),
            },
            PreparationTableStatistics {
                stage: "compiled wire shapes",
                table: "invariant output directions".into(),
                entries: outputs,
                capacity: graph.wires.len() * 4,
                estimated_bytes: 0,
            },
        ]);
        Ok(PlanOutput::WireShapes(shapes))
    }
}
