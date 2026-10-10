//! This pass populates the graph with edges.
//! This pass is *mandatory*. Without it, there would be no links between nodes.

use crate::redpiler::compile_graph::{CompileGraph, CompileLink, LinkType, NodeIdx};
use crate::redpiler::CompilerInput;
use crate::redstone::{self, comparator};
use crate::world::World;
use mchprs_blocks::blocks::{Block, RedstoneWire};
use mchprs_blocks::{BlockDirection, BlockFace, BlockPos};
use petgraph::visit::NodeIndexable;
use rustc_hash::FxHashMap;
use std::collections::VecDeque;

pub(super) fn run<W: World>(
    graph: &mut CompileGraph,
    input: &CompilerInput<'_, W>,
) -> Result<(), super::GraphError> {
    let mut state = InputSearchState::new(input.world, graph);
    state.search();
    state.error.map_or(Ok(()), Err)
}

struct InputSearchState<'a, W: World> {
    world: &'a W,
    graph: &'a mut CompileGraph,
    pos_map: FxHashMap<BlockPos, NodeIdx>,
    error: Option<super::GraphError>,
}

impl<'a, W: World> InputSearchState<'a, W> {
    fn new(world: &'a W, graph: &'a mut CompileGraph) -> InputSearchState<'a, W> {
        let mut pos_map = FxHashMap::default();
        for id in graph.node_indices() {
            if let Some((pos, _)) = graph[id].block {
                pos_map.insert(pos, id);
            }
        }

