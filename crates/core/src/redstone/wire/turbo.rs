//! The implementation of "Redstone Wire Turbo" was largely based on
//! the accelorator created by theosib. For more information, see:
//! https://bugs.mojang.com/browse/MC-81098.

use crate::redstone;
use crate::world::wire_cache::{self, Facts};
use crate::world::World;
use mchprs_blocks::blocks::{Block, RedstoneWire};
use mchprs_blocks::{BlockFace, BlockPos};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::num::NonZeroU32;
#[path = "turbo_cache.rs"]
mod spatial;

thread_local! {
    // Live walk data is discarded; spatial entries expire by generation. Take scratch out
    // before dispatching callbacks: recursive wire walks need independent data.
    static SCRATCH: RefCell<Option<RedstoneWireTurbo>> = const { RefCell::new(None) };
}

pub(super) fn invalidate_scratch() {
    SCRATCH.with(|scratch| *scratch.borrow_mut() = None);
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
    index: u32,
}

impl NodeId {
    fn new(index: usize) -> Self {
        Self {
            index: u32::try_from(index).expect("wire walk exceeds u32 node capacity"),
        }
    }

    #[inline]
    fn index(self) -> usize {
        self.index as usize
    }
}

struct UpdateNode {
    pos: BlockPos,
    /// The cached state of the block
    state: Block,
    /// Cached for this walk only. Wire power changes preserve the block's kind;
    /// other nodes already use an immutable cached state in this algorithm.
    facts: Facts,
    /// This will only be `Some` when all the neighbors are identified.
    // A one-based ID gives Option a zero sentinel without a separate tag.
    neighbors: Option<NonZeroU32>,
    visited: bool,
    xbias: i32,
    zbias: i32,
    layer: u32,
}

impl UpdateNode {
    #[inline]
    fn neighborhood_index(&self) -> usize {
        self.neighbors.expect("wire neighbors not identified").get() as usize - 1
    }

    fn new(world: &impl World, pos: BlockPos) -> UpdateNode {
        let state = world.get_block(pos);
        UpdateNode {
            pos,
            state,
            facts: wire_cache::facts(state),
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
    neighbor_lists: Vec<Neighborhood>,
    spatial: spatial::SpatialNodes,
    node_cache: FxHashMap<BlockPos, NodeId>,
    update_queue: Vec<Vec<NodeId>>,
    current_walk_layer: u32,
}

struct Neighborhood {
    oriented: [NodeId; 24],
    direct: [NodeId; 6],
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
            spatial: Default::default(),
            node_cache: FxHashMap::default(),
            update_queue: vec![vec![], vec![], vec![]],
            current_walk_layer: 0,
        }
    }

