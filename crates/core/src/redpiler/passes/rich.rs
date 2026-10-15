//! Shared geometry, electrical, and callback preparation for the general graph.

use super::utils::{
    callback_table_statistics, electrical_table_statistics, geometry_table_statistics, lowered_table_statistics, record_lowering_input_tables,
    record_preparation_pass, record_tables,
};
use crate::redpiler::analysis::{AnalysisError, AnalysisLimits, AnalysisReport, callbacks, electrical, geometry};
use crate::redpiler::program_graph::{
    CircuitDraft, CompileGraph, EntryState, EntryTick, GraphBuildError, ImportedTick, LoweredCircuit, PowerLink, PowerTarget,
};
use crate::redpiler::{GraphCounts, TaskMonitor};
use crate::world::World;
use mchprs_blocks::BlockPos;
use mchprs_blocks::blocks::Block;
use mchprs_world::TickEntry;
use std::fmt;
use std::time::{Duration, Instant};

#[derive(Debug)]
/// Failure from one of the required stages that prepares a graph for execution.
pub(crate) enum PreparationError {
    /// A retained preparation table crossed the hard memory estimate limit.
    Analysis(AnalysisError),
    /// Geometry preparation failed before a complete graph could be built.
    Geometry(geometry::GeometryError),
    /// Electrical channel preparation failed.
    Electrical(electrical::ElectricalError),
    /// Ordered callback route preparation failed.
    Callbacks(callbacks::CallbackError),
}

