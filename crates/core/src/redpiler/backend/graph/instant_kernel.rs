//! Execute FIFO-local pull-wave steps or cell entries over authoritative graph state.
use super::*;
use std::sync::Arc;

pub(super) struct KernelExecution {
    /// CPU/OS detection occurs once, never inside a predicate's read loop.
    pub(super) clause_matcher: crate::redpiler::program_graph::cpu_kernels::ClauseMatcher,
    /// These Boolean slots are allocated once for the largest admitted expression program.
    scratch: Vec<bool>,
    /// O6 reuses incremental values for each admitted pull-wave expression DAG.
    pull_wave_values: Vec<Option<PullWaveValues>>,
    pull_wave_by_actor: Box<[Option<usize>]>,
    /// This replaces only synchronous computation routes while a wave is committing.
    pub(super) active: Option<Arc<InstantKernel>>,
    /// The active wave's evaluated roots are isolated from shared Event/CPU scratch.
    active_values: Option<Arc<[bool]>>,
    /// Continuation follows Graph's actual FIFO one popped request at a time.
    continuation: Option<PullWaveContinuation>,
    /// This actor is inside its synchronous commit, before the next queued action runs.
    committing: Option<PistonId>,
    /// At least one route was actually replaced by a compiled route during this action.
    compiled_route: bool,
}

struct PullWaveContinuation {
    kernel: Arc<InstantKernel>,
    values: Arc<[bool]>,
    next_pull: usize,
}

struct PullWaveValues {
    evaluator: LogicalValues,
    inputs: Vec<bool>,
}

impl Default for KernelExecution {
    fn default() -> Self {
        Self {
            scratch: Vec::new(),
            pull_wave_values: Vec::new(),
            pull_wave_by_actor: Box::new([]),
            active: None,
            active_values: None,
            continuation: None,
            committing: None,
            compiled_route: false,
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
        let settled_enabled = program.settled_circuit().is_some();
        let mut pull_wave_values = Vec::new();
        let mut pull_wave_by_actor = if settled_enabled {
            vec![None; program.pistons().len()]
        } else {
            Vec::new()
        };
        for kernel in program.instant_kernels().iter().filter(|_| settled_enabled) {
            let cache_index = pull_wave_values.len();
            let values = match &kernel.program {
                KernelProgram::PullWave { program, .. } => LogicalValues::new(program).ok().map(|evaluator| PullWaveValues {
                    inputs: vec![false; program.inputs.len()],
                    evaluator,
                }),
                _ => None,
            };
            if values.is_some() {
                for actor in kernel.actors.iter() {
                    pull_wave_by_actor[actor.index()] = Some(cache_index);
                }
            }
            pull_wave_values.push(values);
        }
        Self {
            scratch: vec![false; size],
            pull_wave_values,
            pull_wave_by_actor: pull_wave_by_actor.into_boxed_slice(),
            ..Self::default()
        }
    }

    #[cfg(test)]
    pub(super) fn disable_incremental_values(&mut self) {
        self.pull_wave_values.iter_mut().for_each(|values| *values = None);
        self.pull_wave_by_actor.fill(None);
    }
}

impl GraphBackend {
    #[cfg(test)]
    pub(super) fn disable_incremental_values(&mut self) {
        self.instant_kernel.disable_incremental_values();
    }

