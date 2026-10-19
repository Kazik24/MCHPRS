//! Derive finite causal programs from the experimental instant boundary catalog.
//!
//! Lowering installs only waves whose effect boundary and queue ownership are certified;
//! the remainder keeps ordinary Graph execution. Discovery order never defines causality.
use super::PassContext;
use super::planning::{InstantAnalysis, PlanBuilder, PlanOutput};
use crate::redpiler::program_graph::CompileGraph;

pub(crate) struct CompileInstantLayers;

impl PlanBuilder for CompileInstantLayers {
    fn name(&self) -> &'static str {
        "compile-instant-layers"
    }

    fn instant_analysis(&self) -> Option<InstantAnalysis> {
        Some(InstantAnalysis::Layers)
    }

    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        let catalog = graph
            .plans
            .instant
            .boundaries
            .as_ref()
            .ok_or("instant layers require the boundary catalog")?;
        if !context.options.stable_instant_inputs {
            return Err("instant layers require the declared stable-input contract".into());
        }
        let layers = match context.instant_layers() {
            Some(layers) => layers.clone(),
            None => crate::redpiler::analysis::instant::layers::compile_layers(graph, catalog, context.monitor)?,
        };
        Ok(PlanOutput::Layers(layers))
    }
}
