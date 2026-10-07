//! Execute the compiled graph using packed nodes and priority-ordered tick queues.

mod compile;
mod instant;
pub mod node;
mod tick;
mod update;

use super::{BackendError, TickScheduler};
use crate::redpiler::compile_graph::CompileGraph;
use crate::redpiler::{block_powered_mut, CompilerOptions};
use crate::redstone::bool_to_ss;
use crate::redstone::noteblock;
use crate::world::World;
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::{Block, ComparatorMode, Instrument};
use mchprs_blocks::BlockPos;
use mchprs_world::{TickEntry, TickPriority};
use node::{Node, NodeId, NodeType, Nodes};
use rustc_hash::FxHashMap;
use std::fmt;
use tracing::{debug, warn};

enum Event {
    CommandBlockPower {
        node_id: NodeId,
        powered: bool,
        capture_condition: bool,
    },
    CommandBlockExecute {
        node_id: NodeId,
    },
    NoteBlockPlay {
        noteblock_id: u16,
    },
    ButtonRelease {
        pos: BlockPos,
    },
}

#[derive(Default)]
pub struct DirectBackend {
    nodes: Nodes,
    blocks: Vec<Option<(BlockPos, Block)>>,
    pos_map: FxHashMap<BlockPos, NodeId>,
    scheduler: TickScheduler<NodeId>,
    events: Vec<Event>,
    noteblock_info: Vec<(BlockPos, Instrument, u32)>,
    command_far_comparators: FxHashMap<NodeId, Vec<NodeId>>,
    instant: Vec<instant::Runtime>,
    instant_dependencies: FxHashMap<NodeId, Vec<usize>>,
    instant_dirty: Vec<bool>,
}

