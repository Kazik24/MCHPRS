//! Runtime admission and phase execution for AOT symbolic kernels.
use super::machine_state::{write_component, write_piston};
use super::*;
use crate::redpiler::program_graph::{KernelFrameLayout, SymbolicKernel};

#[derive(Clone)]
pub(super) struct SymbolicCursor {
    /// This selects the immutable program retained by `CircuitProgram`.
    pub(super) kernel: usize,
    /// This is the next compiled phase to apply.
    pub(super) phase: usize,
    /// This counts complete cycles after the finite prefix.
    pub(super) cycles: u64,
    /// These are all compiled roots evaluated once when stable inputs are admitted.
    pub(super) roots: Box<[bool]>,
    /// These preserve live revision origins until cold state is materialized.
    pub(super) piston_revisions: Box<[u64]>,
    /// These preserve live payload revision origins until cold state is materialized.
    pub(super) resource_revisions: Box<[u64]>,
}

impl SymbolicCursor {
    fn frame<'a>(&self, kernel: &'a SymbolicKernel) -> &'a KernelFrameLayout {
        if self.phase == 0 && self.cycles == 0 {
            &kernel.activation
        } else {
            let previous = if self.phase == 0 { kernel.phases.len() - 1 } else { self.phase - 1 };
            &kernel.phases[previous]
        }
    }
}

pub(super) struct PreparedKernel {
    /// This selects the program whose entry relation matched before the falling edge.
    index: usize,
    /// These frozen root values describe every subsequent admitted phase.
    roots: Box<[bool]>,
}

impl GraphBackend {
    pub(super) fn prepare_symbolic_kernel(&self, trigger: NodeId) -> Option<PreparedKernel> {
        if self.future.clock.phase != GraphPhase::BetweenTicks
            || self.future.clock.scheduled_advanced
            || self.future.clock.logical_tick > u64::MAX - SCHEDULER_BUCKETS as u64
            || self.future.scheduler.iter().next().is_some()
            || !self.future.pending.is_empty()
            || !self.future.piston_requests.is_empty()
            || !self.future.deferred_pistons.is_empty()
        {
            return None;
        }

        let scalars = self.scalar_values()?;
        for (index, kernel) in self.program.symbolic_kernels().iter().enumerate() {
            if kernel.trigger != trigger {
                continue;
            }
            let inputs = kernel
                .inputs
                .iter()
                .map(|&input| input.read(&self.program, &self.state, &kernel.shapes))
                .collect::<Option<Vec<_>>>()?;
            let mut scratch = vec![false; kernel.plan.scratch_len()];
            let mut roots = vec![false; kernel.plan.roots().len()];
            kernel.plan.evaluate(&inputs, &mut scratch, &mut roots).ok()?;
            if !all_roots(&roots, &kernel.entry.conditions) || !matches_roots(&roots, &kernel.entry.state, &scalars) {
                continue;
            }
            return Some(PreparedKernel {
                index,
                roots: roots.into_boxed_slice(),
            });
        }
        None
    }

    pub(super) fn enter_symbolic_kernel(&mut self, prepared: PreparedKernel) {
        let kernel = self.program.symbolic_kernels()[prepared.index].clone();
        let cursor = SymbolicCursor {
            kernel: prepared.index,
            phase: 0,
            cycles: 0,
            roots: prepared.roots,
            piston_revisions: self.state.pistons.iter().map(|state| state.revision).collect(),
            resource_revisions: self.state.resources.iter().map(|state| state.revision).collect(),
        };
        self.apply_hot_components(&kernel, &kernel.activation, &cursor.roots);
        self.append_symbolic_effects(&kernel.activation, &cursor.roots);
        self.counts.instant_transitions += 1;
        self.future.symbolic_cursor = Some(cursor);
    }

    pub(super) fn try_symbolic_kernel_tick(&mut self) -> bool {
        let Some(mut cursor) = self.future.symbolic_cursor.take() else {
            return false;
        };
        if self.future.clock.phase != GraphPhase::BetweenTicks
            || self.future.clock.scheduled_advanced
            || self.future.clock.logical_tick > u64::MAX - SCHEDULER_BUCKETS as u64
        {
            self.future.symbolic_cursor = Some(cursor);
            self.leave_symbolic_kernel();
            return false;
        }
        let Some(kernel) = self.program.symbolic_kernels().get(cursor.kernel).cloned() else {
            panic!("active symbolic kernel is missing from its program");
        };
        let Some(frame) = kernel.phases.get(cursor.phase) else {
            self.future.symbolic_cursor = Some(cursor);
            self.leave_symbolic_kernel();
            return false;
        };
        if cursor.phase + 1 == kernel.phases.len() {
            cursor.phase = kernel.prefix;
            cursor.cycles = cursor.cycles.wrapping_add(1);
        } else {
            cursor.phase += 1;
        }

        self.future.clock.logical_tick += 1;
        self.apply_hot_components(&kernel, frame, &cursor.roots);
        self.append_symbolic_effects(frame, &cursor.roots);
        self.counts.ticks += 1;
        self.counts.instant_transitions += 1;
        self.future.symbolic_cursor = Some(cursor);
        true
    }