impl fmt::Display for PreparationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Analysis(error) => error.fmt(f),
            Self::Geometry(error) => error.fmt(f),
            Self::Electrical(error) => error.fmt(f),
            Self::Callbacks(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for PreparationError {}

/// Fully parsed graph and entry state before lowering into an execution program.
pub(crate) struct PreparedGraph {
    /// This records admission results and the normalized compilation bounds.
    pub(crate) report: AnalysisReport,
    /// This owns the complete graph passed to transformations, export, and lowering.
    pub(crate) graph: CompileGraph,
    /// This owns the clock and pending work imported at the compilation boundary.
    pub(crate) entry: EntryState,
    /// This records the single admission scan performed inside geometry preparation.
    pub(crate) analysis_duration: Duration,
    /// This spans required parsing and the configured transformation sequence.
    pipeline_start: Instant,
}

impl PreparedGraph {
    pub(crate) fn lower(self, monitor: &TaskMonitor) -> Result<LoweredCircuit, GraphBuildError> {
        let logical_tick = self.entry.logical_tick;
        let pending_ticks = self.entry.pending_ticks;
        record_lowering_input_tables(monitor, &self.graph)?;
        let (program, state) = self.graph.lower()?;
        monitor.record_preparation_tables(lowered_table_statistics(&program));
        let positions = program.position_index();
        let imported_ticks = pending_ticks
            .into_iter()
            .map(|entry| ImportedTick {
                target: positions.get(&entry.tick.pos).copied(),
                position: entry.tick.pos,
                block_type: entry.tick.block_type,
                delay: entry.tick.ticks_left,
                priority: entry.tick.tick_priority,
                current_registry_type: entry.current_registry_type,
            })
            .collect();
        Ok(LoweredCircuit {
            program,
            state,
            logical_tick,
            imported_ticks,
            positions,
        })
    }

    pub(crate) fn finish_pipeline_statistics(&self, monitor: &TaskMonitor) {
        monitor.set_final_graph(GraphCounts::of(&self.graph));
        monitor.finish_graph_statistics(self.pipeline_start.elapsed());
    }
}

/// Both execution and read-only inspection use these same required parser stages.
pub(crate) fn prepare(
    world: &impl World,
    bounds: (BlockPos, BlockPos),
    ticks: Vec<TickEntry>,
    monitor: &TaskMonitor,
    limits: AnalysisLimits,
) -> Result<PreparedGraph, PreparationError> {
    let pipeline_start = Instant::now();
    monitor.begin_graph_statistics();
    monitor.set_progress(0);
    monitor.set_max_progress(4);

    monitor.set_message("Identifying nodes and piston contacts".into());
    let start = Instant::now();
    let geometry = geometry::prepare_geometry(world, bounds, &ticks, monitor, limits).map_err(PreparationError::Geometry)?;

    record_tables(monitor, "geometry", geometry_table_statistics("geometry", &geometry.draft)).map_err(PreparationError::Analysis)?;

    let geometry_counts = GraphCounts::new(geometry.draft.nodes.len(), 0);
    record_preparation_pass(
        monitor,
        "Identifying nodes and piston contacts",
        GraphCounts::default(),
        geometry_counts,
        start,
    );

    monitor.set_message("Searching for electrical links".into());
    let start = Instant::now();
    let mut electrical =
        electrical::prepare_electrical(world, geometry.report.bounds, &geometry.draft, monitor).map_err(PreparationError::Electrical)?;
    let mut electrical_tables = electrical_table_statistics("electrical", &electrical);
    electrical_tables.extend(geometry_table_statistics("electrical", &geometry.draft));
    record_tables(monitor, "electrical", electrical_tables).map_err(PreparationError::Analysis)?;
    let electrical_counts = GraphCounts::new(electrical.nodes.len(), electrical.links.len() + electrical.wire_relations.len());
    record_preparation_pass(monitor, "Searching for electrical links", geometry_counts, electrical_counts, start);

    monitor.set_message("Building ordered callback routes".into());
    let start = Instant::now();
    let callbacks = callbacks::prepare_callbacks(world, &geometry.draft, &mut electrical, monitor).map_err(PreparationError::Callbacks)?;
    let mut callback_tables = electrical_table_statistics("callbacks", &electrical);
    callback_tables.extend(callback_table_statistics(&callbacks));
    callback_tables.extend(geometry_table_statistics("callbacks", &geometry.draft));
    record_tables(monitor, "callbacks", callback_tables).map_err(PreparationError::Analysis)?;
    let complete_counts = GraphCounts::new(
        electrical.nodes.len(),
        electrical.links.len() + electrical.wire_relations.len() + callbacks.links.len(),
    );
    record_preparation_pass(monitor, "Building ordered callback routes", electrical_counts, complete_counts, start);

    let power_links = electrical
        .links
        .iter()
        .map(|link| PowerLink {
            from: link.from,
            from_port: link.from_port,
            to: match link.to {
                electrical::ElectricalTarget::Input { node, port } => PowerTarget::Input { node, port },
                electrical::ElectricalTarget::Conductor { site, include_dust } => PowerTarget::Conductor { site, include_dust },
            },
            face: link.face,
            attenuation: link.attenuation,
            guard: link.guard,
        })
        .collect();
    let resources = geometry.draft.resources.into_iter().map(|resource| resource.spec).collect();
    let graph = CircuitDraft {
        nodes: electrical.nodes,
        links: callbacks.links,
        power_links,
        comparator_inputs: electrical.comparators,
        profiles: electrical.profiles,
        guards: electrical.guards,
        contacts: electrical.contacts,
        routes: callbacks.routes,

        wires: electrical.wires,

        wire_relations: electrical.wire_relations,
        sites: electrical.sites,
        pistons: callbacks.pistons,
        resources,
        initial_pistons: geometry.draft.initial_pistons,
        initial_resources: geometry.draft.initial_resources,

        plans: Default::default(),
    };
    let analysis_duration = geometry.analysis_duration;
    let report = geometry.report;
    let pending_ticks = ticks
        .into_iter()
        .map(|tick| {
            let current_registry_type = if !(0..crate::plot::PLOT_BLOCK_HEIGHT).contains(&tick.pos.y) {
                Some(Block::Air.registry_id())
            } else if world.get_chunk(tick.pos.x.div_euclid(16), tick.pos.z.div_euclid(16)).is_some() {
                Some(world.get_block(tick.pos).registry_id())
            } else {
                None
            };
            EntryTick { current_registry_type, tick }
        })
        .collect();
    let entry = EntryState {
        logical_tick: world.piston_state().logical_tick,
        pending_ticks,
    };
    monitor.set_parser_complete(GraphCounts::of(&graph));
    Ok(PreparedGraph {
        report,
        graph,
        entry,
        analysis_duration,
        pipeline_start,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lowered_delivery_storage_is_counted_once() {
        use crate::plot::PlotWorld;
        use crate::redpiler::program_graph::RouteDelivery;
        use crate::world::storage::Chunk;
        let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
        let pos = BlockPos::new(5, 20, 5);
        world.set_block(
            pos,
            Block::Lever {
                lever: mchprs_blocks::blocks::Lever::new(mchprs_blocks::blocks::LeverFace::Floor, mchprs_blocks::BlockDirection::North, false),
            },
        );
        world.set_block(pos.offset(mchprs_blocks::BlockFace::East), Block::RedstoneLamp { lit: false });
        let monitor = TaskMonitor::default();
        let prepared = prepare(
            &world,
            (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15)),
            vec![],
            &monitor,
            AnalysisLimits::default(),
        )
        .unwrap();
        let lowered = prepared.lower(&monitor).unwrap();
        let table = lowered_table_statistics(&lowered.program)
            .into_iter()
            .find(|table| table.table == "deliveries")
            .unwrap();
        assert!(table.entries > 0);
        assert_eq!(table.estimated_bytes, table.entries * std::mem::size_of::<RouteDelivery>());
    }
}
