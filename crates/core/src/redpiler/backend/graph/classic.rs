//! Direct receiver scheduling and cold reference replay used only at compilation.
use super::*;

pub(super) struct ProofRecorder {
    pub(super) source: NodeId,
    pub(super) signal: u8,
    receivers: Vec<NodeId>,
    samples: Vec<(NodeId, ClassicSample)>,
}

pub(crate) struct ClassicReference {
    backend: GraphBackend,
    saved: Vec<(NodeId, ComponentState)>,
    baseline: CircuitState,
    baseline_clock: Clock,
    #[cfg(test)]
    restored_unowned_writes: usize,
    work: usize,
}

/// A delayed receiver cannot change its stored output during this episode.
/// If its first pair equals current, its first distinct pair must schedule;
/// otherwise the first pair schedules and pending dedup suppresses the rest.
pub(super) fn compact_classic_samples(samples: &mut Vec<(NodeId, ClassicSample)>) {
    let mut seen = FxHashSet::default();
    let mut counts = FxHashMap::<NodeId, usize>::default();
    samples.retain(|sample| {
        if !seen.insert(*sample) {
            return false;
        }
        if matches!(sample.1, ClassicSample::Comparator { .. }) {
            let count = counts.entry(sample.0).or_default();
            *count += 1;
            return *count <= 2;
        }
        true
    });
}

impl ClassicReference {
    #[cfg(test)]
    pub(crate) fn has_initial_state(&self) -> bool {
        self.backend.state == self.baseline
            && self.backend.future.scheduler.iter().next().is_none()
            && self.backend.future.pending.is_empty()
            && self.backend.future.piston_requests.is_empty()
            && self.backend.future.deferred_pistons.is_empty()
            && self.backend.future.clock.logical_tick == self.baseline_clock.logical_tick
            && self.backend.future.clock.phase == self.baseline_clock.phase
            && self.backend.future.clock.scheduled_advanced == self.baseline_clock.scheduled_advanced
    }

    #[cfg(test)]
    pub(crate) fn restored_unowned_writes(&self) -> usize {
        self.restored_unowned_writes
    }

    pub(crate) fn work(&self) -> usize {
        self.work
    }
    pub(crate) fn new(graph: &CompileGraph) -> Result<Self, String> {
        let mut graph = graph.clone();
        graph.plans = ExecutionPlans::default();
        for wire in &mut graph.wires {
            wire.execution = Default::default();
        }
        let (program, state) = graph.lower().map_err(|error| error.to_string())?;
        let backend = GraphBackend::new(program, state, 0).map_err(|error| error.to_string())?;
        let baseline = backend.state.clone();
        let baseline_clock = backend.future.clock;
        Ok(Self {
            baseline,
            baseline_clock,
            #[cfg(test)]
            restored_unowned_writes: 0,
            backend,
            saved: Vec::new(),
            work: 0,
        })
    }

    /// State is factored by receiver: its powered flag cannot change synchronously,
    /// and pending membership can only change from absent to present in this episode.
    /// Thus only the first sample of each polarity can ever schedule that receiver.
    pub(crate) fn prove(&mut self, plan: &mut ClassicNetPlan, monitor: &TaskMonitor) -> Result<usize, String> {
        self.replay_plan(plan, monitor, false)
    }

    pub(crate) fn check(&mut self, plan: &ClassicNetPlan, monitor: &TaskMonitor) -> Result<usize, String> {
        self.replay_plan(&mut plan.clone(), monitor, true)
    }

