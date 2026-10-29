//! Read-only plan construction. Builders return data; only this module publishes it.
use super::PassContext;
use crate::redpiler::program_graph::{
    CompileGraph, DustPathPlan, GraphBuildError, PlanChange, SiteId, TurboCallbackIndex, WireCallbackSelection, WireQueryFacts,
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
        output.publish(graph).map_err(|error| error.to_string())?;
        Ok(())
    }
}

/// Each result names its owner instead of exposing the mutable graph to a builder.
pub(crate) enum PlanOutput {
    MixedClassic(crate::redpiler::program_graph::MixedClassicProgram),
    FixedClassic(crate::redpiler::program_graph::FixedClassicPlans),
    FixedSignals(crate::redpiler::program_graph::FixedSignalPlans),
    ClassicNets(Vec<crate::redpiler::program_graph::ClassicNetPlan>),
    TurboCallbacks(Vec<Option<TurboCallbackIndex>>),
    DustPaths(Vec<DustPathPlan>),
    WireQueries(Vec<Option<WireQueryFacts>>),
    WireShapes(Vec<Option<crate::redpiler::program_graph::WireShape>>),
    WireCaptures(Vec<crate::redpiler::program_graph::WireCapture>),
    WireCallbacks(Vec<Option<WireCallbackSelection>>),
    WireHops(Vec<Option<SiteId>>),
}

impl PlanOutput {
    /// Check publication shapes before zip/index operations can discard data or panic.
    pub(super) fn publish(self, graph: &mut CompileGraph) -> Result<PlanChange, GraphBuildError> {
        self.validate_shape(graph)?;
        let change = self.change();
        match self {
            Self::MixedClassic(program) => graph.plans.mixed_classic = program,
            Self::FixedClassic(plans) => graph.plans.fixed_classic = plans,
            Self::FixedSignals(plans) => graph.plans.fixed_signals = plans,
            Self::ClassicNets(nets) => graph.plans.classic_nets = nets,
            Self::TurboCallbacks(indices) => graph.plans.turbo_callback_indices = indices,
            Self::WireCaptures(captures) => graph.plans.wire_captures = captures,
            Self::DustPaths(paths) => graph.plans.dust_paths = paths,
            Self::WireQueries(facts) => {
                for (wire, facts) in graph.wires.iter_mut().zip(facts) {
                    wire.execution.query_facts = facts;
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
        }
        Ok(change)
    }

    fn change(&self) -> PlanChange {
        match self {
            Self::MixedClassic(_) => PlanChange::MixedClassic,
            Self::FixedClassic(_) => PlanChange::FixedClassic,
            Self::FixedSignals(_) => PlanChange::FixedSignals,
            Self::ClassicNets(_) => PlanChange::ClassicNets,
            Self::TurboCallbacks(_) => PlanChange::TurboCallbacks,
            Self::DustPaths(_) => PlanChange::DustPaths,
            Self::WireQueries(_) => PlanChange::WireQueries,
            Self::WireShapes(_) => PlanChange::WireShapes,
            Self::WireCaptures(_) => PlanChange::WireCaptures,
            Self::WireCallbacks(_) => PlanChange::WireCallbacks,
            Self::WireHops(_) => PlanChange::WireHops,
        }
    }

    fn validate_shape(&self, graph: &CompileGraph) -> Result<(), GraphBuildError> {
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
            Self::WireShapes(rows) => Some(("wire shapes", rows.len())),
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
        Ok(())
    }
}
