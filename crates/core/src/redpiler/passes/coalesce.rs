use crate::redpiler::compile_graph::{CompileGraph, LinkType, NodeIdx, NodeType};
use itertools::Itertools;
use petgraph::visit::{EdgeRef, NodeIndexable};
use petgraph::Direction;

pub(super) fn run(graph: &mut CompileGraph) -> Result<(), super::GraphError> {
    for i in 0..graph.node_bound() {
        let idx = NodeIdx::new(i);
        if !graph.contains_node(idx) {
            continue;
        }

        let node = &graph[idx];
        // Comparators depend on the link weight as well as the type,
        // we could implement that later if it's beneficial enough.
        if matches!(node.ty, NodeType::Comparator { .. }) || !node.is_removable() {
            continue;
        }

        let Ok(edge) = graph.edges_directed(idx, Direction::Incoming).exactly_one() else {
            continue;
        };

        if edge.weight().ty != LinkType::Default {
            continue;
        }

        let source = edge.source();
        // Comparators can output less than full strength.
        if matches!(graph[source].ty, NodeType::Comparator { .. }) {
            continue;
        }
        let attenuation = edge.weight().attenuation;
        coalesce_outgoing(graph, source, idx, attenuation);
    }
    Ok(())
}

fn coalesce_outgoing(
    graph: &mut CompileGraph,
    source_idx: NodeIdx,
    into_idx: NodeIdx,
    attenuation: u8,
) {
    // Boolean consumers of 0/15 sources ignore attenuation below 15.
    let binary_inputs = matches!(
        graph[into_idx].ty,
        NodeType::Repeater { .. } | NodeType::Torch
    ) && matches!(
        graph[source_idx].ty,
        NodeType::Repeater { .. }
            | NodeType::Torch
            | NodeType::Observer { .. }
            | NodeType::CopperBulb
            | NodeType::Button
            | NodeType::Lever
            | NodeType::PressurePlate
            | NodeType::Trapdoor
    ) && matches!(graph[source_idx].state.output_strength, 0 | 15);
    let mut walk_outgoing = graph
        .neighbors_directed(source_idx, Direction::Outgoing)
        .detach();
    while let Some(edge_idx) = walk_outgoing.next_edge(graph) {
        let dest_idx = graph.edge_endpoints(edge_idx).unwrap().1;
        if dest_idx == into_idx {
            continue;
        }

        let dest = &graph[dest_idx];
        let into = &graph[into_idx];

        if dest.ty == into.ty
            && dest.state == into.state
            && dest.is_removable()
            && graph[edge_idx].ty == LinkType::Default
            && (graph[edge_idx].attenuation == attenuation
                || (binary_inputs && attenuation < 15 && graph[edge_idx].attenuation < 15))
            && graph
                .neighbors_directed(dest_idx, Direction::Incoming)
                .count()
                == 1
        {
            coalesce(graph, dest_idx, into_idx);
        }
    }
}

fn coalesce(graph: &mut CompileGraph, node: NodeIdx, into: NodeIdx) {
    let mut walk_outgoing = graph.neighbors_directed(node, Direction::Outgoing).detach();
    while let Some(edge_idx) = walk_outgoing.next_edge(graph) {
        let dest = graph.edge_endpoints(edge_idx).unwrap().1;
        let weight = graph.remove_edge(edge_idx).unwrap();
        graph.add_edge(into, dest, weight);
    }
    let removed = graph.remove_node(node).unwrap();
    graph[into].block_aliases.extend(removed.block);
    graph[into].block_aliases.extend(removed.block_aliases);
}
