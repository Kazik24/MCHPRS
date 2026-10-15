//! Exact state normalization: scheduler rotation, time origin and revision origins are unobservable.
use super::*;

pub(crate) const MACHINE_EPOCH: u64 = 64;

fn component_bits(state: &ComponentState) -> Option<u16> {
    Some(match state {
        ComponentState::Electrical(s) => {
            if s.command_entity.is_some() || s.note_instrument.is_some() || s.output_strength > 15 {
                return None;
            }
            u16::from(s.output_strength) | ((s.powered as u16) << 4) | ((s.repeater_locked as u16) << 5) | ((s.pending_tick as u16) << 6)
        }
        ComponentState::Wire(w) => {
            if w.power > 15 {
                return None;
            }
            u16::from(w.power) | ((w.north as u16) << 4) | ((w.south as u16) << 6) | ((w.east as u16) << 8) | ((w.west as u16) << 10)
        }
        ComponentState::Specialized => 0,
    })
}

fn write_component(state: &mut ComponentState, value: u16) -> bool {
    match state {
        ComponentState::Electrical(s) if value < 128 => {
            s.output_strength = (value & 15) as u8;
            s.powered = value & 16 != 0;
            s.repeater_locked = value & 32 != 0;
            s.pending_tick = value & 64 != 0;
        }
        ComponentState::Wire(w) if value < 4096 => {
            let side = |shift: u32| match (value >> shift) & 3u16 {
                0 => Some(RedstoneWireSide::Up),
                1 => Some(RedstoneWireSide::Side),
                2 => Some(RedstoneWireSide::None),
                _ => None,
            };
            let (Some(north), Some(south), Some(east), Some(west)) = (side(4), side(6), side(8), side(10)) else {
                return false;
            };
            *w = RedstoneWire {
                power: (value & 15) as u8,
                north,
                south,
                east,
                west,
            };
        }
        ComponentState::Specialized if value == 0 => {}
        _ => return false,
    }
    true
}

fn piston_bits(state: &PistonState) -> u8 {
    u8::from(state.extended) | (u8::from(state.head_present) << 1) | ((state.availability as u8) << 2)
}

fn write_piston(state: &mut PistonState, value: u8) -> bool {
    state.availability = match value >> 2 {
        0 => PistonAvailability::Ready,
        1 => PistonAvailability::HeadPending,
        2 => PistonAvailability::PublishingHeadRemoval,
        3 => PistonAvailability::BasePendingHeadPresent,
        4 => PistonAvailability::BasePending,
        _ => return false,
    };
    state.extended = value & 1 != 0;
    state.head_present = value & 2 != 0;
    true
}

impl GraphBackend {
    /// Reuse the lookup buffer; equality includes all ordered future obligations.
    pub(crate) fn capture_machine_state(&self, out: &mut MachineState) -> bool {
        if self.clock.phase != GraphPhase::BetweenTicks || self.clock.scheduled_advanced || self.failure.is_some() {
            return false;
        }
        out.components.clear();
        for component in &self.state.components {
            let Some(bits) = component_bits(component) else {
                return false;
            };
            out.components.push(bits);
        }
        out.pistons.clear();
        out.pistons.extend(self.state.pistons.iter().map(piston_bits));
        out.resources.clear();
        out.resources.extend(self.state.resources.iter().map(|s| (s.at, s.enabled)));
        out.ticks.clear();
        for (tick, delay, priority) in self.scheduler.iter() {
            let Some(node) = tick.node else {
                return false;
            };
            let registry = self.registry_type(node);
            if tick.block_type != Some(registry)
                || tick.registry_type != registry
                || tick.current_registry_type != registry
                || tick.position != self.position(node)
            {
                return false;
            }
            out.ticks.push(MachineTick {
                node,
                delay: delay as u32,
                priority,
            });
        }
        // Generated work is unique; imported duplicates or inconsistent membership stay general.
        if self.pending.len() != out.ticks.len()
            || self.pending.values().any(|&count| count != 1)
            || out.ticks.iter().any(|tick| !self.is_pending(tick.node))
        {
            return false;
        }
        out.requests.clear();
        out.requests.extend(self.piston_requests.iter().map(|r| (r.piston, r.action)));
        out.deferred.clear();
        out.deferred.extend(self.deferred_pistons.iter().map(|event| MachineDeferred {
            piston: event.piston,
            revision: event.revision.wrapping_sub(self.state.pistons[event.piston.index()].revision),
            resource: event.resource.map(|r| {
                (
                    r.resource,
                    r.revision.wrapping_sub(self.state.resources[r.resource.index()].revision),
                    r.contact,
                )
            }),
            delay: event.due_tick.wrapping_sub(self.clock.logical_tick),
            extending: event.extending,
        }));
        out.ticks.len() <= 128 && out.requests.len() <= 64 && out.deferred.len() <= 128
    }

