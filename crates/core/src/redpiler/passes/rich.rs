//! Shared geometry, electrical, and callback preparation for the general graph.

use crate::redpiler::analysis::{
    AnalysisError, AnalysisLimits, AnalysisReport, MAX_GRAPH_PREPARATION_BYTES, callbacks,
    electrical, geometry,
};
use crate::redpiler::program_graph::{
    CircuitDraft, CompileGraph, EntryState, EntryTick, GraphBuildError, Guard, GuardId,
    ImportedTick, LoweredCircuit, PowerLink, PowerTarget,
};
use crate::redpiler::{GraphCounts, PassStatistics, PreparationTableStatistics, TaskMonitor};
use crate::world::World;
use mchprs_blocks::BlockPos;
use mchprs_blocks::blocks::Block;
use mchprs_world::TickEntry;
use std::fmt;
use std::time::Instant;

#[derive(Debug)]
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

pub(crate) struct PreparedGraph {
    /// This records admission results and the normalized compilation bounds.
    pub(crate) report: AnalysisReport,
    /// This owns the complete graph passed to transformations, export, and lowering.
    pub(crate) graph: CompileGraph,
    /// This owns the clock and pending work imported at the compilation boundary.
    pub(crate) entry: EntryState,
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
    monitor.begin_graph_statistics(false);
    monitor.set_progress(0);
    monitor.set_max_progress(4);

    monitor.set_message("Identifying nodes and piston contacts".into());
    let start = Instant::now();
    let geometry = geometry::prepare_geometry(world, bounds, &ticks, monitor, limits)
        .map_err(PreparationError::Geometry)?;
    record_tables(
        monitor,
        "geometry",
        geometry_table_statistics("geometry", &geometry.draft),
    )?;
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
    let mut electrical_tables = electrical_table_statistics("electrical", &electrical);
    electrical_tables.extend(geometry_table_statistics("electrical", &geometry.draft));
    record_tables(monitor, "electrical", electrical_tables)?;
    let electrical_counts = GraphCounts {
        nodes: electrical.nodes.len(),
        links: electrical.links.len() + electrical.wire_relations.len(),
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
    let mut callback_tables = electrical_table_statistics("callbacks", &electrical);
    callback_tables.extend(callback_table_statistics(&callbacks));
    callback_tables.extend(geometry_table_statistics("callbacks", &geometry.draft));
    record_tables(monitor, "callbacks", callback_tables)?;
    let complete_counts = GraphCounts {
        nodes: electrical.nodes.len(),
        links: electrical.links.len() + electrical.wire_relations.len() + callbacks.links.len(),
    };
    record_pass(
        monitor,
        "Building ordered callback routes",
        electrical_counts,
        complete_counts,
        start,
    );

    let power_links = electrical
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
        resources: geometry
            .draft
            .resources
            .into_iter()
            .map(|resource| resource.spec)
            .collect(),
        initial_pistons: geometry.draft.initial_pistons,
        initial_resources: geometry.draft.initial_resources,
    };
    let report = geometry.report;
    let pending_ticks = ticks
        .into_iter()
        .map(|tick| EntryTick {
            current_registry_type: if !(0..crate::plot::PLOT_BLOCK_HEIGHT).contains(&tick.pos.y) {
                Some(Block::Air.registry_id())
            } else if world
                .get_chunk(tick.pos.x.div_euclid(16), tick.pos.z.div_euclid(16))
                .is_some()
            {
                Some(world.get_block(tick.pos).registry_id())
            } else {
                None
            },
            tick,
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
        pipeline_start,
    })
}

fn table<T>(
    stage: &'static str,
    name: &'static str,
    entries: usize,
    capacity: usize,
    nested_bytes: usize,
) -> PreparationTableStatistics {
    PreparationTableStatistics {
        stage,
        table: name,
        entries,
        capacity,
        estimated_bytes: capacity
            .saturating_mul(std::mem::size_of::<T>())
            .saturating_add(nested_bytes),
    }
}

fn record_tables(
    monitor: &TaskMonitor,
    stage: &'static str,
    tables: Vec<PreparationTableStatistics>,
) -> Result<(), PreparationError> {
    let estimate = tables
        .iter()
        .map(|table| table.estimated_bytes)
        .sum::<usize>();
    monitor.record_preparation_tables(tables);
    if estimate > MAX_GRAPH_PREPARATION_BYTES {
        return Err(PreparationError::Analysis(
            AnalysisError::PreparationMemoryLimit {
                stage,
                estimated_bytes: estimate,
                limit_bytes: MAX_GRAPH_PREPARATION_BYTES,
            },
        ));
    }
    Ok(())
}

