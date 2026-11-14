use super::super::{
    ScratchError,
    program::{ArrangementSource, PISTON_HEAD_NOTIFICATION, notification_component},
};
use super::Runtime;
use mchprs_blocks::blocks::Block;

impl Runtime {
    /// Compute conditional input attenuation for the current geometry.
    ///
    /// Inputs: An optional changed token and this runtime's routing graph/search scratch space.
    ///
    /// Search either all readers or the readers affected by that token, apply the operation budget, and
    /// return their updated link attenuation values.
    fn routed_values(&mut self, changed: Option<usize>) -> Result<Vec<(usize, u8)>, ScratchError> {
        let program = std::sync::Arc::clone(&self.program);
        let Some(graph) = &program.routing else {
            return Ok(Vec::new());
        };
        let reader_count = changed.map_or_else(|| graph.reader_count(), |token| graph.readers_for_token(token).len());
        #[cfg(feature = "redpilator-trace")]
        if changed.is_some() {
            self.routing_metrics[0] += 1;
            self.routing_metrics[1] += reader_count as u64;
        }
        self.route_search.remaining = changed.map(|_| 1_000_000usize.saturating_sub(self.tick_operations));
        self.route_search.spent = 0;
        let mut updates = std::mem::take(&mut self.route_search.updates);
        updates.clear();
        for offset in 0..reader_count {
            let reader = changed.map_or(offset, |token| graph.readers_for_token(token)[offset]);
            let result = graph.reader_attenuations(
                reader,
                &program.arrangement_links,
                |super::super::routing::Axis::Token(token, _)| self.token_digits[token],
                &mut self.route_search,
                &mut updates,
            );
            if changed.is_some() {
                #[cfg(feature = "redpilator-trace")]
                {
                    self.routing_metrics[2] += self.route_search.spent as u64;
                }
                self.tick_operations += self.route_search.spent;
                self.operations += self.route_search.spent;
            }
            self.route_search.spent = 0;
            result?;
        }
        Ok(updates)
    }

    pub(in crate::redpilator::sampled) fn initialize_routing(&mut self) {
        match self.routed_values(None) {
            Ok(values) => {
                for &(index, attenuation) in &values {
                    self.routing_attenuations[index] = Some(attenuation);
                }
                self.route_search.updates = values;
            }
            Err(error) => self.failure = Some(error),
        }
    }

    /// Apply connectivity changes after a token moves.
    ///
    /// Inputs: The changed token index and a list collecting switched recipients.
    ///
    /// Recompute affected routing links, remove old contributions before adding new ones, invalidate
    /// receiver update stamps and collect newly switched targets.
    fn refresh_routing(&mut self, token: usize, switched: &mut Vec<usize>) {
        if self
            .program
            .routing
            .as_ref()
            .is_none_or(|graph| graph.readers_for_token(token).is_empty())
        {
            return;
        }
        let updates = match self.routed_values(Some(token)) {
            Ok(values) => values,
            Err(error) => {
                self.failure = Some(error);
                return;
            }
        };
        for &(index, attenuation) in &updates {
            let old = self.routing_attenuations[index].unwrap_or(u8::MAX);
            if old == attenuation {
                continue;
            }
            let link = self.program.arrangement_links[index].clone();
            let strength = self.arrangement_output(link.source);
            if old != u8::MAX {
                if !self.consume_component_operation(link.target) {
                    return;
                }
                self.routing_attenuations[index] = Some(u8::MAX);
                self.set_arrangement_contribution(index, link.target, link.side, 0);
                self.operations += 1;
            }
            if attenuation != u8::MAX {
                if !self.consume_component_operation(link.target) {
                    return;
                }
                self.routing_attenuations[index] = Some(attenuation);
                self.set_arrangement_contribution(index, link.target, link.side, strength.saturating_sub(attenuation));
                self.operations += 1;
            }
            if strength > old || strength > attenuation {
                self.updated_in_phase[link.target] = 0;
            }
            if !matches!(self.blocks[link.target], Block::Piston { .. }) && !self.switched[link.target] {
                self.switched[link.target] = true;
                switched.push(link.target);
            }
        }
        self.route_search.updates = updates;
    }

