//! Read-only plan construction. Builders return data; only this module publishes it.
use super::PassContext;
use crate::redpiler::program_graph::{
    CompileGraph, DustPathPlan, GraphBuildError, InstantProxy, InstantRegion, InstantResetPlan, PlanChange, SiteId, TurboCallbackIndex,
    WireCallbackSelection, WireQueryFacts,
};

pub(crate) trait PlanBuilder {
    fn name(&self) -> &'static str;
    /// Request source-derived reference data; this never exempts an output from checking.
    fn instant_analysis(&self) -> Option<InstantAnalysis> {
        None
    }
    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String>;

    #[cfg(test)]
    fn run(&self, graph: &mut CompileGraph, context: &PassContext<'_>) -> Result<(), String> {
        let output = self.build(graph, context)?;
        if context.monitor.cancelled() {
            return Err("cancelled before publishing execution plans".into());
        }
        output.publish(graph).map_err(|error| error.to_string())?;
        Ok(())
    }
}

#[derive(Clone, Copy)]
/// Immutable reference products the runner can prepare from its current validated input.
pub(crate) enum InstantAnalysis {
    /// The complete cell/corridor boundary catalog for this graph and current classic ownership.
    Boundaries,
    /// The causal plan and lowering evidence for this graph and its validated catalog.
    Layers,
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
    /// Check publication shapes before zip/index operations can discard data or panic.
    pub(super) fn publish(self, graph: &mut CompileGraph) -> Result<PlanChange, GraphBuildError> {
        self.validate_shape(graph)?;
        let change = self.change();
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
        Ok(change)
    }

    fn change(&self) -> PlanChange {
        match self {
            Self::MixedClassic(_) => PlanChange::MixedClassic,
            Self::FixedClassic(_) => PlanChange::FixedClassic,
            Self::Boundaries(_) => PlanChange::Boundaries,
            Self::Layers(_) => PlanChange::Layers,
            Self::FixedSignals(_) => PlanChange::FixedSignals,
            Self::ClassicNets(_) => PlanChange::ClassicNets,
            Self::TurboCallbacks(_) => PlanChange::TurboCallbacks,
            Self::DustPaths(_) => PlanChange::DustPaths,
            Self::WireQueries(_) => PlanChange::WireQueries,
            Self::WireCallbacks(_) => PlanChange::WireCallbacks,
            Self::WireHops(_) => PlanChange::WireHops,
            Self::InstantResets(_) => PlanChange::InstantResets,
            Self::Regions(_) => PlanChange::Regions,
            Self::Proxies(_) => PlanChange::Proxies,
        }
    }

    fn validate_shape(&self, graph: &CompileGraph) -> Result<(), GraphBuildError> {
        let aligned = match self {
            Self::WireQueries(rows) => Some(("wire query facts", rows.len())),
            Self::WireCallbacks(rows) => Some(("wire callback selection", rows.len())),
            Self::WireHops(rows) => Some(("wire hop roots", rows.len())),
            Self::TurboCallbacks(rows) if !rows.is_empty() => Some(("Turbo callback indices", rows.len())),
            _ => None,
        };
        if let Some((table, actual)) = aligned {
            if actual != graph.wires.len() {
                return Err(GraphBuildError::StateTableLength {
                    table,
                    descriptions: graph.wires.len(),
                    states: actual,
                });
            }
        }
        if let Self::InstantResets(rows) = self {
            let mut seen = vec![false; graph.wires.len()];
            for &(wire, _) in rows {
                if wire >= seen.len() || std::mem::replace(&mut seen[wire], true) {
                    return Err(GraphBuildError::InvalidInstantReset { wire });
                }
            }
        }
        Ok(())
    }
}
