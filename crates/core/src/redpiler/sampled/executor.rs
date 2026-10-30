use super::super::TickScheduler;
use super::{Operation, Program, ScheduledBlockTick, ScratchError};
use super::program::ArrangementSource;
use mchprs_blocks::blocks::Block;
use mchprs_world::TickPriority;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PistonEvent {
    Extend,
    Retract,
    EarlyRetract,
}

impl PistonEvent {
    fn bit(self) -> u8 { 1 << self as u8 }
}

pub(super) struct Runtime<'a> {
    pub(super) program: &'a Program,
    pub(super) piston_extended: Vec<bool>,
    pub(super) piston_head_present: Vec<bool>,
    pub(super) piston_head_moving: Vec<bool>,
    pub(super) piston_base_moving: Vec<bool>,
    pub(super) token_positions: Vec<Option<mchprs_blocks::BlockPos>>,
    pub(super) group_arrangements: Vec<usize>,
    pub(super) arrangement_contributions: Vec<u8>,
    pub(super) piston_events: std::collections::VecDeque<(usize, PistonEvent)>,
    pub(super) queued_piston_events: Vec<u8>,
    pub(super) motions: Vec<(usize, Option<usize>, mchprs_blocks::BlockPos, u64, bool)>,
    pub(super) motion_time: u64,
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

