//! # [`IdentifyNodes`]
//!
//! This pass populates the graph with nodes using the input given in [`CompilerInput`].
//! This pass is *mandatory*. Without it, the graph will never be populated.
//!
//! If `optimize` is set in [`CompilerOptions`], redstone wires will not be added to the graph.
//!
//! There are no requirements for this pass.

use super::Pass;
use crate::redpiler::compile_graph::{CompileGraph, CompileNode, NodeIdx, NodeState, NodeType};
use crate::redpiler::{CompilerInput, CompilerOptions};
use crate::redstone::{self, comparator, noteblock};
use crate::world::{for_each_block_optimized, World};
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::Block;
use mchprs_blocks::BlockPos;
use rustc_hash::FxHashMap;

pub struct IdentifyNodes;

impl<W: World> Pass<W> for IdentifyNodes {
    fn run_pass(
        &self,
        graph: &mut CompileGraph,
        options: &CompilerOptions,
        input: &CompilerInput<'_, W>,
    ) -> Result<(), super::GraphError> {
        let ignore_wires = options.optimize;
        let plot = input.world;

        let mut nodes_by_position = FxHashMap::default();

        if let Some(boundaries) = input.boundaries {
            for (group, payload) in boundaries.report.payload_groups.iter().enumerate() {
                for &alias in &payload.positions {
                    graph.add_node(CompileNode {
                        ty: NodeType::MobileSource { group, alias },
                        block: None,
                        state: NodeState::ss(if plot.get_block(alias) == Block::RedstoneBlock {
                            15
                        } else {
                            0
                        }),
                        is_input: false,
                        is_output: false,
                    });
                }
            }
            for piston in 0..boundaries.report.pistons.len() {
                if boundaries.executable {
                    break;
                }
                let strength = boundaries.report.recognition[piston]
                    .inputs
                    .sources
                    .iter()
                    .filter(|source| !boundaries.internal_dependency(piston, source))
                    .map(|source| {
                        redstone::source_strength(
                            plot.get_block(source.source),
                            plot,
                            source.source,
                        )
                        .saturating_sub(source.attenuation)
                    })
                    .max()
                    .unwrap_or(0);
                graph.add_node(CompileNode {
                    ty: NodeType::InstantInput { piston },
                    block: None,
                    state: NodeState::ss(strength),
                    is_input: false,
                    is_output: false,
                });
            }
        }

        let (first_pos, second_pos) = input.bounds;

        for_each_block_optimized(plot, first_pos, second_pos, |pos| {
            if input.boundaries.is_some_and(|b| b.is_owned(pos)) {
                return;
            }
            for_pos(graph, &mut nodes_by_position, ignore_wires, plot, pos);
        });

        if let Some(boundaries) = input.boundaries {
            for node in graph.node_weights_mut() {
                if node
                    .block
                    .is_some_and(|(pos, _)| boundaries.is_retained(pos))
                {
                    node.is_input = true;
                }
                if node.block.is_some_and(|(pos, _)| boundaries.is_output(pos)) {
                    node.is_output = true;
                }
            }
        }

        for entry in input.ticks {
            if let Some(&idx) = nodes_by_position.get(&entry.pos) {
                graph[idx].state.pending_tick = true;
            }
        }
        Ok(())
    }

    fn should_run(&self, _: &CompilerOptions) -> bool {
        // Mandatory
        true
    }

    fn status_message(&self) -> &'static str {
        "Identifying nodes"
    }
}

fn for_pos<W: World>(
    graph: &mut CompileGraph,
    nodes_by_position: &mut FxHashMap<BlockPos, NodeIdx>,
    ignore_wires: bool,
    world: &W,
    pos: BlockPos,
) {
    let id = world.get_block_raw(pos);
    let block = Block::from_id(id);

    let Some((ty, state)) = identify_block(block, pos, world) else {
        return;
    };

    let is_input = matches!(
        ty,
        NodeType::Button | NodeType::Lever | NodeType::PressurePlate
    );
    let is_output = matches!(
        ty,
        NodeType::Trapdoor | NodeType::Lamp | NodeType::NoteBlock { .. }
    );
    if ignore_wires && ty == NodeType::Wire && !(is_input | is_output) {
        return;
    }

    let node_idx = graph.add_node(CompileNode {
        ty,
        block: Some((pos, id)),
        state,

        is_input,
        is_output,
    });
    nodes_by_position.insert(pos, node_idx);
}

fn identify_block<W: World>(
    block: Block,
    pos: BlockPos,
    world: &W,
) -> Option<(NodeType, NodeState)> {
    let (ty, state) = match block {
        Block::RedstoneRepeater { repeater } => (
            NodeType::Repeater {
                delay: repeater.delay,
                facing_diode: redstone::is_diode(
                    world.get_block(pos.offset(repeater.facing.opposite().block_face())),
                ),
            },
            NodeState::repeater(repeater.powered, repeater.locked),
        ),
        Block::RedstoneComparator { comparator } => (
            NodeType::Comparator {
                mode: comparator.mode,
                far_input: comparator::get_far_input(world, pos, comparator.facing),
                facing_diode: redstone::is_diode(
                    world.get_block(pos.offset(comparator.facing.opposite().block_face())),
                ),
            },
            NodeState::comparator(
                comparator.powered,
                if let Some(BlockEntity::Comparator { output_strength }) =
                    world.get_block_entity(pos)
                {
                    *output_strength
                } else {
                    0
                },
            ),
        ),
        Block::RedstoneTorch { lit, .. } | Block::RedstoneWallTorch { lit, .. } => {
            (NodeType::Torch, NodeState::simple(lit))
        }
        Block::RedstoneWire { wire } => (NodeType::Wire, NodeState::ss(wire.power)),
        Block::StoneButton { button } => (NodeType::Button, NodeState::simple(button.powered)),
        Block::RedstoneLamp { lit } => (NodeType::Lamp, NodeState::simple(lit)),
        Block::Lever { lever } => (NodeType::Lever, NodeState::simple(lever.powered)),
        Block::StonePressurePlate { powered } => {
            (NodeType::PressurePlate, NodeState::simple(powered))
        }
        block if block.pressure_plate_powered().is_some() => (
            NodeType::PressurePlate,
            NodeState::simple(block.pressure_plate_powered().unwrap()),
        ),
        Block::IronTrapdoor { powered, .. } => (NodeType::Trapdoor, NodeState::simple(powered)),
        Block::RedstoneBlock => (NodeType::Constant, NodeState::ss(15)),
        Block::NoteBlock {
            instrument: _,
            note,
            powered,
        } if noteblock::is_noteblock_unblocked(world, pos) => {
            let instrument = noteblock::get_noteblock_instrument(world, pos);
            (
                NodeType::NoteBlock { instrument, note },
                NodeState::simple(powered),
            )
        }
        block if comparator::has_override(block) => (
            NodeType::Constant,
            NodeState::ss(comparator::get_override(block, world, pos)),
        ),
        _ => return None,
    };
    Some((ty, state))
}
