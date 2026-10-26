//! Compile O6 value logic and its optional device timing boundary.
use super::{
    PassContext,
    planning::{PlanBuilder, PlanOutput},
};
use crate::redpiler::{analysis::instant::layers::logical, program_graph::CompileGraph};

pub(crate) struct CompileSettledCircuit;

impl PlanBuilder for CompileSettledCircuit {
    fn name(&self) -> &'static str {
        "compile-settled-circuit"
    }

    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        let (legacy, devices) = logical::compile_devices(graph, context.monitor)?;
        Ok(PlanOutput::SettledDevices { legacy, devices })
    }
}
