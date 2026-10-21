//! O6 data and timing kernels. Physical topology is used only at activation/projection.
use super::logical_clock::LogicalClock;
use super::*;
use crate::redpiler::program_graph::settled_machine::SettledMachine;
use crate::redpiler::program_graph::{SettledEvaluation, SettledValueKind};

const MAX_FRAME_PASSES: usize = 128;

struct Register {
    storage: PistonId,
    resource: ResourceId,
    near: ContactId,
    far: ContactId,
    source: ContactId,
    output: usize,
}

#[derive(Clone, Copy)]
enum FrameBoundary {
    Power {
        slot: usize,
        source: LayerInputSource,
        clock_node: Option<NodeId>,
        threshold: u8,
    },
    ClockState {
        slot: usize,
        test: StateTest,
    },
}

#[derive(Clone, Copy)]
struct FeedbackSlot {
    slot: usize,
    output: usize,
    polarity: bool,
}

struct FeedbackResource {
    resource: ResourceId,
    source: ContactId,
    other: ContactId,
    output: usize,
}

struct SettledFrame {
    evaluation: SettledEvaluation,
    inputs: Box<[bool]>,
    boundaries: Box<[FrameBoundary]>,
    feedback_slots: Box<[FeedbackSlot]>,
    feedback_resources: Box<[FeedbackResource]>,
    outputs: Box<[(NodeId, usize, SettledValueKind)]>,
    button_nodes: Box<[NodeId]>,
}

pub(super) struct LogicalDevice {
    machine: Option<SettledMachine>,
    frame: Option<SettledFrame>,
    clock: LogicalClock,
    trigger: NodeId,
    external: Box<[bool]>,
    external_nodes: Box<[NodeId]>,
    outputs: Box<[(NodeId, usize)]>,
    registers: Box<[Register]>,
}

