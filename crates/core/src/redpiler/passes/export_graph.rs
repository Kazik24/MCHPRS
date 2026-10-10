use crate::redpiler::compile_graph::{
    CompileGraph, LinkType as CLinkType, NodeIdx, NodeType as CNodeType,
};
use itertools::Itertools;
use mchprs_blocks::blocks::ComparatorMode as CComparatorMode;
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use redpiler_graph::{
    serialize, BlockPos, ComparatorMode, Link, LinkType, Node, NodeState, NodeType,
};
use rustc_hash::FxHashMap;
use std::fs;

fn convert_node(
    graph: &CompileGraph,
    node_idx: NodeIdx,
    nodes_map: &FxHashMap<NodeIdx, usize>,
) -> Node {
    let node = &graph[node_idx];

    let mut inputs = Vec::new();
    for edge in graph.edges_directed(node_idx, Direction::Incoming) {
        let idx = nodes_map[&edge.source()];
        let weight = edge.weight();
        inputs.push(Link {
            ty: match weight.ty {
                CLinkType::Default => LinkType::Default,
                CLinkType::Side => LinkType::Side,
            },
            weight: weight.attenuation,
            to: idx,
        });
    }

    let updates = graph
        .neighbors_directed(node_idx, Direction::Outgoing)
        .map(|idx| nodes_map[&idx])
        .collect();

    let facing_diode = match node.ty {
        CNodeType::Repeater { facing_diode, .. } | CNodeType::Comparator { facing_diode, .. } => {
            facing_diode
        }
        _ => false,
    };

    let comparator_far_input = match node.ty {
        CNodeType::Comparator { far_input, .. } => far_input,
        _ => None,
    };

    Node {
        ty: match node.ty {
            CNodeType::Repeater { delay, .. } => NodeType::Repeater(delay),
            CNodeType::Torch => NodeType::Torch,
            CNodeType::Observer { .. } => {
                unreachable!("observer export rejected before lowering")
            }
            CNodeType::Comparator { mode, .. } => NodeType::Comparator(match mode {
                CComparatorMode::Compare => ComparatorMode::Compare,
                CComparatorMode::Subtract => ComparatorMode::Subtract,
            }),
            CNodeType::Lamp => NodeType::Lamp,
            CNodeType::CopperBulb => unreachable!("copper-bulb export rejected before lowering"),
            CNodeType::Button => NodeType::Button,
            CNodeType::Lever => NodeType::Lever,
            CNodeType::PressurePlate => NodeType::PressurePlate,
            CNodeType::Trapdoor => NodeType::Trapdoor,
            CNodeType::Wire => NodeType::Wire,
            CNodeType::Constant => NodeType::Constant,
            CNodeType::NoteBlock { .. } => NodeType::NoteBlock,
            CNodeType::CommandBlock { .. } => {
                unreachable!("command block export rejected before lowering")
            }
        },
        block: node.block.map(|(pos, id)| {
            (
                BlockPos {
                    x: pos.x,
                    y: pos.y,
                    z: pos.z,
                },
                id,
            )
        }),
        state: NodeState {
            output_strength: node.state.output_strength,
            powered: node.state.powered,
            repeater_locked: node.state.repeater_locked,
        },
        comparator_far_input,
        facing_diode,
        inputs,
        updates,
    }
}

pub(super) fn validate(graph: &CompileGraph) -> Result<(), super::GraphError> {
    if graph
        .node_weights()
        .any(|node| node.ty == CNodeType::CopperBulb)
    {
        return Err(super::GraphError::UnsupportedCopperBulbExport);
    }
    if graph
        .node_weights()
        .any(|node| matches!(node.ty, CNodeType::Observer { .. }))
    {
        return Err(super::GraphError::UnsupportedObserverExport);
    }
    if graph
        .node_weights()
        .any(|node| matches!(node.ty, CNodeType::CommandBlock { .. }))
    {
        return Err(super::GraphError::UnsupportedCommandBlockExport);
    }
    Ok(())
}

pub(super) fn run(graph: &mut CompileGraph) -> Result<(), super::GraphError> {
    validate(graph)?;
    let mut nodes_map = FxHashMap::with_capacity_and_hasher(graph.node_count(), Default::default());
    for node in graph.node_indices() {
        nodes_map.insert(node, nodes_map.len());
    }

    let nodes = graph
        .node_indices()
        .map(|idx| convert_node(graph, idx, &nodes_map))
        .collect_vec();

    fs::write("redpiler_graph.bc", serialize(nodes.as_slice()).unwrap())
        .map_err(super::GraphError::Export)?;
    Ok(())
}
