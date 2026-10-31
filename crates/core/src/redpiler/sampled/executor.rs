use super::program::ArrangementSource;
use super::{Operation, Program, ScratchError};
use crate::world::scheduler::{ScheduledBlockTick, TickScheduler};
use mchprs_blocks::blocks::Block;
use mchprs_world::TickPriority;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PistonEvent {
    Extend,
    Retract,
    EarlyRetract,
}

impl PistonEvent {
    fn bit(self) -> u8 {
        1 << self as u8
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Motion {
    pub(super) piston: usize,
    pub(super) token: Option<usize>,
    pub(super) destination: mchprs_blocks::BlockPos,
    pub(super) start: u64,
    pub(super) landing: u64,
    pub(super) order: usize,
    pub(super) extending: bool,
    pub(super) piston_done: bool,
    pub(super) token_done: bool,
}

pub(crate) struct Runtime {
    pub(super) comparator_outputs: Vec<u8>,
    pub(super) reactive_wire_strengths: Vec<u8>,
    pub(super) program: std::sync::Arc<Program>,
    pub(super) piston_state: mchprs_world::PistonState,
    pub(super) piston_extended: Vec<bool>,
    pub(super) piston_head_present: Vec<bool>,
    pub(super) piston_head_moving: Vec<bool>,
    pub(super) piston_base_moving: Vec<bool>,
    pub(super) token_positions: Vec<Option<mchprs_blocks::BlockPos>>,
    pub(super) group_arrangements: Vec<usize>,
    pub(super) arrangement_contributions: Vec<u8>,
    pub(super) piston_events: std::collections::VecDeque<(usize, PistonEvent)>,
    pub(super) queued_piston_events: Vec<u8>,
    pub(super) motions: Vec<Motion>,
    pub(super) motion_time: u64,
    pub(super) motion_phase: bool,
    pub(super) landing_cursor: Option<usize>,
    pub(super) accepted_piston_actions: usize,
    pub(super) token_movements: usize,
    pub(super) motion_completions: usize,
    pub(super) trace_enabled: bool,
    pub(super) trace_events: Vec<String>,
    pub(super) blocks: Vec<Block>,
    pub(super) pending: TickScheduler<ScheduledBlockTick>,
    pub(super) contributions: Vec<u8>,
    pub(super) watched_strengths: Vec<u8>,
    pub(super) phase: u64,
    pub(super) updated_in_phase: Vec<Option<u64>>,
    pub(super) operations: usize,
    pub(super) deliveries: usize,
    pub(super) scheduled_repeater_ticks: usize,
    pub(super) tick_operations: usize,
    pub(super) failure: Option<ScratchError>,
}

pub(super) fn compare(rear: u8, side: u8, mode: mchprs_blocks::blocks::ComparatorMode) -> (u8, bool) {
    let subtract = mode == mchprs_blocks::blocks::ComparatorMode::Subtract;
    let powered = rear > 0 && (rear > side || (rear == side && !subtract));
    (
        if side > rear {
            0
        } else if subtract {
            rear - side
        } else {
            rear
        },
        powered,
    )
}

impl Runtime {
    fn notify_source_watchers(&mut self, source: usize, shape: bool, update: bool) {
        let watchers = self.program.source_watchers[source].clone();
        for watcher in watchers {
            if !self.consume_operation(self.program.positions[watcher]) {
                return;
            }
            let delivered = if matches!(self.blocks[watcher], Block::Observer { .. }) {
                shape
            } else {
                update
            };
            if self.trace_enabled && delivered {
                self.trace(format!(
                    "deliver {:?} -> {:?} shape={shape} update={update}",
                    self.program.positions[source], self.program.positions[watcher]
                ));
            }
            if delivered {
                self.update_once(watcher);
            }
        }
    }

    fn notify_home_watchers(&mut self, piston: usize, shape_only: bool) {
        let watchers = self.program.home_watchers.get(piston).cloned().unwrap_or_default();
        for watcher in watchers {
            if !self.consume_operation(self.program.positions[watcher]) {
                return;
            }
            if !shape_only || matches!(self.blocks[watcher], Block::Observer { .. }) {
                self.update_once(watcher);
            }
        }
    }
    fn trace(&mut self, event: String) {
        if self.trace_enabled && self.trace_events.len() < 2048 {
            self.trace_events.push(format!(
                "tick={} phase={} seq={} {event}",
                self.motion_time,
                self.phase,
                self.trace_events.len()
            ));
        }
    }
    pub(crate) fn project(&self, position: mchprs_blocks::BlockPos) -> Option<Block> {
        if self
            .motions
            .iter()
            .any(|motion| motion.token.is_some() && !motion.token_done && motion.destination == position)
        {
            return None;
        }
        if let Some(index) = self.token_positions.iter().position(|&at| at == Some(position)) {
            return Some(self.program.tokens[index].block);
        }
        for (index, piston) in self.program.pistons.iter().enumerate() {
            if position == piston.base && self.piston_base_moving[index] {
                return None;
            }
            if position == piston.home {
                if self.piston_head_moving[index] {
                    return None;
                }
                if self.piston_head_present[index] {
                    return Some(Block::PistonHead {
                        head: mchprs_blocks::blocks::RedstonePistonHead {
                            facing: piston.facing,
                            sticky: piston.sticky,
                            short: false,
                        },
                    });
                }
            }
        }
        if self.program.tokens.iter().any(|token| token.cells.contains(&position))
            && self.program.tokens.iter().any(|token| token.position == Some(position))
        {
            return Some(Block::Air);
        }
        Some(
            self.program
                .component_indices
                .get(&position)
                .map(|&index| self.blocks[index])
                .or_else(|| self.program.geometry_indices.get(&position).copied())
                .unwrap_or(Block::Air),
        )
    }
    fn notify_shape(&mut self, position: mchprs_blocks::BlockPos) {
        for index in 0..self.blocks.len() {
            let block = self.blocks[index];
            if let Block::Observer { observer } = block
                && self.program.positions[index].offset(observer.facing.into()) == position
                && !observer.powered
            {
                if !self.consume_operation(self.program.positions[index]) {
                    return;
                }
                let tick = ScheduledBlockTick {
                    pos: self.program.positions[index],
                    block_type: Some(block.registry_id()),
                };
                if !self.pending.contains(&tick) {
                    self.pending.schedule_tick(tick, 1, TickPriority::Normal);
                    if self.trace_enabled {
                        self.trace(format!("observer notified {:?} by shape at {position:?}", self.program.positions[index]));
                    }
                }
            }
        }
    }
    pub(super) fn queue_piston_extension(&mut self, index: usize) {
        if self.queued_piston_events[index] & 1 == 0 {
            self.queued_piston_events[index] |= 1;
            self.piston_events.push_back((index, PistonEvent::Extend));
        }
    }

    fn pushing_motion(&self, piston: usize) -> Option<usize> {
        let piston = &self.program.pistons[piston];
        self.motions.iter().position(|motion| {
            motion.token.is_some()
                && !motion.token_done
                && motion.extending
                && motion.destination == piston.far
                && self.program.pistons[motion.piston].facing == piston.facing
        })
    }

    fn is_early_retract(&self, motion: usize) -> bool {
        let motion = self.motions[motion];
        if self.motion_phase && self.motion_time.saturating_sub(motion.start) > 1 {
            self.landing_cursor.is_some_and(|cursor| motion.order < cursor)
        } else {
            true
        }
    }

    fn add_motion(&mut self, piston: usize, token: Option<usize>, destination: mchprs_blocks::BlockPos, extending: bool) -> usize {
        let landing = self.motion_time + 2;
        let order = self.motions.iter().filter(|motion| motion.landing == landing).count();
        let slot = self.motions.len();
        self.motions.push(Motion {
            piston,
            token,
            destination,
            start: self.motion_time,
            landing,
            order,
            extending,
            piston_done: false,
            token_done: token.is_none(),
        });
        slot
    }

    pub(super) fn update_piston(&mut self, component: usize) {
        let Some(index) = self.program.pistons.iter().position(|p| p.base == self.program.positions[component]) else {
            return;
        };
        if self.piston_base_moving[index] {
            return;
        }
        let powered = self.input_strength(component) > 0;
        if self.trace_enabled {
            let fixed: Vec<_> = self.program.incoming[component]
                .iter()
                .map(|&link| (self.program.positions[self.program.links[link].source], self.contributions[link]))
                .collect();
            let token_inputs = self.token_dependency_signature(usize::MAX, component, None);
            self.trace(format!(
                "piston sample {:?} powered={powered} extended={} fixed={fixed:?} token_inputs={token_inputs:?}",
                self.program.pistons[index].base, self.piston_extended[index]
            ));
        }
        if powered == self.piston_extended[index] {
            return;
        }
        let pushing = self.pushing_motion(index);
        let event = if powered {
            PistonEvent::Extend
        } else if pushing.is_some_and(|motion| self.is_early_retract(motion)) {
            PistonEvent::EarlyRetract
        } else {
            PistonEvent::Retract
        };
        let bit = event.bit();
        if self.queued_piston_events[index] & bit == 0 {
            self.queued_piston_events[index] |= bit;
            self.piston_events.push_back((index, event));
            if self.trace_enabled {
                self.trace(format!("piston request {:?} extending={powered}", self.program.pistons[index].base));
            }
        }
    }

    fn run_piston_events(&mut self) {
        while let Some((index, event)) = self.piston_events.pop_front() {
            let extending = event == PistonEvent::Extend;
            let piston = self.program.pistons[index].clone();
            if !self.consume_operation(piston.base) {
                return;
            }
            self.queued_piston_events[index] &= !event.bit();
            let Some(component) = self.program.positions.iter().position(|&pos| pos == piston.base) else {
                self.failure = Some(ScratchError {
                    position: Some(piston.base),
                    reason: "piston component missing",
                });
                return;
            };
            self.updated_in_phase[component] = None;
            if self.piston_base_moving[index] || (self.input_strength(component) > 0) != extending {
                if self.trace_enabled {
                    self.trace(format!(
                        "piston rejected {:?} extending={extending} reason=power_or_base_motion",
                        piston.base
                    ));
                }
                continue;
            }
            if !extending {
                let early = event == PistonEvent::EarlyRetract;
                if self.piston_head_moving[index] {
                    if let Some(slot) = self
                        .motions
                        .iter()
                        .position(|motion| motion.piston == index && motion.extending && !motion.piston_done)
                    {
                        self.finish_piston_part(slot, true);
                    }
                }
                self.piston_base_moving[index] = true;
                let token = if piston.sticky && !early {
                    self.token_positions.iter().position(|&position| position == Some(piston.far))
                } else {
                    None
                };
                self.add_motion(index, token, piston.home, false);
                self.notify_source_watchers(component, true, true);
                if let Some(token) = token {
                    self.token_movements += 1;
                    if self.trace_enabled {
                        self.trace(format!("token {token} departs {:?} for {:?}", piston.far, piston.home));
                    }
                    self.set_token_pos(token, None);
                }
                if self.trace_enabled {
                    self.trace(format!(
                        "piston accepted {:?} retract token={token:?} landing={}",
                        piston.base,
                        self.motion_time + 2
                    ));
                }
                self.accepted_piston_actions += 1;
                if piston.sticky {
                    if let Some(slot) = self.pushing_motion(index) {
                        self.finish_token_part(slot);
                        continue;
                    }
                }
                if self.piston_head_present[index] {
                    self.piston_head_present[index] = false;
                    self.notify_shape(piston.home);
                }
                continue;
            }
            if self.piston_extended[index] || self.piston_head_present[index] || self.piston_head_moving[index] {
                if self.trace_enabled {
                    self.trace(format!("piston rejected {:?} extend reason=head_out", piston.base));
                }
                continue;
            }
            if self.program.piston_scenery.contains(&piston.home)
                || self
                    .motions
                    .iter()
                    .any(|m| m.token.is_some() && !m.token_done && m.destination == piston.home)
                || self
                    .program
                    .pistons
                    .iter()
                    .enumerate()
                    .any(|(other, p)| other != index && p.home == piston.home && (self.piston_head_present[other] || self.piston_head_moving[other]))
            {
                if self.trace_enabled {
                    self.trace(format!("piston rejected {:?} extend reason=home_blocked", piston.base));
                }
                continue;
            }
            let token = self.token_positions.iter().position(|&position| position == Some(piston.home));
            let onward = token.and_then(|_| self.token_positions.iter().position(|&position| position == Some(piston.far)));
            let mate = self
                .program
                .pistons
                .iter()
                .position(|other| other.group == piston.group && other.far == piston.far && other.facing == piston.facing.opposite());
            let mate_home_open = mate.is_some_and(|mate| {
                let home = self.program.pistons[mate].home;
                !self.program.piston_scenery.contains(&home)
                    && !self.token_positions.contains(&Some(home))
                    && !self
                        .motions
                        .iter()
                        .any(|motion| motion.token.is_some() && !motion.token_done && motion.destination == home)
                    && !self.piston_head_present[mate]
                    && !self.piston_head_moving[mate]
            });
            let far_blocked =
                self.program.piston_scenery.contains(&piston.far)
                    || self
                        .motions
                        .iter()
                        .any(|motion| motion.token.is_some() && !motion.token_done && motion.destination == piston.far)
                    || self.program.pistons.iter().enumerate().any(|(other, p)| {
                        other != index && p.home == piston.far && (self.piston_head_present[other] || self.piston_head_moving[other])
                    })
                    || (onward.is_some() && !mate_home_open);
            if token.is_some() && far_blocked {
                if self.trace_enabled {
                    self.trace(format!("piston rejected {:?} extend reason=far_blocked", piston.base));
                }
                continue;
            }
            if token.is_some_and(|token| !self.program.tokens[token].cells.contains(&piston.far)) {
                self.failure = Some(ScratchError {
                    position: Some(piston.far),
                    reason: "unsupported token destination",
                });
                return;
            }
            if let Some(onward) = onward {
                let mate = mate.expect("an opposed piston is required to push its mate token onward");
                self.token_movements += 1;
                self.set_token_pos(onward, None);
                let slot = self.add_motion(mate, Some(onward), self.program.pistons[mate].home, true);
                self.motions[slot].piston_done = true;
                if self.failure.is_some() {
                    return;
                }
            }
            if let Some(token) = token {
                self.token_movements += 1;
                if self.trace_enabled {
                    self.trace(format!("token {token} departs {:?} for {:?}", piston.home, piston.far));
                }
                self.set_token_pos(token, None);
            }
            self.piston_extended[index] = true;
            if let Block::Piston { mut piston } = self.blocks[component] {
                piston.extended = true;
                self.blocks[component] = Block::Piston { piston };
            }
            self.notify_source_watchers(component, true, true);
            self.piston_head_moving[index] = true;
            self.add_motion(index, token, piston.far, true);
            if self.trace_enabled {
                self.trace(format!(
                    "piston accepted {:?} extend token={token:?} from={:?} to={:?} landing={}",
                    piston.base,
                    piston.home,
                    piston.far,
                    self.motion_time + 2
                ));
            }
            self.notify_shape(piston.home);
            if token.is_some() {
                self.notify_shape(piston.far);
            }
            self.accepted_piston_actions += 1;
        }
    }

    fn finish_token_part(&mut self, slot: usize) {
        let motion = self.motions[slot];
        if let Some(token) = motion.token
            && !motion.token_done
        {
            self.motions[slot].token_done = true;
            self.set_token_pos(token, Some(motion.destination));
        }
    }

    pub(super) fn finish_piston_part(&mut self, slot: usize, interrupted: bool) {
        let motion = self.motions[slot];
        if motion.piston_done {
            return;
        }
        self.motions[slot].piston_done = true;
        let base = self.program.pistons[motion.piston].base;
        let home = self.program.pistons[motion.piston].home;
        if let Some(component) = self.program.positions.iter().position(|&pos| pos == base) {
            self.updated_in_phase[component] = None;
        }
        if motion.extending {
            self.piston_head_moving[motion.piston] = false;
            self.piston_head_present[motion.piston] = !interrupted;
            self.notify_shape(home);
            self.notify_home_watchers(motion.piston, false);
        } else {
            self.piston_base_moving[motion.piston] = false;
            self.piston_extended[motion.piston] = false;
            if let Some(component) = self.program.positions.iter().position(|&pos| pos == base)
                && let Block::Piston { mut piston } = self.blocks[component]
            {
                piston.extended = false;
                self.blocks[component] = Block::Piston { piston };
                self.notify_source_watchers(component, true, true);
            }
        }
        if !interrupted && let Some(component) = self.program.positions.iter().position(|&pos| pos == base) {
            self.update_piston(component);
        }
    }

    fn tick_motions(&mut self) {
        let Some(last_order) = self
            .motions
            .iter()
            .filter(|motion| motion.landing == self.motion_time)
            .map(|motion| motion.order)
            .max()
        else {
            return;
        };
        self.motion_phase = true;
        for cursor in 0..=last_order {
            self.landing_cursor = Some(cursor);
            let Some(slot) = self
                .motions
                .iter()
                .position(|motion| motion.landing == self.motion_time && motion.order == cursor)
            else {
                continue;
            };
            let motion = self.motions[slot];
            if !motion.piston_done || !motion.token_done {
                if !self.consume_operation(self.program.pistons[motion.piston].base) {
                    break;
                }
                self.motion_completions += 1;
                if motion.extending {
                    self.finish_token_part(slot);
                    self.finish_piston_part(slot, false);
                } else {
                    self.finish_piston_part(slot, false);
                    self.finish_token_part(slot);
                }
                if self.trace_enabled {
                    self.trace(format!(
                        "motion complete {:?} token={:?} destination={:?} extending={}",
                        self.program.pistons[motion.piston].base, motion.token, motion.destination, motion.extending
                    ));
                }
            }
            self.motions.remove(slot);
        }
        self.landing_cursor = None;
        self.motion_phase = false;
    }
    fn consume_operation(&mut self, position: mchprs_blocks::BlockPos) -> bool {
        if self.failure.is_some() {
            return false;
        }
        if self.tick_operations >= 1_000_000 {
            self.failure = Some(ScratchError {
                position: Some(position),
                reason: "runtime semantic operation limit exceeded",
            });
            return false;
        }
        self.tick_operations += 1;
        true
    }

    pub(super) fn input_strength(&self, target: usize) -> u8 {
        let fixed = self.program.incoming[target]
            .iter()
            .filter_map(|&id| {
                let source = ArrangementSource::Component(self.program.links[id].source);
                (!self
                    .program
                    .input_arrangements
                    .iter()
                    .any(|group| group.target == target && !group.side && group.conditional_sources.contains(&source)))
                .then_some(self.contributions[id])
            })
            .max()
            .unwrap_or(0);
        self.program
            .arrangement_links
            .iter()
            .enumerate()
            .filter_map(|(index, link)| (link.target == target && !link.side).then_some(self.arrangement_contributions[index]))
            .fold(fixed, u8::max)
    }

    pub(super) fn arrangement_output(&self, source: ArrangementSource) -> u8 {
        match source {
            ArrangementSource::Component(node) => self.output(node),
            ArrangementSource::Token(token) => {
                if matches!(self.program.tokens[token].block, Block::RedstoneBlock) {
                    15
                } else {
                    0
                }
            }
        }
    }

    pub(super) fn set_token_pos(&mut self, token: usize, position: Option<mchprs_blocks::BlockPos>) {
        let old = self.token_positions[token];
        if old == position {
            return;
        }
        let Some(cell_pos) = position.or(old) else {
            return;
        };
        let Some(cell) = self.program.tokens[token].cells.iter().position(|&at| at == cell_pos) else {
            self.failure = Some(ScratchError {
                position: Some(cell_pos),
                reason: "unsupported token position",
            });
            return;
        };
        if old.is_some() && position.is_some() {
            self.failure = Some(ScratchError {
                position: Some(cell_pos),
                reason: "token relocation requires departure before arrival",
            });
            return;
        }
        self.token_positions[token] = position;
        for piston in &self.program.pistons {
            if self.program.tokens[token].cells.contains(&piston.home)
                && let Some(component) = self.program.positions.iter().position(|&pos| pos == piston.base)
            {
                self.updated_in_phase[component] = None;
            }
        }
        let mut switched = Vec::new();
        let bindings = self.program.token_movement_groups.get(token).cloned().unwrap_or_default();
        for (group, axis) in bindings {
            let record = self.program.movement_groups[group].axes[axis].clone();
            let before = self.group_arrangements[group];
            let delta = (cell + 1) * record.stride;
            let after = if position.is_some() { before + delta } else { before - delta };
            self.group_arrangements[group] = after;
            let flips = if position.is_some() {
                record.arrive[after].clone()
            } else {
                record.depart[before].clone()
            };
            for flip in flips {
                if !self.consume_operation(self.program.positions[flip.target]) {
                    return;
                }
                let strength = self.arrangement_output(flip.source);
                self.arrangement_contributions[flip.link] = if flip.entering { strength.saturating_sub(flip.attenuation) } else { 0 };
                if strength > flip.attenuation {
                    self.updated_in_phase[flip.target] = None;
                }
                // WithMoveStep reserves -2 for pistons: flips do not deliver them.
                if !matches!(self.blocks[flip.target], Block::Piston { .. }) && !switched.contains(&flip.target) {
                    switched.push(flip.target);
                }
                self.operations += 1;
            }
        }
        let steps = self
            .program
            .move_steps
            .get(token)
            .and_then(|cells| cells.get(cell))
            .map(|edges| edges[usize::from(position.is_some())].clone())
            .unwrap_or_default();
        // Movement callbacks read the new wire level even when that wire is not a piston watcher.
        for &node in &switched {
            if matches!(self.blocks[node], Block::RedstoneWire { .. }) {
                let strength = self.input_strength(node);
                self.refresh_wire_power(node, strength);
            }
        }
        let mut changed_wires = Vec::new();
        for step in steps {
            if !self.consume_operation(self.program.positions[step.node]) {
                return;
            }
            if step.wire {
                if let Some(index) = switched.iter().position(|&node| node == step.node) {
                    switched.remove(index);
                    if self.mark_changed_wire(step.node) {
                        changed_wires.push(step.node);
                    }
                }
            } else if switched.contains(&step.node) || step.watcher || step.wires.iter().any(|wire| changed_wires.contains(wire)) {
                switched.retain(|&node| node != step.node);
                self.update_once(step.node);
            }
            self.operations += 1;
        }
        for node in switched {
            if !self.consume_operation(self.program.positions[node]) {
                return;
            }
            if self.program.watched_wires.iter().any(|wire| wire.wire == node) {
                if self.mark_changed_wire(node) {
                    changed_wires.push(node);
                }
            } else if matches!(self.blocks[node], Block::RedstoneWire { .. }) {
                let strength = self.input_strength(node);
                self.refresh_wire_power(node, strength);
            } else {
                self.update_once(node);
            }
        }
        for wire in changed_wires {
            if let Some(record) = self.program.watched_wires.iter().find(|record| record.wire == wire) {
                let watchers = record.watchers.clone();
                for watcher in watchers {
                    if !self.consume_operation(self.program.positions[watcher]) {
                        return;
                    }
                    self.update_once(watcher);
                }
            }
        }
    }

    fn mark_changed_wire(&mut self, wire: usize) -> bool {
        let Some(index) = self.program.watched_wires.iter().position(|record| record.wire == wire) else {
            return false;
        };
        let strength = self.input_strength(wire);
        self.refresh_wire_power(wire, strength);
        if strength == self.watched_strengths[index] {
            return false;
        }
        self.watched_strengths[index] = strength;
        true
    }

    fn refresh_wire_power(&mut self, wire: usize, strength: u8) {
        let Block::RedstoneWire { wire: mut state } = self.blocks[wire] else {
            unreachable!()
        };
        let previous = state.power;
        state.power = strength;
        self.blocks[wire] = Block::RedstoneWire { wire: state };
        if previous == strength {
            return;
        }
        let outgoing = self.program.outgoing[wire].clone();
        for index in outgoing {
            let link = self.program.links[index].clone();
            let contribution = strength.saturating_sub(link.attenuation);
            let old = self.contributions[index];
            self.contributions[index] = contribution;
            if (old > 0) != (contribution > 0)
                || (old != contribution
                    && (matches!(self.blocks[link.target], Block::RedstoneComparator { .. })
                        || self.program.watched_wires.iter().any(|record| record.wire == link.target)))
            {
                self.updated_in_phase[link.target] = None;
            }
            if self.trace_enabled && old != contribution {
                self.trace(format!(
                    "link {:?} -> {:?} strength={old}->{contribution}",
                    self.program.positions[wire], self.program.positions[link.target]
                ));
            }
        }
        let outgoing_arrangements = self.program.outgoing_arrangements[wire].clone();
        for index in outgoing_arrangements {
            let link = self.program.arrangement_links[index].clone();
            let contribution = if link.active[self.group_arrangements[link.group]] {
                strength.saturating_sub(link.attenuation)
            } else {
                0
            };
            let old = self.arrangement_contributions[index];
            self.arrangement_contributions[index] = contribution;
            if (old > 0) != (contribution > 0)
                || (old != contribution
                    && (matches!(self.blocks[link.target], Block::RedstoneComparator { .. })
                        || self.program.watched_wires.iter().any(|record| record.wire == link.target)))
            {
                self.updated_in_phase[link.target] = None;
            }
        }
    }

    // shortcut: receiver signatures omit geometry-dependent fixed links, extend arrangement groups to all readers.
    fn token_dependency_signature(&self, changed: usize, target: usize, position: Option<mchprs_blocks::BlockPos>) -> Vec<(ArrangementSource, u8)> {
        let token_position = |token: usize| if token == changed { position } else { self.token_positions[token] };
        let mut result = Vec::new();
        for group in self.program.input_arrangements.iter().filter(|group| group.target == target) {
            let mut index = 0;
            let mut stride = 1;
            for &token in &group.tokens {
                let record = &self.program.tokens[token];
                let digit = token_position(token)
                    .and_then(|pos| record.cells.iter().position(|&cell| cell == pos))
                    .map_or(0, |cell| cell + 1);
                index += digit * stride;
                stride *= record.cells.len() + 1;
            }
            result.extend(group.inputs[index].iter().copied());
        }
        for (token, record) in self.program.tokens.iter().enumerate() {
            if self
                .program
                .input_arrangements
                .iter()
                .any(|group| group.target == target && group.conditional_sources.contains(&ArrangementSource::Token(token)))
            {
                continue;
            }
            let Some(cell) = token_position(token).and_then(|pos| record.cells.iter().position(|&cell| cell == pos)) else {
                continue;
            };
            if let Some(inputs) = self.program.token_sources.get(token).and_then(|cells| cells.get(cell)) {
                result.extend(
                    inputs
                        .iter()
                        .filter_map(|&(receiver, attenuation)| (receiver == target).then_some((ArrangementSource::Token(token), attenuation))),
                );
            }
        }
        result.sort_unstable();
        result
    }

    pub(super) fn output(&self, source: usize) -> u8 {
        match self.blocks[source] {
            Block::RedstoneBlock => {
                let position = self.program.positions[source];
                if self
                    .program
                    .tokens
                    .iter()
                    .any(|record| record.position == Some(position) && matches!(record.block, Block::RedstoneBlock))
                {
                    0
                } else {
                    15
                }
            }
            Block::Lever { lever } => {
                if lever.powered {
                    15
                } else {
                    0
                }
            }
            Block::StoneButton { button } => {
                if button.powered {
                    15
                } else {
                    0
                }
            }
            Block::RedstoneComparator { comparator } => {
                if comparator.powered {
                    self.comparator_outputs[source]
                } else {
                    0
                }
            }
            Block::RedstoneWire { wire } => wire.power,
            Block::Observer { observer } => {
                if observer.powered {
                    15
                } else {
                    0
                }
            }
            Block::RedstoneTorch { lit } | Block::RedstoneWallTorch { lit, .. } => {
                if lit {
                    15
                } else {
                    0
                }
            }
            Block::RedstoneRepeater { repeater } => {
                if repeater.powered {
                    15
                } else {
                    0
                }
            }
            _ => 0,
        }
    }

    pub(super) fn begin_phase(&mut self) {
        self.phase = self.phase.wrapping_add(1);
        self.updated_in_phase.fill(None);
    }

    pub(super) fn dispatch_receiver(&mut self, receiver: usize) {
        self.deliveries += 1;
        let power = self.input_strength(receiver);
        let Block::RedstoneLamp { lit } = self.blocks[receiver] else {
            unreachable!()
        };
        if power > 0 && !lit {
            self.blocks[receiver] = Block::RedstoneLamp { lit: true };
        } else if power == 0 && lit {
            let tick = ScheduledBlockTick {
                pos: self.program.positions[receiver],
                block_type: Some(self.blocks[receiver].registry_id()),
            };
            if !self.pending.contains(&tick) {
                self.pending.schedule_tick(tick, 2, TickPriority::Normal);
            }
        }
    }

    pub(super) fn update_once(&mut self, receiver: usize) {
        if self.updated_in_phase[receiver] == Some(self.phase) {
            if self.trace_enabled {
                self.trace(format!("watcher suppressed {:?}", self.program.positions[receiver]));
            }
            return;
        }
        self.updated_in_phase[receiver] = Some(self.phase);
        if self.trace_enabled {
            self.trace(format!("callback {:?}", self.program.positions[receiver]));
        }
        self.update(receiver);
    }

    pub(super) fn update(&mut self, receiver: usize) {
        match self.blocks[receiver] {
            Block::RedstoneComparator { .. } => self.update_comparator(receiver),
            Block::RedstoneLamp { .. } => self.dispatch_receiver(receiver),
            Block::Piston { .. } => self.update_piston(receiver),
            Block::RedstoneRepeater { .. } => self.update_repeater(receiver),
            Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. } => self.update_torch(receiver),
            Block::Observer { observer } if !observer.powered => {
                let tick = ScheduledBlockTick {
                    pos: self.program.positions[receiver],
                    block_type: Some(self.blocks[receiver].registry_id()),
                };
                if !self.pending.contains(&tick) {
                    self.pending.schedule_tick(tick, 1, TickPriority::Normal);
                }
            }
            _ => {}
        }
    }

    pub(super) fn check_watched_wire(&mut self, record_index: usize) {
        let wire = self.program.watched_wires[record_index].wire;
        let new_strength = self.input_strength(wire);
        if new_strength == self.watched_strengths[record_index] {
            return;
        }
        if self.trace_enabled {
            self.trace(format!(
                "watched wire {:?} strength={}->{}",
                self.program.positions[wire], self.watched_strengths[record_index], new_strength
            ));
        }
        self.watched_strengths[record_index] = new_strength;
        let watcher_count = self.program.watched_wires[record_index].watchers.len();
        for index in 0..watcher_count {
            let watcher = self.program.watched_wires[record_index].watchers[index];
            self.update_once(watcher);
        }
    }

    pub(super) fn execute_output_program(&mut self, component: usize, powered: bool) {
        let reactive = self.program.reactive.iter().any(|&flag| flag);
        let operations = self.program.output_programs[component][usize::from(powered)].clone();
        if reactive {
            for operation in &operations {
                match *operation {
                    Operation::Contribution(index) => {
                        if !self.consume_operation(self.program.positions[component]) {
                            return;
                        }
                        let link = self.program.links[index].clone();
                        let value = self.output(link.source).saturating_sub(link.attenuation);
                        if self.contributions[index] != value {
                            self.updated_in_phase[link.target] = None;
                        }
                        self.contributions[index] = value;
                        self.operations += 1;
                    }
                    Operation::ArrangementContribution(index) => {
                        if !self.consume_operation(self.program.positions[component]) {
                            return;
                        }
                        let link = self.program.arrangement_links[index].clone();
                        let value = if link.active[self.group_arrangements[link.group]] {
                            self.arrangement_output(link.source).saturating_sub(link.attenuation)
                        } else {
                            0
                        };
                        if self.arrangement_contributions[index] != value {
                            self.updated_in_phase[link.target] = None;
                        }
                        self.arrangement_contributions[index] = value;
                        self.operations += 1;
                    }
                    _ => {}
                }
            }
        }
        for operation in &operations {
            if reactive {
                match *operation {
                    Operation::Contribution(_) | Operation::ArrangementContribution(_) => continue,
                    Operation::UpdateLamp(node)
                    | Operation::UpdateRepeater(node)
                    | Operation::UpdateComparator(node)
                    | Operation::UpdateTorch(node)
                        if self.program.reactive[node] =>
                    {
                        continue;
                    }
                    _ => {}
                }
            }
            if !self.consume_operation(self.program.positions[component]) {
                return;
            }
            match *operation {
                Operation::ArrangementContribution(index) => {
                    let link = self.program.arrangement_links[index].clone();
                    let contribution = if link.active[self.group_arrangements[link.group]] {
                        self.arrangement_output(link.source).saturating_sub(link.attenuation)
                    } else {
                        0
                    };
                    if (self.arrangement_contributions[index] > 0) != (contribution > 0)
                        || (self.arrangement_contributions[index] != contribution
                            && (matches!(self.blocks[link.target], Block::RedstoneComparator { .. })
                                || self.program.watched_wires.iter().any(|wire| wire.wire == link.target)))
                    {
                        self.updated_in_phase[link.target] = None;
                    }
                    self.arrangement_contributions[index] = contribution;
                }
                Operation::Contribution(link_index) => {
                    let link = self.program.links[link_index].clone();
                    let old_contribution = self.contributions[link_index];
                    let contribution = self.output(link.source).saturating_sub(link.attenuation);
                    self.contributions[link_index] = contribution;
                    if self.trace_enabled && old_contribution != contribution {
                        self.trace(format!(
                            "link {:?} -> {:?} strength={old_contribution}->{contribution}",
                            self.program.positions[link.source], self.program.positions[link.target]
                        ));
                    }
                    if (old_contribution > 0) != (contribution > 0)
                        || (old_contribution != contribution
                            && (matches!(self.blocks[link.target], Block::RedstoneComparator { .. })
                                || self.program.watched_wires.iter().any(|wire| wire.wire == link.target)))
                    {
                        self.updated_in_phase[link.target] = None;
                    }
                }
                Operation::RefreshWire(slot) => {
                    let strength = self.input_strength(slot);
                    self.refresh_wire_power(slot, strength);
                }
                Operation::CheckWatchedWire(record) => self.check_watched_wire(record),
                Operation::UpdateLamp(slot) => self.dispatch_receiver(slot),
                Operation::UpdateRepeater(slot) => self.update_repeater(slot),
                Operation::UpdatePiston(slot) => self.update_piston(slot),
                Operation::UpdateTorch(slot) => self.update_torch(slot),
                Operation::UpdateComparator(slot) => self.update_comparator(slot),
            }
            self.operations += 1;
        }
        if reactive {
            self.update_reactive(component);
        }
    }

    pub(super) fn side_strength(&self, component: usize) -> u8 {
        let fixed = self
            .program
            .side_links
            .iter()
            .filter(|link| link.target == component)
            .filter(|link| {
                !self.program.input_arrangements.iter().any(|group| {
                    group.target == component && group.side && group.conditional_sources.contains(&ArrangementSource::Component(link.source))
                })
            })
            .map(|link| self.output(link.source).saturating_sub(link.attenuation))
            .max()
            .unwrap_or(0);
        self.program
            .arrangement_links
            .iter()
            .enumerate()
            .filter_map(|(index, link)| (link.target == component && link.side).then_some(self.arrangement_contributions[index]))
            .fold(fixed, u8::max)
    }

    fn comparator_result(&self, component: usize) -> (u8, bool) {
        let Block::RedstoneComparator { comparator } = self.blocks[component] else {
            unreachable!()
        };
        let record = self.program.comparators.iter().find(|record| record.component == component).unwrap();
        let mut rear = self.input_strength(component);
        if let Some(level) = record.rear_override {
            rear = level;
        } else if rear < 15 {
            if let Some(level) = record.far_override {
                rear = level;
            }
        }
        compare(rear, self.side_strength(component), comparator.mode)
    }

    pub(super) fn update_comparator(&mut self, component: usize) {
        let Block::RedstoneComparator { comparator } = self.blocks[component] else {
            unreachable!()
        };
        let (level, powered) = self.comparator_result(component);
        if level == self.comparator_outputs[component] && powered == comparator.powered {
            return;
        }
        let tick = ScheduledBlockTick {
            pos: self.program.positions[component],
            block_type: Some(self.blocks[component].registry_id()),
        };
        if !self.pending.contains(&tick) {
            let diode = self
                .program
                .comparators
                .iter()
                .find(|record| record.component == component)
                .unwrap()
                .output_is_diode;
            self.pending
                .schedule_tick(tick, 1, if diode { TickPriority::High } else { TickPriority::Normal });
        }
    }

    fn tick_comparator(&mut self, component: usize) {
        let Block::RedstoneComparator { mut comparator } = self.blocks[component] else {
            unreachable!()
        };
        let from = self.output(component);
        let (level, powered) = self.comparator_result(component);
        let previous = self.comparator_outputs[component];
        self.comparator_outputs[component] = level;
        if previous == level && comparator.mode == mchprs_blocks::blocks::ComparatorMode::Subtract {
            return;
        }
        let shape = comparator.powered != powered;
        comparator.powered = powered;
        self.blocks[component] = Block::RedstoneComparator { comparator };
        if from != self.output(component) {
            self.execute_output_program(component, self.output(component) > from);
        } else {
            self.update_reactive(component);
        }
        self.notify_source_watchers(component, shape, true);
    }

    fn update_reactive(&mut self, source: usize) {
        for index in self.program.reactive_by_source[source].clone() {
            let record = self.program.reactive_wires[index].clone();
            let mut strength = 0;
            for &(source, attenuation) in &record.sources {
                if !self.consume_operation(self.program.positions[source]) {
                    return;
                }
                let level = if matches!(self.blocks[source], Block::RedstoneBlock) {
                    15
                } else {
                    self.output(source)
                };
                strength = strength.max(level.saturating_sub(attenuation));
                self.operations += 1;
            }
            if self.reactive_wire_strengths[index] == strength {
                continue;
            }
            self.reactive_wire_strengths[index] = strength;
            for &target in &record.recipients {
                if !self.consume_operation(self.program.positions[target]) {
                    return;
                }
                self.update(target);
            }
        }
        for target in self.program.reactive_neighbours[source].clone() {
            if !self.consume_operation(self.program.positions[target]) {
                return;
            }
            self.update(target);
        }
    }

    fn update_torch(&mut self, component: usize) {
        let lit = match self.blocks[component] {
            Block::RedstoneTorch { lit } | Block::RedstoneWallTorch { lit, .. } => lit,
            _ => return,
        };
        if lit == (self.input_strength(component) > 0) {
            let tick = ScheduledBlockTick {
                pos: self.program.positions[component],
                block_type: Some(self.blocks[component].registry_id()),
            };
            if !self.pending.contains(&tick) {
                self.pending.schedule_tick(tick, 1, TickPriority::Normal);
            }
        }
    }

    fn tick_torch(&mut self, component: usize) {
        let lit = match self.blocks[component] {
            Block::RedstoneTorch { lit } | Block::RedstoneWallTorch { lit, .. } => lit,
            _ => return,
        };
        if lit != (self.input_strength(component) > 0) {
            return;
        }
        self.blocks[component] = match self.blocks[component] {
            Block::RedstoneTorch { .. } => Block::RedstoneTorch { lit: !lit },
            Block::RedstoneWallTorch { facing, .. } => Block::RedstoneWallTorch { lit: !lit, facing },
            _ => unreachable!(),
        };
        self.execute_output_program(component, !lit);
        self.notify_source_watchers(component, true, true);
    }

    pub(super) fn update_repeater(&mut self, component: usize) {
        let Block::RedstoneRepeater { mut repeater } = self.blocks[component] else {
            unreachable!()
        };
        if let Some(side_sources) = self
            .program
            .repeaters
            .iter()
            .find(|record| record.component == component)
            .and_then(|record| record.side_sources.as_ref())
        {
            repeater.locked =
                side_sources.iter().any(|&source| {
                    !self.program.input_arrangements.iter().any(|group| {
                        group.target == component && group.side && group.conditional_sources.contains(&ArrangementSource::Component(source))
                    }) && self.output(source) > 0
                }) || self
                    .program
                    .arrangement_links
                    .iter()
                    .enumerate()
                    .any(|(index, link)| link.target == component && link.side && self.arrangement_contributions[index] > 0);
            self.blocks[component] = Block::RedstoneRepeater { repeater };
        }
        if repeater.locked {
            if self.trace_enabled {
                self.trace(format!(
                    "repeater callback {:?} powered={} locked=true rear={} side={} pending={:?}",
                    self.program.positions[component], repeater.powered,
                    self.input_strength(component), self.side_strength(component),
                    self.pending.iter_entries().filter(|entry| entry.pos == self.program.positions[component]).collect::<Vec<_>>()
                ));
            }
            return;
        }
        let rear_powered = self.input_strength(component) > 0;
        if self.trace_enabled {
            self.trace(format!(
                "repeater callback {:?} powered={} locked=false rear={} side={} pending={:?}",
                self.program.positions[component], repeater.powered, self.input_strength(component),
                self.side_strength(component),
                self.pending.iter_entries().filter(|entry| entry.pos == self.program.positions[component]).collect::<Vec<_>>()
            ));
        }
        if rear_powered == repeater.powered {
            return;
        }
        let tick = ScheduledBlockTick {
            pos: self.program.positions[component],
            block_type: Some(self.blocks[component].registry_id()),
        };
        if self.pending.contains(&tick) {
            return;
        }
        let output_is_diode = self
            .program
            .repeaters
            .iter()
            .find(|record| record.component == component)
            .is_some_and(|record| record.output_is_diode);
        let priority = if output_is_diode {
            TickPriority::Highest
        } else if !rear_powered {
            TickPriority::Higher
        } else {
            TickPriority::High
        };
        self.pending.schedule_tick(tick, repeater.delay as usize, priority);
        if self.trace_enabled {
            let remaining = self
                .pending
                .iter_entries()
                .find(|entry| entry.pos == self.program.positions[component])
                .map(|entry| entry.ticks_left);
            self.trace(format!(
                "schedule repeater {:?} remaining={remaining:?} priority={priority:?}",
                self.program.positions[component]
            ));
        }
    }

    pub(crate) fn set_input(&mut self, source_index: usize, powered: bool) -> Result<(), ScratchError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        let Some(&source) = self.program.sources.get(source_index) else {
            return Err(ScratchError {
                position: None,
                reason: "runtime set_input source index out of bounds",
            });
        };
        match self.blocks[source] {
            Block::Lever { lever } if lever.powered != powered => {}
            Block::StoneButton { button } if button.powered != powered => {}
            Block::Lever { .. } | Block::StoneButton { .. } => return Ok(()),
            _ => unreachable!(),
        }
        self.begin_phase();
        match self.blocks[source] {
            Block::Lever { mut lever } => {
                lever.powered = powered;
                self.blocks[source] = Block::Lever { lever };
            }
            Block::StoneButton { mut button } => {
                button.powered = powered;
                self.blocks[source] = Block::StoneButton { button };
                if powered {
                    let tick = ScheduledBlockTick {
                        pos: self.program.positions[source],
                        block_type: Some(self.blocks[source].registry_id()),
                    };
                    self.pending.schedule_tick(tick, 10, TickPriority::Normal);
                }
            }
            _ => unreachable!(),
        }
        if self.trace_enabled {
            self.trace(format!("input {:?} powered={powered}", self.program.positions[source]));
        }
        self.execute_output_program(source, powered);
        self.notify_source_watchers(source, true, true);
        self.failure.clone().map_or(Ok(()), Err)
    }

