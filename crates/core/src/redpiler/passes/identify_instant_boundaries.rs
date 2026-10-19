//! Builds the experimental instant interface without replacing any executable graph work.
//!
//! Cell ownership and connecting corridors are separate. Original row references retain
//! power/QC availability, update/shape activation and observable geometry dependencies.
use super::PassContext;
use super::planning::{InstantAnalysis, PlanBuilder, PlanOutput};
use crate::redpiler::program_graph::CompileGraph;

pub(crate) struct IdentifyInstantBoundaries;

impl PlanBuilder for IdentifyInstantBoundaries {
    fn name(&self) -> &'static str {
        "identify-instant-boundaries"
    }

    fn instant_analysis(&self) -> Option<InstantAnalysis> {
        Some(InstantAnalysis::Boundaries)
    }

    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        let catalog = match context.instant_boundaries() {
            Some(catalog) => catalog.clone(),
            None => crate::redpiler::analysis::instant::boundaries::infer_boundaries(graph, context.monitor)?,
        };
        Ok(PlanOutput::Boundaries(catalog))
    }
}
