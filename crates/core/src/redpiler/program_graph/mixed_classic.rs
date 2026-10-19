//! Exact no-dust reads for mixed circuits; propagation and dynamic faces stay general.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MixedClassicWire {
    /// Maximum of the fixed faces, reading original live or held component state.
    pub(crate) fixed: FixedClassicQuery,
    /// Faces requiring the original query, in BlockFace::values() order.
    pub(crate) general_faces: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MixedClassicProgram {
    /// Empty disables specialization; otherwise entries align with the wire table.
    pub(crate) wires: Box<[MixedClassicWire]>,
    /// Contiguous fixed-face operands, owned once by the immutable program.
    pub(crate) operands: Box<[FixedClassicOperand]>,
}

impl MixedClassicProgram {
    pub(crate) fn compile(graph: &CompileGraph, monitor: &crate::redpiler::TaskMonitor) -> Result<Self, String> {
        if graph.plans.fixed_classic.program.is_some() || graph.wires.is_empty() {
            return Ok(Self::default());
        }
        super::fixed_classic::compile_mixed_wire_power(graph, monitor)
    }

    pub(crate) fn validate(&self, graph: &CompileGraph, monitor: &crate::redpiler::TaskMonitor) -> Result<(), String> {
        if self.wires.is_empty() && self.operands.is_empty() {
            return Ok(());
        }
        if *self != Self::compile(graph, monitor)? {
            return Err("stale or forged mixed classic query program".into());
        }
        Ok(())
    }
}