    fn restore_machine_future(&mut self, state: &MachineState) {
        self.scheduler.clear();
        self.pending.clear();
        for entry in &state.ticks {
            let registry_type = self.registry_type(entry.node);
            let tick = ScheduledNodeTick {
                node: Some(entry.node),
                position: self.position(entry.node),
                block_type: Some(registry_type),
                registry_type,
                current_registry_type: registry_type,
            };
            self.scheduler.schedule_half_tick(tick, entry.delay as usize, entry.priority);
            *self.pending.entry(tick.into()).or_default() += 1;
        }
        self.piston_requests.clear();
        self.piston_requests
            .extend(state.requests.iter().map(|&(piston, action)| PistonRequest { piston, action }));
        self.deferred_pistons.clear();
        self.deferred_pistons.extend(state.deferred.iter().map(|event| DeferredPistonEvent {
            piston: event.piston,
            revision: self.state.pistons[event.piston.index()].revision.wrapping_add(event.revision),
            resource: event.resource.map(|(resource, revision, contact)| pistons::DeferredResourceEvent {
                resource,
                revision: self.state.resources[resource.index()].revision.wrapping_add(revision),
                contact,
            }),
            due_tick: self.clock.logical_tick.wrapping_add(event.delay),
            extending: event.extending,
        }));
    }

