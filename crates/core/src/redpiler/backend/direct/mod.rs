//! Execute the compiled graph using packed nodes and priority-ordered tick queues.

mod compile;
mod instant;
pub mod node;
mod tick;
mod update;

use super::{BackendError, RuntimeTick, TickScheduler};
use crate::redpiler::compile_graph::CompileGraph;
use crate::redpiler::{block_powered_mut, CompilerOptions};
use crate::redstone::bool_to_ss;
use crate::redstone::noteblock;
use crate::world::World;
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::{Block, ComparatorMode, Instrument};
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_world::{TickEntry, TickPriority};
use node::{Node, NodeId, NodeType, Nodes};
use rustc_hash::FxHashMap;
use std::collections::VecDeque;
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
        noteblock_id: u32,
        unblocked: Option<bool>,
    },
    ButtonRelease {
        pos: BlockPos,
    },
    CopperBulbToggle {
        node_id: NodeId,
        lit: bool,
    },
}

struct CommittedChange {
    id: NodeId,
    old_power: u8,
    new_power: u8,
    observed_changed: bool,
    bulb_state_changed: bool,
}

struct BoundaryDelivery {
    source: BlockPos,
    recipients: Vec<(usize, Vec<instant::Sample>)>,
}

#[derive(Default)]
pub struct DirectBackend {
    nodes: Nodes,
    blocks: Vec<Option<(BlockPos, Block)>>,
    block_aliases: FxHashMap<usize, Vec<(BlockPos, Block)>>,
    pos_map: FxHashMap<BlockPos, NodeId>,
    pub(super) scheduler: TickScheduler<RuntimeTick>,
    pub(super) native: Option<super::native::NativePropagation>,
    pub(super) piston_state: mchprs_world::PistonState,
    resolved_inputs: FxHashMap<(BlockPos, bool), NodeId>,
    native_ports: Vec<(BlockPos, bool, NodeId)>,
    native_port_updates: FxHashMap<BlockPos, Vec<usize>>,
    pub(super) assembly_owners: FxHashMap<BlockPos, usize>,
    evaluating_instant: bool,
    instant_changes: Vec<CommittedChange>,
    boundary_deliveries: VecDeque<BoundaryDelivery>,
    native_delivery: Option<BoundaryDelivery>,
    instant_phases: VecDeque<(usize, u64)>,
    events: Vec<Event>,
    noteblock_info: Vec<(BlockPos, Instrument, u32)>,
    far_comparators: FxHashMap<NodeId, Vec<NodeId>>,
    instant: Vec<instant::Runtime>,
    instant_dependencies: FxHashMap<NodeId, Vec<usize>>,
    instant_dirty: Vec<bool>,
    observer_watchers: FxHashMap<NodeId, Vec<NodeId>>,
    instant_observers: FxHashMap<BlockPos, Vec<NodeId>>,
}

impl DirectBackend {
    pub(super) fn native_ports_affected_by(&self, pos: BlockPos) -> Option<&[usize]> {
        self.native_port_updates.get(&pos).map(Vec::as_slice)
    }
    pub(super) fn resolved_input(&self, pos: BlockPos, side: bool) -> Option<u8> {
        self.resolved_inputs
            .get(&(pos, side))
            .map(|&id| self.nodes[id].output_power)
    }

    pub(super) fn dispatch_owned_update(
        &mut self,
        pos: BlockPos,
        _dir: Option<BlockFace>,
        source: Option<BlockPos>,
    ) -> bool {
        if let Some(&region) = self.assembly_owners.get(&pos) {
            if let Some((source, targets)) = self.instant[region].notification_targets(pos, source)
            {
                if self
                    .native_delivery
                    .as_ref()
                    .is_some_and(|batch| batch.source != source)
                {
                    self.finish_native_delivery();
                }
                let captured = self.instant[region].capture_samples(&self.nodes, targets);
                let batch = self
                    .native_delivery
                    .get_or_insert_with(|| BoundaryDelivery {
                        source,
                        recipients: Vec::new(),
                    });
                if let Some((_, samples)) =
                    batch.recipients.iter_mut().find(|(id, _)| *id == region)
                {
                    samples.extend(captured);
                } else {
                    batch.recipients.push((region, captured));
                }
            }
            return true;
        }
        if self.native.as_ref().is_some_and(|native| native.owns(pos)) {
            return false;
        }
        if let Some(&id) = self.pos_map.get(&pos) {
            self.update_node(id);
        }
        true
    }