    pub(super) fn drain_ticks(&mut self) {
        while let Some(tick) = self.pending.this_tick().pop_first() {
            if !self.consume_operation(tick.pos) {
                return;
            }
            if self.trace_enabled {
                self.trace(format!("due tick {:?} type={:?}", tick.pos, tick.block_type));
            }
            if let Some(component) = self.program.positions.iter().position(|&pos| pos == tick.pos) {
                if tick.block_type != Some(self.blocks[component].registry_id()) {
                    continue;
                }
                match self.blocks[component] {
                    Block::RedstoneComparator { .. } => self.tick_comparator(component),
                    Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. } => self.tick_torch(component),
                    Block::RedstoneLamp { .. } => self.dispatch_receiver_tick(component),
                    Block::StoneButton { mut button } if button.powered => {
                        button.powered = false;
                        self.blocks[component] = Block::StoneButton { button };
                        self.execute_output_program(component, false);
                        self.notify_source_watchers(component, true, true);
                    }
                    Block::RedstoneRepeater { .. } => {
                        self.scheduled_repeater_ticks += 1;
                        self.tick_repeater(component);
                    }
                    Block::Observer { mut observer } => {
                        observer.powered = !observer.powered;
                        if self.trace_enabled {
                            self.trace(format!(
                                "observer tick {:?} powered={}",
                                self.program.positions[component], observer.powered
                            ));
                        }
                        self.blocks[component] = Block::Observer { observer };
                        self.notify_source_watchers(component, true, false);
                        if observer.powered {
                            self.pending.schedule_tick(tick, 1, TickPriority::Normal);
                        }
                        self.execute_output_program(component, observer.powered);
                        self.notify_source_watchers(component, false, true);
                    }
                    _ => {}
                }
                self.operations += 1;
            }
        }
    }

    fn tick_repeater(&mut self, component: usize) {
        if self.trace_enabled {
            self.trace(format!("repeater tick {:?}", self.program.positions[component]));
        }
        let Block::RedstoneRepeater { mut repeater } = self.blocks[component] else {
            unreachable!()
        };
        if repeater.locked {
            return;
        }
        let rear_powered = self.input_strength(component) > 0;
        if repeater.powered {
            if rear_powered {
                return;
            }
            repeater.powered = false;
            self.blocks[component] = Block::RedstoneRepeater { repeater };
            self.execute_output_program(component, false);
            self.notify_source_watchers(component, true, true);
        } else {
            if !rear_powered {
                let tick = ScheduledBlockTick {
                    pos: self.program.positions[component],
                    block_type: Some(self.blocks[component].registry_id()),
                };
                self.pending.schedule_tick(tick, repeater.delay as usize, TickPriority::Higher);
            }
            repeater.powered = true;
            self.blocks[component] = Block::RedstoneRepeater { repeater };
            self.execute_output_program(component, true);
            self.notify_source_watchers(component, true, true);
        }
    }

    pub(super) fn dispatch_receiver_tick(&mut self, receiver: usize) {
        if self.input_strength(receiver) == 0 {
            self.blocks[receiver] = Block::RedstoneLamp { lit: false };
        }
    }

    pub(crate) fn tick(&mut self) -> Result<(), ScratchError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        self.motion_time += 1;
        self.begin_phase();
        self.drain_ticks();
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        self.pending.end_last_tick_move_next();
        self.drain_ticks();
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        if !self.program.pistons.is_empty() {
            self.begin_phase();
            self.run_piston_events();
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            self.begin_phase();
            self.tick_motions();
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
        }
        self.tick_operations = 0;
        Ok(())
    }

    pub(crate) fn component_count(&self) -> usize {
        self.program.positions.len()
    }

    pub(crate) fn accepts_control(&self, pos: mchprs_blocks::BlockPos) -> bool {
        self.program.sources.iter().any(|&source| self.program.positions[source] == pos)
    }

    pub(crate) fn use_control(&mut self, pos: mchprs_blocks::BlockPos) -> Result<(), ScratchError> {
        let Some(source) = self.program.sources.iter().position(|&source| self.program.positions[source] == pos) else {
            return Ok(());
        };
        match self.blocks[self.program.sources[source]] {
            Block::Lever { lever } => self.set_input(source, !lever.powered),
            Block::StoneButton { .. } => self.set_input(source, true),
            _ => unreachable!(),
        }
    }

    pub(crate) fn projected_blocks(&self) -> Vec<(mchprs_blocks::BlockPos, Block)> {
        self.program
            .projection_positions
            .iter()
            .copied()
            .into_iter()
            .filter_map(|pos| self.project(pos).map(|block| (pos, block)))
            .collect()
    }
}