    /// Only compilation/validation uses this: runtime transitions retain live revision origins.
    pub(crate) fn restore_machine_state(&mut self, state: &MachineState) -> Result<(), &'static str> {
        if state.components.len() != self.state.components.len()
            || state.pistons.len() != self.state.pistons.len()
            || state.resources.len() != self.state.resources.len()
            || state.ticks.len() > 128
            || state.requests.len() > 64
            || state.deferred.len() > 128
        {
            return Err("invalid machine state dimensions");
        }
        for (component, &value) in self.state.components.iter_mut().zip(&state.components) {
            if !write_component(component, value) {
                return Err("invalid component value");
            }
        }
        for (piston, &value) in self.state.pistons.iter_mut().zip(&state.pistons) {
            if !write_piston(piston, value) {
                return Err("invalid piston pose");
            }
            piston.revision = 0;
        }
        for (index, (resource, &(at, enabled))) in self.state.resources.iter_mut().zip(&state.resources).enumerate() {
            if !self.program.resources()[index].contacts.contains(&at) {
                return Err("invalid payload contact");
            }
            resource.at = at;
            resource.enabled = enabled;
            resource.revision = 0;
        }
        for tick in &state.ticks {
            if tick.delay as usize >= SCHEDULER_BUCKETS
                || (tick.delay == 0 && tick.priority != TickPriority::NanoTick)
                || !self.program.nodes().get(tick.node.index()).is_some_and(|n| {
                    matches!(
                        n.kind,
                        CircuitNodeKind::BuiltinReset { .. } | CircuitNodeKind::Electrical(NodeType::Repeater { .. })
                    )
                })
            {
                return Err("invalid machine tick");
            }
        }
        if state.requests.iter().any(|r| r.0.index() >= self.state.pistons.len()) {
            return Err("invalid machine request");
        }
        for event in &state.deferred {
            if event.piston.index() >= self.state.pistons.len() || event.delay >= SCHEDULER_BUCKETS as u64 {
                return Err("invalid deferred action");
            }
            if let Some((resource, _, contact)) = event.resource {
                if !self
                    .program
                    .resources()
                    .get(resource.index())
                    .is_some_and(|r| r.contacts.contains(&contact))
                {
                    return Err("invalid deferred payload");
                }
            }
        }
        self.clock = Clock {
            logical_tick: MACHINE_EPOCH,
            phase: GraphPhase::BetweenTicks,
            scheduled_advanced: false,
        };
        self.restore_machine_future(state);
        self.dirty.fill(false);
        self.counts = GraphExecutionCounts::default();
        self.effects.clear();
        self.failure = None;
        #[cfg(test)]
        self.set_test_recording(false);
        Ok(())
    }

    /// The exact executor is the compile-time evaluator, never a second runtime simulator.
    pub(crate) fn evaluate_machine_step(
        &mut self,
        before: &MachineState,
        controls: &[NodeId],
        action: usize,
    ) -> Result<(MachineState, MachineTransition), &'static str> {
        self.restore_machine_state(before)?;
        if action == 0 {
            self.tick().map_err(|_| "machine tick failed")?;
        } else {
            let control = controls.get(action - 1).ok_or("invalid machine action")?;
            self.use_block(self.position(*control)).map_err(|_| "machine input failed")?;
        }
        let mut after = MachineState::default();
        if !self.capture_machine_state(&mut after) {
            return Err("unrepresentable machine state");
        }
        // Presentation is append-only until flush; it cannot affect this closed family's reads.
        let effects = self
            .effects
            .iter()
            .map(|effect| {
                let PresentationEffect::BlockAction {
                    position,
                    action: BlockAction::Piston { action, piston },
                } = effect
                else {
                    return Err("unsupported machine presentation");
                };
                let actor = self
                    .program
                    .pistons()
                    .iter()
                    .position(|spec| self.position(spec.node) == *position)
                    .ok_or("invalid machine presentation owner")?;
                Ok((PistonId::new(actor as u32), *action, piston.extended))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let edge = MachineTransition {
            next: 0,
            components: before
                .components
                .iter()
                .zip(&after.components)
                .enumerate()
                .filter_map(|(i, (a, b))| (a != b).then_some((NodeId::new(i as u32), *b)))
                .collect(),
            pistons: self
                .state
                .pistons
                .iter()
                .enumerate()
                .filter_map(|(i, s)| {
                    (before.pistons[i] != after.pistons[i] || s.revision != 0).then_some((PistonId::new(i as u32), after.pistons[i], s.revision))
                })
                .collect(),
            resources: self
                .state
                .resources
                .iter()
                .enumerate()
                .filter_map(|(i, s)| {
                    (before.resources[i] != after.resources[i] || s.revision != 0).then_some((ResourceId::new(i as u32), s.at, s.enabled, s.revision))
                })
                .collect(),
            dirty: self
                .dirty
                .iter()
                .enumerate()
                .filter_map(|(i, &dirty)| dirty.then_some(NodeId::new(i as u32)))
                .collect(),
            effects,
            scheduled: self.counts.scheduled_ticks,
            accepted: self.counts.piston_actions,
            published: self.counts.deferred_publications,
        };
        Ok((after, edge))
    }

    pub(super) fn try_machine_step(&mut self, action: usize) -> bool {
        if self.clock.logical_tick > u64::MAX - SCHEDULER_BUCKETS as u64 {
            return false;
        }
        let Some(machine) = self.program.instant_machine().cloned() else {
            return false;
        };
        let mut key = std::mem::take(&mut self.machine_key);
        let source = if self.capture_machine_state(&mut key) {
            machine.index.get(&key).copied()
        } else {
            None
        };
        self.machine_key = key;
        let Some(source) = source else {
            return false;
        };
        let Some(edge) = machine.transitions[source as usize].get(action).and_then(Option::as_ref) else {
            return false;
        };
        let target = &machine.states[edge.next as usize];
        graph_trace!(
            self.clock.logical_tick,
            self.clock.phase,
            "instant transition source={source} action={action} target={}",
            edge.next
        );
        if action == 0 {
            self.clock.logical_tick += 1;
            self.counts.ticks += 1;
        }
        for &(node, value) in &edge.components {
            let valid = write_component(&mut self.state.components[node.index()], value);
            debug_assert!(valid);
        }
        for &(actor, value, revision) in &edge.pistons {
            let state = &mut self.state.pistons[actor.index()];
            let valid = write_piston(state, value);
            debug_assert!(valid);
            state.revision = state.revision.wrapping_add(revision);
        }
        for &(resource, at, enabled, revision) in &edge.resources {
            let state = &mut self.state.resources[resource.index()];
            state.at = at;
            state.enabled = enabled;
            state.revision = state.revision.wrapping_add(revision);
        }
        for node in &edge.dirty {
            self.dirty[node.index()] = true;
        }
        self.restore_machine_future(target);
        for &(piston, action, extended) in &edge.effects {
            self.accepted(PistonRequest { piston, action }, extended);
        }
        self.counts.scheduled_ticks += edge.scheduled;
        debug_assert_eq!(edge.accepted, edge.effects.len() as u64);
        self.counts.deferred_publications += edge.published;
        self.counts.instant_transitions += 1;
        true
    }
}
