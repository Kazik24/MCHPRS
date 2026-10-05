//! The implementation of "Redstone Wire Turbo" was largely based on
//! the accelorator created by theosib. For more information, see:
//! https://bugs.mojang.com/browse/MC-81098.

use crate::redstone;
use crate::world::World;
use mchprs_blocks::blocks::{Block, RedstoneWire};
use mchprs_blocks::{BlockFace, BlockPos};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::collections::hash_map::Entry;

thread_local! {
    // Only allocation capacity survives a walk. Take the scratch space out
    // before dispatching callbacks: recursive wire walks need independent data.
    static SCRATCH: RefCell<Option<RedstoneWireTurbo>> = const { RefCell::new(None) };
}

#[cfg(test)]
#[path = "turbo_tests.rs"]
mod tests;

fn unwrap_wire(block: Block) -> RedstoneWire {
    match block {
        Block::RedstoneWire { wire } => wire,
        _ => panic!("expected wire"),
    }
}

fn wire_mut(block: &mut Block) -> &mut RedstoneWire {
    match block {
        Block::RedstoneWire { wire } => wire,
        _ => panic!("expected wire"),
    }
}

#[derive(Clone, Copy)]
struct NodeId {
    index: usize,
}

struct UpdateNode {
    pos: BlockPos,
    /// The cached state of the block
    state: Block,
    /// Cached for this walk only. Wire power changes preserve the block's kind;
    /// other nodes already use an immutable cached state in this algorithm.
    needs_update: Option<bool>,
    /// This will only be `Some` when all the neighbors are identified.
    neighbors: Option<usize>,
    visited: bool,
    xbias: i32,
    zbias: i32,
    layer: u32,
}

impl UpdateNode {
    fn new(world: &impl World, pos: BlockPos) -> UpdateNode {
        let state = world.get_block(pos);
        UpdateNode {
            pos,
            state,
            needs_update: None,
            visited: false,
            neighbors: None,
            xbias: 0,
            zbias: 0,
            layer: 0,
        }
    }
}

pub(super) struct RedstoneWireTurbo {
    nodes: Vec<UpdateNode>,
    neighbor_lists: Vec<[NodeId; 24]>,
    node_cache: FxHashMap<BlockPos, NodeId>,
    update_queue: Vec<Vec<NodeId>>,
    current_walk_layer: u32,
}

impl RedstoneWireTurbo {
    // Internal numbering for cardinal directions
    const NORTH: usize = 0;
    const EAST: usize = 1;
    const SOUTH: usize = 2;
    const WEST: usize = 3;

    fn new() -> RedstoneWireTurbo {
        RedstoneWireTurbo {
            nodes: Vec::new(),
            neighbor_lists: Vec::new(),
            node_cache: FxHashMap::default(),
            update_queue: vec![vec![], vec![], vec![]],
            current_walk_layer: 0,
        }
    }

    fn get_node(&self, node_id: NodeId) -> &UpdateNode {
        &self.nodes[node_id.index]
    }

    fn compute_all_neighbors(pos: BlockPos) -> [BlockPos; 24] {
        let BlockPos { x, y, z } = pos;
        [
            BlockPos::new(x - 1, y, z),
            BlockPos::new(x + 1, y, z),
            BlockPos::new(x, y - 1, z),
            BlockPos::new(x, y + 1, z),
            BlockPos::new(x, y, z - 1),
            BlockPos::new(x, y, z + 1),
            // Neighbors of neighbors, in the same order,
            // except that duplicates are not included
            BlockPos::new(x - 2, y, z),
            BlockPos::new(x - 1, y - 1, z),
            BlockPos::new(x - 1, y + 1, z),
            BlockPos::new(x - 1, y, z - 1),
            BlockPos::new(x - 1, y, z + 1),
            BlockPos::new(x + 2, y, z),
            BlockPos::new(x + 1, y - 1, z),
            BlockPos::new(x + 1, y + 1, z),
            BlockPos::new(x + 1, y, z - 1),
            BlockPos::new(x + 1, y, z + 1),
            BlockPos::new(x, y - 2, z),
            BlockPos::new(x, y - 1, z - 1),
            BlockPos::new(x, y - 1, z + 1),
            BlockPos::new(x, y + 2, z),
            BlockPos::new(x, y + 1, z - 1),
            BlockPos::new(x, y + 1, z + 1),
            BlockPos::new(x, y, z - 2),
            BlockPos::new(x, y, z + 2),
        ]
    }

    fn compute_heading(rx: i32, rz: i32) -> usize {
        let code = (rx + 1) + 3 * (rz + 1);
        match code {
            0 => Self::NORTH,
            1 => Self::NORTH,
            2 => Self::EAST,
            3 => Self::WEST,
            4 => Self::WEST,
            5 => Self::EAST,
            6 => Self::SOUTH,
            7 => Self::SOUTH,
            8 => Self::SOUTH,
            _ => unreachable!(),
        }
    }

    // const UPDATE_REDSTONE: [bool; 24] = [
    //     true, true, false, false, true, true,   // 0 to 5
    //     false, true, true, false, false, false, // 6 to 11
    //     true, true, false, false, false, true,  // 12 to 17
    //     true, false, true, true, false, false   // 18 to 23
    // ];

