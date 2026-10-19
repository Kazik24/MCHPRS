//! Pass statistics and table-memory accounting shared by preparation and transformations.

pub(crate) mod instant_mechanisms;

use crate::redpiler::analysis::{AnalysisError, MAX_GRAPH_PREPARATION_BYTES, callbacks, electrical, geometry};
use crate::redpiler::program_graph::{CompileGraph, DustPathId, GraphBuildError, Guard, GuardId, NodeSpec, PistonId, ResourceId, RouteDelivery};
use crate::redpiler::{GraphCounts, PassStatistics, PreparationTableStatistics, TaskMonitor};
use mchprs_blocks::BlockPos;
use mchprs_blocks::blocks::Block;
use std::time::Instant;

fn instant_proxy_bytes(proxies: &[crate::redpiler::program_graph::InstantProxy]) -> usize {
    use crate::redpiler::program_graph::{InstantRoute, InstantStep, PrivateInstantCell};
    proxies
        .iter()
        .map(|proxy| {
            boxed_bytes::<PrivateInstantCell>(proxy.private_cells.len())
                + boxed_bytes::<InstantRoute>(proxy.routes.len())
                + proxy
                    .routes
                    .iter()
                    .map(|route| boxed_bytes::<InstantStep>(route.steps.len()))
                    .sum::<usize>()
        })
        .sum()
}

fn instant_region_bytes(regions: &[crate::redpiler::program_graph::InstantRegion]) -> usize {
    use crate::redpiler::program_graph::{ContactId, NodeId, RouteId, RouteOccurrence, SiteId};
    regions
        .iter()
        .map(|region| {
            boxed_bytes::<NodeId>(region.wires.capacity())
                + boxed_bytes::<PistonId>(region.actors.capacity())
                + boxed_bytes::<ResourceId>(region.resources.capacity())
                + boxed_bytes::<NodeId>(region.resets.capacity() + region.wire_readers.capacity() + region.observers.capacity())
                + boxed_bytes::<ContactId>(region.contacts.capacity())
                + boxed_bytes::<SiteId>(region.readable_sites.capacity())
                + boxed_bytes::<u32>(region.power_inputs.capacity() + region.power_readers.capacity())
                + boxed_bytes::<RouteId>(region.routes.capacity())
                + boxed_bytes::<RouteOccurrence>(region.incoming.capacity() + region.outgoing.capacity())
        })
        .sum()
}

pub(super) fn child_before_parent_order(guards: &[Guard], monitor: &TaskMonitor) -> Result<Vec<usize>, String> {
    let mut marks = vec![0u8; guards.len()];
    let mut order = Vec::with_capacity(guards.len());
    let mut stack = Vec::<(usize, usize)>::new();
    for root in 0..guards.len() {
        if marks[root] == 2 {
            continue;
        }
        marks[root] = 1;
        stack.push((root, 0));
        while !stack.is_empty() {
            if monitor.cancelled() {
                return Err("cancelled while ordering guard dependencies".into());
            }
            let (id, child_index) = *stack.last().unwrap();
            let children = guard_children(&guards[id]);
            if child_index == children.len() {
                marks[id] = 2;
                order.push(id);
                stack.pop();
                continue;
            }
            let child = children[child_index].index();
            stack.last_mut().unwrap().1 += 1;
            match marks[child] {
                0 => {
                    marks[child] = 1;
                    stack.push((child, 0));
                }
                1 => return Err(format!("cyclic guard dependency at guard {child}")),
                _ => {}
            }
        }
    }
    Ok(order)
}

pub(super) fn guard_children(guard: &Guard) -> &[GuardId] {
    match guard {
        Guard::Not(child) => std::slice::from_ref(child),
        Guard::All(children) | Guard::Any(children) => children,
        Guard::Always | Guard::State(_) | Guard::Site { .. } => &[],
    }
}

fn table_memory_statistics<T>(
    stage: &'static str,
    name: &'static str,
    entries: usize,
    capacity: usize,
    nested_bytes: usize,
) -> PreparationTableStatistics {
    PreparationTableStatistics {
        stage,
        table: name.to_owned(),
        entries,
        capacity,
        estimated_bytes: capacity.saturating_mul(std::mem::size_of::<T>()).saturating_add(nested_bytes),
    }
}

// A slice would lose the spare allocation that vector accounting must include.
fn vector_table_statistics<T>(stage: &'static str, name: &'static str, values: &Vec<T>, nested_bytes: usize) -> PreparationTableStatistics {
    table_memory_statistics::<T>(stage, name, values.len(), values.capacity(), nested_bytes)
}

