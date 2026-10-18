//! Direct receiver scheduling and cold reference replay used only at compilation.
use super::*;

pub(super) struct ProofRecorder {
    pub(super) source: NodeId,
    pub(super) signal: u8,
    receivers: Vec<NodeId>,
    samples: Vec<(NodeId, bool)>,
}

pub(crate) struct ClassicReference {
    backend: GraphBackend,
    saved: Vec<(NodeId, ComponentState)>,
    work: usize,
}

impl ClassicReference {
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
        Ok(Self {
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
        for (node, state) in self.saved.drain(..) {
            self.backend.state.components[node.index()] = state;
        }
        self.backend.dirty.fill(false);
        self.backend.wire_walks.clear();
        self.backend.future.scheduler = Default::default();
        self.backend.future.pending.clear();
        result.map(|()| self.work - before)
    }

    fn prove_inner(&mut self, plan: &mut ClassicNetPlan, monitor: &TaskMonitor, checking: bool) -> Result<(), String> {
        let mut proof_work = 0usize;
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
                && (ops.len() > plan.inputs.len() * 4 + 8
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
                    let work = backend.counts.callbacks as usize + backend.counts.wire_walks as usize + plan.wires.len();
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
                    let mut samples = backend.classic_proof.as_ref().unwrap().samples.clone();
                    if !entry.commit {
                        if !samples.is_empty() || backend.counts.wire_walks != 0 {
                            return Err("stable entry is not inert for every committed strength".into());
                        }
                        continue;
                    }
                    let mut seen = FxHashSet::default();
                    samples.retain(|sample| seen.insert(*sample));
                    let pair = ((old as usize) << 4) | new as usize;
                    if checking {
                        let selected = ops
                            .iter()
                            .filter(|op| op.when[pair / 64] & (1u64 << (pair % 64)) != 0)
                            .map(|op| (op.receiver, op.nonzero))
                            .collect::<Vec<_>>();
                        if selected != samples {
                            return Err("compact operations differ from ordered reference samples".into());
                        }
                        continue;
                    }
                    let mut cursor = 0;
                    for (receiver, nonzero) in samples {
                        let found = ops[cursor..]
                            .iter()
                            .position(|op| op.receiver == receiver && op.nonzero == nonzero)
                            .map(|offset| cursor + offset);
                        let position = found.unwrap_or_else(|| {
                            ops.push(ClassicOp {
                                receiver,
                                nonzero,
                                when: [0; 4],
                            });
                            ops.len() - 1
                        });
                        ops[position].when[pair / 64] |= 1u64 << (pair % 64);
                        cursor = position + 1;
                    }
                    if ops.len() > plan.inputs.len() * 4 + 8 {
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
            CircuitNodeKind::Electrical(NodeType::Torch | NodeType::Repeater { .. })
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
        if let Some(ObservedSite {
            properties: ObservedProperties::Electrical { powered, locked },
            ..
        }) = captured
        {
            if *powered != self.node_state(node).powered || *locked != self.node_state(node).repeater_locked {
                return Err(self.unsupported(node, "classic captured receiver state differs from entry state"));
            }
        }
        let nonzero = self.input_strength(node, InputPort::Main) > 0;
        self.classic_proof.as_mut().unwrap().samples.push((node, nonzero));
        Ok(())
    }

    pub(super) fn run_classic_entry(&mut self, index: usize) {
        let entry = &self.program.classic().entries[index];
        if !entry.commit {
            return;
        }
        let net = entry.net as usize;
        let ops = entry.ops.clone();
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
            self.classic_schedule(op.receiver, op.nonzero);
        }
        if old != new {
            let wires = self.program.classic().nets[net].wires.clone();
            for index in wires {
                self.dirty[self.program.classic().wires[index].node.index()] = true;
            }
        }
    }

    fn classic_schedule(&mut self, node: NodeId, nonzero: bool) {
        if self.is_pending(node) {
            return;
        }
        let powered = self.node_state(node).powered;
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
