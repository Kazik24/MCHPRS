//! Execute atomic pull waves or FIFO-local cell entries over authoritative graph state.
use super::*;
use std::sync::Arc;

pub(super) struct KernelExecution {
    /// CPU/OS detection occurs once, never inside a predicate's read loop.
    pub(super) clause_matcher: crate::redpiler::program_graph::cpu_kernels::ClauseMatcher,
    /// These Boolean slots are allocated once for the largest admitted expression program.
    scratch: Vec<bool>,
    /// This replaces only synchronous computation routes while a wave is committing.
    pub(super) active: Option<Arc<InstantKernel>>,
    /// This actor is inside its synchronous commit, before the next queued action runs.
    committing: Option<PistonId>,
}

impl Default for KernelExecution {
    fn default() -> Self {
        Self {
            scratch: Vec::new(),
            active: None,
            committing: None,
            clause_matcher: crate::redpiler::program_graph::cpu_kernels::clause_matcher(),
        }
    }
}

impl KernelExecution {
    pub(super) fn new(program: &CircuitProgram) -> Self {
        let size = program
            .instant_kernels()
            .iter()
            .map(|kernel| match &kernel.program {
                KernelProgram::PullWave { program, .. } => program.expressions.len(),
                KernelProgram::Events(entries) => entries.iter().map(|entry| entry.pins.len()).max().unwrap_or(0),
            })
            .max()
            .unwrap_or(0);
        Self {
            scratch: vec![false; size],
            ..Self::default()
        }
    }
}

impl GraphBackend {
    pub(super) fn run_instant_kernel(&mut self, request: PistonRequest) -> Result<bool, GraphExecutionError> {
        if request.action != mchprs_world::PistonAction::Retract || self.instant_kernel.active.is_some() {
            return Ok(false);
        }
        if let Some(entry) = self.program.cpu_kernel_entry(request.piston) {
            return self.run_cpu_kernel(entry, request);
        }
        let Some((kernel, event_entry)) = self
            .program
            .instant_kernel_binding(request.piston)
            .map(|(kernel, entry)| (kernel.clone(), entry))
        else {
            #[cfg(test)]
            self.record_kernel_attempt(request.piston, 0);
            return Ok(false);
        };
        let KernelProgram::PullWave { program, stationary, pulls } = &kernel.program else {
            return self.run_instant_event(&kernel, event_entry, request);
        };
        let Some(entry) = request.source.filter(|entry| kernel.entries.binary_search(entry).is_ok()) else {
            #[cfg(test)]
            self.record_kernel_attempt(request.piston, if request.source.is_none() { 1 } else { 2 });
            return Ok(false);
        };
        if kernel.actors.iter().any(|actor| self.early_resource_retraction(*actor))
            || kernel.wires.iter().any(|&(node, expected)| {
                let wire = self.wire_state(node);
                wire.power != expected || crate::redstone::wire::is_dot(wire)
            })
        {
            #[cfg(test)]
            self.record_kernel_attempt(request.piston, 3);
            return Ok(false);
        }
        let mut scratch = std::mem::take(&mut self.instant_kernel.scratch);
        let evaluated = program.evaluate(&mut scratch, |input| self.kernel_input(input, entry, request));
        let eligible = evaluated.is_ok() && scratch[stationary.0 as usize] && self.kernel_owns_queue(&kernel, pulls, &scratch, request);
        if !eligible {
            #[cfg(test)]
            self.record_kernel_attempt(request.piston, if evaluated.is_err() || !scratch[stationary.0 as usize] { 4 } else { 5 });
            self.instant_kernel.scratch = scratch;
            return Ok(false);
        }
        // Nothing observable is changed until the full entry and queue ownership checks succeed.
        if kernel.actors.len() != 1 {
            self.future.piston_requests.clear();
        }
        self.instant_kernel.active = Some(kernel.clone());
        let result = self.commit_kernel_pulls(pulls, &scratch, entry);
        self.instant_kernel.active = None;
        self.instant_kernel.scratch = scratch;
        result?;
        self.counts.instant_kernel_invocations += 1;
        #[cfg(test)]
        self.record_kernel_attempt(request.piston, 6);
        Ok(true)
    }

    #[cfg(test)]
    fn record_kernel_attempt(&mut self, actor: PistonId, outcome: usize) {
        if let Some(activity) = self.test_recorder.instant_kernel_activity.get_mut(actor.index()) {
            activity[outcome] += 1;
        }
    }

    fn kernel_owns_queue(&self, kernel: &InstantKernel, pulls: &[KernelPull], values: &[bool], first: PistonRequest) -> bool {
        // A singleton replaces only the already-popped action; callbacks append behind
        // the untouched queue exactly as Graph does. Multi-actor waves still own it all.
        let mut actual = std::iter::once(first).chain(self.future.piston_requests.iter().copied());
        let mut matched = 0;
        for pull in pulls.iter().filter(|pull| pull.external && values[pull.requested.0 as usize]) {
            let Some(request) = actual.next() else { return false };
            if request.piston != pull.actor || request.action != mchprs_world::PistonAction::Retract || request.source != first.source {
                return false;
            }
            matched += 1;
        }
        if kernel.actors.len() == 1 {
            matched == 1
        } else {
            actual.next().is_none()
        }
    }