    fn replay_plan(&mut self, plan: &mut ClassicNetPlan, monitor: &TaskMonitor, checking: bool) -> Result<usize, String> {
        self.saved.clear();
        self.saved.push((plan.source, self.backend.state.components[plan.source.index()].clone()));
        for wire in &plan.wires {
            self.saved.push((wire.node, self.backend.state.components[wire.node.index()].clone()));
        }
        for &node in &plan.guarded_wires {
            self.saved.push((node, self.backend.state.components[node.index()].clone()));
        }
        let receivers = plan.inputs.iter().map(|input| input.receiver).collect::<Vec<_>>();
        self.backend.classic_proof = Some(ProofRecorder {
            source: plan.source,
            signal: 0,
            receivers,
            samples: Vec::new(),
        });
        let before = self.work;
        let result = self.prove_inner(plan, monitor, checking);
        self.backend.classic_proof = None;
        // A rejected candidate may have unexpectedly reached unowned actors.
        // Restore every actual write, not merely the proposed ownership slice.
        #[cfg(test)]
        {
            self.restored_unowned_writes = 0;
        }
        for (index, dirty) in self.backend.dirty.iter_mut().enumerate() {
            if *dirty {
                #[cfg(test)]
                if !self.saved.iter().any(|(node, _)| node.index() == index) {
                    self.restored_unowned_writes += 1;
                }
                self.backend.state.components[index] = self.baseline.components[index].clone();
            }
            *dirty = false;
        }
        for (node, state) in self.saved.drain(..) {
            self.backend.state.components[node.index()] = state;
        }
        self.backend.state.pistons.clone_from(&self.baseline.pistons);
        self.backend.state.resources.clone_from(&self.baseline.resources);
        self.backend.state.classic_signals.clone_from(&self.baseline.classic_signals);
        self.backend.state.classic_active.clone_from(&self.baseline.classic_active);
        self.backend.contact_dirty.fill(false);
        self.backend.dirty_contacts.clear();
        self.backend.effects.clear();
        self.backend.failure = None;
        self.backend.wire_walks.clear();
        self.backend.future.scheduler = Default::default();
        self.backend.future.pending.clear();
        self.backend.future.pending_nodes.fill(0);
        self.backend.future.piston_requests.clear();
        self.backend.future.deferred_pistons.clear();
        self.backend.future.clock = self.baseline_clock;
        result.map(|()| self.work - before)
    }

    fn prove_inner(&mut self, plan: &mut ClassicNetPlan, monitor: &TaskMonitor, checking: bool) -> Result<(), String> {
        // Geometry and external query dependencies are invariant under all
        // hypothetical private/source writes. Validate the full live premise once;
        // each replay below still visits every boundary wire and checks no write.
        let mut proof_work = plan.guarded_wires.len() * 24 + plan.outside.len();
        self.work = self.work.saturating_add(proof_work);
        if !self.backend.classic_guard_wires(&plan.guarded_wires) {
            return Err("guarded callback wire saved state is not stable".into());
        }
        if !self.backend.classic_outside_zero(&plan.outside) {
            return Err("independent main guard is initially nonzero".into());
        }
        // Boolean receivers need two polarities; analog pairs need up to two
        // retained positions for each of the sixteen computed strengths.
        let operation_limit = 8 + plan
            .inputs
            .iter()
            .map(|input| {
                if matches!(
                    self.backend.program.nodes()[input.receiver.index()].kind,
                    CircuitNodeKind::Electrical(NodeType::Comparator { .. })
                ) {
                    32
                } else {
                    4
                }
            })
            .sum::<usize>();
        for entry in &mut plan.entries {
            let shape = matches!(
                self.backend.program.routes()[entry.route.index()].deliveries[entry.ordinal as usize].kind,
                DeliveryKind::Shape { .. }
            );
            if !entry.commit && !entry.ops.is_empty() {
                return Err("inert entry has executable operations".into());
            }
            if shape {
                continue;
            }
            let mut ops = if checking { entry.ops.to_vec() } else { Vec::<ClassicOp>::new() };
            if checking
                && (ops.len() > operation_limit
                    || ops
                        .iter()
                        .any(|op| op.when == [0; 4] || !plan.inputs.iter().any(|input| input.receiver == op.receiver)))
            {
                return Err("invalid compact classic operation".into());
            }
            for old in 0u8..16 {
                for new in 0u8..16 {
                    if !entry.commit && old != new {
                        continue;
                    }
                    if monitor.cancelled() {
                        return Err("cancelled during classic boundary proof".into());
                    }
                    let backend = &mut self.backend;
                    if let ComponentState::Electrical(state) = &mut backend.state.components[plan.source.index()] {
                        state.output_strength = new;
                        state.powered = new > 0;
                    }
                    for wire in &plan.wires {
                        let ComponentState::Wire(state) = &mut backend.state.components[wire.node.index()] else {
                            unreachable!()
                        };
                        state.power = old.saturating_sub(wire.attenuation);
                    }
                    let recorder = backend.classic_proof.as_mut().unwrap();
                    recorder.signal = new;
                    recorder.samples.clear();
                    backend.counts = Default::default();
                    backend.evaluate_seed(entry.wire, None).map_err(|error| error.to_string())?;
                    let work = backend.counts.callbacks as usize
                        + backend.counts.wire_walks as usize
                        + plan.wires.len()
                        + plan.guarded_wires.len()
                        + plan.outside.len();
                    proof_work = proof_work.saturating_add(work);
                    self.work = self.work.saturating_add(work);
                    if proof_work > CLASSIC_PROOF_WORK || self.work > CLASSIC_PROOF_WORK * 16 {
                        return Err("classic reference proof work budget".into());
                    }
                    for wire in &plan.wires {
                        if backend.wire_state(wire.node).power != new.saturating_sub(wire.attenuation) {
                            return Err("seed/Turbo transfer fails entry induction".into());
                        }
                    }
                    if !backend.classic_outside_zero(&plan.outside)
                        || plan.guarded_wires.iter().any(|node| {
                            backend.dirty[node.index()] || backend.state.components[node.index()] != self.baseline.components[node.index()]
                        })
                    {
                        return Err("guarded callback wire changed during reference episode".into());
                    }
                    let mut samples = backend.classic_proof.as_ref().unwrap().samples.clone();
                    if !entry.commit {
                        if !samples.is_empty() || backend.counts.wire_walks != 0 {
                            return Err("stable entry is not inert for every committed strength".into());
                        }
                        continue;
                    }
                    compact_classic_samples(&mut samples);
                    let pair = ((old as usize) << 4) | new as usize;
                    if checking {
                        let selected = ops
                            .iter()
                            .filter(|op| op.when[pair / 64] & (1u64 << (pair % 64)) != 0)
                            .map(|op| (op.receiver, op.sample))
                            .collect::<Vec<_>>();
                        if selected != samples {
                            return Err("compact operations differ from ordered reference samples".into());
                        }
                        continue;
                    }
                    let mut cursor = 0;
                    for (receiver, sample) in samples {
                        let found = ops[cursor..]
                            .iter()
                            .position(|op| op.receiver == receiver && op.sample == sample)
                            .map(|offset| cursor + offset);
                        let position = found.unwrap_or_else(|| {
                            ops.push(ClassicOp {
                                receiver,
                                sample,
                                when: [0; 4],
                            });
                            ops.len() - 1
                        });
                        ops[position].when[pair / 64] |= 1u64 << (pair % 64);
                        cursor = position + 1;
                    }
                    if ops.len() > operation_limit {
                        return Err("boundary program does not compact within operation bound".into());
                    }
                }
            }
            entry.ops = ops.into_boxed_slice();
        }
        Ok(())
    }
}

