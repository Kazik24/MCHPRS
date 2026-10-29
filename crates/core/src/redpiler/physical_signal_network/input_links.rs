//! Guarded, flattened power dependencies for component input reads.
use crate::redpiler::program_graph::{CircuitProgram, GuardId, InputPort, NodeId, PowerPort, PowerTarget, SiteId};

#[derive(Clone)]
pub(super) struct InputLink {
    pub(super) source: SiteId,
    pub(super) port: PowerPort,
    pub(super) attenuation: u8,
    pub(super) guards: Vec<GuardId>,
}

pub(super) struct InputLinks {
    inputs: Vec<Option<Vec<InputLink>>>,
}

impl InputLinks {
    pub(super) fn compile(program: &CircuitProgram) -> Self {
        let mut inputs = Vec::with_capacity(program.nodes().len() * 3);
        for node in 0..program.nodes().len() {
            for port in [InputPort::Main, InputPort::Side, InputPort::Quasi] {
                let mut links = Vec::new();
                let target = PowerTarget::Input {
                    node: NodeId::new(node as u32),
                    port,
                };
                let result = flatten(program, target, 0, &mut Vec::new(), &mut links, 0, &mut 0);
                inputs.push(result.then_some(links));
            }
        }
        Self { inputs }
    }

    pub(super) fn get(&self, node: usize, port: InputPort) -> Option<&[InputLink]> {
        let offset = match port {
            InputPort::Main => 0,
            InputPort::Side => 1,
            InputPort::Quasi => 2,
        };
        self.inputs.get(node.checked_mul(3)?.checked_add(offset)?)?.as_deref()
    }
}

fn flatten(
    program: &CircuitProgram,
    target: PowerTarget,
    attenuation: u8,
    guards: &mut Vec<GuardId>,
    result: &mut Vec<InputLink>,
    depth: usize,
    visited: &mut usize,
) -> bool {
    // Bound expansion as well as recursion; unsupported inputs use the original evaluator.
    *visited += 1;
    if depth > 32 || *visited > 4096 || result.len() > 4096 {
        return false;
    }
    for link in program.power_links_to(target) {
        guards.push(link.guard);
        let attenuation = attenuation.saturating_add(link.attenuation).min(15);
        let valid = if let PowerPort::Conducted { include_dust } = link.from_port {
            flatten(
                program,
                PowerTarget::Conductor {
                    site: link.from,
                    include_dust,
                },
                attenuation,
                guards,
                result,
                depth + 1,
                visited,
            )
        } else {
            result.push(InputLink {
                source: link.from,
                port: link.from_port,
                attenuation,
                guards: guards.clone(),
            });
            true
        };
        guards.pop();
        if !valid {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::backend::graph::{
        piston_probe::tests::{BOUNDS, LEVER, fixture},
        wire_probe::physical_program,
    };
    use crate::redpiler::physical_signal_network::{EntryWork, PhysicalSignalNetwork};
    use crate::redpiler::program_graph::PistonId;

    #[test]
    fn compiled_inputs_match_physical_reads_across_source_and_resource_changes() {
        let world = fixture();
        let (program, state, _) = physical_program(&world, BOUNDS).unwrap();
        let mut network = PhysicalSignalNetwork::compile(&program, &state, &EntryWork::capture(&world, BOUNDS)).unwrap();
        assert!((0..program.nodes().len()).any(|node| network.input_links.get(node, InputPort::Main).is_some_and(|links| !links.is_empty())));
        let resource = program.resource_for_piston(PistonId::new(0)).unwrap();
        let piston = &program.pistons()[0];
        for (powered, contact, enabled) in [
            (true, piston.near, true),
            (true, piston.far, false),
            (true, piston.far, true),
            (false, piston.near, false),
            (false, piston.near, true),
        ] {
            network.set_lever(LEVER, powered).unwrap();
            network.set_resource_contact(resource.index(), contact, enabled).unwrap();
            for node in 0..program.nodes().len() {
                for port in [InputPort::Main, InputPort::Side, InputPort::Quasi] {
                    assert_eq!(
                        network.input_power(node, port),
                        network.input_power_physical(node, port),
                        "node {node}, {port:?}, powered={powered}, enabled={enabled}"
                    );
                }
            }
        }
    }

    #[test]
    fn compiled_input_reads_preserve_masked_wire_sources() {
        use crate::redpiler::backend::graph::wire_probe::tests::{BOUNDS, LEFT, RECEIVER, RIGHT, fixture};
        let world = fixture();
        let (program, state, _) = physical_program(&world, BOUNDS).unwrap();
        let mut network = PhysicalSignalNetwork::compile(&program, &state, &EntryWork::capture(&world, BOUNDS)).unwrap();
        let receiver = program.position_index()[&RECEIVER].index();
        for (left, right) in [(true, true), (true, false), (false, true), (false, false), (true, true)] {
            network.set_lever(LEFT, left).unwrap();
            network.set_lever(RIGHT, right).unwrap();
            let actual = network.input_power(receiver, InputPort::Main).unwrap();
            assert_eq!(actual, network.input_power_physical(receiver, InputPort::Main).unwrap());
            assert_eq!(
                actual,
                match (left, right) {
                    (true, _) => 13,
                    (false, true) => 11,
                    _ => 0,
                }
            );
        }
    }

    #[test]
    fn experimental_dust_corridor_preserves_two_source_masking() {
        use crate::redpiler::backend::graph::wire_probe::tests::{BOUNDS, LEFT, RECEIVER, RIGHT, fixture};
        let world = fixture();
        let (program, state, _) = physical_program(&world, BOUNDS).unwrap();
        let mut network = PhysicalSignalNetwork::compile(&program, &state, &EntryWork::capture(&world, BOUNDS)).unwrap();
        network.enable_dust_corridor().unwrap();
        let receiver = program.position_index()[&RECEIVER].index();
        for (left, right, expected) in [
            (true, true, 13),
            (true, false, 13),
            (false, true, 11),
            (false, false, 0),
            (true, true, 13),
        ] {
            network.set_lever(LEFT, left).unwrap();
            network.set_lever(RIGHT, right).unwrap();
            assert_eq!(network.input_power(receiver, InputPort::Main).unwrap(), expected);
        }
        let (links, settles) = network.dust_corridor_work();
        assert!(links > 0);
        assert_eq!(settles, 0);
    }
}
