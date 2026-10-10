//! This pass populates the graph with nodes using the input given in [`CompilerInput`].
//! This pass is *mandatory*. Without it, the graph will never be populated.
//!
//! If `optimize` is set in [`CompilerOptions`], redstone wires will not be added to the graph.
//!
//! There are no requirements for this pass.

use crate::redpiler::compile_graph::{CompileGraph, CompileNode, NodeIdx, NodeState, NodeType};
use crate::redpiler::{CompilerInput, CompilerOptions};
use crate::redstone::{self, comparator, noteblock};
use crate::world::{for_each_block_optimized, World};
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::Block;
use mchprs_blocks::BlockPos;
use rustc_hash::{FxHashMap, FxHashSet};

pub(super) fn run<W: World>(
    graph: &mut CompileGraph,
    options: &CompilerOptions,
    input: &CompilerInput<'_, W>,
) -> Result<(), super::GraphError> {
    let ignore_wires = options.optimize;
    let plot = input.world;

    let mut nodes_by_position = FxHashMap::default();
    let mut watched = FxHashSet::default();
    for_each_block_optimized(plot, input.bounds.0, input.bounds.1, |pos| {
        if let Block::Observer { observer } = plot.get_block(pos) {
            watched.insert(pos.offset(observer.facing.into()));
        }
    });

    let (first_pos, second_pos) = input.bounds;

    for_each_block_optimized(plot, first_pos, second_pos, |pos| {
        for_pos(
            graph,
            &mut nodes_by_position,
            ignore_wires && !watched.contains(&pos),
            plot,
            pos,
        );
    });

    for pos in &watched {
        if let Some(&id) = nodes_by_position.get(pos) {
            graph[id].is_output = true;
        }
    }
    for node in graph.node_weights() {
        let NodeType::Observer { watched } = node.ty else {
            continue;
        };
        let observer = node.block.unwrap().0;
        let min = input.bounds.0.min(input.bounds.1);
        let max = input.bounds.0.max(input.bounds.1);
        if watched.x < min.x
            || watched.x > max.x
            || watched.y < min.y
            || watched.y > max.y
            || watched.z < min.z
            || watched.z > max.z
        {
            return Err(super::GraphError::UnsupportedObserverWatch {
                observer,
                watched,
                reason: "the watched cell is outside the compiled selection",
            });
        }
    }

    for entry in input.ticks {
        if let Some(&idx) = nodes_by_position.get(&entry.pos) {
            graph[idx].state.pending_tick = true;
        }
    }
    Ok(())
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
    let dynamic_override = match block {
        Block::RedstoneComparator { comparator } => {
            comparator::get_far_input(world, pos, comparator.facing).is_some() && {
                let far = world.get_block(
                    pos.offset(comparator.facing.block_face())
                        .offset(comparator.facing.block_face()),
                );
                far.is_command_block() || far.is_copper_bulb()
            }
        }
        _ => false,
    };
    let is_output = dynamic_override
        || matches!(
            ty,
            NodeType::Trapdoor
                | NodeType::Lamp
                | NodeType::CopperBulb
                | NodeType::NoteBlock { .. }
                | NodeType::CommandBlock { .. }
                | NodeType::Observer { .. }
        );
    if ignore_wires && ty == NodeType::Wire && !(is_input | is_output) {
        return;
    }

    let node_idx = graph.add_node(CompileNode {
        ty,
        block: Some((pos, id)),
        block_aliases: Vec::new(),
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
        Block::Observer { observer } => (
            NodeType::Observer {
                watched: pos.offset(observer.facing.into()),
            },
            NodeState::simple(observer.powered),
        ),
        Block::RedstoneWire { wire } => (NodeType::Wire, NodeState::with_strength(wire.power)),
        Block::StoneButton { button } => (NodeType::Button, NodeState::simple(button.powered)),
        Block::RedstoneLamp { lit } => (NodeType::Lamp, NodeState::simple(lit)),
        block if block.is_copper_bulb() => {
            let (lit, powered) = block.copper_bulb_state().unwrap();
            (
                NodeType::CopperBulb,
                NodeState::comparator(powered, redstone::bool_to_ss(lit)),
            )
        }
        Block::Lever { lever } => (NodeType::Lever, NodeState::simple(lever.powered)),
        Block::StonePressurePlate { powered } => {
            (NodeType::PressurePlate, NodeState::simple(powered))
        }
        block if block.pressure_plate_powered().is_some() => (
            NodeType::PressurePlate,
            NodeState::simple(block.pressure_plate_powered().unwrap()),
        ),
        Block::IronTrapdoor { powered, .. } => (NodeType::Trapdoor, NodeState::simple(powered)),
        Block::RedstoneBlock => (NodeType::Constant, NodeState::with_strength(15)),
        Block::NoteBlock {
            instrument: _,
            note,
            powered,
        } => {
            let instrument = noteblock::get_noteblock_instrument(world, pos);
            (
                NodeType::NoteBlock { instrument, note },
                NodeState::simple(powered),
            )
        }
        block if block.is_command_block() => {
            let entity = match world.get_block_entity(pos) {
                Some(BlockEntity::CommandBlock(entity)) => Some(entity.as_ref()),
                _ => None,
            };
            (
                NodeType::CommandBlock {
                    repeating: block.get_name() == "repeating_command_block",
                    chain: block.get_name() == "chain_command_block",
                    automatic: entity.is_some_and(|entity| entity.automatic),
                    initial_tick: entity.is_some_and(|entity| {
                        entity.automatic
                            && (entity.last_execution < 0
                                || block.get_name() == "repeating_command_block")
                    }),
                },
                NodeState {
                    powered: entity.is_some_and(|entity| entity.powered),
                    output_strength: comparator::get_override(block, world, pos),
                    ..Default::default()
                },
            )
        }
        block if comparator::has_override(block) => (
            NodeType::Constant,
            NodeState::with_strength(comparator::get_override(block, world, pos)),
        ),
        _ => return None,
    };
    Some((ty, state))
}