    fn identify_neighbors(&mut self, world: &mut impl World, upd1: NodeId) {
        let pos = self.nodes[upd1.index].pos;
        let neighbors = Self::compute_all_neighbors(pos);
        let mut neighbors_visited = [false; 24];
        let mut neighbor_nodes = [NodeId { index: 0 }; 24];

        for (i, neighbor_pos) in neighbors.iter().enumerate() {
            let neighbor = match self.node_cache.entry(*neighbor_pos) {
                Entry::Occupied(entry) => *entry.get(),
                Entry::Vacant(entry) => {
                    let node_id = NodeId {
                        index: self.nodes.len(),
                    };
                    self.nodes.push(UpdateNode::new(world, *neighbor_pos));
                    *entry.insert(node_id)
                }
            };

            let node = &self.nodes[neighbor.index];
            neighbor_nodes[i] = neighbor;
            neighbors_visited[i] = node.visited;
        }

        let from_west = neighbors_visited[0] || neighbors_visited[7] || neighbors_visited[8];
        let from_east = neighbors_visited[1] || neighbors_visited[12] || neighbors_visited[13];
        let from_north = neighbors_visited[4] || neighbors_visited[17] || neighbors_visited[20];
        let from_south = neighbors_visited[5] || neighbors_visited[18] || neighbors_visited[21];

        let mut cx = 0;
        let mut cz = 0;
        if from_west {
            cx += 1;
        };
        if from_east {
            cx -= 1;
        };
        if from_north {
            cz += 1;
        };
        if from_south {
            cz -= 1;
        };

        let UpdateNode { xbias, zbias, .. } = &self.nodes[upd1.index];
        let xbias = *xbias;
        let zbias = *zbias;

        let heading;
        if cx == 0 && cz == 0 {
            heading = Self::compute_heading(xbias, zbias);

            for node_id in &neighbor_nodes {
                // if let Some(node_id) = node_id {
                let nn = &mut self.nodes[node_id.index];
                nn.xbias = xbias;
                nn.zbias = zbias;
                // }
            }
        } else {
            if cx != 0 && cz != 0 {
                if xbias != 0 {
                    cz = 0;
                }
                if zbias != 0 {
                    cx = 0;
                }
            }
            heading = Self::compute_heading(cx, cz);

            for node_id in &neighbor_nodes {
                // if let Some(node_id) = node_id {
                let nn = &mut self.nodes[node_id.index];
                nn.xbias = cx;
                nn.zbias = cz;
                // }
            }
        }

        self.orient_neighbors(&neighbor_nodes, upd1, heading);
    }

    const REORDING: [[usize; 24]; 4] = [
        [
            2, 3, 16, 19, 0, 4, 1, 5, 7, 8, 17, 20, 12, 13, 18, 21, 6, 9, 22, 14, 11, 10, 23, 15,
        ],
        [
            2, 3, 16, 19, 4, 1, 5, 0, 17, 20, 12, 13, 18, 21, 7, 8, 22, 14, 11, 15, 23, 9, 6, 10,
        ],
        [
            2, 3, 16, 19, 1, 5, 0, 4, 12, 13, 18, 21, 7, 8, 17, 20, 11, 15, 23, 10, 6, 14, 22, 9,
        ],
        [
            2, 3, 16, 19, 5, 0, 4, 1, 18, 21, 7, 8, 17, 20, 12, 13, 23, 10, 6, 9, 22, 15, 11, 14,
        ],
    ];

    fn orient_neighbors(&mut self, src: &[NodeId; 24], dst_id: NodeId, heading: usize) {
        let dst = &mut self.nodes[dst_id.index];
        let re = Self::REORDING[heading];
        dst.neighbors = Some(self.neighbor_lists.len());
        self.neighbor_lists.push(re.map(|i| src[i]));
    }

    /// This is the start of a great adventure
    pub fn update_surrounding_neighbors(world: &mut impl World, pos: BlockPos) {
        let mut turbo = SCRATCH
            .with(|scratch| scratch.borrow_mut().take())
            .unwrap_or_else(RedstoneWireTurbo::new);
        let mut root_node = UpdateNode::new(world, pos);
        root_node.visited = true;
        let node_id = NodeId { index: 0 };
        turbo.node_cache.insert(pos, node_id);
        turbo.nodes.push(root_node);
        turbo.propagate_changes(world, node_id, 0);
        turbo.breadth_first_walk(world);
        // Invalidate all state and topology, including eligibility and direction
        // metadata, before another world or a piston-modified layout can use it.
        turbo.nodes.clear();
        turbo.neighbor_lists.clear();
        turbo.node_cache.clear();
        for queue in &mut turbo.update_queue {
            queue.clear();
        }
        turbo.current_walk_layer = 0;
        SCRATCH.with(|scratch| *scratch.borrow_mut() = Some(turbo));
    }