        InputSearchState {
            world,
            graph,
            pos_map,
            error: None,
        }
    }

    fn provides_weak_power(&self, block: Block, side: BlockFace, pos: BlockPos) -> bool {
        redstone::power::emits_weak_power(block, self.world, pos, side, false)
    }

    fn provides_strong_power(&self, block: Block, side: BlockFace, pos: BlockPos) -> bool {
        redstone::power::emits_strong_power(block, self.world, pos, side, false)
    }

    fn link_source(&mut self, source: BlockPos, target: NodeIdx, ty: LinkType, distance: u8) {
        if let Some(&node) = self.pos_map.get(&source) {
            self.graph
                .add_edge(node, target, CompileLink::new(ty, distance));
        } else if self.error.is_none() {
            self.error = Some(super::GraphError::MissingSource { pos: source });
        }
    }

    fn wire_reaches_side(
        &self,
        wire: RedstoneWire,
        side: BlockFace,
        pos: BlockPos,
        search_wire: bool,
    ) -> bool {
        match side {
            BlockFace::Top => true,
            BlockFace::Bottom => false,
            _ => {
                search_wire
                    && redstone::power::emits_weak_power(
                        Block::RedstoneWire { wire },
                        self.world,
                        pos,
                        side,
                        true,
                    )
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn get_redstone_links(
        &mut self,
        block: Block,
        side: BlockFace,
        pos: BlockPos,
        link_ty: LinkType,
        distance: u8,
        start_node: NodeIdx,
        search_wire: bool,
    ) {
        if block.is_solid() {
            for side in &BlockFace::values() {
                let pos = pos.offset(*side);
                let block = self.world.get_block(pos);
                if self.provides_strong_power(block, *side, pos) {
                    self.link_source(pos, start_node, link_ty, distance);
                }

                if let Block::RedstoneWire { wire } = block {
                    if search_wire && self.wire_reaches_side(wire, *side, pos, search_wire) {
                        self.search_wire(start_node, pos, link_ty, distance);
                    }
                }
            }
        } else if self.provides_weak_power(block, side, pos) {
            self.link_source(pos, start_node, link_ty, distance);
        } else if let Block::RedstoneWire { wire } = block {
            if self.wire_reaches_side(wire, side, pos, search_wire) {
                self.search_wire(start_node, pos, link_ty, distance);
            }
        }
    }

    fn search_wire(
        &mut self,
        start_node: NodeIdx,
        root_pos: BlockPos,
        link_ty: LinkType,
        distance: u8,
    ) {
        let mut queue: VecDeque<BlockPos> = VecDeque::new();
        let mut discovered = FxHashMap::default();

        discovered.insert(root_pos, distance);
        queue.push_back(root_pos);

        while let Some(pos) = queue.pop_front() {
            let distance = discovered[&pos];
            // Signals cannot survive fifteen wire steps. Stop before distance
            // arithmetic overflows and before traversing irrelevant long nets.
            if distance >= 15 {
                continue;
            }

            let up_pos = pos.offset(BlockFace::Top);
            let up_block = self.world.get_block(up_pos);

            for side in &BlockFace::values() {
                let neighbor_pos = pos.offset(*side);
                let neighbor = self.world.get_block(neighbor_pos);

                self.get_redstone_links(
                    neighbor,
                    *side,
                    neighbor_pos,
                    link_ty,
                    distance,
                    start_node,
                    false,
                );

                if is_wire(self.world, neighbor_pos) && !discovered.contains_key(&neighbor_pos) {
                    queue.push_back(neighbor_pos);
                    discovered.insert(neighbor_pos, distance + 1);
                }

                if side.is_horizontal() {
                    if !up_block.is_solid() && !neighbor.is_transparent() {
                        let neighbor_up_pos = neighbor_pos.offset(BlockFace::Top);
                        if is_wire(self.world, neighbor_up_pos)
                            && !discovered.contains_key(&neighbor_up_pos)
                        {
                            queue.push_back(neighbor_up_pos);
                            discovered.insert(neighbor_up_pos, distance + 1);
                        }
                    }

                    if !neighbor.is_solid() {
                        let neighbor_down_pos = neighbor_pos.offset(BlockFace::Bottom);
                        if is_wire(self.world, neighbor_down_pos)
                            && !discovered.contains_key(&neighbor_down_pos)
                        {
                            queue.push_back(neighbor_down_pos);
                            discovered.insert(neighbor_down_pos, distance + 1);
                        }
                    }
                }
            }
        }
    }

    fn search_diode_inputs(&mut self, id: NodeIdx, pos: BlockPos, facing: BlockDirection) {
        let input_pos = pos.offset(facing.block_face());
        let input_block = self.world.get_block(input_pos);
        self.get_redstone_links(
            input_block,
            facing.block_face(),
            input_pos,
            LinkType::Default,
            0,
            id,
            true,
        )
    }

    fn search_repeater_side(&mut self, id: NodeIdx, pos: BlockPos, side: BlockDirection) {
        let side_pos = pos.offset(side.block_face());
        let side_block = self.world.get_block(side_pos);
        if redstone::is_diode(side_block)
            && self.provides_weak_power(side_block, side.block_face(), side_pos)
        {
            self.link_source(side_pos, id, LinkType::Side, 0);
        }
    }

    fn search_comparator_side(&mut self, id: NodeIdx, pos: BlockPos, side: BlockDirection) {
        let side_pos = pos.offset(side.block_face());
        let side_block = self.world.get_block(side_pos);
        if (redstone::is_diode(side_block)
            && self.provides_weak_power(side_block, side.block_face(), side_pos))
            || matches!(side_block, Block::RedstoneBlock)
        {
            self.link_source(side_pos, id, LinkType::Side, 0);
        } else if matches!(side_block, Block::RedstoneWire { .. }) {
            self.search_wire(id, side_pos, LinkType::Side, 0)
        }
    }

    fn search_node(&mut self, id: NodeIdx, (pos, block_id): (BlockPos, u32)) {
        match Block::from_id(block_id) {
            Block::RedstoneTorch { .. } => {
                let bottom_pos = pos.offset(BlockFace::Bottom);
                let bottom_block = self.world.get_block(bottom_pos);
                self.get_redstone_links(
                    bottom_block,
                    BlockFace::Top,
                    bottom_pos,
                    LinkType::Default,
                    0,
                    id,
                    true,
                );
            }
            Block::RedstoneWallTorch { facing, .. } => {
                let wall_pos = pos.offset(facing.opposite().block_face());
                let wall_block = self.world.get_block(wall_pos);
                self.get_redstone_links(
                    wall_block,
                    facing.opposite().block_face(),
                    wall_pos,
                    LinkType::Default,
                    0,
                    id,
                    true,
                );
            }
            Block::RedstoneComparator { comparator } => {
                let facing = comparator.facing;

                self.search_comparator_side(id, pos, facing.rotate());
                self.search_comparator_side(id, pos, facing.rotate_ccw());

                let input_pos = pos.offset(facing.block_face());
                let input_block = self.world.get_block(input_pos);
                if comparator::has_override(input_block) {
                    self.link_source(input_pos, id, LinkType::Default, 0);
                } else {
                    self.search_diode_inputs(id, pos, facing);
                }
            }
            Block::RedstoneRepeater { repeater } => {
                let facing = repeater.facing;

                self.search_diode_inputs(id, pos, facing);
                self.search_repeater_side(id, pos, facing.rotate());
                self.search_repeater_side(id, pos, facing.rotate_ccw());
            }
            Block::RedstoneWire { .. } => {
                self.search_wire(id, pos, LinkType::Default, 0);
            }
            block
                if matches!(
                    block,
                    Block::RedstoneLamp { .. }
                        | Block::IronTrapdoor { .. }
                        | Block::NoteBlock { .. }
                ) || block.is_command_block()
                    || block.is_copper_bulb() =>
            {
                for face in &BlockFace::values() {
                    let neighbor_pos = pos.offset(*face);
                    let neighbor_block = self.world.get_block(neighbor_pos);
                    self.get_redstone_links(
                        neighbor_block,
                        *face,
                        neighbor_pos,
                        LinkType::Default,
                        0,
                        id,
                        true,
                    );
                }
            }
            _ => {}
        }
    }

    fn search(&mut self) {
        for i in 0..self.graph.node_bound() {
            let idx = NodeIdx::new(i);
            if !self.graph.contains_node(idx) {
                continue;
            }
            let node = &self.graph[idx];
            if let Some(block) = node.block {
                self.search_node(idx, block);
            }
        }
    }
}

fn is_wire(world: &impl World, pos: BlockPos) -> bool {
    matches!(world.get_block(pos), Block::RedstoneWire { .. })
}
