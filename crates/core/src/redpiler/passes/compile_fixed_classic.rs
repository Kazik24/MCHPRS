//! Lower an entire supported fixed circuit into dense typed queries and ordered
//! callback/wire instructions. Unsupported whole circuits retain general graph
//! execution with one explicit decline; no partial region or state owner is added.
//! Requires fixed queries, wire facts and callback admission plans. This is static
//! partial evaluation, not a queue-free settled-weight substitution or synthesis proof.
use super::{
    PassContext,
    planning::{PlanBuilder, PlanOutput},
};
use crate::redpiler::{
    PreparationTableStatistics,
    program_graph::{CompileGraph, FixedClassicPlans, FixedClassicProgram},
};
use std::sync::Arc;

pub(crate) struct CompileFixedClassic;
impl PlanBuilder for CompileFixedClassic {
    fn name(&self) -> &'static str {
        "compile-fixed-classic"
    }
    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        let plans = match FixedClassicProgram::compile_graph(graph, context.monitor) {
            Ok(program) => FixedClassicPlans {
                program: Some(Arc::new(program)),
                decline: None,
            },
            Err(reason) if context.monitor.cancelled() => return Err(reason),
            Err(reason) => FixedClassicPlans {
                program: None,
                decline: Some(reason),
            },
        };
        let mut tables = Vec::new();
        let mut record = |name: &str, entries: usize, bytes: usize| {
            tables.push(PreparationTableStatistics {
                stage: "fixed classic",
                table: name.into(),
                entries,
                capacity: entries,
                estimated_bytes: bytes,
            })
        };
        if let Some(program) = &plans.program {
            let inventory = program.inventory();
            record("executable circuit", 1, inventory.estimated_bytes);
            record("compiled nodes", inventory.nodes, 0);
            record("compiled wires", inventory.wires, 0);
            record("capture aliases", inventory.sites, 0);
            record("compiled routes", inventory.routes, 0);
            record("ordered route instructions", inventory.ops, 0);
            record("signal operands", inventory.operands, 0);
        } else if let Some(reason) = &plans.decline {
            record(&format!("declined: {reason}"), 1, 0);
        }
        context.monitor.record_preparation_tables(tables);
        Ok(PlanOutput::FixedClassic(plans))
    }
}