    pub(super) fn finish_native_delivery(&mut self) {
        if let Some(batch) = self.native_delivery.take() {
            self.boundary_deliveries.push_back(batch);
        }
        while let Some(mut batch) = self.boundary_deliveries.pop_front() {
            // Capture complete fanout against the old bank, then commit every recipient.
            for (region, samples) in &mut batch.recipients {
                *samples =
                    self.instant[*region].capture_samples(&self.nodes, std::mem::take(samples));
            }
            let mut regions = Vec::new();
            for (region, samples) in &batch.recipients {
                self.instant[*region].commit_samples(samples);
                if !regions.contains(region) {
                    regions.push(*region);
                }
                self.instant_dirty[*region] = true;
            }
            self.publish_instant_regions(regions.into_iter(), false);
        }
    }

    pub(super) fn runtime_block_at(&self, pos: BlockPos) -> Option<Block> {
        if self.instant.is_empty() {
            return None;
        }
        if let Some(&region) = self.assembly_owners.get(&pos) {
            if let Some(block) = self.instant[region].geometry_block_at(pos) {
                return Some(block);
            }
        }
        if self.native.as_ref().is_some_and(|native| native.owns(pos)) {
            return None;
        }
        let &id = self.pos_map.get(&pos)?;
        let (_, mut block) = self.blocks[id.index()]?;
        let node = &self.nodes[id];
        if let Some(powered) = block_powered_mut(&mut block) {
            *powered = node.powered;
        }
        if let Some(plate) = block.with_pressure_plate_power(node.powered) {
            block = plate;
        }
        if let Some(bulb) = block.with_copper_bulb_state(node.output_power > 0, node.powered) {
            block = bulb;
        }
        if let Block::RedstoneWire { wire } = &mut block {
            wire.power = node.output_power;
        }
        if let Block::RedstoneRepeater { repeater } = &mut block {
            repeater.locked = node.locked;
        }
        Some(block)
    }
    pub(super) fn record_native(&mut self, pos: BlockPos, block: Block, strength: u8) {
        let Some(&id) = self.pos_map.get(&pos) else {
            return;
        };
        let powered = match block {
            Block::RedstoneRepeater { repeater } => {
                self.nodes[id].locked = repeater.locked;
                repeater.powered
            }
            _ => crate::redpiler::block_powered_mut(&mut block.clone())
                .copied()
                .or_else(|| block.pressure_plate_powered())
                .unwrap_or(false),
        };
        self.commit_node(id, powered, strength);
    }

    pub(super) fn native_port_bindings(&self) -> &[(BlockPos, bool, NodeId)] {
        &self.native_ports
    }

    pub(super) fn commit_native_ports(&mut self, values: Vec<(NodeId, u8)>) {
        for (id, strength) in values {
            self.commit_node(id, strength > 0, strength);
        }
    }
    pub(crate) fn node_count(&self) -> usize {
        self.blocks.len()
    }

    pub(crate) fn region_statistics(&self) -> crate::redpiler::RegionStatistics {
        let mut statistics = crate::redpiler::RegionStatistics::default();
        for runtime in &self.instant {
            runtime.collect_statistics(&mut statistics);
        }
        statistics
    }