    pub(super) fn materialize_symbolic_kernel(&mut self) -> bool {
        let Some(cursor) = self.future.symbolic_cursor.take() else {
            return true;
        };
        let before = self.scalar_values().expect("validated graph state must be readable");
        let result = self.program.symbolic_kernels().get(cursor.kernel).cloned().and_then(|kernel| {
            let frame = cursor.frame(&kernel);
            let state = self.machine_state_for_frame(frame, &cursor.roots)?;
            self.install_symbolic_state(&kernel, &cursor, state)
        });
        self.future.symbolic_cursor = Some(cursor);
        if result.is_some() {
            self.mark_symbolic_changes(&before);
        }
        result.is_some()
    }

    pub(super) fn current_symbolic_boundary(&self) -> Option<MachineState> {
        let cursor = self.future.symbolic_cursor.as_ref()?;
        let kernel = self.program.symbolic_kernels().get(cursor.kernel)?;
        self.machine_state_for_frame(cursor.frame(kernel), &cursor.roots)
    }

    #[cfg(test)]
    pub(super) fn current_symbolic_revisions(&self) -> Option<(Vec<u64>, Vec<u64>)> {
        let cursor = self.future.symbolic_cursor.as_ref()?;
        let kernel = self.program.symbolic_kernels().get(cursor.kernel)?;
        let actors = cursor
            .piston_revisions
            .iter()
            .zip(self.revision_totals(kernel, cursor, true))
            .map(|(&origin, delta)| origin.wrapping_add(delta))
            .collect();
        let resources = cursor
            .resource_revisions
            .iter()
            .zip(self.revision_totals(kernel, cursor, false))
            .map(|(&origin, delta)| origin.wrapping_add(delta))
            .collect();
        Some((actors, resources))
    }

    fn apply_hot_components(&mut self, kernel: &SymbolicKernel, frame: &KernelFrameLayout, roots: &[bool]) {
        for binding in &kernel.hot_components {
            let start = frame.state.start + binding.scalar;
            let value = read_bits(roots, start..start + 6).expect("validated electrical roots fit six bits") as u16;
            let state = self.node_state(binding.node);
            let before = u16::from(state.output_strength) | (u16::from(state.powered) << 4) | (u16::from(state.repeater_locked) << 5);
            if before != value {
                assert!(write_component(&mut self.state.components[binding.node.index()], value));
                self.dirty[binding.node.index()] = true;
            }
        }
    }

    fn leave_symbolic_kernel(&mut self) {
        assert!(
            self.materialize_symbolic_kernel(),
            "validated symbolic kernel must materialize before fallback"
        );
        self.future.symbolic_cursor = None;
    }

    fn install_symbolic_state(&mut self, kernel: &SymbolicKernel, cursor: &SymbolicCursor, state: MachineState) -> Option<()> {
        let piston_revisions = self.revision_totals(kernel, cursor, true);
        let resource_revisions = self.revision_totals(kernel, cursor, false);
        for (index, (&pose, piston)) in state.pistons.iter().zip(&mut self.state.pistons).enumerate() {
            if !write_piston(piston, pose) {
                return None;
            }
            piston.revision = cursor.piston_revisions[index].wrapping_add(piston_revisions[index]);
        }
        for (index, (&(contact, enabled), resource)) in state.resources.iter().zip(&mut self.state.resources).enumerate() {
            if !self.program.resources()[index].contacts.contains(&contact) {
                return None;
            }
            resource.at = contact;
            resource.enabled = enabled;
            resource.revision = cursor.resource_revisions[index].wrapping_add(resource_revisions[index]);
        }
        self.install_components(&state);
        self.restore_machine_future(&state);
        Some(())
    }

    fn install_components(&mut self, state: &MachineState) {
        for (component, &value) in self.state.components.iter_mut().zip(&state.components) {
            assert!(write_component(component, value), "validated symbolic component state must decode");
        }
    }

    fn mark_symbolic_changes(&mut self, before: &[bool]) {
        let after = self.scalar_values().expect("installed symbolic state must be readable");
        let mut offset = 0;
        for (index, node) in self.program.nodes().iter().enumerate() {
            let width = if matches!(node.kind, CircuitNodeKind::Wire(_)) { 14 } else { 6 };
            if before[offset..offset + width] != after[offset..offset + width] {
                self.dirty[index] = true;
            }
            offset += width;
        }
    }