fn boxed_bytes<T>(entries: usize) -> usize {
    entries.saturating_mul(std::mem::size_of::<T>())
}

fn guard_child_bytes(guards: &[Guard]) -> usize {
    guards
        .iter()
        .map(|guard| match guard {
            Guard::All(children) | Guard::Any(children) => boxed_bytes::<GuardId>(children.len()),
            _ => 0,
        })
        .sum()
}

fn geometry_table_statistics(
    stage: &'static str,
    graph: &geometry::GeometryDraft,
) -> Vec<PreparationTableStatistics> {
    use crate::redpiler::program_graph::{CircuitNodeDraft, Profile};
    use geometry::{GeometryContact, GeometryObservation, GeometryPiston, GeometryResource};

    let contact_variant_bytes = graph
        .contacts
        .iter()
        .map(|contact| boxed_bytes::<geometry::GeometryVariant>(contact.variants.capacity()))
        .sum();
    let resource_bytes = graph
        .resources
        .iter()
        .map(|resource| {
            boxed_bytes::<crate::redpiler::program_graph::ContactId>(resource.spec.contacts.len())
                + resource
                    .witnesses
                    .capacity()
                    .saturating_mul(std::mem::size_of::<geometry::ResourceWitness>())
        })
        .sum();
    vec![
        table::<Profile>(
            stage,
            "profiles",
            graph.profiles.len(),
            graph.profiles.capacity(),
            0,
        ),
        table::<CircuitNodeDraft>(stage, "nodes", graph.nodes.len(), graph.nodes.capacity(), 0),
        table::<GeometryContact>(
            stage,
            "contacts",
            graph.contacts.len(),
            graph.contacts.capacity(),
            contact_variant_bytes,
        ),
        table::<GeometryPiston>(
            stage,
            "pistons",
            graph.pistons.len(),
            graph.pistons.capacity(),
            0,
        ),
        table::<GeometryResource>(
            stage,
            "resources",
            graph.resources.len(),
            graph.resources.capacity(),
            resource_bytes,
        ),
        table::<GeometryObservation>(
            stage,
            "observations",
            graph.observations.len(),
            graph.observations.capacity(),
            0,
        ),
    ]
}

fn electrical_table_statistics(
    stage: &'static str,
    graph: &electrical::ElectricalDraft,
) -> Vec<PreparationTableStatistics> {
    use crate::redpiler::program_graph::{
        CircuitNodeDraft, ComparatorInputPlan, ContactSpec, Guard, Profile, SiteSpec, WirePlan,
        WireRelation,
    };

    let contact_variant_bytes = graph
        .contacts
        .iter()
        .map(|contact| {
            boxed_bytes::<crate::redpiler::program_graph::ContactVariant>(contact.variants.len())
        })
        .sum();
    let wire_relation_count = graph.wire_relations.len();
    vec![
        table::<CircuitNodeDraft>(stage, "nodes", graph.nodes.len(), graph.nodes.capacity(), 0),
        table::<Profile>(
            stage,
            "profiles",
            graph.profiles.len(),
            graph.profiles.capacity(),
            0,
        ),
        table::<SiteSpec>(stage, "sites", graph.sites.len(), graph.sites.capacity(), 0),
        table::<Guard>(
            stage,
            "guards",
            graph.guards.len(),
            graph.guards.capacity(),
            guard_child_bytes(&graph.guards),
        ),
        table::<ContactSpec>(
            stage,
            "contacts",
            graph.contacts.len(),
            graph.contacts.capacity(),
            contact_variant_bytes,
        ),
        table::<WirePlan>(
            stage,
            "wire plans",
            graph.wires.len(),
            graph.wires.capacity(),
            0,
        ),
        table::<electrical::ElectricalLink>(
            stage,
            "power links",
            graph.links.len(),
            graph.links.capacity(),
            0,
        ),
        table::<WireRelation>(
            stage,
            "wire relations",
            wire_relation_count,
            graph.wire_relations.capacity(),
            0,
        ),
        table::<ComparatorInputPlan>(
            stage,
            "comparator inputs",
            graph.comparators.len(),
            graph.comparators.capacity(),
            0,
        ),
    ]
}

