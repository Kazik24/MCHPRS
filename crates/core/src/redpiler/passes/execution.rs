//! Conservative ordinary groups include physical notifications as well as power links.
use super::super::compile_graph::{CompileGraph, NodeType};
use mchprs_blocks::BlockPos;
use petgraph::unionfind::UnionFind;
use petgraph::visit::{EdgeRef, IntoEdgeReferences, NodeIndexable};
use rustc_hash::{FxHashMap, FxHashSet};

pub(super) fn plan(graph: &mut CompileGraph, requires_callbacks: impl Fn(BlockPos) -> bool) {
    let mut groups = UnionFind::new(graph.node_bound());
    let mut dependencies = petgraph::graph::DiGraph::<(), ()>::new();
    for _ in 0..graph.node_bound() {
        dependencies.add_node(());
    }
    let positions: FxHashMap<_, _> = graph
        .node_indices()
        .filter_map(|id| graph[id].block.map(|(pos, _)| (pos, id)))
        .collect();
    for edge in graph.edge_references() {
        dependencies.add_edge(
            petgraph::graph::NodeIndex::new(edge.source().index()),
            petgraph::graph::NodeIndex::new(edge.target().index()),
            (),
        );
        if graph[edge.source()].block.is_some() && graph[edge.target()].block.is_some() {
            groups.union(edge.source().index(), edge.target().index());
        }
    }
    for id in graph.node_indices() {
        if let NodeType::Observer { watched } = graph[id].ty {
            if let Some(source) = positions.get(&watched) {
                dependencies.add_edge(
                    petgraph::graph::NodeIndex::new(source.index()),
                    petgraph::graph::NodeIndex::new(id.index()),
                    (),
                );
            }
        }
    }
    // Two cells cover dust steps, solid conduction, observer watches, far inputs
    // and command chains. Overlap enlarges groups even without an electrical edge.
    for (&pos, &id) in &positions {
        for x in -2i32..=2 {
            for y in -2i32..=2 {
                for z in -2i32..=2 {
                    if x.abs() + y.abs() + z.abs() > 2 {
                        continue;
                    }
                    if let Some(other) = positions.get(&(pos + BlockPos::new(x, y, z))) {
                        groups.union(id.index(), other.index());
                    }
                }
            }
        }
    }
    let mut exact = FxHashSet::default();
    for id in graph.node_indices() {
        if matches!(graph[id].ty, NodeType::Comparator { .. })
            || matches!(graph[id].ty, NodeType::Observer { watched } if positions.get(&watched).is_some_and(|&source| graph[source].ty == NodeType::Wire))
            || graph[id]
                .block
                .is_some_and(|(pos, _)| requires_callbacks(pos))
        {
            exact.insert(groups.find(id.index()));
        }
    }
    for component in petgraph::algo::kosaraju_scc(&dependencies) {
        if component.len() > 1 || dependencies.contains_edge(component[0], component[0]) {
            for id in component {
                if graph.contains_node(id) && graph[id].block.is_some() {
                    exact.insert(groups.find(id.index()));
                }
            }
        }
    }
    for id in graph.node_indices().collect::<Vec<_>>() {
        graph[id].native = graph[id].block.is_some() && exact.contains(&groups.find(id.index()));
    }
}