fn slice_table_statistics<T>(stage: &'static str, name: &'static str, values: &[T], nested_bytes: usize) -> PreparationTableStatistics {
    table_memory_statistics::<T>(stage, name, values.len(), values.len(), nested_bytes)
}

pub(super) fn record_tables(monitor: &TaskMonitor, stage: &'static str, tables: Vec<PreparationTableStatistics>) -> Result<(), AnalysisError> {
    let estimate = tables.iter().map(|table| table.estimated_bytes).sum::<usize>();
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

pub(super) fn geometry_table_statistics(stage: &'static str, graph: &geometry::GeometryDraft) -> Vec<PreparationTableStatistics> {
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
        vector_table_statistics(stage, "profiles", &graph.profiles, 0),
        vector_table_statistics(stage, "nodes", &graph.nodes, 0),
        vector_table_statistics(stage, "contacts", &graph.contacts, contact_variant_bytes),
        vector_table_statistics(stage, "pistons", &graph.pistons, 0),
        vector_table_statistics(stage, "resources", &graph.resources, resource_bytes),
        vector_table_statistics(stage, "observations", &graph.observations, 0),
    ]
}

pub(super) fn electrical_table_statistics(stage: &'static str, graph: &electrical::ElectricalDraft) -> Vec<PreparationTableStatistics> {
    let contact_variant_bytes = graph
        .contacts
        .iter()
        .map(|contact| boxed_bytes::<crate::redpiler::program_graph::ContactVariant>(contact.variants.len()))
        .sum();
    vec![
        vector_table_statistics(stage, "nodes", &graph.nodes, 0),
        vector_table_statistics(stage, "profiles", &graph.profiles, 0),
        vector_table_statistics(stage, "sites", &graph.sites, 0),
        vector_table_statistics(stage, "guards", &graph.guards, guard_child_bytes(&graph.guards)),
        vector_table_statistics(stage, "contacts", &graph.contacts, contact_variant_bytes),
        vector_table_statistics(stage, "wire plans", &graph.wires, 0),
        vector_table_statistics(stage, "power links", &graph.links, 0),
        vector_table_statistics(stage, "wire relations", &graph.wire_relations, 0),
        vector_table_statistics(stage, "comparator inputs", &graph.comparators, 0),
    ]
}

pub(super) fn callback_table_statistics(graph: &callbacks::CallbackDraft) -> Vec<PreparationTableStatistics> {
    let delivery_count = graph.routes.iter().map(|route| route.deliveries.len()).sum();
    vec![
        vector_table_statistics("callbacks", "routes", &graph.routes, 0),
        vector_table_statistics("callbacks", "callback links", &graph.links, 0),
        vector_table_statistics("callbacks", "piston specs", &graph.pistons, 0),
        table_memory_statistics::<RouteDelivery>("callbacks", "deliveries", delivery_count, delivery_count, 0),
    ]
}

