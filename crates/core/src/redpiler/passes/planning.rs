//! Read-only plan construction. Builders return data; only this module publishes it.
use super::PassContext;
#[cfg(test)]
use crate::redpiler::program_graph::GuardPredicatePlans;
use crate::redpiler::program_graph::{
    CompileGraph, CpuPowerQueryPlans, DustPathPlan, GraphBuildError, InstantDriveProgram, InstantProxy, InstantRegion, InstantResetPlan,
    InstantTransitionPrograms, InstantTransport, InstantValueRegions, PlanChange, SettledCircuit, SiteId, TurboCallbackIndex, WireCallbackSelection,
    WireQueryFacts,
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
    InstantDrives(InstantDriveProgram),
    InstantTransitionPrograms(InstantTransitionPrograms),
    SignalChannels(crate::redpiler::program_graph::SignalChannels),
    StableInstantLogic(crate::redpiler::program_graph::StableInstantLogic),
    InstantValueRegions(InstantValueRegions),
    SettledCircuit(SettledCircuit),
    ClassicNets(Vec<crate::redpiler::program_graph::ClassicNetPlan>),
    TurboCallbacks(Vec<Option<TurboCallbackIndex>>),
    DustPaths(Vec<DustPathPlan>),
    WireQueries(Vec<Option<WireQueryFacts>>),
    WireShapes(Vec<Option<crate::redpiler::program_graph::WireShape>>),
    WireCaptures(Vec<crate::redpiler::program_graph::WireCapture>),
    WireCaptureMasks(Vec<u32>),
    InstantEvents(Vec<crate::redpiler::program_graph::InstantKernel>),
    CpuKernels(crate::redpiler::program_graph::CpuKernelPlans),
    CpuPowerQueries(CpuPowerQueryPlans),
    #[cfg(test)]
    GuardPredicates(GuardPredicatePlans),
    BudMemoryReads(Vec<crate::redpiler::program_graph::BudMemoryRead>),
    InstantTransport(InstantTransport),
    InstantWireNodes(Vec<Option<crate::redpiler::program_graph::InstantWireNode>>),
    WireCallbacks(Vec<Option<WireCallbackSelection>>),
    AbstractInstantNets(Vec<bool>),
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
                graph.plans.settled_circuit = None;
            }
            Self::Layers(layers) => {
                graph.plans.instant.layers = Some(layers);
                graph.plans.settled_circuit = None;
            }
            Self::FixedSignals(plans) => graph.plans.fixed_signals = plans,
            Self::InstantDrives(plans) => graph.plans.instant_drives = plans,
            Self::InstantTransitionPrograms(plans) => graph.plans.instant_transition_programs = plans,
            Self::SignalChannels(plans) => graph.plans.signal_channels = plans,
            Self::StableInstantLogic(plans) => graph.plans.stable_instant_logic = plans,
            Self::InstantValueRegions(plans) => graph.plans.instant_value_regions = plans,
            Self::SettledCircuit(plan) => graph.plans.settled_circuit = Some(plan),
            Self::ClassicNets(nets) => graph.plans.classic_nets = nets,
            Self::TurboCallbacks(indices) => graph.plans.turbo_callback_indices = indices,
            Self::WireCaptures(captures) => graph.plans.wire_captures = captures,
            Self::WireCaptureMasks(masks) => {
                for (wire, mask) in graph.wires.iter_mut().zip(masks) {
                    wire.execution.mutable_captures = Some(mask);
                }
            }
            Self::InstantEvents(kernels) => {
                graph.plans.instant_events = kernels;
                graph.plans.cpu_kernels = Default::default();
            }
            Self::CpuKernels(plans) => graph.plans.cpu_kernels = plans,
            Self::CpuPowerQueries(plans) => graph.plans.cpu_power_queries = plans,
            #[cfg(test)]
            Self::GuardPredicates(plans) => graph.plans.guard_predicates = plans,
            Self::BudMemoryReads(reads) => {
                graph.plans.bud_memory_reads = reads;
                graph.plans.settled_circuit = None;
            }
            Self::InstantTransport(plan) => graph.plans.instant_transport = plan,
            Self::DustPaths(paths) => graph.plans.dust_paths = paths,
            Self::WireQueries(facts) => {
                for (wire, facts) in graph.wires.iter_mut().zip(facts) {
                    wire.execution.query_facts = facts;
                }
            }
            Self::AbstractInstantNets(masks) => {
                for (wire, enabled) in graph.wires.iter_mut().zip(masks) {
                    wire.execution.abstract_instant_net = enabled;
                }
            }
            Self::InstantWireNodes(nodes) => {
                for (wire, node) in graph.wires.iter_mut().zip(nodes) {
                    wire.execution.instant_node = node;
                }
            }
            Self::WireShapes(shapes) => {
                for (wire, shape) in graph.wires.iter_mut().zip(shapes) {
                    wire.execution.shape = shape;
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
                graph.plans.settled_circuit = None;
            }
            Self::Proxies(proxies) => {
                graph.plans.instant.proxies = proxies;
                graph.plans.settled_circuit = None;
            }
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
            Self::InstantDrives(_) => PlanChange::InstantDrives,
            Self::InstantTransitionPrograms(_) => PlanChange::InstantTransitionPrograms,
            Self::SignalChannels(_) => PlanChange::SignalChannels,
            Self::StableInstantLogic(_) => PlanChange::StableInstantLogic,
            Self::InstantValueRegions(_) => PlanChange::InstantValueRegions,
            Self::SettledCircuit(_) => PlanChange::SettledCircuit,
            Self::ClassicNets(_) => PlanChange::ClassicNets,
            Self::TurboCallbacks(_) => PlanChange::TurboCallbacks,
            Self::DustPaths(_) => PlanChange::DustPaths,
            Self::WireQueries(_) => PlanChange::WireQueries,
            Self::WireShapes(_) => PlanChange::WireShapes,
            Self::WireCaptures(_) => PlanChange::WireCaptures,
            Self::WireCaptureMasks(_) => PlanChange::WireCaptureMasks,
            Self::InstantEvents(_) => PlanChange::InstantEvents,
            Self::CpuKernels(_) => PlanChange::CpuKernels,
            Self::CpuPowerQueries(_) => PlanChange::CpuPowerQueries,
            #[cfg(test)]
            Self::GuardPredicates(_) => PlanChange::GuardPredicates,
            Self::BudMemoryReads(_) => PlanChange::BudMemoryReads,
            Self::InstantTransport(_) => PlanChange::InstantTransport,
            Self::InstantWireNodes(_) => PlanChange::InstantWireNodes,
            Self::AbstractInstantNets(_) => PlanChange::AbstractInstantNets,
            Self::WireCallbacks(_) => PlanChange::WireCallbacks,
            Self::WireHops(_) => PlanChange::WireHops,
            Self::InstantResets(_) => PlanChange::InstantResets,
            Self::Regions(_) => PlanChange::Regions,
            Self::Proxies(_) => PlanChange::Proxies,
        }
    }

    fn validate_shape(&self, graph: &CompileGraph) -> Result<(), GraphBuildError> {
        if let Self::StableInstantLogic(plan) = self {
            if !plan.nets.is_empty() && plan.nets.len() != graph.plans.instant_transport.nets.len() {
                return Err(GraphBuildError::StateTableLength {
                    table: "stable instant logic",
                    descriptions: graph.plans.instant_transport.nets.len(),
                    states: plan.nets.len(),
                });
            }
        }
        if let Self::InstantValueRegions(plan) = self {
            if !plan.nets.is_empty() && plan.nets.len() != graph.plans.instant_transport.nets.len() {
                return Err(GraphBuildError::StateTableLength {
                    table: "instant value regions",
                    descriptions: graph.plans.instant_transport.nets.len(),
                    states: plan.nets.len(),
                });
            }
        }
        if let Self::InstantTransitionPrograms(plan) = self {
            if plan.nets.len() != graph.plans.instant_transport.nets.len() {
                return Err(GraphBuildError::StateTableLength {
                    table: "instant transition programs",
                    descriptions: graph.plans.instant_transport.nets.len(),
                    states: plan.nets.len(),
                });
            }
            for (index, net) in plan.nets.iter().enumerate() {
                if net
                    .as_ref()
                    .is_some_and(|net| net.wire_queries.len() != graph.plans.instant_transport.nets[index].wires.len())
                {
                    return Err(GraphBuildError::InvalidInstantRegion {
                        table: "instant transition program shape",
                        index,
                    });
                }
            }
        }
        if let Self::SignalChannels(plan) = self {
            if plan.channels.len() != graph.plans.instant_transport.nets.len() {
                return Err(GraphBuildError::StateTableLength {
                    table: "signal channels",
                    descriptions: graph.plans.instant_transport.nets.len(),
                    states: plan.channels.len(),
                });
            }
        }
        if let Self::CpuKernels(plans) = self {
            if plans.require_true.len() != plans.require_false.len() || plans.entries.len() > graph.pistons.len() {
                return Err(GraphBuildError::InvalidInstantRegion {
                    table: "CPU kernel shape",
                    index: 0,
                });
            }
        }
        if let Self::CpuPowerQueries(plans) = self {
            if plans.entries.len() > graph.pistons.len() || plans.packed.require_true.len() != plans.packed.require_false.len() {
                return Err(GraphBuildError::InvalidInstantRegion {
                    table: "CPU power query shape",
                    index: 0,
                });
            }
        }
        #[cfg(test)]
        if let Self::GuardPredicates(plans) = self {
            if plans.require_true.len() != plans.require_false.len()
                || plans.by_guard.len() != 0 && plans.by_guard.len() != graph.guards.len()
                || plans.stats.compiled != plans.predicates.len()
                || plans.stats.compiled != plans.by_guard.iter().filter(|&&entry| entry != u32::MAX).count()
                || plans
                    .by_guard
                    .iter()
                    .any(|&entry| entry != u32::MAX && entry as usize >= plans.predicates.len())
            {
                return Err(GraphBuildError::InvalidInstantRegion {
                    table: "guard predicate shape",
                    index: 0,
                });
            }
        }
        if let Self::WireCaptures(rows) = self {
            if !rows.is_empty() && rows.len() != graph.sites.len() {
                return Err(GraphBuildError::StateTableLength {
                    table: "wire captures",
                    descriptions: graph.sites.len(),
                    states: rows.len(),
                });
            }
        }
        let aligned = match self {
            Self::WireQueries(rows) => Some(("wire query facts", rows.len())),
            Self::InstantDrives(plan) => Some(("instant drive wire map", plan.by_wire.len())),
            Self::WireShapes(rows) => Some(("wire shapes", rows.len())),
            Self::WireCaptureMasks(rows) => Some(("wire capture masks", rows.len())),
            Self::InstantWireNodes(rows) => Some(("instant wire nodes", rows.len())),
            Self::AbstractInstantNets(rows) => Some(("abstract instant nets", rows.len())),
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