fn callback_table_statistics(graph: &callbacks::CallbackDraft) -> Vec<PreparationTableStatistics> {
    use crate::redpiler::program_graph::{LinkSpec, PistonSpec, Route, RouteDelivery};
    vec![
        table::<Route>(
            "callbacks",
            "routes",
            graph.routes.len(),
            graph.routes.capacity(),
            0,
        ),
        table::<LinkSpec>(
            "callbacks",
            "callback links",
            graph.links.len(),
            graph.links.capacity(),
            0,
        ),
        table::<PistonSpec>(
            "callbacks",
            "piston specs",
            graph.pistons.len(),
            graph.pistons.capacity(),
            0,
        ),
        table::<RouteDelivery>(
            "callbacks",
            "deliveries",
            graph
                .routes
                .iter()
                .map(|route| route.deliveries.len())
                .sum(),
            graph
                .routes
                .iter()
                .map(|route| route.deliveries.len())
                .sum(),
            0,
        ),
    ]
}

fn record_lowering_input_tables(
    monitor: &TaskMonitor,
    graph: &CompileGraph,
) -> Result<(), GraphBuildError> {
    use crate::redpiler::program_graph::{
        ComparatorInputPlan, ContactSpec, NodeSpec, PistonSpec, Profile, ResourceSpec, WirePlan,
    };
    let contact_variant_bytes = graph
        .contacts
        .iter()
        .map(|contact| {
            boxed_bytes::<crate::redpiler::program_graph::ContactVariant>(contact.variants.len())
        })
        .sum();
    let resource_contact_bytes = graph
        .resources
        .iter()
        .map(|resource| {
            boxed_bytes::<crate::redpiler::program_graph::ContactId>(resource.contacts.len())
        })
        .sum();
    let tables = vec![
        table::<crate::redpiler::program_graph::CircuitNodeDraft>(
            "lowering input",
            "nodes",
            graph.nodes.len(),
            graph.nodes.capacity(),
            0,
        ),
        table::<crate::redpiler::program_graph::LinkSpec>(
            "lowering input",
            "callback links",
            graph.links.len(),
            graph.links.capacity(),
            0,
        ),
        table::<PowerLink>(
            "lowering input",
            "power links",
            graph.power_links.len(),
            graph.power_links.capacity(),
            0,
        ),
        table::<crate::redpiler::program_graph::SiteSpec>(
            "lowering input",
            "sites",
            graph.sites.len(),
            graph.sites.capacity(),
            0,
        ),
        table::<Guard>(
            "lowering input",
            "guards",
            graph.guards.len(),
            graph.guards.capacity(),
            guard_child_bytes(&graph.guards),
        ),
        table::<crate::redpiler::program_graph::WireRelation>(
            "lowering input",
            "wire relations",
            graph.wire_relations.len(),
            graph.wire_relations.capacity(),
            0,
        ),
        table::<crate::redpiler::program_graph::Route>(
            "lowering input",
            "routes",
            graph.routes.len(),
            graph.routes.capacity(),
            0,
        ),
        table::<crate::redpiler::program_graph::RouteDelivery>(
            "lowering input",
            "deliveries",
            graph
                .routes
                .iter()
                .map(|route| route.deliveries.len())
                .sum(),
            graph
                .routes
                .iter()
                .map(|route| route.deliveries.len())
                .sum(),
            0,
        ),
        table::<NodeSpec>(
            "lowering input",
            "lowered nodes projected",
            graph.nodes.len(),
            graph.nodes.len(),
            0,
        ),
        table::<Profile>(
            "lowering input",
            "profiles",
            graph.profiles.len(),
            graph.profiles.capacity(),
            boxed_bytes::<Block>(graph.profiles.len()),
        ),
        table::<ContactSpec>(
            "lowering input",
            "contacts",
            graph.contacts.len(),
            graph.contacts.capacity(),
            contact_variant_bytes,
        ),
        table::<WirePlan>(
            "lowering input",
            "wire plans",
            graph.wires.len(),
            graph.wires.capacity(),
            0,
        ),
        table::<PistonSpec>(
            "lowering input",
            "pistons",
            graph.pistons.len(),
            graph.pistons.capacity(),
            0,
        ),
        table::<ResourceSpec>(
            "lowering input",
            "resources",
            graph.resources.len(),
            graph.resources.capacity(),
            resource_contact_bytes,
        ),
        table::<ComparatorInputPlan>(
            "lowering input",
            "comparator inputs",
            graph.comparator_inputs.len(),
            graph.comparator_inputs.capacity(),
            0,
        ),
        table::<crate::redpiler::program_graph::PistonState>(
            "lowering input",
            "initial piston states",
            graph.initial_pistons.len(),
            graph.initial_pistons.capacity(),
            0,
        ),
        table::<crate::redpiler::program_graph::ResourceState>(
            "lowering input",
            "initial resource states",
            graph.initial_resources.len(),
            graph.initial_resources.capacity(),
            0,
        ),
        table::<(BlockPos, crate::redpiler::program_graph::NodeId)>(
            "lowering input",
            "position index projected",
            graph
                .nodes
                .iter()
                .filter(|node| node.position.is_some())
                .count(),
            graph.nodes.len(),
            0,
        ),
        table::<(
            crate::redpiler::program_graph::PowerTarget,
            std::ops::Range<usize>,
        )>(
            "lowering input",
            "power index projected",
            graph.power_links.len(),
            graph.power_links.len(),
            0,
        ),
    ];
    let estimate = tables.iter().map(|table| table.estimated_bytes).sum();
    monitor.record_preparation_tables(tables);
    if estimate > MAX_GRAPH_PREPARATION_BYTES {
        return Err(GraphBuildError::PreparationMemoryLimit {
            stage: "lowering",
            estimated_bytes: estimate,
            limit_bytes: MAX_GRAPH_PREPARATION_BYTES,
        });
    }
    Ok(())
}