    pub(super) fn run_instant_kernel(&mut self, request: PistonRequest) -> Result<bool, GraphExecutionError> {
        if let Some(mut continuation) = self.instant_kernel.continuation.take() {
            let KernelProgram::PullWave { pulls, .. } = &continuation.kernel.program else {
                unreachable!("a PullWave continuation always retains its kernel program")
            };
            let next = pulls
                .iter()
                .enumerate()
                .skip(continuation.next_pull)
                .find(|(_, pull)| continuation.values[pull.requested.0 as usize])
                .map(|(index, pull)| (index, pull.accepted));
            if let Some((index, accepted_root)) = next
                && request.action == mchprs_world::PistonAction::Retract
                && pulls[index].actor == request.piston
                && pulls[index].origin.source_matches(request.source)
            {
                continuation.next_pull = index + 1;
                let kernel = continuation.kernel.clone();
                let values = continuation.values.clone();
                let accepted = values[accepted_root.0 as usize];
                self.instant_kernel.continuation = Some(continuation);
                return self.run_pull_wave_action(&kernel, &values, index, request, accepted);
            }
            // A foreign or unexpected request is authoritative; discard this wave suffix.
        }
        if self.logical_bud_owner_by_actor.get(request.piston.index()).is_some_and(Option::is_some) {
            return Ok(false);
        }
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
        // Every region actor must be free of separately managed runtime owners.
        if kernel.actors.iter().any(|actor| {
            self.logical_bud_owner_by_actor.get(actor.index()).is_some_and(Option::is_some)
                || self.native_action_clock_owner.get(actor.index()).is_some_and(Option::is_some)
                || self.active_frame_retained_actors.contains(actor)
        }) {
            return Ok(false);
        }
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
        let cache_index = self.instant_kernel.pull_wave_by_actor.get(request.piston.index()).copied().flatten();
        let mut cached = cache_index.and_then(|index| self.instant_kernel.pull_wave_values[index].take());
        let mut scratch = std::mem::take(&mut self.instant_kernel.scratch);
        let logical_evaluation = if let Some(values) = cached.as_mut() {
            for (slot, &input) in values.inputs.iter_mut().zip(program.inputs.iter()) {
                *slot = self.kernel_input(input, entry, request);
            }
            self.counts.logical_kernel_evaluations += 1;
            Some(values.evaluator.evaluate(&values.inputs))
        } else {
            None
        };
        let evaluated = match logical_evaluation {
            Some(Ok(operations)) => {
                self.counts.logical_kernel_operations += operations as u64;
                Ok(())
            }
            Some(Err(_)) => {
                // A malformed cached program must never suppress the exact kernel.
                cached = None;
                program.evaluate(&mut scratch, |input| self.kernel_input(input, entry, request))
            }
            None => program.evaluate(&mut scratch, |input| self.kernel_input(input, entry, request)),
        };
        let values = cached.as_ref().map_or(scratch.as_slice(), |values| values.evaluator.values());
        let first_pull = pulls.iter().position(|pull| {
            pull.origin.is_boundary()
                && values[pull.requested.0 as usize]
                && pull.actor == request.piston
                && request.action == mchprs_world::PistonAction::Retract
                && pull.origin.source_matches(request.source)
        });
        let eligible = evaluated.is_ok()
            && values[stationary.0 as usize]
            && first_pull.is_some_and(|index| self.kernel_owns_queue(pulls, values, index, request));
        if !eligible {
            #[cfg(test)]
            self.record_kernel_attempt(request.piston, if evaluated.is_err() || !values[stationary.0 as usize] { 4 } else { 5 });
            if let Some(index) = cache_index {
                self.instant_kernel.pull_wave_values[index] = cached;
            }
            self.instant_kernel.scratch = scratch;
            return Ok(false);
        }
        // The first request starts a continuation; later requests must appear at the
        // predicted point in Graph's own FIFO or the remaining suffix runs natively.
        let first_pull = first_pull.unwrap();
        let wave_values: Arc<[bool]> = Arc::from(values);
        self.instant_kernel.continuation = Some(PullWaveContinuation {
            kernel: kernel.clone(),
            values: wave_values.clone(),
            next_pull: first_pull + 1,
        });
        if let Some(index) = cache_index {
            self.instant_kernel.pull_wave_values[index] = cached;
        }
        self.instant_kernel.scratch = scratch;
        let accepted = wave_values[pulls[first_pull].accepted.0 as usize];
        self.run_pull_wave_action(&kernel, &wave_values, first_pull, request, accepted)
    }

    #[cfg(test)]
    fn record_kernel_attempt(&mut self, actor: PistonId, outcome: usize) {
        if let Some(activity) = self.test_recorder.instant_kernel_activity.get_mut(actor.index()) {
            activity[outcome] += 1;
        }
    }

    fn kernel_owns_queue(&self, pulls: &[KernelPull], values: &[bool], first_index: usize, first: PistonRequest) -> bool {
        // Verify all already-queued region requests in order while leaving unrelated
        // FIFO entries in place. A foreign pop later cancels the predicted suffix.
        let mut expected = pulls
            .iter()
            .enumerate()
            .filter(|(_, pull)| pull.origin.is_boundary() && values[pull.requested.0 as usize]);
        let Some((expected_index, first_expected)) = expected.next() else {
            return false;
        };
        if expected_index != first_index
            || first_expected.actor != first.piston
            || !first_expected.origin.source_matches(first.source)
            || !pulls[first_index].origin.is_boundary()
        {
            return false;
        }
        let mut actual = self.future.piston_requests.iter().copied();
        for (_, pull) in expected {
            if actual
                .find(|request| {
                    request.piston == pull.actor
                        && request.action == mchprs_world::PistonAction::Retract
                        && pull.origin.source_matches(request.source)
                })
                .is_none()
            {
                return false;
            }
        }
        true
    }

