//! O6 data and timing kernels. Physical topology is used only at activation/projection.
use super::logical_clock::LogicalClock;
use super::*;
use crate::redpiler::program_graph::settled_machine::SettledMachine;

struct Register {
    storage: PistonId,
    resource: ResourceId,
    near: ContactId,
    far: ContactId,
    source: ContactId,
    output: usize,
}

pub(super) struct LogicalDevice {
    // shortcut: interpreter reconstruction is unfinished; this O6 device path remains experimental.
    machine: SettledMachine,
    clock: LogicalClock,
    trigger: NodeId,
    external: Box<[bool]>,
    external_nodes: Box<[NodeId]>,
    outputs: Box<[(NodeId, usize)]>,
    registers: Box<[Register]>,
}

impl GraphBackend {
    pub(super) fn install_logical_device(&mut self) -> Result<(), String> {
        let Some(circuit) = self.program.settled_circuit() else { return Ok(()) };
        let Some(timing) = &circuit.timing else { return Ok(()) };
        if !self.future.pending.is_empty() {
            return Err("logical device requires a stationary compilation entry".into());
        }
        let clock = LogicalClock::new(&self.program, &self.state, timing)?;
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
            machine,
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
            let step = device
                .clock
                .advance(self.node_state(device.trigger).powered)
                .map_err(|kind| self.unsupported(device.trigger, kind))?;
            if step.compute {
                for (value, &node) in device.external.iter_mut().zip(device.external_nodes.iter()) {
                    *value = device.clock.source_value(node).expect("validated clock kernel input");
                }
                let operations = device.machine.step(&device.external).expect("validated logical device inputs");
                self.counts.logical_kernel_evaluations += 1;
                self.counts.logical_kernel_operations += operations as u64;
            }
            if step.publish {
                for &(node, output) in &device.outputs {
                    let powered = device.machine.outputs()[output] != 0;
                    self.set_node(node, powered, 15 * u8::from(powered), None, false)?;
                }
            }
            if step.commit {
                for register in &device.registers {
                    let at = if device.machine.outputs()[register.output] != 0 {
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
}
