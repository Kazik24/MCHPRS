//! Read-only plan construction. Builders return data; only this module publishes it.
use super::PassContext;
use crate::redpiler::program_graph::{
    CompileGraph, DustPathPlan, InstantMachine, InstantProxy, InstantRegion, InstantResetPlan, SiteId, SymbolicKernel, TurboCallbackIndex,
    WireQueryFacts,
};
use std::sync::Arc;

pub(crate) trait PlanBuilder {
    fn name(&self) -> &'static str;
    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String>;

    #[cfg(test)]
    fn run(&self, graph: &mut CompileGraph, context: &PassContext<'_>) -> Result<(), String> {
        let output = self.build(graph, context)?;
        if context.monitor.cancelled() {
            return Err("cancelled before publishing execution plans".into());
        }
        output.publish(graph);
        Ok(())
    }
}

/// Each result names its owner instead of exposing the mutable graph to a builder.
pub(crate) enum PlanOutput {
    TurboCallbacks(Vec<Option<TurboCallbackIndex>>),
    DustPaths(Vec<DustPathPlan>),
    WireQueries(Vec<Option<WireQueryFacts>>),
    WireHops(Vec<Option<SiteId>>),
    InstantResets(Vec<(usize, InstantResetPlan)>),
    Regions(Vec<InstantRegion>),
    Proxies(Vec<InstantProxy>),
    Machine(Option<Arc<InstantMachine>>),
    SymbolicKernels(Vec<Arc<SymbolicKernel>>),
    SymbolicRegions(crate::redpiler::program_graph::SymbolicRegionCompilation),
}

impl PlanOutput {
    pub(super) fn publish(self, graph: &mut CompileGraph) {
        // A rebuilt input must not leave a machine proof tied to the previous plans.
        if !matches!(&self, Self::Machine(_) | Self::SymbolicKernels(_) | Self::SymbolicRegions(_)) {
            graph.plans.instant.machine = None;
        }
        if !matches!(&self, Self::Machine(_) | Self::SymbolicKernels(_) | Self::SymbolicRegions(_)) {
            graph.plans.instant.symbolic_regions.clear();
            graph.plans.instant.symbolic_region_declines.clear();
        }
        if !matches!(&self, Self::SymbolicKernels(_)) {
            graph.plans.instant.symbolic_kernels.clear();
        }
        match self {
            Self::TurboCallbacks(indices) => graph.plans.turbo_callback_indices = indices,
            Self::DustPaths(paths) => graph.plans.dust_paths = paths,
            Self::WireQueries(facts) => {
                for (wire, facts) in graph.wires.iter_mut().zip(facts) {
                    wire.execution.query_facts = facts;
                }
            }
            Self::WireHops(roots) => {
                for (wire, root) in graph.wires.iter_mut().zip(roots) {
                    wire.execution.hop_root = root;
                }
            }
            Self::InstantResets(resets) => {
                for wire in &mut graph.wires {
                    wire.execution.instant_reset = None;
                }
                for (wire, plan) in resets {
                    graph.wires[wire].execution.instant_reset = Some(plan);
                }
            }
            Self::Regions(regions) => {
                graph.plans.instant.regions = regions;
                graph.plans.instant.proxies.clear();
            }
            Self::Proxies(proxies) => graph.plans.instant.proxies = proxies,
            Self::Machine(machine) => graph.plans.instant.machine = machine,
            Self::SymbolicKernels(kernels) => graph.plans.instant.symbolic_kernels = kernels,
            Self::SymbolicRegions(compilation) => {
                graph.plans.instant.symbolic_regions = compilation.programs;
                graph.plans.instant.symbolic_region_declines = compilation.declines;
            }
        }
    }
}