    fn commit_kernel_pulls(&mut self, pulls: &[KernelPull], values: &[bool], entry: RouteId) -> Result<(), GraphExecutionError> {
        for pull in pulls {
            if values[pull.accepted.0 as usize] {
                let extended = self.state.pistons[pull.actor.index()].extended;
                self.retract_connections(pull.actor, false)?;
                self.accepted(
                    PistonRequest {
                        piston: pull.actor,
                        action: mchprs_world::PistonAction::Retract,
                        source: Some(entry),
                    },
                    extended,
                );
            }
        }
        Ok(())
    }

    fn run_instant_event(&mut self, kernel: &InstantKernel, entry_index: usize, request: PistonRequest) -> Result<bool, GraphExecutionError> {
        let KernelProgram::Events(entries) = &kernel.program else {
            unreachable!()
        };
        if !self.instant_event_ready(request.piston) {
            return Ok(false);
        }
        let entry = &entries[entry_index];
        let node = self.program.pistons()[request.piston.index()].node;
        if self.input_strength(node, InputPort::Main) > 0 || self.input_strength(node, InputPort::Quasi) > 0 {
            return self.commit_instant_event(request, false);
        }
        let accepted = match &entry.pins[entry.valid.index()] {
            CellPin::Gate {
                kind: CellGateKind::All,
                inputs,
            } if inputs.is_empty() => true,
            CellPin::Gate {
                kind: CellGateKind::Any,
                inputs,
            } if inputs.is_empty() => false,
            _ => {
                let mut values = std::mem::take(&mut self.instant_kernel.scratch);
                for (index, pin) in entry.pins.iter().enumerate() {
                    values[index] = match pin {
                        CellPin::Read(CellRead::State(test)) => self.state.matches(*test),
                        CellPin::Read(CellRead::Contribution(link)) => {
                            self.guard_matches(link.guard, None) && self.site_power(link.from, link.from_port) > link.attenuation
                        }
                        CellPin::Read(CellRead::Activation(_)) => unreachable!("acceptance cannot depend on a later activation"),
                        CellPin::Gate { kind, inputs } => match kind {
                            CellGateKind::Buffer => values[inputs[0].index()],
                            CellGateKind::Invert => !values[inputs[0].index()],
                            CellGateKind::All => inputs.iter().all(|input| values[input.index()]),
                            CellGateKind::Any => inputs.iter().any(|input| values[input.index()]),
                        },
                    };
                }
                let result = values[entry.valid.index()];
                self.instant_kernel.scratch = values;
                result
            }
        };
        self.commit_instant_event(request, accepted)
    }

    pub(super) fn instant_event_ready(&self, actor: PistonId) -> bool {
        let state = self.state.pistons[actor.index()];
        if state.availability != PistonAvailability::Ready || !state.extended {
            return false;
        }
        let spec = self.program.pistons()[actor.index()];
        // Interrupted extensions require restoration outside the ready-pull contract.
        !matches!(spec.lane, PistonLane::SingleResource { far }
            if self.piston_resource_at(actor, far).is_some_and(|resource|
                self.pending_resource_extension(resource, far, spec.facing).is_some()))
    }

    pub(super) fn commit_instant_event(&mut self, request: PistonRequest, accepted: bool) -> Result<bool, GraphExecutionError> {
        // The original commit retains callbacks, FIFO, source witnesses and deferred revisions.
        self.instant_kernel.committing = accepted.then_some(request.piston);
        let result = self.accept_compiled_pull(request, accepted);
        self.instant_kernel.committing = None;
        result?;
        self.counts.instant_kernel_invocations += 1;
        Ok(true)
    }

    pub(super) fn instant_event_noop(&self, receiver: CallbackReceiver) -> bool {
        let Some(actor) = self.instant_kernel.committing else { return false };
        if !self.state.pistons[actor.index()].availability.base_pending() {
            return false;
        }
        match receiver {
            CallbackReceiver::PistonBase(target) | CallbackReceiver::PistonHead(target) => target == actor,
            CallbackReceiver::Component(node) => match self.program.nodes()[node.index()].kind {
                CircuitNodeKind::Piston(target) => target == actor,
                CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::PressurePlate | NodeType::Constant) => true,
                _ => false,
            },
            CallbackReceiver::Contact(_) => true,
        }
    }

    pub(super) fn deliver_kernel_route(&mut self, route: &KernelRoute) -> Result<(), GraphExecutionError> {
        for &node in &route.clear_wires {
            let mut wire = self.wire_state(node);
            wire.power = 0;
            self.set_wire(node, wire);
        }
        for &delivery in &route.deliveries {
            self.deliver_callback(route.route, delivery, None)?;
        }
        Ok(())
    }

    fn kernel_input(&self, input: LayerInput, entry: RouteId, first: PistonRequest) -> bool {
        match input.source {
            LayerInputSource::BoundaryEvent { occurrence, .. } => occurrence.route == entry,
            LayerInputSource::BoundaryRequest { occurrence, actor, .. } => {
                occurrence.route == entry
                    && std::iter::once(first).chain(self.future.piston_requests.iter().copied()).any(|request| {
                        request.piston == actor && request.action == mchprs_world::PistonAction::Retract && request.source == Some(entry)
                    })
            }
            LayerInputSource::SourceAbove { site, port, threshold } => self.site_power(site, port) > threshold,
            LayerInputSource::PowerContribution { link, .. } | LayerInputSource::LocalResetOutput { link, .. } => {
                self.guard_matches(link.guard, None) && self.site_power(link.from, link.from_port) > link.attenuation
            }
            LayerInputSource::State(test) | LayerInputSource::BoundaryState { test, .. } => self.state.matches(test),
        }
    }
}