impl DirectBackend {
    #[cfg(test)]
    pub(crate) fn logical_stats(&self) -> Vec<(u64, u64, Vec<(BlockPos, bool)>)> {
        self.instant.iter().filter_map(|runtime| runtime.logical_stats()).collect()
    }
    #[cfg(test)]
    pub(crate) fn sampled_pistons(&self) -> FxHashMap<BlockPos, (bool, bool)> {
        self.instant.iter().flat_map(|runtime| runtime.sampled_pistons()).map(|(pos, retracted, settled)| (pos, (retracted, settled))).collect()
    }
    #[cfg(test)]
    pub(crate) fn sampled_signals(&self) -> Vec<(BlockPos, u8)> {
        self.instant.iter().flat_map(|r| r.sampled_signals()).collect()
    }
    #[cfg(test)]
    pub(crate) fn sampled_sources(&self) -> Vec<(BlockPos,u8)> { self.instant.iter().flat_map(|r|r.sampled_sources(&self.nodes)).collect() }
    #[cfg(test)]
    pub(crate) fn ordinary_sources(&self) -> Vec<(BlockPos, u8)> {
        self.blocks.iter().enumerate().filter_map(|(id, entry)| {
            let (pos, block) = (*entry)?;
            matches!(block, Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. }
                | Block::RedstoneRepeater { .. } | Block::RedstoneComparator { .. })
                .then_some((pos, self.nodes[self.nodes.get(id)].output_power))
        }).collect()
    }
    #[cfg(test)]
    pub(crate) fn sampled_geometry(&self) -> Vec<(BlockPos,Block)> { self.instant.iter().flat_map(|r|r.sampled_geometry()).collect() }
    #[cfg(test)]
    pub(crate) fn sampled_power(&self) -> Vec<(BlockPos, bool)> { self.instant.iter().flat_map(|r|r.sampled_power(&self.nodes)).collect() }
    fn process_command_outputs(&mut self, world: &mut impl World) {
        if !self.events.iter().any(|event| {
            matches!(
                event,
                Event::CommandBlockPower { .. } | Event::CommandBlockExecute { .. }
            )
        }) {
            return;
        }
        let mut remaining = Vec::new();
        let mut events = std::mem::take(&mut self.events);
        while !events.is_empty() {
            for event in events.drain(..) {
                match event {
                    Event::CommandBlockPower {
                        node_id,
                        powered,
                        capture_condition,
                    } => {
                        if let Some((pos, _)) = self.blocks[node_id.index()] {
                            crate::redstone::command_block::set_output_power(
                                world,
                                pos,
                                powered,
                                capture_condition,
                            );
                        }
                    }
                    Event::CommandBlockExecute { node_id } => {
                        let Some((pos, _)) = self.blocks[node_id.index()] else {
                            continue;
                        };
                        let executed = crate::redstone::command_block::tick_output(world, pos);
                        for pos in executed {
                            if let Some(&id) = self.pos_map.get(&pos) {
                                let strength = crate::redstone::comparator::get_override(
                                    world.get_block(pos),
                                    world,
                                    pos,
                                );
                                self.set_node(id, self.nodes[id].powered, strength);
                            }
                        }
                        if let NodeType::CommandBlock {
                            repeating: true,
                            automatic,
                            ..
                        } = self.nodes[node_id].ty
                        {
                            if (self.nodes[node_id].powered || automatic)
                                && !self.nodes[node_id].pending_tick
                            {
                                self.nodes[node_id].pending_tick = true;
                                self.scheduler
                                    .schedule_half_tick(node_id, 1, TickPriority::Normal);
                                crate::redstone::command_block::set_output_power(
                                    world,
                                    pos,
                                    self.nodes[node_id].powered,
                                    true,
                                );
                            }
                        }
                    }
                    other => remaining.push(other),
                }
            }
            events = std::mem::take(&mut self.events);
        }
        self.events = remaining;
    }

    pub(crate) fn compile(
        &mut self,
        graph: CompileGraph,
        ticks: Vec<TickEntry>,
        options: &CompilerOptions,
        instant: Vec<crate::redpiler::instant::program::PreparedInstant>,
    ) -> Result<(), BackendError> {
        compile::compile(self, graph, ticks, options, instant)
    }
    #[inline]
    fn schedule_tick(&mut self, node_id: NodeId, delay: usize, priority: TickPriority) {
        self.scheduler.schedule_tick(node_id, delay, priority);
    }

    fn refresh_outputs(&mut self, source: NodeId) {
        if !self
            .instant
            .iter()
            .any(|runtime| runtime.output_depends_on(source))
        {
            return;
        }
        let mut runtimes = std::mem::take(&mut self.instant);
        let mut changes = Vec::new();
        for runtime in &mut runtimes {
            if runtime.output_depends_on(source) {
                changes.extend(runtime.output_changes(&self.nodes));
            }
        }
        self.instant = runtimes;
        for (node, strength) in changes {
            if self.nodes[node].output_power != strength { self.set_node(node, strength != 0, strength); }
        }
    }

    fn set_node(&mut self, node_id: NodeId, powered: bool, new_power: u8) {
        let node = &mut self.nodes[node_id];
        let old_power = node.output_power;

        node.changed = true;
        node.powered = powered;
        node.output_power = new_power;
        let update_count = node.updates.len();
        let mut runtimes = std::mem::take(&mut self.instant);
        let mut instant_changes = Vec::new();
        for &region in self.instant_dependencies.get(&node_id).into_iter().flatten() {
            instant_changes.extend(runtimes[region].notify_sequential_source(node_id, &self.nodes));
        }
        self.instant = runtimes;
        for i in 0..update_count {
            let node = &self.nodes[node_id];
            let update_link = unsafe { *node.updates.get_unchecked(i) };
            let side = update_link.side();
            let distance = update_link.attenuation();
            let update = update_link.node();

            let update_ref = &mut self.nodes[update];
            let inputs = if side {
                &mut update_ref.side_inputs
            } else {
                &mut update_ref.default_inputs
            };

            let old_power = old_power.saturating_sub(distance);
            let new_power = new_power.saturating_sub(distance);

            if old_power == new_power {
                continue;
            }

            // Safety: signal strength is never larger than 15
            unsafe {
                *inputs.strength_counts.get_unchecked_mut(old_power as usize) -= 1;
                *inputs.strength_counts.get_unchecked_mut(new_power as usize) += 1;
            }

            update::update_node(
                &mut self.scheduler,
                &mut self.events,
                &mut self.nodes,
                update,
            );
        }
        if old_power != new_power {
            for &region in self.instant_dependencies.get(&node_id).into_iter().flatten() {
                self.instant_dirty[region] = true;
            }
            for &comparator in self
                .command_far_comparators
                .get(&node_id)
                .into_iter()
                .flatten()
            {
                if let NodeType::Comparator { far_input, .. } = &mut self.nodes[comparator].ty {
                    *far_input = node::NonMaxU8::new(new_power);
                }
                update::update_node(
                    &mut self.scheduler,
                    &mut self.events,
                    &mut self.nodes,
                    comparator,
                );
            }
            self.refresh_outputs(node_id);
        }
        for (id, strength) in instant_changes {
            if self.nodes[id].output_power != strength {
                self.set_node(id, strength != 0, strength);
            }
        }
    }

    pub(crate) fn tick_with_world<W: World>(&mut self, world: &mut W) {
        self.process_command_outputs(world);
        world.piston_state_mut().logical_tick += 1;
        self.tick_after_callbacks(|backend| backend.process_command_outputs(world));
        self.process_command_outputs(world);
    }

    pub(crate) fn inspect(&mut self, pos: BlockPos) {
        let Some(node_id) = self.pos_map.get(&pos) else {
            debug!("could not find node at pos {}", pos);
            return;
        };

        debug!("Node {:?}: {:#?}", node_id, self.nodes[*node_id]);
    }

    pub(crate) fn reset<W: World>(&mut self, world: &mut W, io_only: bool) {
        // Display flushing can clear dirty flags without writing hidden nodes.
        // Handoff must materialize their current strengths, including ordinary
        // dust between a virtual region supply and its consumer.
        for node in self.nodes.inner_mut() {
            if !matches!(node.ty, NodeType::Constant) {
                node.changed = true;
            }
        }
        self.flush(world, false);
        for runtime in std::mem::take(&mut self.instant) {
            runtime.materialize(world);
        }
        self.scheduler.reset(world, &self.blocks);

        let nodes = std::mem::take(&mut self.nodes);

        for (i, node) in nodes.into_inner().iter().enumerate() {
            let Some((pos, block)) = self.blocks[i] else {
                continue;
            };
            if matches!(node.ty, NodeType::Comparator { .. }) {
                let block_entity = BlockEntity::Comparator {
                    output_strength: node.output_power,
                };
                world.set_block_entity(pos, block_entity);
            }

            // Constants never acquire virtual state. Closing a container after
            // activation may change its physical presentation (a barrel's open
            // property), which must not be undone by a stale compile snapshot.
            if io_only && !node.is_io && !matches!(node.ty, NodeType::Constant) {
                world.set_block(pos, block);
            }
        }

        self.pos_map.clear();
        self.noteblock_info.clear();
        self.command_far_comparators.clear();
        self.instant_dependencies.clear();
        self.instant_dirty.clear();
        self.events.clear();
    }

    pub(crate) fn on_use_block(&mut self, pos: BlockPos) {
        let node_id = self.pos_map[&pos];
        let node = &self.nodes[node_id];
        match node.ty {
            NodeType::Button => {
                if node.powered {
                    return;
                }
                self.schedule_tick(node_id, 10, TickPriority::Normal);
                self.set_node(node_id, true, 15);
            }
            NodeType::Lever => {
                self.set_node(node_id, !node.powered, bool_to_ss(!node.powered));
            }
            _ => warn!("Tried to use a {:?} redpiler node", node.ty),
        }
        for runtime in &mut self.instant {
            runtime.observe_action(pos, self.nodes[node_id].output_power);
        }
    }

    pub(crate) fn set_pressure_plate(&mut self, pos: BlockPos, powered: bool) {
        let node_id = self.pos_map[&pos];
        let node = &self.nodes[node_id];
        match node.ty {
            NodeType::PressurePlate => {
                self.set_node(node_id, powered, bool_to_ss(powered));
            }
            _ => warn!("Tried to set pressure plate state for a {:?}", node.ty),
        }
    }

    pub(crate) fn tick(&mut self) {
        self.tick_after_callbacks(|_| {});
    }

    fn tick_after_callbacks(&mut self, mut after_callback: impl FnMut(&mut Self)) {
        for runtime in &mut self.instant { runtime.begin_tick(); }
        let mut queues = self.scheduler.queues_this_tick_move_next();

        for node_id in queues.drain_iter() {
            self.tick_node(node_id);
            after_callback(self);
        }

        self.scheduler.end_tick(queues);
        let mut runtimes = std::mem::take(&mut self.instant);
        let mut changes = Vec::new();
        for (region, runtime) in runtimes.iter_mut().enumerate() {
            let sources_changed = std::mem::replace(&mut self.instant_dirty[region], false);
            changes.extend(runtime.advance(&self.nodes, sources_changed));
        }
        self.instant = runtimes;
        for (id, strength) in changes {
            if self.nodes[id].output_power != strength { self.set_node(id, strength != 0, strength); }
        }
    }

    pub(crate) fn flush<W: World>(&mut self, world: &mut W, io_only: bool) {
        self.process_command_outputs(world);
        for event in self.events.drain(..) {
            match event {
                Event::NoteBlockPlay { noteblock_id } => {
                    let (pos, instrument, note) = self.noteblock_info[noteblock_id as usize];
                    if noteblock::is_noteblock_unblocked(world, pos) {
                        noteblock::play_note(world, pos, instrument, note);
                    }
                }
                Event::ButtonRelease { pos } => world.play_sound(
                    pos,
                    crate::sound::event_id("block.stone_button.click_off"),
                    4,
                    1.0,
                    1.0,
                ),
                Event::CommandBlockPower { .. } | Event::CommandBlockExecute { .. } => {
                    unreachable!("command events processed before display flush")
                }
            }
        }
        for (i, node) in self.nodes.inner_mut().iter_mut().enumerate() {
            let Some((pos, block)) = &mut self.blocks[i] else {
                continue;
            };
            if node.changed && (!io_only || node.is_io) {
                if let Some(powered) = block_powered_mut(block) {
                    *powered = node.powered
                }
                if let Some(plate) = block.with_pressure_plate_power(node.powered) {
                    *block = plate;
                }
                if let Block::RedstoneWire { wire, .. } = block {
                    wire.power = node.output_power
                };
                if let Block::RedstoneRepeater { repeater } = block {
                    repeater.locked = node.locked;
                }
                world.set_block(*pos, *block);
            }
            node.changed = false;
        }
    }
}