    fn propagate_changes(&mut self, world: &mut impl World, upd1: NodeId, layer: u32) {
        if self.nodes[upd1.index].neighbors.is_none() {
            self.identify_neighbors(world, upd1);
        }

        let neighbors = self.neighbor_lists[self.nodes[upd1.index].neighbors.unwrap()];

        let layer1 = layer + 1;

        for neighbor_id in neighbors {
            let neighbor = &mut self.nodes[neighbor_id.index];
            if layer1 > neighbor.layer {
                neighbor.layer = layer1;
                if *neighbor
                    .needs_update
                    .get_or_insert_with(|| redstone::has_neighbor_update(neighbor.state))
                {
                    self.update_queue[1].push(neighbor_id);
                }
            }
        }

        let layer2 = layer + 2;

        for neighbor_id in &neighbors[0..4] {
            let neighbor = &mut self.nodes[neighbor_id.index];
            if layer2 > neighbor.layer {
                neighbor.layer = layer2;
                if *neighbor
                    .needs_update
                    .get_or_insert_with(|| redstone::has_neighbor_update(neighbor.state))
                {
                    self.update_queue[2].push(*neighbor_id);
                }
            }
        }
    }

    fn breadth_first_walk(&mut self, world: &mut impl World) {
        self.shift_queue();
        self.current_walk_layer = 1;

        // TODO: add piston (3 tick search)
        while !self.update_queue[0].is_empty() || !self.update_queue[1].is_empty() {
            // Propagation only appends to the next two layers. Keep the current
            // layer in place and process its original length in the original order.
            let count = self.update_queue[0].len();
            for index in 0..count {
                let node_id = self.update_queue[0][index];
                match self.nodes[node_id.index].state {
                    Block::RedstoneWire { .. } => {
                        self.update_node(world, node_id, self.current_walk_layer);
                    }
                    //todo since we might want to impl pistons as a type of wire, we need to add case here (?)

                    // This only works because updating any other block than a wire will
                    // never change the state of the block. If that changes in the future,
                    // the cached state will need to be updated
                    block => redstone::update(block, world, self.nodes[node_id.index].pos, None),
                }
            }

            self.shift_queue();
            self.current_walk_layer += 1;
        }

        self.current_walk_layer = 0;
    }

    fn shift_queue(&mut self) {
        let mut t = self.update_queue.remove(0);
        t.clear();
        self.update_queue.push(t);
    }

    fn update_node(&mut self, world: &mut impl World, upd1: NodeId, layer: u32) {
        let old_wire = {
            let node = &mut self.nodes[upd1.index];
            node.visited = true;
            unwrap_wire(node.state)
        };

        let new_wire = self.calculate_current_changes(world, upd1);
        if old_wire.power != new_wire.power {
            wire_mut(&mut self.nodes[upd1.index].state).power = new_wire.power;

            self.propagate_changes(world, upd1, layer);
        }
    }

    const RS_NEIGHBORS: [usize; 4] = [4, 5, 6, 7];
    const RS_NEIGHBORS_UP: [usize; 4] = [9, 11, 13, 15];
    const RS_NEIGHBORS_DN: [usize; 4] = [8, 10, 12, 14];

    fn calculate_current_changes(&mut self, world: &mut impl World, upd: NodeId) -> RedstoneWire {
        let mut wire = unwrap_wire(self.nodes[upd.index].state);
        let i = wire.power;
        let mut block_power = 0;

        if self.nodes[upd.index].neighbors.is_none() {
            self.identify_neighbors(world, upd);
        }

        let pos = self.nodes[upd.index].pos;

        let mut wire_power = 0;
        for side in &BlockFace::values() {
            let neighbor_pos = pos.offset(*side);
            let neighbor = self.nodes[self.node_cache[&neighbor_pos].index].state;
            wire_power = wire_power.max(redstone::get_redstone_power_no_dust(
                neighbor,
                world,
                neighbor_pos,
                *side,
            ));
        }

        if wire_power < 15 {
            let neighbors = &self.neighbor_lists[self.nodes[upd.index].neighbors.unwrap()];

            let center_up = self.nodes[neighbors[1].index].state;

            for m in 0..4 {
                let n = Self::RS_NEIGHBORS[m];

                let neighbor_id = neighbors[n];
                let neighbor = self.get_node(neighbor_id).state;
                block_power = self.get_max_current_strength(neighbor_id, block_power);

                if !neighbor.is_solid() {
                    let neighbor_down = neighbors[Self::RS_NEIGHBORS_DN[m]];
                    block_power = self.get_max_current_strength(neighbor_down, block_power);
                } else if !center_up.is_solid() && !neighbor.is_transparent() {
                    let neighbor_up = neighbors[Self::RS_NEIGHBORS_UP[m]];
                    block_power = self.get_max_current_strength(neighbor_up, block_power);
                }
            }
        }

        let mut j = block_power.saturating_sub(1);
        if wire_power > j {
            j = wire_power;
        }
        if i != j {
            wire.power = j;
            world.set_block(pos, Block::RedstoneWire { wire });
        }
        wire
    }

    fn get_max_current_strength(&self, upd: NodeId, strength: u8) -> u8 {
        let node = &self.nodes[upd.index];
        if let Block::RedstoneWire { wire } = node.state {
            wire.power.max(strength)
        } else {
            strength
        }
    }
}