impl SettledFrame {
    fn new(backend: &GraphBackend, circuit: &SettledCircuit, clock: &LogicalClock) -> Result<Self, String> {
        if !circuit.declines.is_empty() || !circuit.retained_effects.is_empty() {
            return Err("logical frame requires a complete circuit without retained effects".into());
        }

        let mut feedback_outputs = FxHashMap::<ResourceId, (ContactId, usize)>::default();
        let mut outputs = Vec::new();
        for (index, output) in circuit.outputs.iter().enumerate() {
            match output.target {
                SettledTarget::Power(PowerTarget::Input { node, port: InputPort::Main }) => outputs.push((node, index, output.kind)),
                SettledTarget::ResourceState { resource, source } => {
                    if output.kind != SettledValueKind::Bool {
                        return Err("logical frame feedback output must be Boolean".into());
                    }
                    let contacts = &backend.program.resources()[resource.index()].contacts;
                    if contacts.len() < 2 || !contacts.contains(&source) {
                        return Err("logical frame feedback must include its canonical source contact".into());
                    }
                    if feedback_outputs.insert(resource, (source, index)).is_some() {
                        return Err("logical frame resource has multiple next-state outputs".into());
                    }
                }
                _ => return Err("logical frame has an unsupported output target".into()),
            }
        }

        let mut boundaries = Vec::new();
        let mut button_nodes = FxHashSet::default();
        let mut feedback_slots = Vec::new();
        let mut feedback_resources = Vec::new();
        let mut feedback_seen = FxHashSet::default();
        for (slot, input) in circuit.program.inputs.iter().enumerate() {
            match input.source {
                LayerInputSource::State(StateTest::ResourceAt { resource, contact })
                | LayerInputSource::BoundaryState {
                    test: StateTest::ResourceAt { resource, contact },
                    ..
                } => {
                    let Some(&(source, output)) = feedback_outputs.get(&resource) else {
                        return Err("logical frame has an unbound resource-state input".into());
                    };
                    let contacts = &backend.program.resources()[resource.index()].contacts;
                    if !contacts.contains(&contact) {
                        return Err("logical frame reads a non-member feedback contact".into());
                    }
                    // shortcut: retain canonical-source occupancy, not which of up to five near owners wins.
                    feedback_slots.push(FeedbackSlot {
                        slot,
                        output,
                        polarity: contact == source,
                    });
                    if feedback_seen.insert(resource) {
                        let other = contacts.iter().copied().find(|&candidate| candidate != source).unwrap();
                        feedback_resources.push(FeedbackResource {
                            resource,
                            source,
                            other,
                            output,
                        });
                    }
                }
                LayerInputSource::State(test) | LayerInputSource::BoundaryState { test, .. } => {
                    if clock.state_value(test).is_none() {
                        return Err(format!("logical frame has an unbound external state input: {test:?}"));
                    }
                    boundaries.push(FrameBoundary::ClockState { slot, test });
                }
                LayerInputSource::SourceAbove { site, port, threshold } => {
                    let site_spec = &backend.program.sites()[site.index()];
                    let clock_node = match site_spec.source {
                        SiteSource::Component(node) if clock.source_value(node).is_some() => Some(node),
                        SiteSource::Component(node) => {
                            match backend.program.nodes()[node.index()].kind {
                                CircuitNodeKind::Electrical(NodeType::Button) => {
                                    button_nodes.insert(node);
                                }
                                CircuitNodeKind::Electrical(NodeType::Lever | NodeType::PressurePlate) => {}
                                _ => {
                                    return Err(format!(
                                        "logical frame external power source is not a lever, button, or pressure plate: {node:?}"
                                    ));
                                }
                            }
                            None
                        }
                        SiteSource::Constant(_) => None,
                        SiteSource::Contact(contact) => {
                            return Err(format!("logical frame has an unbound contact power input: {contact:?}"));
                        }
                    };
                    if clock_node.is_some() && !matches!(port, PowerPort::Output | PowerPort::Weak(_) | PowerPort::Strong(_)) {
                        return Err("logical frame clock boundary requires a direct component power port".into());
                    }
                    boundaries.push(FrameBoundary::Power {
                        slot,
                        source: input.source,
                        clock_node,
                        threshold,
                    });
                }
                other => return Err(format!("logical frame has an unsupported input binding: {other:?}")),
            }
        }
        if feedback_seen.len() != feedback_outputs.len() {
            return Err("logical frame has feedback output without a matching input".into());
        }

        Ok(Self {
            evaluation: SettledEvaluation::new(circuit)?,
            inputs: vec![false; circuit.program.inputs.len()].into_boxed_slice(),
            boundaries: boundaries.into_boxed_slice(),
            feedback_slots: feedback_slots.into_boxed_slice(),
            feedback_resources: feedback_resources.into_boxed_slice(),
            outputs: outputs.into_boxed_slice(),
            button_nodes: button_nodes.into_iter().collect::<Vec<_>>().into_boxed_slice(),
        })
    }

    fn evaluate(&mut self) -> Result<usize, &'static str> {
        for slot in &self.feedback_slots {
            self.inputs[slot.slot] = slot.polarity;
        }
        let mut operations = 0;
        for _ in 0..MAX_FRAME_PASSES {
            operations += self.evaluation.evaluate(&self.inputs)?;
            let mut changed = false;
            for slot in &self.feedback_slots {
                let next = (self.evaluation.outputs()[slot.output] != 0) == slot.polarity;
                changed |= self.inputs[slot.slot] != next;
                self.inputs[slot.slot] = next;
            }
            if !changed {
                return Ok(operations);
            }
        }
        Err("logical frame feedback did not converge within 128 passes")
    }
}

