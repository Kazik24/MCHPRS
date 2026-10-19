//! Read-only plan construction. Builders return data; only this module publishes it.
use super::PassContext;
use crate::redpiler::program_graph::{
    CompileGraph, DustPathPlan, InstantProxy, InstantRegion, InstantResetPlan, SiteId, TurboCallbackIndex, WireCallbackSelection, WireQueryFacts,
};

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
    MixedClassic(crate::redpiler::program_graph::MixedClassicProgram),
    FixedClassic(crate::redpiler::program_graph::FixedClassicPlans),
    Boundaries(crate::redpiler::program_graph::BoundaryCatalog),
    Layers(crate::redpiler::program_graph::LayeredInstantPlan),
    FixedSignals(crate::redpiler::program_graph::FixedSignalPlans),
    ClassicNets(Vec<crate::redpiler::program_graph::ClassicNetPlan>),
    TurboCallbacks(Vec<Option<TurboCallbackIndex>>),
    DustPaths(Vec<DustPathPlan>),
    WireQueries(Vec<Option<WireQueryFacts>>),
    WireCallbacks(Vec<Option<WireCallbackSelection>>),
    WireHops(Vec<Option<SiteId>>),
    InstantResets(Vec<(usize, InstantResetPlan)>),
    Regions(Vec<InstantRegion>),
    Proxies(Vec<InstantProxy>),
}

impl PlanOutput {
    pub(super) fn publish(self, graph: &mut CompileGraph) {
        match self {
            Self::MixedClassic(program) => graph.plans.mixed_classic = program,
            Self::FixedClassic(plans) => graph.plans.fixed_classic = plans,
            Self::Boundaries(catalog) => {
                graph.plans.instant.layers = None;
                graph.plans.instant.boundaries = Some(catalog);
                graph.plans.instant.regions.clear();
                graph.plans.instant.proxies.clear();
            }
            Self::Layers(layers) => graph.plans.instant.layers = Some(layers),
            Self::FixedSignals(plans) => graph.plans.fixed_signals = plans,
            Self::ClassicNets(nets) => graph.plans.classic_nets = nets,
            Self::TurboCallbacks(indices) => graph.plans.turbo_callback_indices = indices,
            Self::DustPaths(paths) => graph.plans.dust_paths = paths,
            Self::WireQueries(facts) => {
                for (wire, facts) in graph.wires.iter_mut().zip(facts) {
                    wire.execution.query_facts = facts;
                }
            }
            Self::WireCallbacks(plans) => {
                for (wire, plan) in graph.wires.iter_mut().zip(plans) {
                    wire.execution.callback_selection = plan;
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
                graph.plans.instant.layers = None;
                graph.plans.instant.boundaries = None;
                graph.plans.instant.regions = regions;
                graph.plans.instant.proxies.clear();
            }
            Self::Proxies(proxies) => graph.plans.instant.proxies = proxies,
        }
    }
}