/// Set node for use in `update`. None of the nodes here have usable output power,
/// so this function does not set that.
fn set_node(node: &mut Node, powered: bool) {
    node.powered = powered;
    node.changed = true;
}

fn set_node_locked(node: &mut Node, locked: bool) {
    node.locked = locked;
    node.changed = true;
}

#[inline]
fn schedule_tick(
    scheduler: &mut TickScheduler<NodeId>,
    node_id: NodeId,
    node: &mut Node,
    delay: usize,
    priority: TickPriority,
) {
    node.pending_tick = true;
    scheduler.schedule_tick(node_id, delay, priority);
}

// Ignore strength zero; any nonzero counter at strengths 1..15 means powered.
const BOOL_INPUT_MASK: u128 = u128::from_ne_bytes([
    0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
]);

fn has_main_input(node: &Node) -> bool {
    u128::from_le_bytes(node.default_inputs.strength_counts) & BOOL_INPUT_MASK != 0
}

fn has_side_input(node: &Node) -> bool {
    u128::from_le_bytes(node.side_inputs.strength_counts) & BOOL_INPUT_MASK != 0
}

fn strongest_input(array: &[u8; 16]) -> u32 {
    // Each byte counts links at that strength. The highest nonzero byte is
    // the strongest input; reading all sixteen bytes keeps this scan constant-time.
    let value = u128::from_le_bytes(*array);
    if value == 0 {
        0
    } else {
        15 - (value.leading_zeros() >> 3)
    }
}

