use super::super::TickScheduler;
use super::{Operation, Program, ScheduledBlockTick, ScratchError};
use super::program::ArrangementSource;
use mchprs_blocks::blocks::Block;
use mchprs_world::TickPriority;

pub(super) struct Runtime<'a> {
    pub(super) program: &'a Program,
    pub(super) piston_extended: Vec<bool>,
    pub(super) piston_head_present: Vec<bool>,
    pub(super) piston_head_moving: Vec<bool>,
    pub(super) piston_base_moving: Vec<bool>,
    pub(super) token_positions: Vec<Option<mchprs_blocks::BlockPos>>,
    pub(super) piston_events: std::collections::VecDeque<(usize, bool)>,
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
        if self.program.tokens.iter().any(|token| token.cells.contains(&position))
            && self.program.tokens.iter().any(|token| token.position == Some(position)) {
            return Some(Block::Air);
        }
        Some(self.program.positions.iter().position(|&at| at == position).map(|index| self.blocks[index]).unwrap_or(Block::Air))
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
            self.piston_events.push_back((index, true));
        }
    }

    fn update_piston(&mut self, component: usize) {
        let Some(index) = self.program.pistons.iter().position(|p| p.base == self.program.positions[component]) else { return; };
        if self.piston_base_moving[index] { return; }
        let powered = self.input_strength(component) > 0;
        if self.trace_enabled {
            let fixed: Vec<_> = self.program.incoming[component].iter().map(|&link| (self.program.positions[self.program.links[link].source], self.contributions[link])).collect();
            let token_inputs = self.token_dependency_signature(usize::MAX, component, None);
            self.trace(format!("piston sample {:?} powered={powered} extended={} fixed={fixed:?} token_inputs={token_inputs:?}", self.program.pistons[index].base, self.piston_extended[index]));
        }
        if powered == self.piston_extended[index] { return; }
        let bit = if powered { 1 } else { 2 };
        if self.queued_piston_events[index] & bit == 0 {
            self.queued_piston_events[index] |= bit;
            self.piston_events.push_back((index, powered));
            if self.trace_enabled { self.trace(format!("piston request {:?} extending={powered}", self.program.pistons[index].base)); }
        }
    }

    fn run_piston_events(&mut self) {
        while let Some((index, extending)) = self.piston_events.pop_front() {
            let piston = &self.program.pistons[index];
            if !self.consume_operation(piston.base) { return; }
            self.queued_piston_events[index] &= !(if extending { 1 } else { 2 });
            let Some(component) = self.program.positions.iter().position(|&pos| pos == piston.base) else {
                self.failure = Some(ScratchError { position: Some(piston.base), reason: "piston component missing" });
                return;
            };
            if self.piston_base_moving[index] || (self.input_strength(component) > 0) != extending {
                if self.trace_enabled { self.trace(format!("piston rejected {:?} extending={extending} reason=power_or_base_motion", piston.base)); }
                continue;
            }
            if !extending {
                if !self.piston_extended[index] { continue; }
                let early = self.piston_head_moving[index];
                if early {
                    if let Some(slot) = self.motions.iter().position(|motion| motion.0 == index && motion.4) {
                        let (_, token, destination, _, _) = self.motions[slot];
                        self.motions[slot].0 = usize::MAX;
                        self.motions[slot].1 = None;
                        self.piston_head_moving[index] = false;
                        if let Some(token) = token {
                            self.token_positions[token] = Some(destination);
                            self.refresh_token_wires(token);
                            self.run_token_updates(token, destination, true);
                            self.notify_shape(destination);
                        }
                    }
                }
                self.piston_base_moving[index] = true;
                self.notify_shape(piston.base);
                let token = if piston.sticky && !early { self.token_positions.iter().position(|&position| position == Some(piston.far)) } else { None };
                if let Some(token) = token {
                    self.token_positions[token] = None;
                    self.token_movements += 1;
                    if self.trace_enabled { self.trace(format!("token {token} departs {:?} for {:?}", piston.far, piston.home)); }
                    self.notify_shape(piston.far);
                    self.refresh_token_wires(token);
                    self.run_token_updates(token, piston.far, false);
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
                self.token_positions[token] = None;
                self.token_movements += 1;
                if self.trace_enabled { self.trace(format!("token {token} departs {:?} for {:?}", piston.home, piston.far)); }
                self.notify_shape(piston.home);
                self.refresh_token_wires(token);
                self.run_token_updates(token, piston.home, false);
            }
            self.piston_extended[index] = true;
            if let Block::Piston { mut piston } = self.blocks[component] {
                piston.extended = true;
                self.blocks[component] = Block::Piston { piston };
            }
            self.notify_shape(piston.base);
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
                if let Some(token) = token { self.token_positions[token] = Some(destination); self.refresh_token_wires(token); self.run_token_updates(token, destination, true); }
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
                if let Some(token) = token { self.token_positions[token] = Some(destination); self.refresh_token_wires(token); self.run_token_updates(token, destination, true); }
            }
            self.motion_completions += 1;
            if self.trace_enabled { self.trace(format!("motion complete {:?} token={token:?} destination={destination:?} extending={extending}", self.program.pistons[piston].base)); }
            self.notify_shape(destination);
            self.notify_shape(self.program.pistons[piston].home);
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
        let group = self.program.input_arrangements.iter().find(|group| group.target == target);
        let fixed = self.program.incoming[target].iter().filter_map(|&id| {
            let source = ArrangementSource::Component(self.program.links[id].source);
            (!group.is_some_and(|group| group.conditional_sources.contains(&source))).then_some(self.contributions[id])
        }).max().unwrap_or(0);
        let conditional = group.map_or(0, |group| {
            let mut index = 0;
            let mut stride = 1;
            for &token in &group.tokens {
                let record = &self.program.tokens[token];
                let digit = self.token_positions[token].and_then(|pos| record.cells.iter().position(|&cell| cell == pos)).map_or(0, |cell| cell + 1);
                index += digit * stride;
                stride *= record.cells.len() + 1;
            }
            group.inputs[index].iter().map(|&(source, attenuation)| {
                let strength: u8 = match source {
                    ArrangementSource::Component(component) => self.output(component),
                    ArrangementSource::Token(token) => if self.token_positions[token].is_some() { 15 } else { 0 },
                };
                strength.saturating_sub(attenuation)
            }).max().unwrap_or(0)
        });
        self.program.tokens.iter().enumerate().filter_map(|(index, token)| {
            if group.is_some_and(|group| group.conditional_sources.contains(&ArrangementSource::Token(index))) { return None; }
            let position = self.token_positions[index]?;
            let cell = token.cells.iter().position(|&candidate| candidate == position)?;
            self.program.token_sources.get(index)?.get(cell)?.iter()
                .filter_map(|&(receiver, attenuation)| (receiver == target).then_some(15u8.saturating_sub(attenuation))).max()
        }).fold(fixed.max(conditional), u8::max)
    }

    fn refresh_token_wires(&mut self, token: usize) {
        let mut wires = Vec::new();
        if let Some(arrangements) = self.program.token_sources.get(token) {
            for &(target, _) in arrangements.iter().flatten() {
                if matches!(self.blocks[target], Block::RedstoneWire { .. }) && !wires.contains(&target) { wires.push(target); }
            }
        }
        for group in &self.program.input_arrangements {
            if group.tokens.contains(&token) && !wires.contains(&group.target) { wires.push(group.target); }
        }
        for wire in wires {
            if !self.consume_operation(self.program.positions[wire]) { return; }
            if let Block::RedstoneWire { wire: mut state } = self.blocks[wire] {
                state.power = self.input_strength(wire);
                self.blocks[wire] = Block::RedstoneWire { wire: state };
            }
            if let Some(record) = self.program.watched_wires.iter().position(|record| record.wire == wire) {
                self.check_watched_wire(record);
            }
        }
    }

    fn run_token_updates(&mut self, token: usize, position: mchprs_blocks::BlockPos, arrival: bool) {
        let Some(cell) = self.program.tokens[token].cells.iter().position(|&candidate| candidate == position) else { return; };
        let Some(updates) = self.program.token_updates.get(token).and_then(|cells| cells.get(cell)) else { return; };
        let watchers = self.program.token_watchers.get(token).and_then(|cells| cells.get(cell));
        for &component in &updates[usize::from(arrival)] {
            if !self.consume_operation(self.program.positions[component]) { return; }
            let watched = watchers.is_some_and(|watchers| watchers.contains(&component));
            let from = if arrival { None } else { Some(position) };
            let to = if arrival { Some(position) } else { None };
            let before = self.token_dependency_signature(token, component, from);
            let after = self.token_dependency_signature(token, component, to);
            let switched = before != after;
            if switched && before.iter().chain(&after).any(|&(source, attenuation)| {
                let strength: u8 = match source {
                    ArrangementSource::Component(index) => self.output(index),
                    ArrangementSource::Token(index) => if index == token || self.token_positions[index].is_some() { 15 } else { 0 },
                };
                strength > attenuation && (before.contains(&(source, attenuation)) != after.contains(&(source, attenuation)))
            }) {
                self.updated_in_phase[component] = None;
            }
            match self.blocks[component] {
                Block::Piston { .. } | Block::RedstoneRepeater { .. } | Block::RedstoneLamp { .. } if watched || switched => self.update_once(component),
                Block::RedstoneWire { mut wire } => {
                    wire.power = self.input_strength(component);
                    self.blocks[component] = Block::RedstoneWire { wire };
                }
                _ => {}
            }
            self.operations += 1;
        }
        if let Some(watchers) = self.program.token_watchers.get(token).and_then(|cells| cells.get(cell)) {
            for &component in watchers {
                if !self.consume_operation(self.program.positions[component]) { return; }
                self.update_once(component);
            }
        }
    }

    // shortcut: receiver signatures omit geometry-dependent fixed links, extend arrangement groups to all readers.
    fn token_dependency_signature(&self, changed: usize, target: usize, position: Option<mchprs_blocks::BlockPos>) -> Vec<(ArrangementSource, u8)> {
        let token_position = |token: usize| if token == changed { position } else { self.token_positions[token] };
        let group = self.program.input_arrangements.iter().find(|group| group.target == target);
        let mut result = Vec::new();
        if let Some(group) = group {
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
            if group.is_some_and(|group| group.conditional_sources.contains(&ArrangementSource::Token(token))) { continue; }
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
                Operation::Contribution(link_index) => {
                    let link = &self.program.links[link_index];
                    let old_contribution = self.contributions[link_index];
                    let contribution = self.output(link.source).saturating_sub(link.attenuation);
                    self.contributions[link_index] = contribution;
                    if self.trace_enabled && old_contribution != contribution { self.trace(format!("link {:?} -> {:?} strength={old_contribution}->{contribution}", self.program.positions[link.source], self.program.positions[link.target])); }
                    if (old_contribution > 0) != (contribution > 0) {
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
            }
            self.operations += 1;
        }
    }

    pub(super) fn update_repeater(&mut self, component: usize) {
        let Block::RedstoneRepeater { mut repeater } = self.blocks[component] else {
            unreachable!()
        };
        if let Some(side_sources) = self.program.repeaters.iter().find(|record| record.component == component).and_then(|record| record.side_sources.as_ref()) {
            repeater.locked = side_sources.iter().any(|&source| self.output(source) > 0);
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
        self.failure.clone().map_or(Ok(()), Err)
    }

    pub(super) fn drain_ticks(&mut self) {
        while let Some(tick) = self.pending.this_tick().pop_first() {
            if !self.consume_operation(tick.pos) { return; }
            if let Some(component) = self.program.positions.iter().position(|&pos| pos == tick.pos) {
                match self.blocks[component] {
                    Block::RedstoneLamp { .. } => self.dispatch_receiver_tick(component),
                    Block::RedstoneRepeater { .. } => {
                        self.scheduled_repeater_ticks += 1;
                        self.tick_repeater(component);
                    }
                    Block::Observer { mut observer } => {
                        observer.powered = !observer.powered;
                        if self.trace_enabled { self.trace(format!("observer tick {:?} powered={}", self.program.positions[component], observer.powered)); }
                        self.blocks[component] = Block::Observer { observer };
                        if observer.powered {
                            self.pending.schedule_tick(tick, 1, TickPriority::Normal);
                        }
                        self.execute_output_program(component, observer.powered);
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