pub(crate) fn record_lowering_input_tables(monitor: &TaskMonitor, graph: &CompileGraph) -> Result<(), GraphBuildError> {
    let contact_variant_bytes = graph
        .contacts
        .iter()
        .map(|contact| boxed_bytes::<crate::redpiler::program_graph::ContactVariant>(contact.variants.len()))
        .sum();
    let resource_contact_bytes = graph
        .resources
        .iter()
        .map(|resource| boxed_bytes::<crate::redpiler::program_graph::ContactId>(resource.contacts.len()))
        .sum();
    let delivery_count = graph.routes.iter().map(|route| route.deliveries.len()).sum();
    let tables = vec![
        PreparationTableStatistics {
            stage: "lowering input",
            table: "whole fixed executable".into(),
            entries: usize::from(graph.plans.fixed_classic.program.is_some()),
            capacity: usize::from(graph.plans.fixed_classic.program.is_some()),
            estimated_bytes: graph
                .plans
                .fixed_classic
                .program
                .as_ref()
                .map_or(0, |program| program.inventory().estimated_bytes),
        },
        vector_table_statistics(
            "lowering input",
            "classic certificates",
            &graph.plans.classic_nets,
            graph
                .plans
                .classic_nets
                .iter()
                .map(|net| {
                    std::mem::size_of_val(net.wires.as_ref())
                        + std::mem::size_of_val(net.inputs.as_ref())
                        + std::mem::size_of_val(net.entries.as_ref())
                        + net.entries.iter().map(|entry| std::mem::size_of_val(entry.ops.as_ref())).sum::<usize>()
                })
                .sum(),
        ),
        PreparationTableStatistics {
            stage: "lowering input",
            table: "classic direct tables projected".into(),
            entries: graph.plans.classic_nets.len(),
            capacity: graph.plans.classic_nets.len(),
            estimated_bytes: crate::redpiler::program_graph::ClassicProgram::estimate_plans(
                &graph.plans.classic_nets,
                graph.nodes.len(),
                graph.routes.len(),
            ) + graph.plans.classic_nets.len(),
        },
        vector_table_statistics(
            "lowering input",
            "instant regions",
            &graph.plans.instant.regions,
            instant_region_bytes(&graph.plans.instant.regions),
        ),
        vector_table_statistics(
            "lowering input",
            "instant proxies",
            &graph.plans.instant.proxies,
            instant_proxy_bytes(&graph.plans.instant.proxies),
        ),
        vector_table_statistics("lowering input", "nodes", &graph.nodes, 0),
        vector_table_statistics("lowering input", "callback links", &graph.links, 0),
        vector_table_statistics("lowering input", "power links", &graph.power_links, 0),
        vector_table_statistics("lowering input", "sites", &graph.sites, 0),
        vector_table_statistics("lowering input", "guards", &graph.guards, guard_child_bytes(&graph.guards)),
        vector_table_statistics("lowering input", "wire relations", &graph.wire_relations, 0),
        vector_table_statistics("lowering input", "routes", &graph.routes, 0),
        table_memory_statistics::<crate::redpiler::program_graph::RouteDelivery>("lowering input", "deliveries", delivery_count, delivery_count, 0),
        table_memory_statistics::<NodeSpec>("lowering input", "lowered nodes projected", graph.nodes.len(), graph.nodes.len(), 0),
        vector_table_statistics("lowering input", "profiles", &graph.profiles, boxed_bytes::<Block>(graph.profiles.len())),
        vector_table_statistics("lowering input", "contacts", &graph.contacts, contact_variant_bytes),
        vector_table_statistics("lowering input", "wire plans", &graph.wires, 0),
        vector_table_statistics("lowering input", "Turbo callback indices", &graph.plans.turbo_callback_indices, 0),
        vector_table_statistics(
            "lowering input",
            "dust paths",
            &graph.plans.dust_paths,
            graph
                .plans
                .dust_paths
                .iter()
                .map(|path| boxed_bytes::<crate::redpiler::program_graph::NodeId>(path.wires.len()))
                .sum(),
        ),
        vector_table_statistics("lowering input", "pistons", &graph.pistons, 0),
        vector_table_statistics("lowering input", "resources", &graph.resources, resource_contact_bytes),
        vector_table_statistics("lowering input", "comparator inputs", &graph.comparator_inputs, 0),
        vector_table_statistics("lowering input", "initial piston states", &graph.initial_pistons, 0),
        vector_table_statistics("lowering input", "initial resource states", &graph.initial_resources, 0),
        table_memory_statistics::<(BlockPos, crate::redpiler::program_graph::NodeId)>(
            "lowering input",
            "position index projected",
            graph.nodes.iter().filter(|node| node.position.is_some()).count(),
            graph.nodes.len(),
            0,
        ),
        table_memory_statistics::<(crate::redpiler::program_graph::PowerTarget, std::ops::Range<usize>)>(
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

pub(crate) fn lowered_table_statistics(program: &crate::redpiler::program_graph::CircuitProgram) -> Vec<PreparationTableStatistics> {
    let routes = program.routes();
    let delivery_count = routes.iter().map(|route| route.deliveries.len()).sum();
    vec![
        PreparationTableStatistics {
            stage: "lowering",
            table: "whole fixed executable".into(),
            entries: usize::from(program.fixed_classic().is_some()),
            capacity: usize::from(program.fixed_classic().is_some()),
            estimated_bytes: program.fixed_classic().map_or(0, |fixed| fixed.inventory().estimated_bytes),
        },
        PreparationTableStatistics {
            stage: "lowering",
            table: "classic signal tables".into(),
            entries: program.classic().nets.len(),
            capacity: program.classic().nets.len(),
            estimated_bytes: program.classic().estimated_bytes(),
        },
        PreparationTableStatistics {
            stage: "lowering",
            table: "committed classic signals".into(),
            entries: program.classic().nets.len(),
            capacity: program.classic().nets.len(),
            estimated_bytes: program.classic().nets.len(),
        },
        slice_table_statistics(
            "lowering",
            "instant proxies",
            program.instant_proxies(),
            instant_proxy_bytes(program.instant_proxies()),
        ),
        table_memory_statistics::<Option<(u32, u32)>>(
            "lowering",
            "instant route bindings",
            routes.len(),
            routes.len(),
            boxed_bytes::<Option<PistonId>>(program.resources().len()),
        ),
        slice_table_statistics("lowering", "nodes", program.nodes(), 0),
        slice_table_statistics("lowering", "power links", program.power_links(), 0),
        slice_table_statistics("lowering", "comparator inputs", program.comparator_inputs(), 0),
        slice_table_statistics("lowering", "profiles", program.profiles(), boxed_bytes::<Block>(program.profiles().len())),
        slice_table_statistics(
            "lowering",
            "contacts",
            program.contacts(),
            program
                .contacts()
                .iter()
                .map(|contact| boxed_bytes::<crate::redpiler::program_graph::ContactVariant>(contact.variants.len()))
                .sum(),
        ),
        slice_table_statistics("lowering", "sites", program.sites(), 0),
        slice_table_statistics("lowering", "guards", program.guards(), guard_child_bytes(program.guards())),
        slice_table_statistics("lowering", "routes", routes, 0),
        table_memory_statistics::<RouteDelivery>("lowering", "deliveries", delivery_count, delivery_count, 0),
        slice_table_statistics("lowering", "wire plans", program.wires(), 0),
        slice_table_statistics("lowering", "Turbo callback indices", program.turbo_callback_indices(), 0),
        slice_table_statistics(
            "lowering",
            "dust paths",
            program.dust_paths(),
            program
                .dust_paths()
                .iter()
                .map(|path| boxed_bytes::<crate::redpiler::program_graph::NodeId>(path.wires.len()))
                .sum(),
        ),
        table_memory_statistics::<Option<DustPathId>>("lowering", "dust path wire bindings", program.wires().len(), program.wires().len(), 0),
        slice_table_statistics("lowering", "pistons", program.pistons(), 0),
        slice_table_statistics(
            "lowering",
            "resources",
            program.resources(),
            program
                .resources()
                .iter()
                .map(|resource| boxed_bytes::<crate::redpiler::program_graph::ContactId>(resource.contacts.len()))
                .sum(),
        ),
        table_memory_statistics::<Option<ResourceId>>("lowering", "piston lane bindings", program.pistons().len(), program.pistons().len(), 0),
        table_memory_statistics::<Option<u32>>(
            "lowering",
            "component plan bindings",
            program.nodes().len().saturating_mul(3),
            program.nodes().len().saturating_mul(3),
            0,
        ),
        table_memory_statistics::<ResourceId>(
            "lowering",
            "resource contact candidates",
            program.resources().iter().map(|resource| resource.contacts.len()).sum(),
            program.resources().iter().map(|resource| resource.contacts.len()).sum(),
            boxed_bytes::<std::ops::Range<u32>>(program.contacts().len()),
        ),
        table_memory_statistics::<PistonId>(
            "lowering",
            "base piston candidates",
            program.pistons().len(),
            program.pistons().len(),
            boxed_bytes::<std::ops::Range<u32>>(program.contacts().len()),
        ),
        table_memory_statistics::<PistonId>(
            "lowering",
            "head piston candidates",
            program.pistons().len(),
            program.pistons().len(),
            boxed_bytes::<std::ops::Range<u32>>(program.contacts().len()),
        ),
    ]
}

pub(super) fn record_preparation_pass(monitor: &TaskMonitor, name: &'static str, before: GraphCounts, after: GraphCounts, start: Instant) {
    monitor.record_pass(PassStatistics {
        name,
        enabled: true,
        before,
        after,
        duration: start.elapsed(),
        validation_duration: std::time::Duration::ZERO,
    });
    monitor.inc_progress();
}

pub(super) fn record_transformation(
    monitor: &TaskMonitor,
    name: &'static str,
    before: GraphCounts,
    after: GraphCounts,
    start: Instant,
    validation_duration: std::time::Duration,
) {
    monitor.record_transformation(PassStatistics {
        name,
        enabled: true,
        before,
        after,
        duration: start.elapsed(),
        validation_duration,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_statistics_preserve_spare_capacity_and_nested_allocations() {
        let mut values = Vec::<u32>::with_capacity(8);
        values.extend([1, 2]);
        let vector = vector_table_statistics("test", "vector", &values, 7);
        let slice = slice_table_statistics("test", "slice", &values, 7);
        assert_eq!(vector.entries, 2);
        assert_eq!(vector.capacity, values.capacity());
        assert_eq!(vector.estimated_bytes, values.capacity() * std::mem::size_of::<u32>() + 7);
        assert_eq!(slice.entries, 2);
        assert_eq!(slice.capacity, 2);
        assert_eq!(slice.estimated_bytes, 2 * std::mem::size_of::<u32>() + 7);
        assert_eq!(slice.stage, "test");
        assert_eq!(slice.table, "slice");
    }

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
                vec![table_memory_statistics::<u8>("test", "bytes", estimated_bytes, estimated_bytes, 0)],
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
