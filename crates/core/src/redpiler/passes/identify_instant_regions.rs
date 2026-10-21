//! Records complete ownership and query/event boundaries before region contraction.
//!
//! Shared contacts, conserved payloads and reset drivers form one ownership closure.
//! Every owned site and state stays observable; crossing occurrences retain their
//! original rows, captures, directions, duplicates and order. No event is deleted.
//! The indexed closure scans routes once; storage is linear in the exposed boundary.
//! Reset provenance alone is not a proof of a combinational truth table.
//! The following proxy pass must supply its own stronger certificate.
use super::PassContext;
use super::planning::{PlanBuilder, PlanOutput};
use crate::redpiler::program_graph::CompileGraph;

pub(crate) struct IdentifyInstantRegions;

impl PlanBuilder for IdentifyInstantRegions {
    fn name(&self) -> &'static str {
        "identify-instant-regions"
    }
    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        use crate::redpiler::analysis::instant_regions::{infer_regions, infer_stable_regions};
        let regions = if context.options.assumes_stable_instant_inputs() {
            infer_stable_regions(graph, context.monitor)?
        } else {
            infer_regions(graph, context.monitor)?
        };
        Ok(PlanOutput::Regions(regions))
    }
}
