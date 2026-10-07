use crate::redpiler::compile_graph::{CompileGraph, LinkType, NodeIdx, NodeType};
use crate::world::World;
use mchprs_blocks::blocks::{Block, ComparatorMode};
use petgraph::visit::{EdgeRef, NodeIndexable};
use petgraph::Direction;
use tracing::trace;

pub(super) fn run(graph: &mut CompileGraph, world: &impl World) -> Result<(), super::GraphError> {
    loop {
        let num_folded = fold(graph, world);
        if num_folded == 0 {
            break;
        }
        trace!("Fold iteration: {} nodes", num_folded);
    }
    Ok(())
}

fn fold(graph: &mut CompileGraph, world: &impl World) -> usize {
    let mut num_folded = 0;

    'nodes: for i in 0..graph.node_bound() {
        let idx = NodeIdx::new(i);
        if !graph.contains_node(idx) || graph[idx].state.pending_tick {
            continue;
        }
        if let Some((pos, id)) = graph[idx].block {
            if let Block::RedstoneComparator { comparator } = Block::from_id(id) {
                let far = pos
                    .offset(comparator.facing.block_face())
                    .offset(comparator.facing.block_face());
                if world.get_block(far).is_command_block() {
                    continue;
                }
            }
        }

        let mut default_power = 0;
        let mut side_power = 0;
        for edge in graph.edges_directed(idx, Direction::Incoming) {
            let constant = &graph[edge.source()];
            if constant.ty != NodeType::Constant {
                continue 'nodes;
            }

            match edge.weight().ty {
                LinkType::Default => {
                    default_power = default_power.max(
                        constant
                            .state
                            .output_strength
                            .saturating_sub(edge.weight().attenuation),
                    )
                }
                LinkType::Side => {
                    side_power = side_power.max(
                        constant
                            .state
                            .output_strength
                            .saturating_sub(edge.weight().attenuation),
                    )
                }
            }
        }

        let new_power = match graph[idx].ty {
            NodeType::Comparator {
                mode, far_input, ..
            } => {
                if let Some(far_override) = far_input {
                    if default_power < 15 {
                        default_power = far_override;
                    }
                }
                match mode {
                    ComparatorMode::Compare => {
                        if default_power >= side_power {
                            default_power
                        } else {
                            0
                        }
                    }
                    ComparatorMode::Subtract => default_power.saturating_sub(side_power),
                }
            }
            NodeType::Repeater { .. } => {
                if graph[idx].state.repeater_locked {
                    graph[idx].state.output_strength
                } else if default_power > 0 {
                    15
                } else {
                    0
                }
            }
            NodeType::Torch => {
                if default_power > 0 {
                    0
                } else {
                    15
                }
            }
            _ => continue,
        };

        // A node whose current output differs from its eventual constant still
        // needs to propagate that transition through the runtime scheduler.
        if graph[idx].state.output_strength != new_power {
            continue;
        }
        graph[idx].ty = NodeType::Constant;
        graph[idx].state.output_strength = new_power;

        let mut incoming = graph.neighbors_directed(idx, Direction::Incoming).detach();
        while let Some(edge) = incoming.next_edge(graph) {
            graph.remove_edge(edge);
        }

        num_folded += 1;
    }

    num_folded
}
