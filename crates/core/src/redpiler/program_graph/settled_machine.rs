//! Logical computation steps over caller-sampled boundaries and persistent registers.
use super::*;

/// The caller owns clock cadence, output publication and retained outside effects.
#[derive(Debug)]
pub(crate) struct SettledMachine {
    evaluation: SettledEvaluation,
    inputs: Box<[bool]>,
    external_inputs: Box<[LayerInput]>,
    external_slots: Box<[usize]>,
    register_writes: Box<[(usize, usize)]>,
}

impl SettledMachine {
    pub(crate) fn new(circuit: &SettledCircuit, initial_inputs: &[bool]) -> Result<Self, String> {
        if initial_inputs.len() != circuit.program.inputs.len() {
            return Err("logical input count does not match the program".into());
        }
        if !circuit.declines.is_empty() {
            return Err("a partial settled circuit cannot execute as a complete machine".into());
        }
        let mut registers = FxHashMap::default();
        for (index, output) in circuit.outputs.iter().enumerate() {
            let (resource, source) = match output.target {
                SettledTarget::Register(read) => (read.resource, read.source),
                SettledTarget::ResourceState { resource, source } => (resource, source),
                SettledTarget::Power(_) | SettledTarget::ResetExcludedPower { .. } => continue,
                #[cfg(test)]
                SettledTarget::ComponentOutput(_) => continue,
            };
            if output.kind != SettledValueKind::Bool {
                return Err("logical register output must be Boolean".into());
            }
            if registers.insert(resource, (source, index)).is_some() {
                return Err("logical register has multiple next-state writes".into());
            }
        }
        let mut external_inputs = Vec::new();
        let mut external_slots = Vec::new();
        let mut register_writes = Vec::new();
        for (slot, &input) in circuit.program.inputs.iter().enumerate() {
            if let LayerInputSource::State(StateTest::ResourceAt { resource, contact }) = input.source
                && let Some(&(source, output)) = registers.get(&resource)
            {
                if contact != source {
                    return Err("logical register input is not its canonical source contact".into());
                }
                register_writes.push((slot, output));
            } else {
                external_inputs.push(input);
                external_slots.push(slot);
            }
        }
        Ok(Self {
            evaluation: SettledEvaluation::new(circuit)?,
            inputs: initial_inputs.to_vec().into_boxed_slice(),
            external_inputs: external_inputs.into_boxed_slice(),
            external_slots: external_slots.into_boxed_slice(),
            register_writes: register_writes.into_boxed_slice(),
        })
    }

    pub(crate) fn external_inputs(&self) -> &[LayerInput] {
        &self.external_inputs
    }