fn lowered_table_statistics(
    program: &crate::redpiler::program_graph::CircuitProgram,
) -> Vec<PreparationTableStatistics> {
    use crate::redpiler::program_graph::{
        ComparatorInputPlan, ContactSpec, Guard, LinkSpec, NodeSpec, PistonSpec, PowerLink,
        Profile, ResourceSpec, Route, RouteDelivery, SiteSpec, WirePlan,
    };
    let routes = program.routes();
    vec![
        table::<NodeSpec>(
            "lowering",
            "nodes",
            program.nodes().len(),
            program.nodes().len(),
            0,
        ),
        table::<LinkSpec>(
            "lowering",
            "callback links",
            program.links().len(),
            program.links().len(),
            0,
        ),
        table::<PowerLink>(
            "lowering",
            "power links",
            program.power_links().len(),
            program.power_links().len(),
            0,
        ),
        table::<ComparatorInputPlan>(
            "lowering",
            "comparator inputs",
            program.comparator_inputs().len(),
            program.comparator_inputs().len(),
            0,
        ),
        table::<Profile>(
            "lowering",
            "profiles",
            program.profiles().len(),
            program.profiles().len(),
            boxed_bytes::<Block>(program.profiles().len()),
        ),
        table::<ContactSpec>(
            "lowering",
            "contacts",
            program.contacts().len(),
            program.contacts().len(),
            program
                .contacts()
                .iter()
                .map(|contact| {
                    boxed_bytes::<crate::redpiler::program_graph::ContactVariant>(
                        contact.variants.len(),
                    )
                })
                .sum(),
        ),
        table::<SiteSpec>(
            "lowering",
            "sites",
            program.sites().len(),
            program.sites().len(),
            0,
        ),
        table::<Guard>(
            "lowering",
            "guards",
            program.guards().len(),
            program.guards().len(),
            guard_child_bytes(program.guards()),
        ),
        table::<Route>("lowering", "routes", routes.len(), routes.len(), 0),
        table::<RouteDelivery>(
            "lowering",
            "deliveries",
            routes.iter().map(|route| route.deliveries.len()).sum(),
            routes.iter().map(|route| route.deliveries.len()).sum(),
            0,
        ),
        table::<WirePlan>(
            "lowering",
            "wire plans",
            program.wires().len(),
            program.wires().len(),
            0,
        ),
        table::<PistonSpec>(
            "lowering",
            "pistons",
            program.pistons().len(),
            program.pistons().len(),
            0,
        ),
        table::<ResourceSpec>(
            "lowering",
            "resources",
            program.resources().len(),
            program.resources().len(),
            program
                .resources()
                .iter()
                .map(|resource| {
                    boxed_bytes::<crate::redpiler::program_graph::ContactId>(
                        resource.contacts.len(),
                    )
                })
                .sum(),
        ),
    ]
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