    #[cfg(test)]
    pub(crate) fn logical_stats(&self) -> Vec<(u64, u64, Vec<(BlockPos, bool)>)> {
        self.instant
            .iter()
            .filter_map(|runtime| runtime.logical_stats())
            .collect()
    }
    #[cfg(test)]
    pub(crate) fn deliver_notifications(&mut self, notifications: &[(BlockPos, BlockPos)]) {
        for &(recipient, source) in notifications {
            self.dispatch_owned_update(recipient, None, Some(source));
        }
        self.finish_native_delivery();
    }
    #[cfg(test)]
    pub(crate) fn native_owns(&self, pos: BlockPos) -> bool {
        self.native.as_ref().is_some_and(|native| native.owns(pos))
    }
    #[cfg(test)]
    pub(crate) fn ordinary_sources(&self) -> Vec<(BlockPos, u8)> {
        self.blocks
            .iter()
            .enumerate()
            .filter_map(|(id, entry)| {
                let (pos, block) = (*entry)?;
                matches!(
                    block,
                    Block::RedstoneTorch { .. }
                        | Block::RedstoneWallTorch { .. }
                        | Block::RedstoneRepeater { .. }
                        | Block::RedstoneComparator { .. }
                )
                .then_some((pos, self.nodes[self.nodes.get(id)].output_power))
            })
            .collect()
    }
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
                                self.evaluate_instant(false);
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
                                self.scheduler.schedule_half_tick(
                                    node_id.into(),
                                    1,
                                    TickPriority::Normal,
                                );
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
        self.scheduler
            .schedule_tick(node_id.into(), delay, priority);
    }

    fn set_node(&mut self, node_id: NodeId, powered: bool, new_power: u8) {
        let change = self.commit_node(node_id, powered, new_power);
        self.notify_change(change);
    }

    fn commit_node(&mut self, node_id: NodeId, powered: bool, new_power: u8) -> CommittedChange {
        let node = &mut self.nodes[node_id];
        let previous = update::observed_state(node);
        let old_power = node.output_power;

        node.changed = true;
        node.powered = powered;
        node.output_power = new_power;
        let update_count = node.updates.len();
        let state_changed = previous != update::observed_state(node);
        let bulb_state_changed = matches!(node.ty, NodeType::CopperBulb) && state_changed;
        // A source can feed both consumer channels; publish all strengths before callbacks.
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
        }
        if old_power != new_power || bulb_state_changed {
            for &region in self
                .instant_dependencies
                .get(&node_id)
                .into_iter()
                .flatten()
            {
                self.instant_dirty[region] = true;
            }
        }
        CommittedChange {
            id: node_id,
            old_power,
            new_power,
            observed_changed: state_changed,
            bulb_state_changed,
        }
    }

    fn notify_change(&mut self, change: CommittedChange) {
        if change.observed_changed {
            self.notify_observer_watchers(change.id);
        }
        for i in 0..self.nodes[change.id].updates.len() {
            let update_link = self.nodes[change.id].updates[i];
            let distance = update_link.attenuation();
            if change.old_power.saturating_sub(distance)
                != change.new_power.saturating_sub(distance)
                || change.bulb_state_changed
            {
                self.update_node(update_link.node());
            }
        }
        if change.old_power != change.new_power || change.bulb_state_changed {
            for &comparator in self.far_comparators.get(&change.id).into_iter().flatten() {
                if let NodeType::Comparator { far_input, .. } = &mut self.nodes[comparator].ty {
                    *far_input = node::NonMaxU8::new(change.new_power);
                }
                update::update_node(
                    &mut self.scheduler,
                    &mut self.events,
                    &mut self.nodes,
                    comparator,
                );
            }
        }
    }

    /// Apply an input change and capture effects that depend on its arrival time.
    /// Bulb latches update immediately; note eligibility reads committed memory.
    fn update_node(&mut self, id: NodeId) {
        if let Some(native) = &self.native {
            if let Some((pos, _)) = self.blocks[id.index()].filter(|(pos, _)| native.owns(*pos)) {
                self.native_update(pos);
                return;
            }
        }
        let event_start = self.events.len();
        let node = &self.nodes[id];
        if matches!(node.ty, NodeType::CopperBulb) {
            let powered = has_main_input(node);
            if powered != node.powered {
                let lit = (node.output_power > 0) ^ powered;
                if powered {
                    self.events
                        .push(Event::CopperBulbToggle { node_id: id, lit });
                }
                self.set_node(id, powered, bool_to_ss(lit));
            }
        } else if update::update_node(&mut self.scheduler, &mut self.events, &mut self.nodes, id) {
            if let Some(Event::NoteBlockPlay {
                noteblock_id,
                unblocked,
            }) = self.events.get_mut(event_start)
            {
                // Keep note eligibility at its power rise, independent of display cadence.
                let above = self.noteblock_info[*noteblock_id as usize]
                    .0
                    .offset(BlockFace::Top);
                *unblocked = self
                    .instant
                    .iter()
                    .find_map(|runtime| runtime.memory_block_at(above))
                    .map(|block| block == Block::Air);
            }
            self.notify_observer_watchers(id);
        }
    }

    fn notify_observer_watchers(&mut self, source: NodeId) {
        if let Some(observers) = self.observer_watchers.get(&source) {
            update::notify_observers(&mut self.scheduler, &mut self.nodes, observers);
        }
    }

    /// Propagate region outputs until no source changes or delivered samples remain.
    /// Each pass commits one writer's samples before processing the next writer.
    pub(super) fn evaluate_instant(&mut self, clock_event: bool) {
        if self.evaluating_instant {
            return;
        }
        self.evaluating_instant = true;
        if clock_event {
            let elapsed = self.instant.first().map_or(0, |runtime| runtime.elapsed());
            while let Some(index) = self
                .instant_phases
                .iter()
                .position(|&(_, deadline)| deadline <= elapsed)
            {
                let (region, _) = self.instant_phases.remove(index).unwrap();
                self.publish_instant_regions(std::iter::once(region), true);
                self.finish_native_delivery();
            }
        }
        loop {
            self.finish_native_delivery();
            self.publish_instant_regions(0..self.instant.len(), false);
            if !self.instant_dirty.iter().any(|&dirty| dirty)
                && self.boundary_deliveries.is_empty()
                && self.native_delivery.is_none()
            {
                break;
            }
        }
        self.evaluating_instant = false;
    }

    fn publish_instant_regions(
        &mut self,
        regions: impl IntoIterator<Item = usize>,
        clock_event: bool,
    ) {
        let mut outputs = Vec::new();
        let mut geometry = Vec::new();
        for region in regions {
            let changed = std::mem::replace(&mut self.instant_dirty[region], false);
            if !changed && !clock_event {
                continue;
            }
            let changes = self.instant[region].advance(&self.nodes, changed, clock_event);
            if outputs.is_empty() {
                outputs = changes;
            } else {
                outputs.extend(changes);
            }
            geometry.extend(self.instant[region].geometry_changes());
            let deadline = self.instant[region].next_phase_deadline();
            let scheduled = self
                .instant_phases
                .iter()
                .find(|&&(id, _)| id == region)
                .map(|&(_, deadline)| deadline);
            if scheduled != deadline {
                self.instant_phases.retain(|&(id, _)| id != region);
                if let Some(deadline) = deadline {
                    self.instant_phases.push_back((region, deadline));
                }
            }
            for (source, targets) in self.instant[region].take_deliveries() {
                self.boundary_deliveries.push_back(BoundaryDelivery {
                    source,
                    recipients: vec![(region, targets)],
                });
            }
        }
        let mut committed = std::mem::take(&mut self.instant_changes);
        committed.extend(
            outputs
                .into_iter()
                .map(|(id, strength)| self.commit_node(id, strength != 0, strength)),
        );
        // Commit the complete response before any consumer reads either channel.
        for change in committed.drain(..) {
            self.notify_change(change);
        }
        self.instant_changes = committed;
        for pos in geometry {
            if let Some(observers) = self.instant_observers.get(&pos) {
                update::notify_observers(&mut self.scheduler, &mut self.nodes, observers);
            }
            self.native_geometry_notification(pos);
        }
    }

    pub(crate) fn tick_with_world<W: World>(&mut self, world: &mut W) {
        self.native_commands(world);
        self.process_command_outputs(world);
        world.piston_state_mut().logical_tick += 1;
        self.piston_state.logical_tick = world.piston_state().logical_tick;
        self.tick_after_callbacks(Some(world));
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
        self.restore_native(world);
        // Display flushing can clear dirty flags without writing hidden nodes.
        // Handoff must materialize their current strengths, including ordinary
        // dust between a virtual region supply and its consumer.
        for node in self.nodes.inner_mut() {
            if !matches!(node.ty, NodeType::Constant) {
                node.changed = true;
            }
        }
        self.flush(world, false);
        // Logical dust restoration reads ordinary comparator entities.
        // Export their current analog strengths before materializing regions.
        let nodes = std::mem::take(&mut self.nodes);

        for (i, node) in nodes.into_inner().iter().enumerate() {
            let Some((pos, block)) = self.blocks[i] else {
                continue;
            };
            if self.native.as_ref().is_some_and(|native| native.owns(pos)) {
                continue;
            }
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

        for runtime in std::mem::take(&mut self.instant) {
            runtime.materialize(world);
        }
        self.scheduler
            .reset_runtime(world, &self.blocks, &self.block_aliases);

        self.pos_map.clear();
        self.block_aliases.clear();
        self.noteblock_info.clear();
        self.far_comparators.clear();
        self.instant_dependencies.clear();
        self.instant_dirty.clear();
        self.observer_watchers.clear();
        self.instant_observers.clear();
        self.events.clear();
    }

    pub(crate) fn on_use_block(&mut self, pos: BlockPos) {
        if self.native.as_ref().is_some_and(|native| native.owns(pos)) {
            self.native_on_use(pos);
            self.evaluate_instant(false);
            return;
        }
        let node_id = self.pos_map[&pos];
        let node = &self.nodes[node_id];
        match node.ty {
            NodeType::Button => {
                if node.powered {
                    return;
                }
                self.schedule_tick(node_id.into(), 10, TickPriority::Normal);
                self.set_node(node_id, true, 15);
            }
            NodeType::Lever => {
                self.set_node(node_id, !node.powered, bool_to_ss(!node.powered));
            }
            _ => warn!("Tried to use a {:?} redpiler node", node.ty),
        }
        self.evaluate_instant(false);
    }

    pub(crate) fn set_pressure_plate(&mut self, pos: BlockPos, powered: bool) {
        if self.native.as_ref().is_some_and(|native| native.owns(pos)) {
            self.native_pressure_plate(pos, powered);
            self.evaluate_instant(false);
            return;
        }
        let node_id = self.pos_map[&pos];
        let node = &self.nodes[node_id];
        match node.ty {
            NodeType::PressurePlate => {
                self.set_node(node_id, powered, bool_to_ss(powered));
            }
            _ => warn!("Tried to set pressure plate state for a {:?}", node.ty),
        }
        self.evaluate_instant(false);
    }

    pub(crate) fn tick(&mut self) {
        self.piston_state.logical_tick += 1;
        self.tick_after_callbacks::<crate::plot::PlotWorld>(None);
    }

    fn tick_after_callbacks<W: World>(&mut self, mut output: Option<&mut W>) {
        self.piston_state.phase = mchprs_world::AdvancePhase::ScheduledTicks;
        for runtime in &mut self.instant {
            runtime.begin_tick();
        }
        // Imported work at the current deadline runs before advancing, as in the interpreter.
        for advance in [false, true] {
            if advance {
                self.scheduler.end_last_tick_move_next();
            }
            while let Some(entry) = self.scheduler.this_tick().pop_first() {
                match entry {
                    RuntimeTick::Node(node_id) => self.tick_node(node_id),
                    RuntimeTick::Block(entry) => {
                        if let Some(output) = output.as_deref_mut() {
                            self.native_tick(entry, Some(output));
                        } else {
                            self.native_tick(entry, None);
                        }
                    }
                }
                if let Some(output) = output.as_deref_mut() {
                    self.process_command_outputs(output);
                }
                // Separate delivered events retain scheduler order and committed memory.
                self.evaluate_instant(false);
            }
        }
        // Owned periodic clocks sample after ordinary events at this deadline.
        self.piston_state.phase = mchprs_world::AdvancePhase::PistonEvents;
        self.evaluate_instant(false);
        self.evaluate_instant(true);
        for runtime in &mut self.instant {
            runtime.end_tick();
        }
        self.piston_state.phase = mchprs_world::AdvancePhase::BetweenTicks;
    }

    pub(crate) fn flush<W: World>(&mut self, world: &mut W, io_only: bool) {
        self.flush_native(world, io_only);
        self.process_command_outputs(world);
        self.evaluate_instant(false);
        for event in self.events.drain(..) {
            match event {
                Event::CopperBulbToggle { node_id, lit } => {
                    if let Some((pos, _)) = self.blocks[node_id.index()] {
                        crate::redstone::copper_bulb::play_toggle(world, pos, lit);
                    }
                }
                Event::NoteBlockPlay {
                    noteblock_id,
                    unblocked,
                } => {
                    let (pos, instrument, note) = self.noteblock_info[noteblock_id as usize];
                    if unblocked.unwrap_or_else(|| noteblock::is_noteblock_unblocked(world, pos)) {
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
            if self.blocks[i]
                .is_some_and(|(pos, _)| self.native.as_ref().is_some_and(|native| native.owns(pos)))
            {
                node.changed = false;
                continue;
            }
            if node.changed && (!io_only || node.is_io) {
                for (pos, block) in self.blocks[i]
                    .iter_mut()
                    .chain(self.block_aliases.get_mut(&i).into_iter().flatten())
                {
                    if let Some(powered) = block_powered_mut(block) {
                        *powered = node.powered
                    }
                    if let Some(plate) = block.with_pressure_plate_power(node.powered) {
                        *block = plate;
                    }
                    if let Some(bulb) =
                        block.with_copper_bulb_state(node.output_power > 0, node.powered)
                    {
                        *block = bulb;
                    }
                    if let Block::RedstoneWire { wire, .. } = block {
                        wire.power = node.output_power
                    };
                    if let Block::RedstoneRepeater { repeater } = block {
                        repeater.locked = node.locked;
                    }
                    world.set_block(*pos, *block);
                }
            }
            node.changed = false;
        }
        if !io_only {
            for runtime in &mut self.instant {
                runtime.flush_memory(world);
            }
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
    scheduler: &mut TickScheduler<RuntimeTick>,
    node_id: NodeId,
    node: &mut Node,
    delay: usize,
    priority: TickPriority,
) {
    node.pending_tick = true;
    scheduler.schedule_tick(node_id.into(), delay, priority);
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
                NodeType::Observer => "Observer".to_string(),
                NodeType::Comparator { mode, .. } => format!(
                    "Comparator({})",
                    match mode {
                        ComparatorMode::Compare => "Cmp",
                        ComparatorMode::Subtract => "Sub",
                    }
                ),
                NodeType::Lamp => "Lamp".to_string(),
                NodeType::CopperBulb => "CopperBulb".to_string(),
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
