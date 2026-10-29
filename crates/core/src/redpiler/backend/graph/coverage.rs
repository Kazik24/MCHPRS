//! Cold reporting only: no counters, scans or allocations are added to ticking.
use super::*;
use crate::redpiler::GraphExecutionCoverage;

impl GraphBackend {
    pub(crate) fn execution_coverage(&self) -> GraphExecutionCoverage {
        let classic = self.program.classic();
        GraphExecutionCoverage {
            component_nodes: self
                .program
                .nodes()
                .iter()
                .filter(|node| !matches!(node.kind, CircuitNodeKind::Contact(_)))
                .count(),
            wire_nodes: self.program.wires().len(),
            piston_nodes: self.program.pistons().len(),
            compiled_classic_wires: classic
                .nets
                .iter()
                .enumerate()
                .filter(|(index, _)| self.state.classic_active[*index])
                .map(|(_, net)| net.wires.len())
                .sum(),
            certified_classic_wires: classic.wires.len(),
            fixed_classic_nodes: self.fixed_classic.as_ref().map_or(0, |program| program.nodes.len()),
            fixed_classic_wires: self.fixed_classic.as_ref().map_or(0, |program| program.wires.len()),
        }
    }
}