    fn run_pull_wave_action(
        &mut self,
        kernel: &Arc<InstantKernel>,
        values: &Arc<[bool]>,
        pull_index: usize,
        request: PistonRequest,
        accepted: bool,
    ) -> Result<bool, GraphExecutionError> {
        let KernelProgram::PullWave { pulls, .. } = &kernel.program else {
            unreachable!("PullWave action must use a PullWave program")
        };
        let pull = &pulls[pull_index];
        if pull.actor != request.piston || !pull.origin.source_matches(request.source) {
            return Ok(false);
        }
        let live_accepted = self.native_action_accepted(request);
        if live_accepted != accepted {
            // A cut changed since the wave snapshot; let Graph own this action and its suffix.
            self.instant_kernel.continuation = None;
            self.instant_kernel.active = None;
            self.instant_kernel.active_values = None;
        } else {
            self.instant_kernel.active = Some(kernel.clone());
            self.instant_kernel.active_values = Some(values.clone());
        }
        self.instant_kernel.compiled_route = false;
        self.instant_kernel.committing = live_accepted.then_some(request.piston);
        let result = self.accept_action_using(request, None);
        self.instant_kernel.committing = None;
        self.instant_kernel.active = None;
        self.instant_kernel.active_values = None;
        result?;
        if live_accepted == accepted && self.instant_kernel.compiled_route {
            self.counts.instant_kernel_invocations += 1;
        }
        #[cfg(test)]
        self.record_kernel_attempt(request.piston, 6);
        Ok(true)
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

    pub(super) fn deliver_kernel_route(&mut self, route: &KernelRoute, captured: Option<&[Option<ObservedSite>]>) -> Result<(), GraphExecutionError> {
        if route.steps.iter().any(|step| matches!(&step.kind, KernelRouteStepKind::NativeHandoff(_))) {
            // A callback can change later reads; preserve the whole route's native prefix.
            self.instant_kernel.continuation = None;
            self.instant_kernel.active = None;
            self.instant_kernel.active_values = None;
            return self.deliver_route(route.route, captured);
        }
        self.instant_kernel.compiled_route = true;
        for step in &route.steps {
            match &step.kind {
                KernelRouteStepKind::ClearWires(nodes) => {
                    self.counts.instant_kernel_wire_walks_elided += 1;
                    for &node in nodes.iter() {
                        let mut wire = self.wire_state(node);
                        wire.power = 0;
                        self.set_wire(node, wire);
                    }
                }
                KernelRouteStepKind::EnqueuePull(index) => {
                    let kernel = self.instant_kernel.active.as_ref().expect("wave route has active kernel");
                    let values = self.instant_kernel.active_values.as_ref().expect("wave route has active values");
                    let KernelProgram::PullWave { pulls, .. } = &kernel.program else {
                        unreachable!("active route kernel is not a PullWave")
                    };
                    let pull = &pulls[*index];
                    if values[pull.requested.0 as usize] {
                        self.enqueue_piston(PistonRequest {
                            piston: pull.actor,
                            action: mchprs_world::PistonAction::Retract,
                            source: Some(pull.origin.internal_cause().expect("only internal pull recipes enqueue requests").route),
                        });
                    }
                }
                KernelRouteStepKind::Deliver(delivery) => {
                    self.deliver_callback(
                        route.route,
                        *delivery,
                        captured,
                        Some(RouteOccurrence {
                            route: route.route,
                            ordinal: step.ordinal,
                        }),
                    )?;
                }
                KernelRouteStepKind::NativeHandoff(_) => unreachable!("routes with native cuts hand off before executing compiled steps"),
            }
        }
        Ok(())
    }

    pub(super) fn cancel_pull_wave_continuation(&mut self) {
        self.instant_kernel.continuation = None;
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
            source => self
                .sample_settled_input(source)
                .expect("non-event kernel inputs are statically supported"),
        }
    }
}
