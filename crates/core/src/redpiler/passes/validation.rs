//! Required validation stages, independent of optimization flags and developer pass ordering.
//!
//! Rewrites can change the circuit and require a full check. Builders borrow the
//! circuit immutably and publish typed products, so checks follow the actual changed
//! product and its dependents. Unreviewed ownership changes retain complete checks.
use super::{Pass, PassContext, PassPipelineError};
use crate::redpiler::TaskMonitor;
use crate::redpiler::analysis::instant::layers::LayerCompilationContext;
use crate::redpiler::program_graph::{BoundaryCatalog, CompileGraph, LayeredInstantPlan, PlanChange};
use std::time::{Duration, Instant};

#[cfg(test)]
mod tests;

/// Reference data is derived here from the runner's input, never from a builder's output.
pub(in crate::redpiler) enum PreparedInstant {
    /// The canonical catalog before publication changes any plan fields.
    Boundaries(BoundaryCatalog),
    Layers {
        /// The canonical causal program used for exact candidate comparison.
        plan: LayeredInstantPlan,
        /// Matching source evidence transferred only after that comparison succeeds.
        context: LayerCompilationContext,
    },
}

impl PreparedInstant {
    fn prepare(request: super::planning::InstantAnalysis, graph: &CompileGraph, context: &PassContext<'_>) -> Result<Self, String> {
        use super::planning::InstantAnalysis;
        match request {
            InstantAnalysis::Boundaries => Ok(Self::Boundaries(crate::redpiler::analysis::instant::boundaries::infer_boundaries(
                graph,
                context.monitor,
            )?)),
            InstantAnalysis::Layers => {
                let catalog = graph
                    .plans
                    .instant
                    .boundaries
                    .as_ref()
                    .ok_or("instant layers require the boundary catalog")?;
                if !context.options.assumes_stable_instant_inputs() {
                    return Err("instant layers require the declared stable-input contract".into());
                }
                let (plan, analysis) = crate::redpiler::analysis::instant::layers::compile_layers_with_context(graph, catalog, context.monitor)?;
                Ok(Self::Layers { plan, context: analysis })
            }
        }
    }
}

pub(super) fn input(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<Option<LayerCompilationContext>, PassPipelineError> {
    graph.validate_for_lowering(monitor).map_err(PassPipelineError::InvalidInput)
}

pub(super) fn run_checked(
    pass: &Pass,
    graph: &mut CompileGraph,
    context: &PassContext<'_>,
    layers: &mut Option<LayerCompilationContext>,
) -> Result<Duration, PassPipelineError> {
    let name = pass.name();
    let failed = |message| PassPipelineError::Failed { pass: name, message };
    let malformed = |source| PassPipelineError::MalformedOutput { pass: name, source };
    let validation_start;
    match pass {
        Pass::Rewrite(rewrite) => {
            *layers = None;
            rewrite.run(graph, context).map_err(failed)?;
            validation_start = Instant::now();
            *layers = graph.validate_for_lowering(context.monitor).map_err(malformed)?;
        }
        Pass::Build(builder) => {
            let reference = builder
                .instant_analysis()
                .map(|request| PreparedInstant::prepare(request, graph, context))
                .transpose()
                .map_err(failed)?;
            let builder_context = PassContext {
                prepared_instant: reference.as_ref(),
                ..*context
            };
            let output = builder.build(graph, &builder_context).map_err(failed)?;
            if context.monitor.cancelled() {
                return Err(PassPipelineError::Cancelled);
            }
            let change = output.publish(graph).map_err(malformed)?;
            validation_start = Instant::now();
            let checked = match (reference, change) {
                (Some(PreparedInstant::Boundaries(expected)), PlanChange::Boundaries) => {
                    graph.validate_boundary_reference(&expected, context.monitor).map_err(malformed)?;
                    None
                }
                (Some(PreparedInstant::Layers { plan, context: analysis }), PlanChange::Layers) => {
                    graph.validate_layer_reference(&plan).map_err(malformed)?;
                    Some(analysis)
                }
                _ => graph.validate_plan_change(change, context.monitor).map_err(malformed)?,
            };
            if change.replaces_layer_analysis() || checked.is_some() {
                *layers = checked;
            }
        }
    }
    Ok(validation_start.elapsed())
}