    fn scalar_values(&self) -> Option<Vec<bool>> {
        let mut values = Vec::new();
        for (index, node) in self.program.nodes().iter().enumerate() {
            match (&node.kind, &self.state.components[index]) {
                (CircuitNodeKind::Wire(_), ComponentState::Wire(wire)) => {
                    push_bits(&mut values, u64::from(wire.power), 4);
                    values.extend([false, false]);
                    for side in [wire.north, wire.south, wire.east, wire.west] {
                        let (up, flat) = side_roots(side);
                        values.extend([up, flat]);
                    }
                }
                (CircuitNodeKind::Wire(_), _) | (_, ComponentState::Wire(_)) => return None,
                (_, ComponentState::Electrical(state)) => {
                    push_bits(&mut values, state.output_strength as u64, 4);
                    values.extend([state.powered, state.repeater_locked]);
                }
                (_, ComponentState::Specialized) => values.extend([false; 6]),
            }
        }
        for piston in &self.state.pistons {
            values.extend([piston.extended, piston.head_present]);
            values.extend((0..5).map(|index| piston.availability as usize == index));
        }
        for (spec, state) in self.program.resources().iter().zip(&self.state.resources) {
            values.extend(spec.contacts.iter().map(|&contact| contact == state.at));
            values.push(state.enabled);
        }
        Some(values)
    }

    fn machine_state_for_frame(&self, frame: &KernelFrameLayout, roots: &[bool]) -> Option<MachineState> {
        let mut values = MachineState::default();
        let mut scalar = frame.state.start;
        for (index, node) in self.program.nodes().iter().enumerate() {
            let strength = read_bits(roots, scalar..scalar + 4)? as u16;
            let powered = *roots.get(scalar + 4)?;
            let locked = *roots.get(scalar + 5)?;
            scalar += 6;
            let packed = match (&node.kind, &self.state.components[index]) {
                (CircuitNodeKind::Wire(_), ComponentState::Wire(_)) => {
                    let mut bits = strength;
                    for side in 0..4 {
                        let up = *roots.get(scalar)?;
                        let flat = *roots.get(scalar + 1)?;
                        scalar += 2;
                        let encoded = match (up, flat) {
                            (true, false) => 0,
                            (false, true) => 1,
                            (false, false) => 2,
                            (true, true) => return None,
                        };
                        bits |= encoded << (4 + side * 2);
                    }
                    bits
                }
                (CircuitNodeKind::Wire(_), _) | (_, ComponentState::Wire(_)) => return None,
                (_, ComponentState::Electrical(_)) => strength | (u16::from(powered) << 4) | (u16::from(locked) << 5),
                (_, ComponentState::Specialized) if strength == 0 && !powered && !locked => 0,
                (_, ComponentState::Specialized) => return None,
            };
            values.components.push(packed);
        }
        for _ in 0..self.program.pistons().len() {
            let extended = *roots.get(scalar)?;
            let head = *roots.get(scalar + 1)?;
            scalar += 2;
            let mut availability = None;
            for state in 0..5 {
                if *roots.get(scalar + state)? {
                    if availability.replace(state as u8).is_some() {
                        return None;
                    }
                }
            }
            scalar += 5;
            values.pistons.push(extended as u8 | ((head as u8) << 1) | (availability? << 2));
        }
        for resource in self.program.resources() {
            let mut selected = None;
            for &contact in resource.contacts.iter() {
                if *roots.get(scalar)? {
                    if selected.replace(contact).is_some() {
                        return None;
                    }
                }
                scalar += 1;
            }
            let enabled = *roots.get(scalar)?;
            scalar += 1;
            values.resources.push((selected?, enabled));
        }
        if scalar != frame.state.end {
            return None;
        }
        for event in frame.ticks.iter().filter(|event| roots.get(event.enabled) == Some(&true)) {
            values.ticks.push(MachineTick {
                node: event.node,
                delay: u32::try_from(event.delay).ok()?,
                priority: event.priority,
            });
        }
        let mut pending = vec![false; self.program.nodes().len()];
        for tick in &values.ticks {
            if std::mem::replace(&mut pending[tick.node.index()], true) {
                return None;
            }
        }
        if !matches_roots(roots, &frame.pending, &pending) {
            return None;
        }
        for event in frame.requests.iter().filter(|event| roots.get(event.enabled) == Some(&true)) {
            values.requests.push((event.actor, event.action));
        }
        for event in frame.publications.iter().filter(|event| roots.get(event.enabled) == Some(&true)) {
            let resource = event.resource.as_ref().and_then(|payload| {
                roots.get(payload.enabled).copied().filter(|enabled| *enabled).map(|_| {
                    (
                        payload.resource,
                        read_bits(roots, payload.revision_age.clone()).unwrap().wrapping_neg(),
                        payload.contact,
                    )
                })
            });
            values.deferred.push(MachineDeferred {
                piston: event.actor,
                revision: read_bits(roots, event.actor_revision_age.clone())?.wrapping_neg(),
                resource,
                delay: event.delay,
                extending: event.extending,
            });
        }
        Some(values)
    }

