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
            instant_kernel_nodes: self.program.instant_kernels().iter().map(|kernel| kernel.actors.len() + kernel.wires.len()).sum(),
            native_nodes: 0,
            private_proxy_pistons: self.program.instant_proxies().iter().map(|proxy| proxy.private_cells.len()).sum(),
            reset_transfer_entries: self.program.wires().iter().filter(|wire| wire.execution.instant_reset.is_some()).count(),
            instant_instruction_routes: self.program.instant_proxies().iter().map(|proxy| proxy.routes.len()).sum(),
            builtin_reset_clocks: self
                .program
                .nodes()
                .iter()
                .filter(|node| matches!(node.kind, CircuitNodeKind::BuiltinReset { .. }))
                .count(),
        }
    }
}
