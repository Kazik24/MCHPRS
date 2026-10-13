//! Prepare the general circuit graph independently of its executor or export format.

use crate::redpiler::analysis::{AnalysisLimits, AnalysisReport, callbacks, electrical, geometry};
use crate::redpiler::program_graph::{
    CircuitDraft, CircuitProgram, CircuitState, GraphBuildError, PowerLink, PowerTarget,
};
use crate::redpiler::{GraphCounts, PassStatistics, TaskMonitor};
use crate::world::World;
use mchprs_blocks::BlockPos;
use mchprs_world::TickEntry;
use std::fmt;
use std::time::Instant;

#[derive(Debug)]
pub(crate) enum PreparationError {
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
            Self::Geometry(error) => error.fmt(f),
            Self::Electrical(error) => error.fmt(f),
            Self::Callbacks(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for PreparationError {}

pub(crate) struct PreparedGraph {
    /// This records admission results and the normalized compilation bounds.
    pub(crate) report: AnalysisReport,
    /// This describes bounded piston contacts and shared resources.
    pub(crate) geometry: geometry::GeometryDraft,
    /// This describes components, power channels, and wire query plans.
    pub(crate) electrical: electrical::ElectricalDraft,
    /// This describes ordered callback routes and their typed links.
    pub(crate) callbacks: callbacks::CallbackDraft,
}

impl PreparedGraph {
    pub(crate) fn lower(self) -> Result<(CircuitProgram, CircuitState), GraphBuildError> {
        let power_links = self
            .electrical
            .links
            .iter()
            .map(|link| PowerLink {
                from: link.from,
                from_port: link.from_port,
                to: match link.to {
                    electrical::ElectricalTarget::Input { node, port } => {
                        PowerTarget::Input { node, port }
                    }
                    electrical::ElectricalTarget::Conductor { site, include_dust } => {
                        PowerTarget::Conductor { site, include_dust }
                    }
                },
                face: link.face,
                attenuation: link.attenuation,
                guard: link.guard,
            })
            .collect();
        CircuitDraft {
            nodes: self.electrical.nodes,
            links: self.callbacks.links,
            power_links,
            comparator_inputs: self.electrical.comparators,
            profiles: self.electrical.profiles,
            guards: self.electrical.guards,
            contacts: self.electrical.contacts,
            routes: self.callbacks.routes,
            wires: self.electrical.wires,
            sites: self.electrical.sites,
            pistons: self.callbacks.pistons,
            resources: self
                .geometry
                .resources
                .into_iter()
                .map(|resource| resource.spec)
                .collect(),
            initial_pistons: self.geometry.initial_pistons,
            initial_resources: self.geometry.initial_resources,
        }
        .lower()
    }
}

/// Both execution and read-only inspection use these same required parser passes.
pub(crate) fn prepare(
    world: &impl World,
    bounds: (BlockPos, BlockPos),
    ticks: &[TickEntry],
    monitor: &TaskMonitor,
    limits: AnalysisLimits,
) -> Result<PreparedGraph, PreparationError> {
    let pipeline_start = Instant::now();
    monitor.begin_graph_statistics(false);
    monitor.set_progress(0);
    // Three required parser passes, followed by executable backend construction.
    monitor.set_max_progress(4);

    monitor.set_message("Identifying nodes and piston contacts".into());
    let start = Instant::now();
    let geometry = geometry::prepare_geometry(world, bounds, ticks, monitor, limits)
        .map_err(PreparationError::Geometry)?;
    let geometry_counts = GraphCounts {
        nodes: geometry.draft.nodes.len(),
        links: 0,
    };
    record_pass(
        monitor,
        "Identifying nodes and piston contacts",
        GraphCounts::default(),
        geometry_counts,
        start,
    );

    monitor.set_message("Searching for electrical links".into());
    let start = Instant::now();
    let mut electrical =
        electrical::prepare_electrical(world, geometry.report.bounds, &geometry.draft, monitor)
            .map_err(PreparationError::Electrical)?;
    let electrical_counts = GraphCounts {
        nodes: electrical.nodes.len(),
        links: electrical.links.len(),
    };
    record_pass(
        monitor,
        "Searching for electrical links",
        geometry_counts,
        electrical_counts,
        start,
    );

    monitor.set_message("Building ordered callback routes".into());
    let start = Instant::now();
    let callbacks = callbacks::prepare_callbacks(world, &geometry.draft, &mut electrical, monitor)
        .map_err(PreparationError::Callbacks)?;
    let complete_counts = GraphCounts {
        nodes: electrical.nodes.len(),
        links: electrical.links.len() + callbacks.links.len(),
    };
    record_pass(
        monitor,
        "Building ordered callback routes",
        electrical_counts,
        complete_counts,
        start,
    );
    monitor.finish_graph_statistics(pipeline_start.elapsed());
    Ok(PreparedGraph {
        report: geometry.report,
        geometry: geometry.draft,
        electrical,
        callbacks,
    })
}

fn record_pass(
    monitor: &TaskMonitor,
    name: &'static str,
    before: GraphCounts,
    after: GraphCounts,
    start: Instant,
) {
    monitor.record_pass(PassStatistics {
        name,
        enabled: true,
        before,
        after,
        duration: start.elapsed(),
    });
    monitor.inc_progress();
}
