//! Pass statistics and table-memory accounting shared by preparation and transformations.

use crate::redpiler::analysis::{
    AnalysisError, MAX_GRAPH_PREPARATION_BYTES, callbacks, electrical, geometry,
};
use crate::redpiler::program_graph::{CompileGraph, GraphBuildError, Guard, GuardId, PowerLink};
use crate::redpiler::{GraphCounts, PassStatistics, PreparationTableStatistics, TaskMonitor};
use mchprs_blocks::BlockPos;
use mchprs_blocks::blocks::Block;
use std::time::Instant;

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

pub(super) fn record_tables(
    monitor: &TaskMonitor,
    stage: &'static str,
    tables: Vec<PreparationTableStatistics>,
) -> Result<(), AnalysisError> {
    let estimate = tables
        .iter()
        .map(|table| table.estimated_bytes)
        .sum::<usize>();
    monitor.record_preparation_tables(tables);
    if estimate > MAX_GRAPH_PREPARATION_BYTES {
        return Err(AnalysisError::PreparationMemoryLimit {
            stage,
            estimated_bytes: estimate,
            limit_bytes: MAX_GRAPH_PREPARATION_BYTES,
        });
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

pub(super) fn geometry_table_statistics(
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

pub(super) fn electrical_table_statistics(
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

pub(super) fn callback_table_statistics(
    graph: &callbacks::CallbackDraft,
) -> Vec<PreparationTableStatistics> {
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

pub(super) fn record_lowering_input_tables(
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

pub(super) fn lowered_table_statistics(
    program: &crate::redpiler::program_graph::CircuitProgram,
) -> Vec<PreparationTableStatistics> {
    use crate::redpiler::program_graph::{
        ComparatorInputPlan, ContactSpec, Guard, NodeSpec, PistonId, PistonSpec, PowerLink,
        Profile, ResourceId, ResourceSpec, Route, RouteDelivery, SiteSpec, WirePlan,
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
        table::<Option<ResourceId>>(
            "lowering",
            "piston lane bindings",
            program.pistons().len(),
            program.pistons().len(),
            0,
        ),
        table::<Option<u32>>(
            "lowering",
            "component plan bindings",
            program.nodes().len().saturating_mul(3),
            program.nodes().len().saturating_mul(3),
            0,
        ),
        table::<ResourceId>(
            "lowering",
            "resource contact candidates",
            program
                .resources()
                .iter()
                .map(|resource| resource.contacts.len())
                .sum(),
            program
                .resources()
                .iter()
                .map(|resource| resource.contacts.len())
                .sum(),
            boxed_bytes::<std::ops::Range<u32>>(program.contacts().len()),
        ),
        table::<PistonId>(
            "lowering",
            "base piston candidates",
            program.pistons().len(),
            program.pistons().len(),
            boxed_bytes::<std::ops::Range<u32>>(program.contacts().len()),
        ),
        table::<PistonId>(
            "lowering",
            "head piston candidates",
            program.pistons().len(),
            program.pistons().len(),
            boxed_bytes::<std::ops::Range<u32>>(program.contacts().len()),
        ),
    ]
}

pub(super) fn record_preparation_pass(
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

pub(super) fn record_transformation(
    monitor: &TaskMonitor,
    name: &'static str,
    before: GraphCounts,
    after: GraphCounts,
    start: Instant,
) {
    monitor.record_transformation(PassStatistics {
        name,
        enabled: true,
        before,
        after,
        duration: start.elapsed(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preparation_memory_limit_accepts_boundary_and_records_rejected_estimate() {
        for estimated_bytes in [
            MAX_GRAPH_PREPARATION_BYTES - 1,
            MAX_GRAPH_PREPARATION_BYTES,
            MAX_GRAPH_PREPARATION_BYTES + 1,
        ] {
            let monitor = TaskMonitor::default();
            let result = record_tables(
                &monitor,
                "test",
                vec![table::<u8>(
                    "test",
                    "bytes",
                    estimated_bytes,
                    estimated_bytes,
                    0,
                )],
            );

            assert_eq!(monitor.preparation_stage_estimate("test"), estimated_bytes);
            if estimated_bytes <= MAX_GRAPH_PREPARATION_BYTES {
                result.unwrap();
            } else {
                assert!(matches!(
                    result,
                    Err(AnalysisError::PreparationMemoryLimit {
                        stage: "test",
                        estimated_bytes: recorded_bytes,
                        limit_bytes: MAX_GRAPH_PREPARATION_BYTES,
                    }) if recorded_bytes == estimated_bytes
                ));
            }
        }
    }
}
