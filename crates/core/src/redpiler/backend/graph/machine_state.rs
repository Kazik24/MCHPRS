//! Exact state normalization: scheduler rotation, time origin and revision origins are unobservable.
use super::*;
use mchprs_world::PistonAction;

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

pub(super) fn write_component(state: &mut ComponentState, value: u16) -> bool {
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

pub(super) fn write_piston(state: &mut PistonState, value: u8) -> bool {
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
        self.capture_graph_boundary(out) && out.ticks.len() <= 128 && out.requests.len() <= 64 && out.deferred.len() <= 128
    }

    /// Capture a complete boundary without the retired finite compiler's queue-size limits.
    pub(super) fn capture_graph_boundary(&self, out: &mut MachineState) -> bool {
        if self.has_active_kernel() {
            let Some(state) = self.current_kernel_state() else {
                return false;
            };
            *out = state;
            return true;
        }
        if self.future.clock.phase != GraphPhase::BetweenTicks || self.future.clock.scheduled_advanced || self.failure.is_some() {
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
        for (tick, delay, priority) in self.future.scheduler.iter() {
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
        if self.future.pending.len() != out.ticks.len()
            || self.future.pending.values().any(|&count| count != 1)
            || out.ticks.iter().any(|tick| !self.is_pending(tick.node))
        {
            return false;
        }
        out.requests.clear();
        out.requests.extend(self.future.piston_requests.iter().map(|r| (r.piston, r.action)));
        out.deferred.clear();
        out.deferred.extend(self.future.deferred_pistons.iter().map(|event| MachineDeferred {
            piston: event.piston,
            revision: event.revision.wrapping_sub(self.state.pistons[event.piston.index()].revision),
            resource: event.resource.map(|r| {
                (
                    r.resource,
                    r.revision.wrapping_sub(self.state.resources[r.resource.index()].revision),
                    r.contact,
                )
            }),
            delay: event.due_tick.wrapping_sub(self.future.clock.logical_tick),
            extending: event.extending,
        }));
        true
    }

    pub(super) fn restore_machine_future(&mut self, state: &MachineState) {
        self.future.scheduler.clear();
        self.future.pending.clear();
        for entry in &state.ticks {
            let registry_type = self.registry_type(entry.node);
            let tick = ScheduledNodeTick {
                node: Some(entry.node),
                position: self.position(entry.node),
                block_type: Some(registry_type),
                registry_type,
                current_registry_type: registry_type,
            };
            self.future.scheduler.schedule_half_tick(tick, entry.delay as usize, entry.priority);
            *self.future.pending.entry(tick.into()).or_default() += 1;
        }
        self.future.piston_requests.clear();
        self.future
            .piston_requests
            .extend(state.requests.iter().map(|&(piston, action)| PistonRequest { piston, action }));
        self.future.deferred_pistons.clear();
        self.future
            .deferred_pistons
            .extend(state.deferred.iter().map(|event| DeferredPistonEvent {
                piston: event.piston,
                revision: self.state.pistons[event.piston.index()].revision.wrapping_add(event.revision),
                resource: event.resource.map(|(resource, revision, contact)| pistons::DeferredResourceEvent {
                    resource,
                    revision: self.state.resources[resource.index()].revision.wrapping_add(revision),
                    contact,
                }),
                due_tick: self.future.clock.logical_tick.wrapping_add(event.delay),
                extending: event.extending,
            }));
    }

    /// Only compilation/validation uses this: runtime transitions retain live revision origins.
    pub(crate) fn restore_machine_state(&mut self, state: &MachineState) -> Result<(), &'static str> {
        self.future.kernel_cursor = None;
        self.future.symbolic_cursor = None;
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
        self.future.clock = Clock {
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

    pub(crate) fn materialize_kernel_state(&mut self) -> bool {
        if self.future.symbolic_cursor.is_some() {
            return self.materialize_symbolic_kernel();
        }
        let Some(kernel) = self.program.instant_kernel().cloned() else {
            return false;
        };
        let Some(cursor) = self.future.kernel_cursor.clone() else {
            return true;
        };
        let Some(state) = self.current_kernel_state() else {
            return false;
        };
        if state.components.len() != self.state.components.len()
            || state.pistons.len() != self.state.pistons.len()
            || state.resources.len() != self.state.resources.len()
        {
            return false;
        }
        for (component, &value) in self.state.components.iter_mut().zip(&state.components) {
            if !write_component(component, value) {
                return false;
            }
        }
        let piston_revisions = self.kernel_revision_totals(&kernel, &cursor, true);
        for (index, (piston, &pose)) in self.state.pistons.iter_mut().zip(&state.pistons).enumerate() {
            if !write_piston(piston, pose) {
                return false;
            }
            piston.revision = cursor.piston_revisions[index].wrapping_add(piston_revisions[index]);
        }
        let resource_revisions = self.kernel_revision_totals(&kernel, &cursor, false);
        for (index, (resource, &(contact, enabled))) in self.state.resources.iter_mut().zip(&state.resources).enumerate() {
            if !self.program.resources()[index].contacts.contains(&contact) {
                return false;
            }
            resource.at = contact;
            resource.enabled = enabled;
            resource.revision = cursor.resource_revisions[index].wrapping_add(resource_revisions[index]);
        }
        self.restore_machine_future(&state);
        true
    }

    pub(super) fn try_enter_kernel(&mut self) {
        let Some(kernel) = self.program.instant_kernel().cloned() else {
            return;
        };
        if self.future.clock.logical_tick > u64::MAX - SCHEDULER_BUCKETS as u64
            || kernel.period == 0
            || usize::from(kernel.prefix).checked_add(usize::from(kernel.period)) != Some(kernel.phases.len())
        {
            return;
        }
        let mut mask = 0u8;
        for (bit, &node) in kernel.data_controls.iter().enumerate() {
            if !matches!(self.state.components.get(node.index()), Some(ComponentState::Electrical(_))) {
                return;
            }
            if self.node_state(node).powered {
                mask |= 1 << bit;
            }
        }
        if self.node_state(kernel.trigger).powered {
            return;
        }
        let mut actual = MachineState::default();
        if !self.capture_machine_state(&mut actual) {
            return;
        }
        let Some(expected) = evaluate_kernel_state(&kernel, &kernel.entry, mask) else {
            return;
        };
        if actual != expected {
            return;
        }
        self.future.kernel_cursor = Some(KernelCursor {
            mask,
            phase: 0,
            cycles: 0,
            piston_revisions: self.state.pistons.iter().map(|state| state.revision).collect(),
            resource_revisions: self.state.resources.iter().map(|state| state.revision).collect(),
        });
    }

    pub(super) fn try_kernel_tick(&mut self) -> bool {
        if self.future.symbolic_cursor.is_some() {
            return self.try_symbolic_kernel_tick();
        }
        let Some((mask, phase)) = self.future.kernel_cursor.as_ref().map(|cursor| (cursor.mask, cursor.phase)) else {
            return false;
        };
        if self.future.clock.phase != GraphPhase::BetweenTicks
            || self.future.clock.scheduled_advanced
            || self.future.clock.logical_tick > u64::MAX - SCHEDULER_BUCKETS as u64
        {
            self.leave_kernel_before_general_execution();
            return false;
        }
        let Some(execution) = self.program.instant_execution().cloned() else {
            panic!("active kernel cursor has no lowered execution plan");
        };
        let Some(frame) = execution.phase(mask, phase) else {
            self.leave_kernel_before_general_execution();
            return false;
        };

        graph_trace!(self.future.clock.logical_tick, self.future.clock.phase, "instant kernel phase={phase}");
        for &(node, value) in &frame.writes {
            assert!(
                write_component(&mut self.state.components[node.index()], value),
                "validated kernel component root must match its compiled owner"
            );
        }
        for &node in &frame.dirty {
            self.dirty[node.index()] = true;
        }
        for &(piston, action, extended) in &frame.effects {
            self.accepted(PistonRequest { piston, action }, extended);
        }
        self.future.clock.logical_tick += 1;
        self.counts.ticks += 1;
        self.counts.scheduled_ticks += frame.scheduled;
        self.counts.deferred_publications += frame.published;
        self.counts.instant_transitions += 1;
        let cursor = self.future.kernel_cursor.as_mut().expect("active kernel cursor remains installed");
        if phase + 1 == execution.phase_count {
            cursor.phase = execution.prefix;
            cursor.cycles = cursor.cycles.wrapping_add(1);
        } else {
            cursor.phase = phase + 1;
        }
        true
    }

    fn leave_kernel_before_general_execution(&mut self) {
        assert!(
            self.materialize_kernel_state(),
            "validated kernel state must materialize before general Graph execution"
        );
        self.future.kernel_cursor = None;
    }

    pub(super) fn current_kernel_state(&self) -> Option<MachineState> {
        if self.future.symbolic_cursor.is_some() {
            return self.current_symbolic_boundary();
        }
        let kernel = self.program.instant_kernel()?;
        let cursor = self.future.kernel_cursor.as_ref()?;
        let state = if cursor.phase == 0 && cursor.cycles == 0 {
            &kernel.entry
        } else {
            let previous = if cursor.phase == 0 { kernel.phases.len() - 1 } else { cursor.phase - 1 };
            &kernel.phases.get(previous)?.state
        };
        evaluate_kernel_state(kernel, state, cursor.mask)
    }

    #[cfg(test)]
    pub(crate) fn semantic_revisions(&self) -> (Vec<u64>, Vec<u64>) {
        if let Some(revisions) = self.current_symbolic_revisions() {
            return revisions;
        }
        let Some(cursor) = self.future.kernel_cursor.as_ref() else {
            return (
                self.state.pistons.iter().map(|state| state.revision).collect(),
                self.state.resources.iter().map(|state| state.revision).collect(),
            );
        };
        let Some(kernel) = self.program.instant_kernel() else {
            return (Vec::new(), Vec::new());
        };
        let piston_deltas = self.kernel_revision_totals(kernel, cursor, true);
        let resource_deltas = self.kernel_revision_totals(kernel, cursor, false);
        (
            cursor
                .piston_revisions
                .iter()
                .zip(piston_deltas)
                .map(|(&origin, delta)| origin.wrapping_add(delta))
                .collect(),
            cursor
                .resource_revisions
                .iter()
                .zip(resource_deltas)
                .map(|(&origin, delta)| origin.wrapping_add(delta))
                .collect(),
        )
    }

    fn kernel_revision_totals(&self, kernel: &InstantKernel, cursor: &KernelCursor, pistons: bool) -> Vec<u64> {
        let count = if pistons { self.state.pistons.len() } else { self.state.resources.len() };
        let mut totals = vec![0u64; count];
        let prefix = usize::from(kernel.prefix);
        let end = prefix + usize::from(kernel.period);
        if cursor.phase < prefix {
            add_kernel_revisions(kernel, cursor.mask, 0, cursor.phase, pistons, &mut totals);
            return totals;
        }
        add_kernel_revisions(kernel, cursor.mask, 0, prefix, pistons, &mut totals);
        let mut cycle = vec![0u64; count];
        add_kernel_revisions(kernel, cursor.mask, prefix, end, pistons, &mut cycle);
        for (total, delta) in totals.iter_mut().zip(cycle) {
            *total = total.wrapping_add(delta.wrapping_mul(cursor.cycles));
        }
        add_kernel_revisions(kernel, cursor.mask, prefix, cursor.phase, pistons, &mut totals);
        totals
    }
}

fn add_kernel_revisions(kernel: &InstantKernel, mask: u8, start: usize, end: usize, pistons: bool, totals: &mut [u64]) {
    for frame in &kernel.phases[start..end] {
        if pistons {
            for &(id, expression) in &frame.piston_revisions {
                let index = id.index();
                totals[index] = totals[index].wrapping_add(kernel.evaluate(expression, mask));
            }
        } else {
            for &(id, expression) in &frame.resource_revisions {
                let index = id.index();
                totals[index] = totals[index].wrapping_add(kernel.evaluate(expression, mask));
            }
        }
    }
}

fn evaluate_kernel_state(kernel: &InstantKernel, expression: &KernelStateExpr, mask: u8) -> Option<MachineState> {
    let mut state = MachineState {
        components: expression
            .components
            .iter()
            .map(|&id| u16::try_from(kernel.evaluate(id, mask)).ok())
            .collect::<Option<_>>()?,
        pistons: expression
            .pistons
            .iter()
            .map(|&id| u8::try_from(kernel.evaluate(id, mask)).ok())
            .collect::<Option<_>>()?,
        resources: expression
            .resources
            .iter()
            .map(|&(contact, enabled)| {
                Some((
                    ContactId::try_from_index(kernel.evaluate(contact, mask) as usize)?,
                    kernel.evaluate(enabled, mask) != 0,
                ))
            })
            .collect::<Option<_>>()?,
        ticks: Vec::new(),
        requests: Vec::new(),
        deferred: Vec::new(),
    };
    let future = &expression.future;
    let tick_len = usize::try_from(kernel.evaluate(future.tick_len, mask)).ok()?;
    let request_len = usize::try_from(kernel.evaluate(future.request_len, mask)).ok()?;
    let deferred_len = usize::try_from(kernel.evaluate(future.deferred_len, mask)).ok()?;
    if tick_len > future.ticks.len() || request_len > future.requests.len() || deferred_len > future.deferred.len() {
        return None;
    }
    for tick in &future.ticks[..tick_len] {
        let node = NodeId::try_from_index(kernel.evaluate(tick.node, mask) as usize)?;
        let delay = u32::try_from(kernel.evaluate(tick.delay, mask)).ok()?;
        let priority = match kernel.evaluate(tick.priority, mask) {
            0 => TickPriority::Highest,
            1 => TickPriority::Higher,
            2 => TickPriority::High,
            3 => TickPriority::Normal,
            _ => return None,
        };
        state.ticks.push(MachineTick { node, delay, priority });
    }
    for request in &future.requests[..request_len] {
        let piston = PistonId::try_from_index(kernel.evaluate(request.piston, mask) as usize)?;
        let action = decode_action(kernel.evaluate(request.action, mask))?;
        state.requests.push((piston, action));
    }
    for event in &future.deferred[..deferred_len] {
        let piston = PistonId::try_from_index(kernel.evaluate(event.piston, mask) as usize)?;
        let revision = kernel.evaluate(event.revision, mask);
        let resource = match kernel.evaluate(event.resource, mask) {
            u64::MAX => None,
            resource => Some((
                ResourceId::try_from_index(resource as usize)?,
                kernel.evaluate(event.resource_revision, mask),
                ContactId::try_from_index(kernel.evaluate(event.contact, mask) as usize)?,
            )),
        };
        state.deferred.push(MachineDeferred {
            piston,
            revision,
            resource,
            delay: kernel.evaluate(event.delay, mask),
            extending: kernel.evaluate(event.extending, mask) != 0,
        });
    }
    Some(state)
}

fn decode_action(value: u64) -> Option<PistonAction> {
    match value {
        0 => Some(PistonAction::Extend),
        1 => Some(PistonAction::Retract),
        2 => Some(PistonAction::RetractWithoutPull),
        _ => None,
    }
}