    fn get_node(&self, node_id: NodeId) -> &UpdateNode {
        &self.nodes[node_id.index()]
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
        let pos = self.nodes[upd1.index()].pos;
        let cached = world.wire_neighborhood(pos);
        let local;
        let neighbors = if let Some(ref cached) = cached {
            cached.as_ref()
        } else {
            local = wire_cache::positions(pos).map(|pos| crate::world::WireNeighbor {
                pos,
                cell: world.wire_location(pos),
            });
            &local
        };
        let mut neighbors_visited = [false; 24];
        let mut neighbor_nodes = [NodeId { index: 0 }; 24];

        for (i, location) in neighbors.iter().enumerate() {
            let neighbor = self.node_at(world, *location);

            let node = &self.nodes[neighbor.index()];
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

        let UpdateNode { xbias, zbias, .. } = &self.nodes[upd1.index()];
        let xbias = *xbias;
        let zbias = *zbias;

        let heading;
        if cx == 0 && cz == 0 {
            heading = Self::compute_heading(xbias, zbias);

            for node_id in &neighbor_nodes {
                // if let Some(node_id) = node_id {
                let nn = &mut self.nodes[node_id.index()];
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
                let nn = &mut self.nodes[node_id.index()];
                nn.xbias = cx;
                nn.zbias = cz;
                // }
            }
        }

        self.orient_neighbors(&neighbor_nodes, upd1, heading);
    }

    fn node_at(&mut self, world: &impl World, location: crate::world::WireNeighbor) -> NodeId {
        if let Some(cell) = location.cell {
            if let Some(node) = self.spatial.get(cell) {
                return node;
            }
            let node = NodeId::new(self.nodes.len());
            self.nodes.push(UpdateNode::new(world, location.pos));
            self.spatial.put(cell, node);
            node
        } else {
            match self.node_cache.entry(location.pos) {
                Entry::Occupied(entry) => *entry.get(),
                Entry::Vacant(entry) => {
                    let node = NodeId::new(self.nodes.len());
                    self.nodes.push(UpdateNode::new(world, location.pos));
                    *entry.insert(node)
                }
            }
        }
    }

    fn orient_neighbors(&mut self, src: &[NodeId; 24], dst_id: NodeId, heading: usize) {
        let dst = &mut self.nodes[dst_id.index()];
        let re = super::TURBO_ORDER[heading];
        dst.neighbors = Some(
            u32::try_from(self.neighbor_lists.len())
                .ok()
                .and_then(|index| index.checked_add(1))
                .and_then(NonZeroU32::new)
                .expect("wire walk exceeds u32 neighborhood capacity"),
        );
        self.neighbor_lists.push(Neighborhood {
            oriented: re.map(|i| src[i]),
            direct: [src[3], src[2], src[4], src[5], src[1], src[0]],
        });
    }

    /// This is the start of a great adventure
    pub fn update_surrounding_neighbors(world: &mut impl World, pos: BlockPos) {
        let mut turbo = SCRATCH
            .with(|scratch| scratch.borrow_mut().take())
            .unwrap_or_else(RedstoneWireTurbo::new);
        turbo.spatial.begin();
        let node_id = turbo.node_at(
            world,
            crate::world::WireNeighbor {
                pos,
                cell: world.wire_location(pos),
            },
        );
        turbo.nodes[node_id.index()].visited = true;
        turbo.propagate_changes(world, node_id, 0);
        turbo.breadth_first_walk(world);
        // Discard all live state, orientation and traversal metadata. Canonical
        // addresses live in the world's topology cache; spatial entries expire
        // at the next begin(). Neither cache contains a live block snapshot.
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
        if self.nodes[upd1.index()].neighbors.is_none() {
            self.identify_neighbors(world, upd1);
        }

        let neighbors = self.neighbor_lists[self.nodes[upd1.index()].neighborhood_index()].oriented;

        let layer1 = layer + 1;

        for neighbor_id in neighbors {
            let neighbor = &mut self.nodes[neighbor_id.index()];
            if layer1 > neighbor.layer {
                neighbor.layer = layer1;
                if neighbor.facts.updates {
                    self.update_queue[1].push(neighbor_id);
                }
            }
        }

        let layer2 = layer + 2;

        for neighbor_id in &neighbors[0..4] {
            let neighbor = &mut self.nodes[neighbor_id.index()];
            if layer2 > neighbor.layer {
                neighbor.layer = layer2;
                if neighbor.facts.updates {
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
                match self.nodes[node_id.index()].state {
                    Block::RedstoneWire { .. } => {
                        self.update_node(world, node_id, self.current_walk_layer);
                    }
                    //todo since we might want to impl pistons as a type of wire, we need to add case here (?)

                    // This only works because updating any other block than a wire will
                    // never change the state of the block. If that changes in the future,
                    // the cached state will need to be updated
                    block => redstone::update(block, world, self.nodes[node_id.index()].pos, None),
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
            let node = &mut self.nodes[upd1.index()];
            node.visited = true;
            unwrap_wire(node.state)
        };

        let new_wire = self.calculate_current_changes(world, upd1);
        if old_wire.power != new_wire.power {
            wire_mut(&mut self.nodes[upd1.index()].state).power = new_wire.power;

            self.propagate_changes(world, upd1, layer);
        }
    }

    const RS_NEIGHBORS: [usize; 4] = [4, 5, 6, 7];
    const RS_NEIGHBORS_UP: [usize; 4] = [9, 11, 13, 15];
    const RS_NEIGHBORS_DN: [usize; 4] = [8, 10, 12, 14];

    fn calculate_current_changes(&mut self, world: &mut impl World, upd: NodeId) -> RedstoneWire {
        let mut wire = unwrap_wire(self.nodes[upd.index()].state);
        let i = wire.power;
        let mut block_power = 0;

        if self.nodes[upd.index()].neighbors.is_none() {
            self.identify_neighbors(world, upd);
        }

        let pos = self.nodes[upd.index()].pos;

        let mut wire_power = 0;
        let direct = self.neighbor_lists[self.nodes[upd.index()].neighborhood_index()].direct;
        for (side, neighbor_id) in BlockFace::values().iter().zip(direct) {
            let neighbor_pos = pos.offset(*side);
            let neighbor = self.nodes[neighbor_id.index()].state;
            wire_power = wire_power.max(redstone::get_redstone_power_no_dust(
                neighbor,
                world,
                neighbor_pos,
                *side,
            ));
        }

        if wire_power < 15 {
            let neighbors =
                &self.neighbor_lists[self.nodes[upd.index()].neighborhood_index()].oriented;

            let center_up = self.nodes[neighbors[1].index()].facts;

            for m in 0..4 {
                let n = Self::RS_NEIGHBORS[m];

                let neighbor_id = neighbors[n];
                let neighbor = self.get_node(neighbor_id).facts;
                block_power = self.get_max_current_strength(neighbor_id, block_power);

                if !neighbor.solid {
                    let neighbor_down = neighbors[Self::RS_NEIGHBORS_DN[m]];
                    block_power = self.get_max_current_strength(neighbor_down, block_power);
                } else if !center_up.solid && !neighbor.transparent {
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
        let node = &self.nodes[upd.index()];
        if let Block::RedstoneWire { wire } = node.state {
            wire.power.max(strength)
        } else {
            strength
        }
    }
}