impl Runtime<'_> {
    fn notify_source_watchers(&mut self, source: usize, shape: bool, update: bool) {
        let watchers = self.program.source_watchers[source].clone();
        for watcher in watchers {
            if !self.consume_operation(self.program.positions[watcher]) { return; }
            if if matches!(self.blocks[watcher], Block::Observer { .. }) { shape } else { update } {
                self.update_once(watcher);
            }
        }
    }

    fn notify_home_watchers(&mut self, piston: usize, shape_only: bool) {
        let watchers = self.program.home_watchers.get(piston).cloned().unwrap_or_default();
        for watcher in watchers {
            if !self.consume_operation(self.program.positions[watcher]) { return; }
            if !shape_only || matches!(self.blocks[watcher], Block::Observer { .. }) { self.update_once(watcher); }
        }
    }
    fn trace(&mut self, event: String) {
        if self.trace_enabled && self.trace_events.len() < 2048 {
            self.trace_events.push(format!("tick={} phase={} seq={} {event}", self.motion_time, self.phase, self.trace_events.len()));
        }
    }
    pub(super) fn project(&self, position: mchprs_blocks::BlockPos) -> Option<Block> {
        if self.motions.iter().any(|motion| motion.1.is_some() && motion.2 == position) {
            return None;
        }
        if let Some(index) = self.token_positions.iter().position(|&at| at == Some(position)) {
            return Some(self.program.tokens[index].block);
        }
        for (index, piston) in self.program.pistons.iter().enumerate() {
            if position == piston.base && self.piston_base_moving[index] { return None; }
            if position == piston.home {
                if self.piston_head_moving[index] { return None; }
                if self.piston_head_present[index] {
                    return Some(Block::PistonHead { head: mchprs_blocks::blocks::RedstonePistonHead { facing: piston.facing, sticky: piston.sticky, short: false } });
                }
            }
        }
        if self.program.tokens.iter().any(|token| token.cells.contains(&position))
            && self.program.tokens.iter().any(|token| token.position == Some(position)) {
            return Some(Block::Air);
        }
        Some(self.program.positions.iter().position(|&at| at == position).map(|index| self.blocks[index])
            .or_else(|| self.program.geometry.iter().find_map(|&(at, block)| (at == position).then_some(block))).unwrap_or(Block::Air))
    }
    fn notify_shape(&mut self, position: mchprs_blocks::BlockPos) {
        for index in 0..self.blocks.len() {
            let block = self.blocks[index];
            if let Block::Observer { observer } = block
                && self.program.positions[index].offset(observer.facing.into()) == position
                && !observer.powered {
                if !self.consume_operation(self.program.positions[index]) { return; }
                let tick = ScheduledBlockTick { pos: self.program.positions[index], block_type: Some(block.registry_id()) };
                if !self.pending.contains(&tick) {
                    self.pending.schedule_tick(tick, 1, TickPriority::Normal);
                    if self.trace_enabled { self.trace(format!("observer notified {:?} by shape at {position:?}", self.program.positions[index])); }
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

    pub(super) fn update_piston(&mut self, component: usize) {
        let Some(index) = self.program.pistons.iter().position(|p| p.base == self.program.positions[component]) else { return; };
        if self.piston_base_moving[index] { return; }
        let powered = self.input_strength(component) > 0;
        if self.trace_enabled {
            let fixed: Vec<_> = self.program.incoming[component].iter().map(|&link| (self.program.positions[self.program.links[link].source], self.contributions[link])).collect();
            let token_inputs = self.token_dependency_signature(usize::MAX, component, None);
            self.trace(format!("piston sample {:?} powered={powered} extended={} fixed={fixed:?} token_inputs={token_inputs:?}", self.program.pistons[index].base, self.piston_extended[index]));
        }
        if powered == self.piston_extended[index] { return; }
        let piston = &self.program.pistons[index];
        // shortcut: landing-phase age/order classification awaits independent motion-part records.
        let pushing = self.motions.iter().any(|motion| motion.0 != usize::MAX && motion.1.is_some() && motion.4 && motion.2 == piston.far && self.program.pistons[motion.0].facing == piston.facing);
        let event = if powered { PistonEvent::Extend } else if pushing { PistonEvent::EarlyRetract } else { PistonEvent::Retract };
        let bit = event.bit();
        if self.queued_piston_events[index] & bit == 0 {
            self.queued_piston_events[index] |= bit;
            self.piston_events.push_back((index, event));
            if self.trace_enabled { self.trace(format!("piston request {:?} extending={powered}", self.program.pistons[index].base)); }
        }
    }

    fn run_piston_events(&mut self) {
        while let Some((index, event)) = self.piston_events.pop_front() {
            let extending = event == PistonEvent::Extend;
            let piston = &self.program.pistons[index];
            if !self.consume_operation(piston.base) { return; }
            self.queued_piston_events[index] &= !event.bit();
            let Some(component) = self.program.positions.iter().position(|&pos| pos == piston.base) else {
                self.failure = Some(ScratchError { position: Some(piston.base), reason: "piston component missing" });
                return;
            };
            self.updated_in_phase[component] = None;
            if self.piston_base_moving[index] || (self.input_strength(component) > 0) != extending {
                if self.trace_enabled { self.trace(format!("piston rejected {:?} extending={extending} reason=power_or_base_motion", piston.base)); }
                continue;
            }
            if !extending {
                let early = event == PistonEvent::EarlyRetract;
                if self.piston_head_moving[index] {
                    if let Some(slot) = self.motions.iter().position(|motion| motion.0 == index && motion.4) {
                        let (_, token, destination, _, _) = self.motions[slot];
                        self.motions[slot].0 = usize::MAX;
                        self.motions[slot].1 = None;
                        self.piston_head_moving[index] = false;
                        if let Some(token) = token {
                            self.set_token_pos(token, Some(destination));
                        }
                    }
                }
                self.piston_base_moving[index] = true;
                self.notify_source_watchers(component, true, true);
                let token = if piston.sticky && !early { self.token_positions.iter().position(|&position| position == Some(piston.far)) } else { None };
                if let Some(token) = token {
                    self.token_movements += 1;
                    if self.trace_enabled { self.trace(format!("token {token} departs {:?} for {:?}", piston.far, piston.home)); }
                    self.set_token_pos(token, None);
                }
                self.piston_head_present[index] = false;
                self.motions.push((index, token, piston.home, self.motion_time + 2, false));
                if self.trace_enabled { self.trace(format!("piston accepted {:?} retract token={token:?} landing={}", piston.base, self.motion_time + 2)); }
                self.notify_shape(piston.home);
                self.accepted_piston_actions += 1;
                continue;
            }
            if self.piston_extended[index] || self.piston_head_present[index] || self.piston_head_moving[index] {
                if self.trace_enabled { self.trace(format!("piston rejected {:?} extend reason=head_out", piston.base)); }
                continue;
            }
            if self.program.piston_scenery.contains(&piston.home) || self.motions.iter().any(|m| m.1.is_some() && m.2 == piston.home) || self.program.pistons.iter().enumerate().any(|(other, p)| other != index && p.home == piston.home && (self.piston_head_present[other] || self.piston_head_moving[other])) {
                if self.trace_enabled { self.trace(format!("piston rejected {:?} extend reason=home_blocked", piston.base)); }
                continue;
            }
            let token = self.token_positions.iter().position(|&position| position == Some(piston.home));
            if token.is_some() && (self.program.piston_scenery.contains(&piston.far) || self.token_positions.contains(&Some(piston.far)) || self.motions.iter().any(|motion| motion.1.is_some() && motion.2 == piston.far) || self.program.pistons.iter().enumerate().any(|(other, p)| other != index && p.home == piston.far && (self.piston_head_present[other] || self.piston_head_moving[other]))) {
                if self.trace_enabled { self.trace(format!("piston rejected {:?} extend reason=far_blocked", piston.base)); }
                continue;
            }
            if let Some(token) = token {
                if !self.program.tokens[token].cells.contains(&piston.far) {
                    self.failure = Some(ScratchError { position: Some(piston.far), reason: "unsupported token destination" });
                    return;
                }
                self.token_movements += 1;
                if self.trace_enabled { self.trace(format!("token {token} departs {:?} for {:?}", piston.home, piston.far)); }
                self.set_token_pos(token, None);
            }
            self.piston_extended[index] = true;
            if let Block::Piston { mut piston } = self.blocks[component] {
                piston.extended = true;
                self.blocks[component] = Block::Piston { piston };
            }
            self.notify_source_watchers(component, true, true);
            self.piston_head_moving[index] = true;
            self.motions.push((index, token, piston.far, self.motion_time + 2, true));
            if self.trace_enabled { self.trace(format!("piston accepted {:?} extend token={token:?} from={:?} to={:?} landing={}", piston.base, piston.home, piston.far, self.motion_time + 2)); }
            self.notify_shape(piston.home);
            if token.is_some() { self.notify_shape(piston.far); }
            self.accepted_piston_actions += 1;
        }
    }

    fn tick_motions(&mut self) {
        let mut index = 0;
        while index < self.motions.len() {
            if self.motions[index].3 > self.motion_time { index += 1; continue; }
            let (piston, token, destination, _, extending) = self.motions.remove(index);
            if piston == usize::MAX { continue; }
            if !self.consume_operation(self.program.pistons[piston].base) { return; }
            if extending {
                if let Some(token) = token { self.set_token_pos(token, Some(destination)); }
                self.piston_head_moving[piston] = false;
                self.piston_head_present[piston] = true;
            } else {
                self.piston_base_moving[piston] = false;
                self.piston_extended[piston] = false;
                if let Some(component) = self.program.positions.iter().position(|&pos| pos == self.program.pistons[piston].base)
                    && let Block::Piston { mut piston } = self.blocks[component] {
                    piston.extended = false;
                    self.blocks[component] = Block::Piston { piston };
                }
                self.notify_shape(self.program.pistons[piston].base);
                if let Some(token) = token { self.set_token_pos(token, Some(destination)); }
            }
            self.motion_completions += 1;
            if self.trace_enabled { self.trace(format!("motion complete {:?} token={token:?} destination={destination:?} extending={extending}", self.program.pistons[piston].base)); }
            self.notify_shape(self.program.pistons[piston].home);
            self.notify_home_watchers(piston, false);
            if let Some(component) = self.program.positions.iter().position(|&pos| pos == self.program.pistons[piston].base) { self.update_piston(component); }
        }
    }
    fn consume_operation(&mut self, position: mchprs_blocks::BlockPos) -> bool {
        if self.failure.is_some() { return false; }
        if self.tick_operations >= 1_000_000 {
            self.failure = Some(ScratchError { position: Some(position), reason: "runtime semantic operation limit exceeded" });
            return false;
        }
        self.tick_operations += 1;
        true
    }

    pub(super) fn input_strength(&self, target: usize) -> u8 {
        let fixed = self.program.incoming[target].iter().filter_map(|&id| {
            let source = ArrangementSource::Component(self.program.links[id].source);
            (!self.program.input_arrangements.iter().any(|group| group.target == target && !group.side && group.conditional_sources.contains(&source))).then_some(self.contributions[id])
        }).max().unwrap_or(0);
        self.program.arrangement_links.iter().enumerate().filter_map(|(index, link)| {
            (link.target == target && !link.side).then_some(self.arrangement_contributions[index])
        }).fold(fixed, u8::max)
    }

    pub(super) fn arrangement_output(&self, source: ArrangementSource) -> u8 {
        match source {
            ArrangementSource::Component(node) => self.output(node),
            ArrangementSource::Token(token) => if matches!(self.program.tokens[token].block, Block::RedstoneBlock) { 15 } else { 0 },
        }
    }

    pub(super) fn set_token_pos(&mut self, token: usize, position: Option<mchprs_blocks::BlockPos>) {
        let old = self.token_positions[token];
        if old == position { return; }
        let Some(cell_pos) = position.or(old) else { return; };
        let Some(cell) = self.program.tokens[token].cells.iter().position(|&at| at == cell_pos) else {
            self.failure = Some(ScratchError { position: Some(cell_pos), reason: "unsupported token position" });
            return;
        };
        if old.is_some() && position.is_some() {
            self.failure = Some(ScratchError { position: Some(cell_pos), reason: "token relocation requires departure before arrival" });
            return;
        }
        self.token_positions[token] = position;
        let mut switched = Vec::new();
        let bindings = self.program.token_movement_groups.get(token).cloned().unwrap_or_default();
        for (group, axis) in bindings {
            let record = &self.program.movement_groups[group].axes[axis];
            let before = self.group_arrangements[group];
            let delta = (cell + 1) * record.stride;
            let after = if position.is_some() { before + delta } else { before - delta };
            self.group_arrangements[group] = after;
            let flips = if position.is_some() { &record.arrive[after] } else { &record.depart[before] };
            for flip in flips {
                if !self.consume_operation(self.program.positions[flip.target]) { return; }
                let strength = self.arrangement_output(flip.source);
                self.arrangement_contributions[flip.link] = if flip.entering { strength.saturating_sub(flip.attenuation) } else { 0 };
                if strength > flip.attenuation { self.updated_in_phase[flip.target] = None; }
                // MAIR's WithMoveStep reserves -2 for pistons: flips do not deliver them.
                if !matches!(self.blocks[flip.target], Block::Piston { .. }) && !switched.contains(&flip.target) {
                    switched.push(flip.target);
                }
                self.operations += 1;
            }
        }
        let steps = self.program.move_steps.get(token).and_then(|cells| cells.get(cell))
            .map(|edges| edges[usize::from(position.is_some())].clone()).unwrap_or_default();
        let mut changed_wires = Vec::new();
        for step in steps {
            if !self.consume_operation(self.program.positions[step.node]) { return; }
            if step.wire {
                if let Some(index) = switched.iter().position(|&node| node == step.node) {
                    switched.remove(index);
                    if self.mark_changed_wire(step.node) { changed_wires.push(step.node); }
                }
            } else if switched.contains(&step.node) || step.watcher || step.wires.iter().any(|wire| changed_wires.contains(wire)) {
                switched.retain(|&node| node != step.node);
                self.update_once(step.node);
            }
            self.operations += 1;
        }
        for node in switched {
            if !self.consume_operation(self.program.positions[node]) { return; }
            if self.program.watched_wires.iter().any(|wire| wire.wire == node) {
                if self.mark_changed_wire(node) { changed_wires.push(node); }
            } else if let Block::RedstoneWire { mut wire } = self.blocks[node] {
                wire.power = self.input_strength(node);
                self.blocks[node] = Block::RedstoneWire { wire };
            } else {
                self.update_once(node);
            }
        }
        for wire in changed_wires {
            if let Some(record) = self.program.watched_wires.iter().find(|record| record.wire == wire) {
                let watchers = record.watchers.clone();
                for watcher in watchers {
                    if !self.consume_operation(self.program.positions[watcher]) { return; }
                    self.update_once(watcher);
                }
            }
        }
    }

    fn mark_changed_wire(&mut self, wire: usize) -> bool {
        let Some(index) = self.program.watched_wires.iter().position(|record| record.wire == wire) else { return false; };
        let strength = self.input_strength(wire);
        if let Block::RedstoneWire { wire: mut state } = self.blocks[wire] {
            state.power = strength;
            self.blocks[wire] = Block::RedstoneWire { wire: state };
        }
        if strength == self.watched_strengths[index] { return false; }
        self.watched_strengths[index] = strength;
        true
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
                let digit = token_position(token).and_then(|pos| record.cells.iter().position(|&cell| cell == pos)).map_or(0, |cell| cell + 1);
                index += digit * stride;
                stride *= record.cells.len() + 1;
            }
            result.extend(group.inputs[index].iter().copied());
        }
        for (token, record) in self.program.tokens.iter().enumerate() {
            if self.program.input_arrangements.iter().any(|group| group.target == target && group.conditional_sources.contains(&ArrangementSource::Token(token))) { continue; }
            let Some(cell) = token_position(token).and_then(|pos| record.cells.iter().position(|&cell| cell == pos)) else { continue; };
            if let Some(inputs) = self.program.token_sources.get(token).and_then(|cells| cells.get(cell)) {
                result.extend(inputs.iter().filter_map(|&(receiver, attenuation)| (receiver == target).then_some((ArrangementSource::Token(token), attenuation))));
            }
        }
        result.sort_unstable();
        result
    }

    pub(super) fn output(&self, source: usize) -> u8 {
        match self.blocks[source] {
            Block::RedstoneBlock => {
                let position = self.program.positions[source];
                if self.program.tokens.iter().any(|record| record.position == Some(position) && matches!(record.block, Block::RedstoneBlock)) {
                    0
                } else { 15 }
            },
            Block::Lever { lever } => {
                if lever.powered {
                    15
                } else {
                    0
                }
            }
            Block::RedstoneWire { wire } => wire.power,
            Block::Observer { observer } => if observer.powered { 15 } else { 0 },
            Block::RedstoneTorch { lit } | Block::RedstoneWallTorch { lit, .. } => if lit { 15 } else { 0 },
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
            if self.trace_enabled { self.trace(format!("watcher suppressed {:?}", self.program.positions[receiver])); }
            return;
        }
        self.updated_in_phase[receiver] = Some(self.phase);
        match self.blocks[receiver] {
            Block::RedstoneLamp { .. } => self.dispatch_receiver(receiver),
            Block::Piston { .. } => self.update_piston(receiver),
            Block::RedstoneRepeater { .. } => self.update_repeater(receiver),
            Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. } => self.update_torch(receiver),
            Block::Observer { observer } if !observer.powered => {
                let tick = ScheduledBlockTick { pos: self.program.positions[receiver], block_type: Some(self.blocks[receiver].registry_id()) };
                if !self.pending.contains(&tick) { self.pending.schedule_tick(tick, 1, TickPriority::Normal); }
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
        if self.trace_enabled { self.trace(format!("watched wire {:?} strength={}->{}", self.program.positions[wire], self.watched_strengths[record_index], new_strength)); }
        self.watched_strengths[record_index] = new_strength;
        let watcher_count = self.program.watched_wires[record_index].watchers.len();
        for index in 0..watcher_count {
            let watcher = self.program.watched_wires[record_index].watchers[index];
            self.update_once(watcher);
        }
    }

    pub(super) fn execute_output_program(&mut self, component: usize, powered: bool) {
        for operation in &self.program.output_programs[component][usize::from(powered)] {
            if !self.consume_operation(self.program.positions[component]) { return; }
            match *operation {
                Operation::ArrangementContribution(index) => {
                    let link = &self.program.arrangement_links[index];
                    let contribution = if link.active[self.group_arrangements[link.group]] {
                        self.arrangement_output(link.source).saturating_sub(link.attenuation)
                    } else { 0 };
                    if (self.arrangement_contributions[index] > 0) != (contribution > 0)
                        || (self.arrangement_contributions[index] != contribution && self.program.watched_wires.iter().any(|wire| wire.wire == link.target)) {
                        self.updated_in_phase[link.target] = None;
                    }
                    self.arrangement_contributions[index] = contribution;
                }
                Operation::Contribution(link_index) => {
                    let link = &self.program.links[link_index];
                    let old_contribution = self.contributions[link_index];
                    let contribution = self.output(link.source).saturating_sub(link.attenuation);
                    self.contributions[link_index] = contribution;
                    if self.trace_enabled && old_contribution != contribution { self.trace(format!("link {:?} -> {:?} strength={old_contribution}->{contribution}", self.program.positions[link.source], self.program.positions[link.target])); }
                    if (old_contribution > 0) != (contribution > 0)
                        || (old_contribution != contribution && self.program.watched_wires.iter().any(|wire| wire.wire == link.target)) {
                        self.updated_in_phase[link.target] = None;
                    }
                }
                Operation::RefreshWire(slot) => {
                    let Block::RedstoneWire { mut wire } = self.blocks[slot] else {
                        unreachable!()
                    };
                    wire.power = self.input_strength(slot);
                    self.blocks[slot] = Block::RedstoneWire { wire };
                }
                Operation::CheckWatchedWire(record) => self.check_watched_wire(record),
                Operation::UpdateLamp(slot) => self.dispatch_receiver(slot),
                Operation::UpdateRepeater(slot) => self.update_repeater(slot),
                Operation::UpdatePiston(slot) => self.update_piston(slot),
                Operation::UpdateTorch(slot) => self.update_torch(slot),
            }
            self.operations += 1;
        }
    }

    fn update_torch(&mut self, component: usize) {
        let lit = match self.blocks[component] {
            Block::RedstoneTorch { lit } | Block::RedstoneWallTorch { lit, .. } => lit,
            _ => return,
        };
        if lit == (self.input_strength(component) > 0) {
            let tick = ScheduledBlockTick { pos: self.program.positions[component], block_type: Some(self.blocks[component].registry_id()) };
            if !self.pending.contains(&tick) { self.pending.schedule_tick(tick, 1, TickPriority::Normal); }
        }
    }

    fn tick_torch(&mut self, component: usize) {
        let lit = match self.blocks[component] {
            Block::RedstoneTorch { lit } | Block::RedstoneWallTorch { lit, .. } => lit,
            _ => return,
        };
        if lit != (self.input_strength(component) > 0) { return; }
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
        if let Some(side_sources) = self.program.repeaters.iter().find(|record| record.component == component).and_then(|record| record.side_sources.as_ref()) {
            repeater.locked = side_sources.iter().any(|&source| !self.program.input_arrangements.iter().any(|group| group.target == component && group.side && group.conditional_sources.contains(&ArrangementSource::Component(source))) && self.output(source) > 0)
                || self.program.arrangement_links.iter().enumerate().any(|(index, link)| link.target == component && link.side && self.arrangement_contributions[index] > 0);
            self.blocks[component] = Block::RedstoneRepeater { repeater };
        }
        if repeater.locked {
            return;
        }
        let rear_powered = self.input_strength(component) > 0;
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
            let remaining = self.pending.iter_entries().find(|entry| entry.pos == self.program.positions[component]).map(|entry| entry.ticks_left);
            self.trace(format!("schedule repeater {:?} remaining={remaining:?} priority={priority:?}", self.program.positions[component]));
        }
    }

    pub(super) fn set_input(&mut self, source_index: usize, powered: bool) -> Result<(), ScratchError> {
        if let Some(error) = &self.failure { return Err(error.clone()); }
        let Some(&source) = self.program.sources.get(source_index) else {
            return Err(ScratchError { position: None, reason: "runtime set_input source index out of bounds" });
        };
        let Block::Lever { lever } = self.blocks[source] else { unreachable!() };
        if lever.powered == powered {
            return Ok(());
        }
        self.begin_phase();
        let Block::Lever { mut lever } = self.blocks[source] else {
            unreachable!()
        };
        lever.powered = powered;
        self.blocks[source] = Block::Lever { lever };
        if self.trace_enabled { self.trace(format!("input {:?} powered={powered}", self.program.positions[source])); }
        self.execute_output_program(source, powered);
        self.notify_source_watchers(source, true, true);
        self.failure.clone().map_or(Ok(()), Err)
    }

    pub(super) fn drain_ticks(&mut self) {
        while let Some(tick) = self.pending.this_tick().pop_first() {
            if !self.consume_operation(tick.pos) { return; }
            if let Some(component) = self.program.positions.iter().position(|&pos| pos == tick.pos) {
                match self.blocks[component] {
                    Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. } => self.tick_torch(component),
                    Block::RedstoneLamp { .. } => self.dispatch_receiver_tick(component),
                    Block::RedstoneRepeater { .. } => {
                        self.scheduled_repeater_ticks += 1;
                        self.tick_repeater(component);
                    }
                    Block::Observer { mut observer } => {
                        observer.powered = !observer.powered;
                        if self.trace_enabled { self.trace(format!("observer tick {:?} powered={}", self.program.positions[component], observer.powered)); }
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
        if self.trace_enabled { self.trace(format!("repeater tick {:?}", self.program.positions[component])); }
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

    pub(super) fn tick(&mut self) -> Result<(), ScratchError> {
        if let Some(error) = &self.failure { return Err(error.clone()); }
        self.motion_time += 1;
        self.begin_phase();
        self.drain_ticks();
        if let Some(error) = &self.failure { return Err(error.clone()); }
        self.pending.end_last_tick_move_next();
        self.drain_ticks();
        if let Some(error) = &self.failure { return Err(error.clone()); }
        if !self.program.pistons.is_empty() {
            self.begin_phase();
            self.run_piston_events();
            if let Some(error) = &self.failure { return Err(error.clone()); }
            self.begin_phase();
            self.tick_motions();
            if let Some(error) = &self.failure { return Err(error.clone()); }
        }
        self.tick_operations = 0;
        Ok(())
    }
}