impl GraphBackend {
    pub(super) fn install_logical_device(&mut self) -> Result<(), String> {
        let Some(circuit) = self.program.settled_circuit() else { return Ok(()) };
        let Some(timing) = &circuit.timing else { return Ok(()) };
        let is_frame = circuit
            .outputs
            .iter()
            .any(|output| matches!(output.target, SettledTarget::ResourceState { .. }));
        if !self.future.pending.is_empty() || !self.future.piston_requests.is_empty() || !self.future.deferred_pistons.is_empty() {
            // Keep entry-time graph work on the ordinary executor; never abandon a live queue.
            #[cfg(test)]
            eprintln!("logical device activation declined: pending graph work; frame={is_frame}");
            return Ok(());
        }
        if is_frame && self.future.scheduler.iter().next().is_some() {
            #[cfg(test)]
            eprintln!("logical device activation declined: scheduled work; frame=true");
            return Ok(());
        }
        let clock = match LogicalClock::new(&self.program, &self.state, timing) {
            Ok(clock) => clock,
            Err(_error) if is_frame => {
                #[cfg(test)]
                eprintln!("logical device activation declined: clock: {_error}");
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        if is_frame {
            let frame = match SettledFrame::new(self, circuit, &clock) {
                Ok(frame) => frame,
                Err(_error) => {
                    #[cfg(test)]
                    eprintln!("logical device activation declined: frame: {_error}");
                    return Ok(());
                }
            };
            self.logical_device = Some(LogicalDevice {
                machine: None,
                frame: Some(frame),
                external: Box::new([]),
                external_nodes: Box::new([]),
                outputs: Box::new([]),
                registers: Box::new([]),
                clock,
                trigger: timing.trigger,
            });
            return Ok(());
        }

        let inputs = circuit
            .program
            .inputs
            .iter()
            .map(|input| self.sample_settled_input(input.source))
            .collect::<Result<Vec<_>, _>>()?;
        let machine = SettledMachine::new(circuit, &inputs)?;
        let mut external_nodes = Vec::new();
        for input in machine.external_inputs() {
            let LayerInputSource::SourceAbove { site, .. } = input.source else {
                return Err(format!("logical device has unresolved physical input {:?}", input.source));
            };
            let SiteSource::Component(node) = self.program.sites()[site.index()].source else {
                return Err("logical device has a non-component boundary source".into());
            };
            if clock.source_value(node).is_none() {
                return Err(format!("logical device reads outside its clock kernel: {node:?}"));
            }
            external_nodes.push(node);
        }
        let mut outputs = Vec::new();
        let mut registers = Vec::new();
        for (index, output) in circuit.outputs.iter().enumerate() {
            match output.target {
                SettledTarget::Power(PowerTarget::Input { node, port: InputPort::Main }) => outputs.push((node, index)),
                SettledTarget::Register(read) => {
                    let spec = self.program.pistons()[read.storage.index()];
                    let PistonLane::SingleResource { far } = spec.lane else {
                        return Err("logical register has no canonical lane".into());
                    };
                    registers.push(Register {
                        storage: read.storage,
                        resource: read.resource,
                        near: spec.near,
                        far,
                        source: read.source,
                        output: index,
                    });
                }
                _ => return Err("logical device has an unsupported public output".into()),
            }
        }
        self.logical_device = Some(LogicalDevice {
            external: vec![false; external_nodes.len()].into_boxed_slice(),
            external_nodes: external_nodes.into_boxed_slice(),
            machine: Some(machine),
            frame: None,
            clock,
            trigger: timing.trigger,
            outputs: outputs.into_boxed_slice(),
            registers: registers.into_boxed_slice(),
        });
        Ok(())
    }

    pub(super) fn tick_logical_device(&mut self) -> Result<(), GraphExecutionError> {
        let mut device = self.logical_device.take().expect("installed logical device");
        self.counts.ticks += 1;
        self.future.clock.logical_tick += 1;
        let result = (|| {
            if let Some(frame) = &device.frame {
                self.advance_frame_scheduler(device.trigger, &frame.button_nodes)?;
            }
            let step = device
                .clock
                .advance(self.node_state(device.trigger).powered)
                .map_err(|kind| self.unsupported(device.trigger, kind))?;
            if let Some(frame) = &mut device.frame {
                if step.compute {
                    for boundary in frame.boundaries.iter().copied() {
                        let (slot, value) = match boundary {
                            FrameBoundary::Power {
                                slot,
                                source: _,
                                clock_node: Some(node),
                                threshold,
                                ..
                            } => (slot, device.clock.source_value(node).is_some_and(|powered| powered && 15 > threshold)),
                            FrameBoundary::Power { slot, source, .. } => (
                                slot,
                                self.sample_settled_input(source).map_err(|kind| self.unsupported(device.trigger, kind))?,
                            ),
                            FrameBoundary::ClockState { slot, test } => (
                                slot,
                                device
                                    .clock
                                    .state_value(test)
                                    .ok_or_else(|| self.unsupported(device.trigger, "unbound logical clock state"))?,
                            ),
                        };
                        frame.inputs[slot] = value;
                    }
                    let operations = frame.evaluate().map_err(|kind| self.unsupported(device.trigger, kind))?;
                    self.counts.logical_kernel_evaluations += 1;
                    self.counts.logical_kernel_operations += operations as u64;
                    for feedback in &frame.feedback_resources {
                        let at = if frame.evaluation.outputs()[feedback.output] != 0 {
                            feedback.source
                        } else {
                            feedback.other
                        };
                        let resource = &mut self.state.resources[feedback.resource.index()];
                        if resource.at != at || !resource.enabled {
                            resource.revision = resource.revision.wrapping_add(1);
                        }
                        resource.at = at;
                        resource.enabled = true;
                    }
                    for &(node, output, kind) in &frame.outputs {
                        let value = frame.evaluation.outputs()[output];
                        let strength = match kind {
                            SettledValueKind::Bool => 15 * u8::from(value != 0),
                            SettledValueKind::Strength => value,
                        };
                        if matches!(self.program.nodes()[node.index()].kind, CircuitNodeKind::Wire(_)) {
                            let mut wire = self.wire_state(node);
                            wire.power = strength;
                            self.set_wire(node, wire);
                        } else {
                            self.set_node(node, strength != 0, strength, None, false)?;
                        }
                    }
                }
            } else if step.compute {
                let machine = device.machine.as_mut().expect("register machine installed");
                for (value, &node) in device.external.iter_mut().zip(device.external_nodes.iter()) {
                    *value = device.clock.source_value(node).expect("validated logical device input");
                }
                let operations = machine.step(&device.external).expect("validated logical device inputs");
                self.counts.logical_kernel_evaluations += 1;
                self.counts.logical_kernel_operations += operations as u64;
            }
            if step.publish && device.frame.is_none() {
                let machine = device.machine.as_ref().expect("register machine installed");
                for &(node, output) in &device.outputs {
                    let strength = 15 * u8::from(machine.outputs()[output] != 0);
                    self.set_node(node, strength != 0, strength, None, false)?;
                }
            }
            if step.commit && device.frame.is_none() {
                let machine = device.machine.as_ref().expect("register machine installed");
                for register in &device.registers {
                    let at = if machine.outputs()[register.output] != 0 {
                        register.source
                    } else if register.source == register.far {
                        register.near
                    } else {
                        register.far
                    };
                    let piston = &mut self.state.pistons[register.storage.index()];
                    piston.extended = at == register.far;
                    piston.head_present = piston.extended;
                    piston.availability = PistonAvailability::Ready;
                    let resource = &mut self.state.resources[register.resource.index()];
                    resource.at = at;
                    resource.enabled = true;
                }
            }
            Ok(())
        })();
        self.logical_device = Some(device);
        result
    }

    pub(super) fn advance_frame_scheduler(&mut self, trigger: NodeId, button_nodes: &[NodeId]) -> Result<(), GraphExecutionError> {
        for (tick, _, _) in self.future.scheduler.iter() {
            if tick.registry_type == tick.current_registry_type && !tick.node.is_some_and(|node| button_nodes.contains(&node)) {
                return Err(self.unsupported(trigger, "logical frame has an unexpected scheduled tick"));
            }
        }
        self.drain_frame_button_ticks(trigger, button_nodes)?;
        self.future.scheduler.end_last_tick_move_next();
        self.drain_frame_button_ticks(trigger, button_nodes)
    }

    fn drain_frame_button_ticks(&mut self, trigger: NodeId, button_nodes: &[NodeId]) -> Result<(), GraphExecutionError> {
        while let Some(tick) = self.future.scheduler.this_tick().pop_first() {
            self.popped(tick);
            if tick.registry_type != tick.current_registry_type {
                continue;
            }
            let Some(node) = tick.node.filter(|node| button_nodes.contains(node)) else {
                return Err(self.unsupported(trigger, "logical frame has an unexpected scheduled tick"));
            };
            self.counts.scheduled_ticks += 1;
            if self.node_state(node).powered {
                self.set_node(node, false, 0, None, false)?;
            }
        }
        Ok(())
    }
}