    fn append_symbolic_effects(&mut self, frame: &KernelFrameLayout, roots: &[bool]) {
        for event in frame.effects.iter().filter(|event| roots.get(event.enabled) == Some(&true)) {
            self.accepted(
                PistonRequest {
                    piston: event.actor,
                    action: event.action,
                },
                roots[event.extended],
            );
        }
    }

    fn revision_totals(&self, kernel: &SymbolicKernel, cursor: &SymbolicCursor, pistons: bool) -> Vec<u64> {
        let count = if pistons { self.state.pistons.len() } else { self.state.resources.len() };
        let actor_count = self.state.pistons.len();
        let base = revision_values(&kernel.entry.revision_deltas, &cursor.roots, pistons, actor_count);
        let prefix_end = kernel.prefix.saturating_sub(1);
        let prefix_values = if kernel.prefix == 0 {
            revision_values(&kernel.activation.revision_deltas, &cursor.roots, pistons, actor_count)
        } else {
            revision_values(&kernel.phases[prefix_end].revision_deltas, &cursor.roots, pistons, actor_count)
        };
        let cycle_end = revision_values(
            &kernel.phases[kernel.prefix + kernel.period - 1].revision_deltas,
            &cursor.roots,
            pistons,
            actor_count,
        );
        let mut totals = vec![0; count];
        if cursor.phase == 0 && cursor.cycles == 0 {
            let activation = revision_values(&kernel.activation.revision_deltas, &cursor.roots, pistons, actor_count);
            return activation.iter().zip(base).map(|(current, base)| current.wrapping_sub(base)).collect();
        }
        if cursor.phase < kernel.prefix {
            let current = revision_values(&kernel.phases[cursor.phase - 1].revision_deltas, &cursor.roots, pistons, actor_count);
            return current.iter().zip(base).map(|(current, base)| current.wrapping_sub(base)).collect();
        }
        let cycles = cursor.cycles;
        for index in 0..count {
            let prefix_delta = prefix_values[index].wrapping_sub(base[index]);
            let cycle_delta = cycle_end[index].wrapping_sub(prefix_values[index]);
            totals[index] = prefix_delta.wrapping_add(cycle_delta.wrapping_mul(cycles));
        }
        if cursor.phase > kernel.prefix {
            let current = revision_values(&kernel.phases[cursor.phase - 1].revision_deltas, &cursor.roots, pistons, actor_count);
            for index in 0..count {
                totals[index] = totals[index].wrapping_add(current[index].wrapping_sub(prefix_values[index]));
            }
        }
        totals
    }
}

fn all_roots(values: &[bool], range: &std::ops::Range<usize>) -> bool {
    values.get(range.clone()).is_some_and(|slice| slice.iter().all(|&value| value))
}

fn matches_roots(values: &[bool], range: &std::ops::Range<usize>, expected: &[bool]) -> bool {
    values.get(range.clone()) == Some(expected)
}

fn push_bits(values: &mut Vec<bool>, value: u64, count: usize) {
    values.extend((0..count).map(|bit| value & (1 << bit) != 0));
}

fn read_bits(values: &[bool], range: std::ops::Range<usize>) -> Option<u64> {
    if range.len() > u64::BITS as usize {
        return None;
    }
    Some(
        values
            .get(range.clone())?
            .iter()
            .enumerate()
            .fold(0, |bits, (bit, &set)| bits | (u64::from(set) << bit)),
    )
}

fn side_roots(side: RedstoneWireSide) -> (bool, bool) {
    match side {
        RedstoneWireSide::Up => (true, false),
        RedstoneWireSide::Side => (false, true),
        RedstoneWireSide::None => (false, false),
    }
}

fn revision_values(ranges: &[std::ops::Range<usize>], roots: &[bool], pistons: bool, piston_count: usize) -> Vec<u64> {
    ranges
        .iter()
        .skip(if pistons { 0 } else { piston_count })
        .take(if pistons { piston_count } else { usize::MAX })
        .map(|range| read_bits(roots, range.clone()).expect("validated revision roots fit u64"))
        .collect()
}
