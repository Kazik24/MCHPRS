//! Stable-device execution: a shared Boolean data path and a small timing island.
use super::*;
use crate::redpiler::program_graph::settled_machine::SettledMachine;

pub(super) struct LogicalDevice {
    machine: SettledMachine,
    external: Vec<bool>,
    nodes: Box<[bool]>,
    actors: Box<[bool]>,
    outputs: Box<[(NodeId, usize)]>,
    clock: PistonId,
    sampled: bool,
}

impl GraphBackend {
    pub(super) fn install_logical_device(&mut self) -> Result<(), String> {
        let Some(circuit) = self.program.settled_circuit() else { return Ok(()) };
        let Some(timing) = &circuit.timing else { return Ok(()) };
        if !self.future.pending.is_empty() {
            return Err("logical device requires a stationary compilation entry".into());
        }
        let mut nodes = vec![false; self.program.nodes().len()];
        let mut actors = vec![false; self.program.pistons().len()];
        for node in &timing.nodes {
            nodes[node.index()] = true;
        }
        for actor in &timing.actors {
            actors[actor.index()] = true;
        }
        let inputs = circuit
            .program
            .inputs
            .iter()
            .map(|input| self.sample_settled_input(input.source))
            .collect::<Result<Vec<_>, _>>()?;
        let machine = SettledMachine::new(circuit, &inputs)?;
        for input in machine.external_inputs() {
            let LayerInputSource::SourceAbove { site, .. } = input.source else {
                return Err(format!("logical device has unresolved physical input {:?}", input.source));
            };
            if !matches!(self.program.sites()[site.index()].source, SiteSource::Component(node) if nodes[node.index()]) {
                return Err(format!("logical device reads outside its timing island: {:?}", input.source));
            }
        }
        let outputs = circuit
            .outputs
            .iter()
            .enumerate()
            .filter_map(|(index, output)| match output.target {
                SettledTarget::Power(PowerTarget::Input { node, port: InputPort::Main }) => Some((node, index)),
                _ => None,
            })
            .collect::<Vec<_>>();
        self.logical_device = Some(LogicalDevice {
            external: vec![false; machine.external_inputs().len()],
            machine,
            nodes: nodes.into_boxed_slice(),
            actors: actors.into_boxed_slice(),
            outputs: outputs.into_boxed_slice(),
            clock: timing.clock,
            sampled: false,
        });
        Ok(())
    }

    pub(super) fn logical_node_allowed(&self, node: NodeId) -> bool {
        self.logical_device.as_ref().is_none_or(|device| device.nodes[node.index()])
    }

    pub(super) fn logical_actor_allowed(&self, actor: PistonId) -> bool {
        self.logical_device.as_ref().is_none_or(|device| device.actors[actor.index()])
    }

    pub(super) fn logical_receiver_allowed(&self, receiver: CallbackReceiver) -> bool {
        match receiver {
            CallbackReceiver::Component(node) => self.logical_node_allowed(node),
            CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor) => self.logical_actor_allowed(actor),
            CallbackReceiver::Contact(_) => self.logical_device.is_none(),
        }
    }

    pub(super) fn logical_clock_action(&mut self, request: PistonRequest) -> Result<(), GraphExecutionError> {
        if !self.logical_device.as_ref().is_some_and(|device| device.clock == request.piston) {
            return Ok(());
        }
        let mut device = self.logical_device.take().unwrap();
        let result = (|| {
            match request.action {
                mchprs_world::PistonAction::Retract => {
                    for (slot, input) in device.external.iter_mut().zip(device.machine.external_inputs()) {
                        *slot = self.sample_settled_input(input.source).expect("validated logical device input");
                    }
                    let operations = device.machine.step(&device.external).expect("validated logical device inputs");
                    self.counts.logical_kernel_evaluations += 1;
                    self.counts.logical_kernel_operations += operations as u64;
                    device.sampled = true;
                }
                mchprs_world::PistonAction::Extend if device.sampled => {
                    for &(node, output) in &device.outputs {
                        let powered = device.machine.outputs()[output] != 0;
                        self.set_node(node, powered, 15 * u8::from(powered), None, false)?;
                    }
                    device.sampled = false;
                }
                _ => {}
            }
            Ok(())
        })();
        self.logical_device = Some(device);
        result
    }
}