impl GraphBackend {
    #[cold]
    pub(super) fn record_classic_sample(&mut self, node: NodeId, captured: Option<&ObservedSite>) -> Result<(), GraphExecutionError> {
        let delayed = matches!(
            self.program.nodes()[node.index()].kind,
            CircuitNodeKind::Electrical(NodeType::Torch | NodeType::Repeater { .. } | NodeType::Comparator { .. })
        );
        if !delayed {
            if matches!(
                self.program.nodes()[node.index()].kind,
                CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::PressurePlate)
            ) {
                return Ok(());
            }
            return Err(self.unsupported(node, "unowned classic proof callback"));
        }
        if !self.classic_proof.as_ref().unwrap().receivers.contains(&node) {
            return Err(self.unsupported(node, "unrepresented classic proof receiver"));
        }
        let comparator = matches!(
            self.program.nodes()[node.index()].kind,
            CircuitNodeKind::Electrical(NodeType::Comparator { .. })
        );
        if !comparator
            && let Some(ObservedSite {
                properties: ObservedProperties::Electrical { powered, locked },
                ..
            }) = captured
        {
            if *powered != self.node_state(node).powered || *locked != self.node_state(node).repeater_locked {
                return Err(self.unsupported(node, "classic captured receiver state differs from entry state"));
            }
        }
        let sample = if comparator {
            ClassicSample::Comparator {
                strength: self.comparator_output(node),
                powered: self.comparator_powered(node),
            }
        } else {
            ClassicSample::Boolean {
                nonzero: self.input_strength(node, InputPort::Main) > 0,
            }
        };
        self.classic_proof.as_mut().unwrap().samples.push((node, sample));
        Ok(())
    }

    fn classic_outside_zero(&self, inputs: &[ClassicOutside]) -> bool {
        inputs.iter().all(|input| {
            let strength = match &self.state.components[input.node.index()] {
                ComponentState::Electrical(state) => state.output_strength,
                ComponentState::Wire(_) => self.wire_state(input.node).power,
                ComponentState::Specialized => 0,
            };
            strength.saturating_sub(input.attenuation) == 0
        })
    }

    /// Check before any kernel effect. The certificate proves both transfer modes
    /// identical and the consulted electrical state independent of private writes.
    fn classic_guard_wires(&mut self, nodes: &[NodeId]) -> bool {
        for &node in nodes {
            let plan = self.wire_plan(node).clone();
            let wire = self.wire_state(node);
            let raw = self.raw_wire_sides(&plan, wire);
            if crate::redstone::wire::regulate_sides(wire, raw) != wire
                || BlockFace::values()
                    .into_iter()
                    .any(|face| crate::redstone::wire::on_neighbor_changed_from(wire, raw, face) != wire)
                || self.seed_power(&plan).power != wire.power
            {
                return false;
            }
        }
        true
    }

    /// Materialization is a one-way barrier before the original callback writes.
    /// It preserves the old committed values, never the source's newest output.
    pub(super) fn deopt_classic_net(&mut self, net: usize) {
        if !self.state.classic_active[net] {
            return;
        }
        let signal = self.state.classic_signals[net];
        for index in self.program.classic().nets[net].wires.clone() {
            let wire = self.program.classic().wires[index];
            let ComponentState::Wire(state) = &mut self.state.components[wire.node.index()] else {
                unreachable!()
            };
            state.power = signal.saturating_sub(wire.attenuation);
            self.invalidate_wire_programs(wire.node);
        }
        self.state.classic_active[net] = false;
    }

    /// False asks the caller to execute the original occurrence in the same frame.
    pub(super) fn run_classic_entry(&mut self, index: usize) -> bool {
        let entry = &self.program.classic().entries[index];
        let net = entry.net as usize;
        if !self.state.classic_active[net] {
            return false;
        }
        if !entry.commit {
            return true;
        }
        let ops = entry.ops.clone();
        let outside = self.program.classic().nets[net].outside.clone();
        if !self.classic_outside_zero(&self.program.classic().outside[outside]) {
            self.deopt_classic_net(net);
            return false;
        }
        let guards = self.program.classic().nets[net].guarded_wires.clone();
        for index in guards {
            let node = self.program.classic().guarded_wires[index];
            if !self.classic_guard_wires(&[node]) {
                self.deopt_classic_net(net);
                return false;
            }
        }
        let source = self.program.classic().nets[net].source;
        let old = self.state.classic_signals[net];
        let new = self.node_state(source).output_strength;
        let pair = ((old as usize) << 4) | new as usize;
        self.state.classic_signals[net] = new;
        self.counts.classic_net_invocations += 1;
        for index in ops {
            let op = self.program.classic().ops[index];
            if op.when[pair / 64] & (1u64 << (pair % 64)) == 0 {
                continue;
            }
            self.counts.classic_receiver_ops += 1;
            self.classic_schedule(op.receiver, op.sample);
        }
        if old != new {
            let wires = self.program.classic().nets[net].wires.clone();
            for index in wires {
                let node = self.program.classic().wires[index].node;
                self.dirty[node.index()] = true;
                self.invalidate_wire_programs(node);
            }
        }
        true
    }

    fn classic_schedule(&mut self, node: NodeId, sample: ClassicSample) {
        if self.is_pending(node) {
            return;
        }
        let current = self.node_state(node);
        if let ClassicSample::Comparator { strength, powered } = sample {
            if strength != current.output_strength || powered != current.powered {
                let facing_diode = matches!(
                    self.program.nodes()[node.index()].kind,
                    CircuitNodeKind::Electrical(NodeType::Comparator { facing_diode: true, .. })
                );
                self.schedule(node, 1, if facing_diode { TickPriority::High } else { TickPriority::Normal });
            }
            return;
        }
        let ClassicSample::Boolean { nonzero } = sample else { unreachable!() };
        let powered = current.powered;
        match self.program.nodes()[node.index()].kind {
            CircuitNodeKind::Electrical(NodeType::Torch) if powered == nonzero => {
                self.schedule(node, 1, TickPriority::Normal);
            }
            CircuitNodeKind::Electrical(NodeType::Repeater { delay, facing_diode }) if powered != nonzero => {
                self.schedule(
                    node,
                    delay as usize,
                    if facing_diode {
                        TickPriority::Highest
                    } else if !nonzero {
                        TickPriority::Higher
                    } else {
                        TickPriority::High
                    },
                );
            }
            _ => {}
        }
    }
}