fn input_strengths(node: &Node) -> (u8, u8) {
    let input_power = strongest_input(&node.default_inputs.strength_counts) as u8;

    let side_input_power = strongest_input(&node.side_inputs.strength_counts) as u8;

    (input_power, side_input_power)
}

// With validated 0..15 strengths, wrapping subtraction puts underflow above 15.
// This preserves the compact comparator calculation in the tick and update loops.
fn calculate_comparator_output(mode: ComparatorMode, input_strength: u8, power_on_sides: u8) -> u8 {
    let difference = input_strength.wrapping_sub(power_on_sides);
    if difference <= 15 {
        match mode {
            ComparatorMode::Compare => input_strength,
            ComparatorMode::Subtract => difference,
        }
    } else {
        0
    }
}

impl fmt::Display for DirectBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "digraph {{")?;
        for (id, node) in self.nodes.inner().iter().enumerate() {
            if matches!(node.ty, NodeType::Wire) {
                continue;
            }
            let label = match node.ty {
                NodeType::Repeater { delay, .. } => format!("Repeater({})", delay),
                NodeType::Torch => "Torch".to_string(),
                NodeType::Comparator { mode, .. } => format!(
                    "Comparator({})",
                    match mode {
                        ComparatorMode::Compare => "Cmp",
                        ComparatorMode::Subtract => "Sub",
                    }
                ),
                NodeType::Lamp => "Lamp".to_string(),
                NodeType::Button => "Button".to_string(),
                NodeType::Lever => "Lever".to_string(),
                NodeType::PressurePlate => "PressurePlate".to_string(),
                NodeType::Trapdoor => "Trapdoor".to_string(),
                NodeType::Wire => "Wire".to_string(),
                NodeType::Constant => format!("Constant({})", node.output_power),
                NodeType::InstantSource => format!("InstantSource({})", node.output_power),
                NodeType::NoteBlock { .. } => "NoteBlock".to_string(),
                NodeType::CommandBlock { .. } => "CommandBlock".to_string(),
            };
            let pos = if let Some((pos, _)) = self.blocks[id] {
                format!("{}, {}, {}", pos.x, pos.y, pos.z)
            } else {
                "No Pos".to_string()
            };
            writeln!(f, "    n{} [ label = \"{}\\n({})\" ];", id, label, pos)?;
            for link in node.updates.iter() {
                let out_index = link.node().index();
                let distance = link.attenuation();
                let color = if link.side() { ",color=\"blue\"" } else { "" };
                writeln!(
                    f,
                    "    n{} -> n{} [ label = \"{}\"{} ];",
                    id, out_index, distance, color
                )?;
            }
        }
        writeln!(f, "}}")
    }
}
