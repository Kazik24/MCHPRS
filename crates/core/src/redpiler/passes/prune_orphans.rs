//! This pass removes any nodes in the graph that aren't transitively connected to an output redstone component by using Depth-First-Search.

use crate::redpiler::compile_graph::CompileGraph;
use itertools::Itertools;
use petgraph::Direction;
use rustc_hash::FxHashSet;

pub(super) fn run(graph: &mut CompileGraph) -> Result<(), super::GraphError> {
    let mut to_visit = graph
        .node_indices()
        .filter(|&idx| !graph[idx].is_removable())
        .collect_vec();

    let mut visited = FxHashSet::default();
    while let Some(idx) = to_visit.pop() {
        if visited.insert(idx) {
            to_visit.extend(graph.neighbors_directed(idx, Direction::Incoming));
        }
    }

    graph.retain_nodes(|_, idx| visited.contains(&idx));
    Ok(())
}
