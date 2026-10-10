//! A constant side input bounds a subtract comparator's output after side-edge attenuation.
//! The bound also includes its saved output until the comparator reevaluates.
//! Outgoing edges with attenuation at least that bound can be safely removed.
//!
//! Basically, links from comparators that could never possibly output a signal great enough that
//! it won't be zero'd out by the weight of the link get removed.

use crate::redpiler::compile_graph::{CompileGraph, LinkType, NodeIdx, NodeType};
use mchprs_blocks::blocks::ComparatorMode;
use petgraph::Direction;
use petgraph::visit::{EdgeRef, NodeIndexable};

pub(super) fn run(graph: &mut CompileGraph) -> Result<(), super::GraphError> {
    for i in 0..graph.node_bound() {
        let idx = NodeIdx::new(i);
        if !graph.contains_node(idx) || graph[idx].state.pending_tick {
            continue;
        }

        if !matches!(
            graph[idx].ty,
            NodeType::Comparator {
                mode: ComparatorMode::Subtract,
                ..
            }
        ) {
            continue;
        }

        // For simiplicity, we always use 15 here. A more complex implementation in the future
        // might want to properly calculate this.
        let max_input: u8 = 15;

        let mut side_inputs = graph
            .edges_directed(idx, Direction::Incoming)
            .filter(|e| e.weight().ty == LinkType::Side);
        let Some(constant_edge) = side_inputs.next() else {
            continue;
        };
        let constant_idx = constant_edge.source();

        // We only accept one constant input for now. In the future we might wan't to coalesce
        // multiple constant inputs together to make this work, most likely in another pass.
        if side_inputs.next().is_some() {
            continue;
        }

        if graph[constant_idx].ty != NodeType::Constant {
            continue;
        }

        let side_power = graph[constant_idx]
            .state
            .output_strength
            .saturating_sub(constant_edge.weight().attenuation);
        let max_output = max_input
            .saturating_sub(side_power)
            .max(graph[idx].state.output_strength);

        // Now we can go through all the outgoing nodes and remove the ones with a weight that
        // is too high.
        let mut outgoing = graph.neighbors_directed(idx, Direction::Outgoing).detach();
        while let Some((edge_idx, _)) = outgoing.next(graph) {
            if graph[edge_idx].attenuation >= max_output {
                graph.remove_edge(edge_idx);
            }
        }
    }
    Ok(())
}