    /// Evaluate once from old register values, then commit all next-state bits.
    pub(crate) fn step(&mut self, external_values: &[bool]) -> Result<usize, &'static str> {
        let operations = self.observe(external_values)?;
        for &(slot, output) in &self.register_writes {
            self.inputs[slot] = self.evaluation.outputs()[output] != 0;
        }
        Ok(operations)
    }

    /// Refresh power outputs from a settled computation frame without committing writes.
    /// Clock/reset pulses belong to the caller's event adapter, not this data sample.
    pub(crate) fn observe(&mut self, external_values: &[bool]) -> Result<usize, &'static str> {
        if external_values.len() != self.external_slots.len() {
            return Err("logical boundary input count does not match the machine");
        }
        for (&slot, &value) in self.external_slots.iter().zip(external_values) {
            self.inputs[slot] = value;
        }
        self.evaluation.evaluate(&self.inputs)
    }

    pub(crate) fn outputs(&self) -> &[u8] {
        self.evaluation.outputs()
    }

    #[cfg(test)]
    pub(crate) fn input_values(&self) -> &[bool] {
        &self.inputs
    }

    #[cfg(test)]
    pub(crate) fn set_heap_reference_for_test(&mut self, enabled: bool) {
        self.evaluation.set_heap_reference_for_test(enabled);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn swap_registers() -> SettledCircuit {
        let inputs = (0..2)
            .map(|index| LayerInput {
                source: LayerInputSource::State(StateTest::ResourceAt {
                    resource: ResourceId::new(index),
                    contact: ContactId::new(index),
                }),
                role: LayerInputRole::RequestSample,
            })
            .collect::<Vec<_>>();
        let outputs = (0..2)
            .map(|index| SettledOutput {
                target: SettledTarget::Register(BudMemoryRead {
                    storage: PistonId::new(index),
                    reader: PistonId::new(index + 2),
                    resource: ResourceId::new(index),
                    source: ContactId::new(index),
                    link: PowerLink {
                        from: SiteId::new(index),
                        from_port: PowerPort::Output,
                        to: PowerTarget::Conductor {
                            site: SiteId::new(index),
                            include_dust: true,
                        },
                        face: BlockFace::Bottom,
                        attenuation: 0,
                        guard: GuardId::new(0),
                    },
                }),
                kind: SettledValueKind::Bool,
                roots: Box::new([LayerExprId(1 - index)]),
            })
            .collect::<Vec<_>>();
        SettledCircuit {
            program: ReducedLayerProgram {
                inputs: inputs.into_boxed_slice(),
                expressions: Box::new([LayerExpr::Input(LayerInputId(0)), LayerExpr::Input(LayerInputId(1))]),
                roots: Box::new([(LayerRoot::Output(0), LayerExprId(1)), (LayerRoot::Output(1), LayerExprId(0))]),
            },
            outputs: outputs.into_boxed_slice(),
            retained_cells: Box::new([]),
            retained_effects: Box::new([]),
            declines: Box::new([]),
            timing: None,
        }
    }

    #[test]
    fn logical_steps_commit_cross_coupled_registers_simultaneously() {
        let circuit = swap_registers();
        let mut machine = SettledMachine::new(&circuit, &[true, false]).unwrap();
        assert!(machine.external_inputs().is_empty());
        for _ in 0..3 {
            machine.step(&[]).unwrap();
            assert_eq!(machine.input_values(), &[false, true]);
            assert_eq!(machine.outputs(), &[0, 1]);
            machine.step(&[]).unwrap();
            assert_eq!(machine.input_values(), &[true, false]);
            assert_eq!(machine.outputs(), &[1, 0]);
        }
        assert!(machine.step(&[false]).is_err());
        assert_eq!(machine.input_values(), &[true, false], "invalid samples must not mutate register state");
    }

    #[test]
    fn resource_feedback_commits_simultaneously_and_rejects_aliased_writes() {
        let mut circuit = swap_registers();
        for output in &mut circuit.outputs {
            let SettledTarget::Register(read) = output.target else { unreachable!() };
            output.target = SettledTarget::ResourceState {
                resource: read.resource,
                source: read.source,
            };
        }
        let mut machine = SettledMachine::new(&circuit, &[true, false]).unwrap();
        assert!(machine.external_inputs().is_empty());
        machine.step(&[]).unwrap();
        assert_eq!(machine.input_values(), &[false, true]);
        machine.step(&[]).unwrap();
        assert_eq!(machine.input_values(), &[true, false]);
        circuit.outputs[1].target = swap_registers().outputs[0].target;
        assert!(SettledMachine::new(&circuit, &[true, false]).is_err());
    }

    #[test]
    fn observing_between_clock_edges_preserves_register_state_and_reuses_values() {
        let circuit = swap_registers();
        let mut machine = SettledMachine::new(&circuit, &[true, false]).unwrap();
        assert_eq!(machine.observe(&[]).unwrap(), 2);
        assert_eq!(machine.input_values(), &[true, false]);
        assert_eq!(machine.outputs(), &[0, 1]);
        assert_eq!(machine.observe(&[]).unwrap(), 0);
        assert_eq!(machine.step(&[]).unwrap(), 0);
        assert_eq!(machine.input_values(), &[false, true]);
        machine.observe(&[]).unwrap();
        assert_eq!(machine.input_values(), &[false, true]);
        assert_eq!(machine.outputs(), &[1, 0]);
    }

    #[test]
    fn logical_machine_rejects_partial_or_ambiguous_register_bindings() {
        let mut circuit = swap_registers();
        assert!(SettledMachine::new(&circuit, &[]).is_err());
        circuit.declines = Box::new(["unsupported output".into()]);
        assert!(SettledMachine::new(&circuit, &[false, false]).is_err());
        circuit.declines = Box::new([]);
        circuit.program.inputs[0].source = LayerInputSource::State(StateTest::ResourceAt {
            resource: ResourceId::new(0),
            contact: ContactId::new(2),
        });
        assert!(SettledMachine::new(&circuit, &[false, false]).is_err());
        circuit = swap_registers();
        circuit.outputs[1].target = circuit.outputs[0].target;
        assert!(SettledMachine::new(&circuit, &[false, false]).is_err());
    }
}
