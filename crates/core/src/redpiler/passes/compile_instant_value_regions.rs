//! Compile bounded electrical-distance rows inside certified transport nets.
use super::{
    PassContext,
    planning::{PlanBuilder, PlanOutput},
};
use crate::redpiler::{PreparationTableStatistics, program_graph::infer_instant_value_regions};

pub(crate) struct CompileInstantValueRegions;

impl PlanBuilder for CompileInstantValueRegions {
    fn name(&self) -> &'static str {
        "compile-instant-value-regions"
    }

    fn build(&self, graph: &crate::redpiler::program_graph::CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        let plan = infer_instant_value_regions(graph, &graph.plans.instant_transport, &graph.plans.instant_drives, context.monitor)?;
        let stats = &plan.stats;
        context.monitor.record_preparation_tables(vec![
            PreparationTableStatistics {
                stage: "instant value regions",
                table: "component wire maps".into(),
                entries: stats.wires,
                capacity: stats.wires,
                estimated_bytes: stats.wires * std::mem::size_of::<u32>() * 3,
            },
            PreparationTableStatistics {
                stage: "instant value regions",
                table: "compact wire capture eligibility".into(),
                entries: stats.compact_wire_captures,
                capacity: stats.compact_capture_slots,
                estimated_bytes: stats.compact_capture_slots * std::mem::size_of::<bool>(),
            },
            PreparationTableStatistics {
                stage: "instant value regions",
                table: "drive groups and conditional arcs".into(),
                entries: stats.drive_groups + stats.conditional_arcs,
                capacity: stats.drive_groups + stats.conditional_arcs,
                estimated_bytes: (stats.drive_groups * std::mem::size_of::<crate::redpiler::program_graph::ValueDriveGroup>())
                    + (stats.conditional_arcs * std::mem::size_of::<crate::redpiler::program_graph::ValueConditionalArc>()),
            },
            PreparationTableStatistics {
                stage: "instant value regions",
                table: "all value-region tables".into(),
                entries: stats.distance_rows,
                capacity: stats.distance_rows,
                estimated_bytes: stats.bytes,
            },
        ]);
        tracing::info!(
            nets = stats.nets,
            regions = stats.regions,
            admitted = stats.admitted,
            declined = stats.declined,
            wires = stats.wires,
            groups = stats.drive_groups,
            conditional_arcs = stats.conditional_arcs,
            rows = stats.distance_rows,
            work = stats.work,
            bytes = stats.bytes,
            declines = ?stats.declines,
            "compiled instant electrical value regions"
        );
        Ok(PlanOutput::InstantValueRegions(plan))
    }
}
