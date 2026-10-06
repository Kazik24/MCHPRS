use std::collections::hash_map::Entry;

use super::Pass;
use crate::redpiler::compile_graph::{CompileGraph, CompileNode, NodeIdx, NodeState, NodeType};
use crate::redpiler::{CompilerInput, CompilerOptions};
use crate::world::World;
use petgraph::unionfind::UnionFind;
use petgraph::visit::{EdgeRef, IntoEdgeReferences, NodeIndexable};
use petgraph::Direction;
use rustc_hash::{FxHashMap, FxHashSet};

pub struct ConstantCoalesce;

impl<W: World> Pass<W> for ConstantCoalesce {
    fn run_pass(&self, graph: &mut CompileGraph, _: &CompilerOptions, _: &CompilerInput<'_, W>) {
        let mut vertex_sets = UnionFind::new(graph.node_bound());
        for edge in graph.edge_references() {
            let (src, dest) = (edge.source(), edge.target());
            let node = &graph[src];
            if node.ty != NodeType::Constant || !node.is_removable() {
                vertex_sets.union(graph.to_index(src), graph.to_index(dest));
            }
        }

        let mut constant_nodes = FxHashMap::default();
        let mut replacements = FxHashSet::default();
        for i in 0..graph.node_bound() {
            let idx = NodeIdx::new(i);
            if !graph.contains_node(idx) || replacements.contains(&idx) {
                continue;
            }
            let node = &graph[idx];
            if node.ty != NodeType::Constant || !node.is_removable() {
                continue;
            }
            let ss = node.state.output_strength;

            let mut neighbors = graph.neighbors_directed(idx, Direction::Outgoing).detach();
            while let Some((edge, dest)) = neighbors.next(graph) {
                let weight = graph.remove_edge(edge).unwrap();
                let subgraph_component = vertex_sets.find(graph.to_index(dest));

                let constant_idx = match constant_nodes.entry((subgraph_component, ss)) {
                    Entry::Occupied(entry) => *entry.get(),
                    Entry::Vacant(entry) => {
                        let constant_idx = graph.add_node(CompileNode {
                            ty: NodeType::Constant,
                            block: None,
                            state: NodeState::ss(ss),
                            is_input: false,
                            is_output: false,
                        });
                        replacements.insert(constant_idx);
                        *entry.insert(constant_idx)
                    }
                };
                graph.add_edge(constant_idx, dest, weight);
            }
            graph.remove_node(idx);
        }
    }

    fn status_message(&self) -> &'static str {
        "Coalescing constants"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::compile_graph::CompileLink;
    use mchprs_blocks::BlockPos;

    #[test]
    fn replacement_constant_survives_reused_graph_slot() {
        let mut graph = CompileGraph::new();
        let node = |ty, state, is_output| CompileNode {
            ty,
            state,
            is_output,
            is_input: false,
            block: None,
        };
        let constant = graph.add_node(node(NodeType::Constant, NodeState::ss(15), false));
        let hole = graph.add_node(node(NodeType::Lamp, NodeState::default(), false));
        let output = graph.add_node(node(NodeType::Lamp, NodeState::default(), true));
        graph.remove_node(hole);
        graph.add_edge(constant, output, CompileLink::default(0));
        let world = crate::plot::PlotWorld::from_chunks(0, 0, Vec::new(), Default::default());
        let input = CompilerInput {
            world: &world,
            bounds: (BlockPos::new(0, 0, 0), BlockPos::new(0, 0, 0)),
            ticks: &[],
        };
        ConstantCoalesce.run_pass(&mut graph, &CompilerOptions::default(), &input);
        let inputs: Vec<_> = graph
            .neighbors_directed(output, Direction::Incoming)
            .collect();
        assert_eq!(inputs.len(), 1);
        assert_eq!(graph[inputs[0]].state.output_strength, 15);
    }
}