    pub(in crate::redpilator::sampled) fn arrangement_output(&self, source: ArrangementSource) -> u8 {
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

    /// Move a payload and update the circuit connections it changes.
    ///
    /// Inputs: The token index, its new optional world position and current runtime state.
    ///
    /// Update its position and encoded digit, apply the recorded departure/arrival steps, refresh routing
    /// and notify affected wires and receivers.
    pub(in crate::redpilator::sampled) fn set_token_pos(&mut self, token: usize, position: Option<mchprs_blocks::BlockPos>) {
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
        self.token_digits[token] = if position.is_some() { cell + 1 } else { 0 };
        self.mark_dirty(self.program.token_cells[token][cell]);
        let mut switched = std::mem::take(&mut self.movement_switched);
        let mut changed_wires = std::mem::take(&mut self.movement_changed_wires);
        self.run_move_steps(token, cell, position.is_some(), &mut switched, &mut changed_wires);
        for &node in switched.iter() {
            self.switched[node] = false;
        }
        for &wire in &changed_wires {
            self.changed_wires[wire] = false;
        }
        switched.clear();
        changed_wires.clear();
        self.movement_switched = switched;
        self.movement_changed_wires = changed_wires;
    }

    /// Replay the sampled callbacks for a token leaving or entering a cell.
    ///
    /// Inputs: The token and cell indexes, arrival flag, and scratch lists for switched components and changed wires.
    ///
    /// Adjust arrangement groups and contributions, replay each callback with watcher suppression and
    /// operation accounting, and collect follow-up updates in order.
    fn run_move_steps(&mut self, token: usize, cell: usize, arriving: bool, switched: &mut Vec<usize>, changed_wires: &mut Vec<usize>) {
        self.refresh_routing(token, switched);
        if self.failure.is_some() {
            return;
        }
        for &component in &self.program.token_piston_components[token] {
            self.updated_in_phase[component] = 0;
        }
        let transitions = self
            .program
            .movement_cells
            .get(token)
            .and_then(|cells| cells.get(cell))
            .cloned()
            .unwrap_or(0..0);
        let fixed = self
            .program
            .movement_fixed_ranges
            .get(token)
            .and_then(|cells| cells.get(cell))
            .and_then(Option::as_ref)
            .map(|edges| edges[usize::from(arriving)].clone())
            .filter(|flips| {
                self.tick_operations.checked_add(flips.len()).is_some_and(|total| total <= 1_000_000)
                    && transitions.clone().all(|index| {
                        let record = &self.program.movement_transitions[index];
                        let before = self.group_arrangements[record.group];
                        let after = if arriving {
                            before.checked_add(record.delta)
                        } else {
                            before.checked_sub(record.delta)
                        };
                        before < record.states && after.is_some_and(|state| state < record.states)
                    })
            });
        if let Some(flips) = fixed {
            // Flips do not call receivers or change source outputs; distinct groups can advance together.
            for index in transitions {
                let record = &self.program.movement_transitions[index];
                let state = &mut self.group_arrangements[record.group];
                *state = if arriving { *state + record.delta } else { *state - record.delta };
            }
            self.tick_operations += flips.len();
            self.replay_movement_flips::<false>(flips, switched);
        } else {
            for index in transitions {
                let record = &self.program.movement_transitions[index];
                let before = self.group_arrangements[record.group];
                let after = if arriving {
                    before.checked_add(record.delta)
                } else {
                    before.checked_sub(record.delta)
                };
                let Some(after) = after.filter(|&state| before < record.states && state < record.states) else {
                    self.failure = Some(ScratchError {
                        position: self.program.tokens[token].position,
                        reason: "invalid movement transition",
                    });
                    return;
                };
                let state = if record.fixed_state != usize::MAX {
                    record.fixed_state
                } else if arriving {
                    after
                } else {
                    before
                };
                let base = if arriving { record.arrive } else { record.depart };
                let flips = self.program.movement_ranges[base + state].clone();
                self.group_arrangements[record.group] = after;
                if !self.replay_movement_flips::<true>(flips, switched) {
                    return;
                }
            }
        }
        let steps = self
            .program
            .move_steps
            .get(token)
            .and_then(|cells| cells.get(cell))
            .map_or(0..0, |edges| edges[usize::from(arriving)].clone());
        // Landing delivers shape changes before ordinary neighbor power callbacks.
        self.notify_shape(self.program.token_cells[token][cell]);
        // Movement callbacks read the new wire level even when that wire is not a piston watcher.
        for &node in switched.iter() {
            if matches!(self.blocks[node], Block::RedstoneWire { .. }) {
                let strength = self.input_strength(node);
                self.refresh_wire_power(node, strength);
            }
        }
        for index in steps {
            let step = &self.program.movement_steps[index];
            let notification = step.node;
            let node = notification_component(notification);
            let via_head = notification & PISTON_HEAD_NOTIFICATION != 0;
            let wire = step.wire;
            let watcher = step.watcher;
            let deliver = (!via_head && self.switched[node])
                || watcher
                || self.program.movement_wires[step.wires.clone()]
                    .iter()
                    .any(|&wire| self.changed_wires[wire]);
            if !self.consume_component_operation(node) {
                return;
            }
            if wire {
                if self.switched[node] {
                    self.switched[node] = false;
                    if self.mark_changed_wire(node) {
                        self.changed_wires[node] = true;
                        changed_wires.push(node);
                    }
                }
            } else if deliver {
                if !via_head {
                    self.switched[node] = false;
                }
                if matches!(self.blocks[node], Block::Observer { .. }) && !watcher {
                    self.update_wire_watcher(notification);
                } else {
                    self.update_once(notification);
                }
            }
            self.operations += 1;
        }
        for &node in switched.iter() {
            if !self.switched[node] {
                continue;
            }
            if !self.consume_component_operation(node) {
                return;
            }
            if self.program.watched_by_component[node].is_some() {
                if self.mark_changed_wire(node) {
                    self.changed_wires[node] = true;
                    changed_wires.push(node);
                }
            } else if matches!(self.blocks[node], Block::RedstoneWire { .. }) {
                let strength = self.input_strength(node);
                self.refresh_wire_power(node, strength);
            } else {
                self.update_once(node);
            }
        }
        for &wire in changed_wires.iter() {
            if let Some(index) = self.program.watched_by_component[wire].map(|index| index.get() - 1) {
                for offset in 0..self.program.watched_wires[index].watchers.len() {
                    let watcher = self.program.watched_wires[index].watchers[offset];
                    if !self.consume_component_operation(notification_component(watcher)) {
                        return;
                    }
                    self.update_wire_watcher(watcher);
                }
            }
        }
    }

    #[inline]
    fn replay_movement_flips<const GUARDED: bool>(&mut self, flips: std::ops::Range<usize>, switched: &mut Vec<usize>) -> bool {
        for index in flips {
            let flip = self.program.movement_flips[index];
            let target = flip.target as usize;
            if GUARDED && !self.consume_component_operation(target) {
                return false;
            }
            let contribution = match flip.source() {
                ArrangementSource::Component(node) => self.output(node).saturating_sub(flip.attenuation),
                ArrangementSource::Token(_) => flip.token_contribution,
            };
            self.set_arrangement_contribution(flip.link as usize, target, flip.side, if flip.entering { contribution } else { 0 });
            if contribution != 0 {
                self.updated_in_phase[target] = 0;
            }
            // WithMoveStep reserves -2 for pistons: flips do not deliver them.
            if !matches!(self.blocks[target], Block::Piston { .. }) && !self.switched[target] {
                self.switched[target] = true;
                switched.push(target);
            }
            self.operations += 1;
        }
        true
    }

    fn mark_changed_wire(&mut self, wire: usize) -> bool {
        let Some(index) = self.program.watched_by_component[wire].map(|index| index.get() - 1) else {
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

    /// Change a wire's stored strength and dependent signals.
    ///
    /// Inputs: The wire component ID, new level and this runtime's electrical indexes.
    ///
    /// Update its block state, refresh outgoing fixed and conditional contributions, invalidate affected
    /// receivers and propagate any reactive wire dependencies.
    pub(in crate::redpilator::sampled) fn refresh_wire_power(&mut self, wire: usize, strength: u8) {
        let Block::RedstoneWire { wire: mut state } = self.blocks[wire] else {
            unreachable!()
        };
        let previous = state.power;
        if previous == strength {
            return;
        }
        state.power = strength;
        self.set_component(wire, Block::RedstoneWire { wire: state });
        for offset in 0..self.program.outgoing[wire].len() {
            let index = self.program.outgoing[wire][offset];
            let link = self.program.links[index];
            let contribution = strength.saturating_sub(link.attenuation);
            let old = self.contributions[index];
            self.set_contribution(index, contribution);
            if (old > 0) != (contribution > 0)
                || (old != contribution
                    && (matches!(self.blocks[link.target], Block::RedstoneComparator { .. })
                        || self.program.watched_by_component[link.target].is_some()))
            {
                self.updated_in_phase[link.target] = 0;
            }
            #[cfg(any(test, feature = "redpilator-trace"))]
            if runtime_trace_enabled!(self) && old != contribution {
                runtime_trace!(
                    self,
                    "link {:?} -> {:?} strength={old}->{contribution}",
                    self.program.positions[wire],
                    self.program.positions[link.target]
                );
            }
        }
        for offset in 0..self.program.outgoing_arrangements[wire].len() {
            let index = self.program.outgoing_arrangements[wire][offset];
            let (target, side, contribution) = self.arrangement_contribution(index);
            let old = self.arrangement_contributions[index];
            self.set_arrangement_contribution(index, target, side, contribution);
            if (old > 0) != (contribution > 0)
                || (old != contribution
                    && (matches!(self.blocks[target], Block::RedstoneComparator { .. }) || self.program.watched_by_component[target].is_some()))
            {
                self.updated_in_phase[target] = 0;
            }
        }
    }

    /// Describe a receiver's conditional sources for a proposed token position.
    ///
    /// Inputs: The changed token ID, receiver component ID, hypothetical position and current runtime.
    ///
    /// Substitute the proposed placement, query routing when available or select the matching arrangement
    /// rows, add unshadowed token sources and return the sorted source/attenuation pairs.
    // shortcut: receiver signatures omit geometry-dependent fixed links, extend arrangement groups to all readers.
    pub(super) fn token_dependency_signature(
        &self,
        changed: usize,
        target: usize,
        position: Option<mchprs_blocks::BlockPos>,
    ) -> Result<Vec<(ArrangementSource, u8)>, ScratchError> {
        let token_position = |token: usize| if token == changed { position } else { self.token_positions[token] };
        if let Some(graph) = &self.program.routing
            && let Some(reader) = graph.reader_for_component(target)
        {
            return graph
                .reader_sources(
                    reader,
                    |super::super::routing::Axis::Token(token, _)| {
                        token_position(token)
                            .and_then(|pos| self.program.tokens[token].cells.iter().position(|&cell| cell == pos))
                            .map_or(0, |cell| cell + 1)
                    },
                    &mut super::super::routing::Search::default(),
                )
                .map(|sources| {
                    sources
                        .into_iter()
                        .filter_map(|((source, side), attenuation)| (!side).then_some((source, attenuation)))
                        .collect()
                });
        }
        let mut result = Vec::new();
        let arrangements: Vec<_> = self.program.input_arrangements.iter().filter(|group| group.target == target).collect();
        for group in &arrangements {
            let mut index = 0;
            let mut stride = 1;
            for (axis, &token) in group.tokens.iter().enumerate() {
                let record = &self.program.tokens[token];
                let digit = token_position(token)
                    .and_then(|pos| record.cells.iter().position(|&cell| cell == pos))
                    .map_or(0, |cell| cell + 1);
                index += group.digit(axis, digit) * stride;
                stride *= group.radix(axis, record.cells.len() + 1);
            }
            result.extend(group.inputs[index].iter().copied());
        }
        for (token, record) in self.program.tokens.iter().enumerate() {
            if arrangements
                .iter()
                .any(|group| group.conditional_sources.contains(&ArrangementSource::Token(token)))
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
        Ok(result)
    }
}
