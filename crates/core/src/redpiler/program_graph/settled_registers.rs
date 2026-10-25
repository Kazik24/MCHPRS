//! Canonical quiescent state bindings for settled Boolean BUD registers.
//!
//! This maps a register bit back to the two-contact payload and its unique
//! storage piston. It is only valid at a stationary handoff cut; it does not
//! schedule motion or reproduce callbacks.
use super::*;
use rustc_hash::FxHashSet;

#[cfg(test)]
#[path = "../../../benches/support/cpus.rs"]
#[allow(dead_code)]
mod cpus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RegisterBinding {
    output: usize,
    resource: ResourceId,
    storage: PistonId,
    near: ContactId,
    far: ContactId,
    source: ContactId,
}

/// Validated bindings from Boolean register outputs to canonical storage state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SettledRegisterBindings {
    registers: Box<[RegisterBinding]>,
    output_count: usize,
}

impl SettledRegisterBindings {
    pub(crate) fn new(program: &CircuitProgram, circuit: &SettledCircuit) -> Result<Self, String> {
        Self::from_specs(circuit, program.pistons(), program.resources(), program.contacts())
    }

    fn from_specs(circuit: &SettledCircuit, pistons: &[PistonSpec], resources: &[ResourceSpec], contacts: &[ContactSpec]) -> Result<Self, String> {
        circuit.validate()?;
        if !circuit.declines.is_empty() {
            return Err("cannot bind registers from a partial settled circuit".into());
        }

        let mut registers = Vec::new();
        let mut resources_seen = FxHashSet::default();
        let mut storage_seen = FxHashSet::default();
        let mut contacts_seen = FxHashSet::default();

        for (output_index, output) in circuit.outputs.iter().enumerate() {
            let SettledTarget::Register(read) = output.target else {
                continue;
            };
            if output.kind != SettledValueKind::Bool || output.roots.len() != 1 {
                return Err("settled BUD register output must be one Boolean root".into());
            }
            if read.storage.index() >= pistons.len() || read.reader.index() >= pistons.len() || read.resource.index() >= resources.len() {
                return Err("settled BUD register references an unknown actor or resource".into());
            }
            if read.storage == read.reader {
                return Err("settled BUD reader cannot be its storage piston".into());
            }
            if !resources_seen.insert(read.resource) || !storage_seen.insert(read.storage) {
                return Err("settled BUD resources and storage pistons must be unique".into());
            }

            let storage = pistons[read.storage.index()];
            let PistonLane::SingleResource { far } = storage.lane else {
                return Err("settled BUD storage must have one canonical two-contact lane".into());
            };
            if !storage.sticky || storage.facing != BlockFace::Bottom || storage.near == far {
                return Err("settled BUD storage must be a sticky downward-facing piston".into());
            }
            if read.source != far {
                return Err("settled BUD bit must read the storage piston’s Far contact".into());
            }

            let resource = &resources[read.resource.index()];
            if resource.contacts.len() != 2
                || !resource.contacts.contains(&storage.near)
                || !resource.contacts.contains(&far)
                || storage.near.index() >= contacts.len()
                || far.index() >= contacts.len()
            {
                return Err("settled BUD resource must occupy exactly the storage Near/Far contacts".into());
            }
            if resources
                .iter()
                .enumerate()
                .any(|(index, other)| index != read.resource.index() && (other.contacts.contains(&storage.near) || other.contacts.contains(&far)))
            {
                return Err("settled BUD contacts are shared by another resource".into());
            }
            let near_position = storage.position.offset(BlockFace::Bottom);
            let far_position = near_position.offset(BlockFace::Bottom);
            if contacts[storage.near.index()].position != near_position || contacts[far.index()].position != far_position {
                return Err("settled BUD contacts are not the storage piston’s canonical Near/Far positions".into());
            }
            if !contacts_seen.insert(storage.near) || !contacts_seen.insert(far) {
                return Err("settled BUD storage contacts must be unique".into());
            }

            let touches_lane = |piston: &PistonSpec| {
                [piston.base, piston.near, piston.far]
                    .into_iter()
                    .any(|contact| contact == storage.near || contact == far)
            };
            if pistons
                .iter()
                .enumerate()
                .any(|(index, piston)| index != read.storage.index() && touches_lane(piston))
            {
                return Err("settled BUD storage contacts must not be reachable by a foreign piston".into());
            }

            registers.push(RegisterBinding {
                output: output_index,
                resource: read.resource,
                storage: read.storage,
                near: storage.near,
                far,
                source: read.source,
            });
        }

        if registers.is_empty() {
            return Err("settled circuit has no Boolean BUD registers".into());
        }
        Ok(Self {
            registers: registers.into_boxed_slice(),
            output_count: circuit.outputs.len(),
        })
    }

    pub(crate) fn len(&self) -> usize {
        self.registers.len()
    }

    /// Materialize bits at a stationary cut. The caller must first certify no scheduled or
    /// deferred work touches these owners; this method checks local physical quiescence only.
    /// Validation completes before any state is changed.
    /// Revisions are preserved: no piston event is being created by this handoff.
    pub(crate) fn apply_quiescent(&self, state: &mut CircuitState, outputs: &[u8]) -> Result<(), String> {
        if outputs.len() != self.output_count {
            return Err("settled output count does not match its bindings".into());
        }

        let mut updates = Vec::with_capacity(self.registers.len());
        for binding in self.registers.iter() {
            let bit = match outputs[binding.output] {
                0 => false,
                1 => true,
                _ => return Err("settled Boolean register output is not Boolean".into()),
            };
            let Some(piston) = state.pistons.get(binding.storage.index()).copied() else {
                return Err("settled register storage state is missing".into());
            };
            let Some(resource) = state.resources.get(binding.resource.index()).copied() else {
                return Err("settled register resource state is missing".into());
            };
            if piston.availability != PistonAvailability::Ready || piston.extended != piston.head_present {
                return Err("settled register handoff requires a quiescent piston with matching head state".into());
            }
            if !resource.enabled || (resource.at != binding.near && resource.at != binding.far) {
                return Err("settled register handoff requires its enabled resource in the canonical lane".into());
            }
            if resource.at == binding.far {
                if !piston.extended {
                    return Err("Far-contact resource disagrees with the retracted storage piston".into());
                }
            } else if piston.extended {
                return Err("Near-contact resource disagrees with the extended storage piston".into());
            }

            let at = if bit {
                binding.source
            } else if binding.source == binding.far {
                binding.near
            } else {
                binding.far
            };
            let extended = at == binding.far;
            updates.push((
                binding.storage,
                PistonState {
                    extended,
                    head_present: extended,
                    availability: PistonAvailability::Ready,
                    revision: piston.revision,
                },
                binding.resource,
                ResourceState {
                    at,
                    enabled: true,
                    revision: resource.revision,
                },
            ));
        }

        for (storage, piston, resource, resource_state) in updates {
            state.pistons[storage.index()] = piston;
            state.resources[resource.index()] = resource_state;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plot::PlotWorld;
    use crate::redpiler::analysis::AnalysisLimits;
    use crate::redpiler::{CompilerOptions, TaskMonitor, passes};
    use serde_json::Value;
    use std::path::Path;

    #[cfg(test)]

    fn piston(id: u32, near: ContactId, far: ContactId, sticky: bool) -> PistonSpec {
        PistonSpec {
            node: NodeId::new(id),
            position: BlockPos::new(0, 10, id as i32),
            facing: BlockFace::Bottom,
            sticky,
            base: ContactId::new(id + 10),
            near,
            far,
            lane: PistonLane::SingleResource { far },
            routes: PistonRoutes {
                base_changed: RouteId::new(0),
                near_changed: RouteId::new(0),
                far_changed: None,
                far_shape_changed: None,
                base_restored: RouteId::new(0),
                head_restored: RouteId::new(0),
                resource_restored_near: None,
                resource_restored_far: None,
            },
        }
    }

    fn circuit(reads: &[(u32, u32, u32, u32)]) -> SettledCircuit {
        let outputs = reads
            .iter()
            .map(|&(storage, reader, resource, source)| SettledOutput {
                target: SettledTarget::Register(BudMemoryRead {
                    storage: PistonId::new(storage),
                    reader: PistonId::new(reader),
                    resource: ResourceId::new(resource),
                    source: ContactId::new(source),
                    link: PowerLink {
                        from: SiteId::new(0),
                        from_port: PowerPort::Output,
                        to: PowerTarget::Conductor {
                            site: SiteId::new(0),
                            include_dust: true,
                        },
                        face: BlockFace::Bottom,
                        attenuation: 0,
                        guard: GuardId::new(0),
                    },
                }),
                kind: SettledValueKind::Bool,
                roots: Box::new([LayerExprId(0)]),
            })
            .collect::<Vec<_>>();
        SettledCircuit {
            program: ReducedLayerProgram {
                inputs: Box::new([]),
                expressions: Box::new([LayerExpr::Constant(false)]),
                roots: Box::new([(LayerRoot::Output(0), LayerExprId(0))]),
            },
            outputs: outputs.into_boxed_slice(),
            firing: None,
            retained_cells: Box::new([]),
            retained_effects: Box::new([]),
            declines: Box::new([]),
            timing: None,
        }
    }

    fn contacts(positions: &[BlockPos]) -> Vec<ContactSpec> {
        positions
            .iter()
            .enumerate()
            .map(|(index, &position)| ContactSpec {
                node: NodeId::new(index as u32 + 100),
                position,
                variants: Box::new([]),
            })
            .collect()
    }

    fn prepared_counter() -> CompileGraph {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/instant-pistons-io/fixtures/counter_basic.json");
        let manifest: Value = serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
        let world: PlotWorld = cpus::load_manifest(&manifest).unwrap();
        let monitor = TaskMonitor::default();
        monitor.set_budget_multiplier(8);
        let mut prepared = passes::rich::prepare(&world, world.get_corners(), vec![], &monitor, AnalysisLimits::for_budget(8)).unwrap();
        let options = CompilerOptions {
            optimization_level: Some(6),
            budget_multiplier: 8,
            stable_instant_inputs: true,
            ..Default::default()
        };
        passes::run_graph_pipeline(
            &mut prepared.graph,
            &options,
            &prepared.entry,
            &passes::default_graph_pipeline(&options),
            &monitor,
        )
        .unwrap();
        prepared.graph
    }

    #[test]
    fn counter_o6_register_bindings_roundtrip_all_sixteen_physical_bits() {
        let graph = prepared_counter();
        let monitor = TaskMonitor::default();
        let (program, _) = graph.clone().lower_with_monitor(&monitor).expect("COUNTER O6 lowers");
        let circuit = program.settled_circuit().expect("O6 retained the settled circuit");
        let bindings = SettledRegisterBindings::new(&program, circuit).expect("all COUNTER registers have safe storage bindings");
        assert_eq!(bindings.len(), 16);

        let mut state = CircuitState {
            classic_signals: Box::new([]),
            classic_active: Box::new([]),
            components: Box::new([]),
            pistons: graph.initial_pistons.clone().into_boxed_slice(),
            resources: graph.initial_resources.clone().into_boxed_slice(),
        };
        let before = state.clone();
        let mut output_values = vec![0; circuit.outputs.len()];
        let mut registered_pistons = FxHashSet::default();
        let mut registered_resources = FxHashSet::default();
        for (output_index, output) in circuit.outputs.iter().enumerate() {
            let SettledTarget::Register(read) = output.target else {
                continue;
            };
            assert!(registered_pistons.insert(read.storage));
            assert!(registered_resources.insert(read.resource));
            output_values[output_index] = u8::from(state.resources[read.resource.index()].at == read.source);
        }
        assert_eq!(registered_pistons.len(), 16);
        assert_eq!(registered_resources.len(), 16);

        bindings
            .apply_quiescent(&mut state, &output_values)
            .expect("sampled COUNTER state is stationary");
        assert_eq!(
            state, before,
            "projecting the currently sampled 16-bit word must not change physical state"
        );

        let changed = bindings.registers.iter().step_by(3).map(|binding| binding.output).collect::<Vec<_>>();
        for &output_index in &changed {
            output_values[output_index] ^= 1;
        }
        let mut projected = before.clone();
        bindings
            .apply_quiescent(&mut projected, &output_values)
            .expect("selected bit changes remain quiescent");
        for (output_index, output) in circuit.outputs.iter().enumerate() {
            let SettledTarget::Register(read) = output.target else {
                continue;
            };
            let should_change = changed.contains(&output_index);
            let expected_at = if output_values[output_index] != 0 {
                read.source
            } else {
                graph.pistons[read.storage.index()].near
            };
            assert_eq!(projected.resources[read.resource.index()].at, expected_at);
            assert_eq!(
                projected.pistons[read.storage.index()].extended,
                expected_at == graph.pistons[read.storage.index()].far
            );
            if !should_change {
                assert_eq!(projected.resources[read.resource.index()], before.resources[read.resource.index()]);
                assert_eq!(projected.pistons[read.storage.index()], before.pistons[read.storage.index()]);
            } else {
                assert_ne!(projected.resources[read.resource.index()], before.resources[read.resource.index()]);
                assert_ne!(projected.pistons[read.storage.index()], before.pistons[read.storage.index()]);
            }
        }
        for (index, piston) in before.pistons.iter().enumerate() {
            if !registered_pistons.contains(&PistonId::new(index as u32)) {
                assert_eq!(projected.pistons[index], *piston, "projection must not alter an unregistered piston");
            }
        }
        for (index, resource) in before.resources.iter().enumerate() {
            if !registered_resources.contains(&ResourceId::new(index as u32)) {
                assert_eq!(
                    projected.resources[index], *resource,
                    "projection must not alter an unregistered resource"
                );
            }
        }
        assert_eq!(projected.classic_signals, before.classic_signals);
        assert_eq!(projected.classic_active, before.classic_active);
        assert_eq!(projected.components, before.components);
    }

    fn state(at: ContactId, extended: bool, pistons: usize, resources: usize) -> CircuitState {
        CircuitState {
            classic_signals: Box::new([]),
            classic_active: Box::new([]),
            components: Box::new([]),
            pistons: (0..pistons)
                .map(|_| PistonState {
                    extended,
                    head_present: extended,
                    availability: PistonAvailability::Ready,
                    revision: 7,
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            resources: (0..resources)
                .map(|_| ResourceState {
                    at,
                    enabled: true,
                    revision: 11,
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        }
    }

    #[test]
    fn register_bits_map_to_canonical_near_and_far_quiescent_states() {
        let circuit = circuit(&[(0, 1, 0, 1)]);
        let near = ContactId::new(0);
        let far = ContactId::new(1);
        let pistons = [piston(0, near, far, true), piston(1, ContactId::new(2), ContactId::new(3), false)];
        let resources = [ResourceSpec {
            material: ProfileId::new(0),
            contacts: Box::new([near, far]),
        }];
        let positions = [
            BlockPos::new(0, 9, 0),
            BlockPos::new(0, 8, 0),
            BlockPos::new(0, 9, 1),
            BlockPos::new(0, 8, 1),
        ];
        let contact_specs = contacts(&positions);
        let bindings = SettledRegisterBindings::from_specs(&circuit, &pistons, &resources, &contact_specs).expect("canonical register binding");
        assert_eq!(bindings.len(), 1);

        let mut state = state(near, false, 2, 1);
        bindings.apply_quiescent(&mut state, &[1]).unwrap();
        assert_eq!(
            state.resources[0],
            ResourceState {
                at: far,
                enabled: true,
                revision: 11
            }
        );
        assert_eq!(
            state.pistons[0],
            PistonState {
                extended: true,
                head_present: true,
                availability: PistonAvailability::Ready,
                revision: 7,
            }
        );
        bindings.apply_quiescent(&mut state, &[0]).unwrap();
        assert_eq!(
            state.resources[0],
            ResourceState {
                at: near,
                enabled: true,
                revision: 11
            }
        );
        assert!(!state.pistons[0].extended && !state.pistons[0].head_present);
    }

    #[test]
    fn all_register_states_validate_before_any_write() {
        let circuit = circuit(&[(0, 2, 0, 1), (1, 3, 1, 3)]);
        let n0 = ContactId::new(0);
        let f0 = ContactId::new(1);
        let n1 = ContactId::new(2);
        let f1 = ContactId::new(3);
        let pistons = [
            piston(0, n0, f0, true),
            piston(1, n1, f1, true),
            piston(2, ContactId::new(4), ContactId::new(5), false),
            piston(3, ContactId::new(6), ContactId::new(7), false),
        ];
        let resources = [
            ResourceSpec {
                material: ProfileId::new(0),
                contacts: Box::new([n0, f0]),
            },
            ResourceSpec {
                material: ProfileId::new(0),
                contacts: Box::new([n1, f1]),
            },
        ];
        let positions = [
            BlockPos::new(0, 9, 0),
            BlockPos::new(0, 8, 0),
            BlockPos::new(0, 9, 1),
            BlockPos::new(0, 8, 1),
            BlockPos::new(0, 9, 2),
            BlockPos::new(0, 8, 2),
            BlockPos::new(0, 9, 3),
            BlockPos::new(0, 8, 3),
        ];
        let contact_specs = contacts(&positions);
        let bindings = SettledRegisterBindings::from_specs(&circuit, &pistons, &resources, &contact_specs).unwrap();
        let mut state = CircuitState {
            classic_signals: Box::new([]),
            classic_active: Box::new([]),
            components: Box::new([]),
            pistons: Box::new([
                PistonState {
                    extended: false,
                    head_present: false,
                    availability: PistonAvailability::Ready,
                    revision: 1,
                },
                PistonState {
                    extended: true,
                    head_present: true,
                    availability: PistonAvailability::BasePending,
                    revision: 2,
                },
                PistonState {
                    extended: false,
                    head_present: false,
                    availability: PistonAvailability::Ready,
                    revision: 3,
                },
                PistonState {
                    extended: false,
                    head_present: false,
                    availability: PistonAvailability::Ready,
                    revision: 4,
                },
            ]),
            resources: Box::new([
                ResourceState {
                    at: n0,
                    enabled: true,
                    revision: 5,
                },
                ResourceState {
                    at: f1,
                    enabled: true,
                    revision: 6,
                },
            ]),
        };
        let before = state.clone();
        assert!(bindings.apply_quiescent(&mut state, &[1, 0]).is_err());
        assert_eq!(state, before, "later validation failure must leave earlier registers untouched");
    }

    #[test]
    fn duplicate_registers_and_noncanonical_contact_sets_are_rejected() {
        let near = ContactId::new(0);
        let far = ContactId::new(1);
        let pistons = [piston(0, near, far, true), piston(1, ContactId::new(2), ContactId::new(3), false)];
        let duplicate = circuit(&[(0, 1, 0, 1), (0, 1, 0, 1)]);
        let resource = ResourceSpec {
            material: ProfileId::new(0),
            contacts: Box::new([near, far]),
        };
        let positions = [
            BlockPos::new(0, 9, 0),
            BlockPos::new(0, 8, 0),
            BlockPos::new(0, 9, 1),
            BlockPos::new(0, 8, 1),
        ];
        let contact_specs = contacts(&positions);
        assert!(SettledRegisterBindings::from_specs(&duplicate, &pistons, std::slice::from_ref(&resource), &contact_specs).is_err());

        let single_register = circuit(&[(0, 1, 0, 1)]);
        let extra_contact = ResourceSpec {
            material: ProfileId::new(0),
            contacts: Box::new([near, far, ContactId::new(2)]),
        };
        assert!(SettledRegisterBindings::from_specs(&single_register, &pistons, &[extra_contact], &contact_specs).is_err());
        assert!(
            SettledRegisterBindings::from_specs(
                &single_register,
                &[piston(0, near, far, false), pistons[1]],
                std::slice::from_ref(&resource),
                &contact_specs
            )
            .is_err()
        );
        let duplicate_owner = [pistons[0], pistons[1], piston(2, near, far, true)];
        assert!(SettledRegisterBindings::from_specs(&single_register, &duplicate_owner, std::slice::from_ref(&resource), &contact_specs).is_err());
        let foreign_contact_owner = [pistons[0], pistons[1], piston(2, ContactId::new(4), far, true)];
        assert!(
            SettledRegisterBindings::from_specs(&single_register, &foreign_contact_owner, std::slice::from_ref(&resource), &contact_specs).is_err(),
            "a foreign piston touching one storage contact with a different lane must also be rejected"
        );
        let wrong_positions = contacts(&[
            BlockPos::new(0, 9, 0),
            BlockPos::new(0, 7, 0),
            BlockPos::new(0, 9, 1),
            BlockPos::new(0, 8, 1),
        ]);
        assert!(SettledRegisterBindings::from_specs(&single_register, &pistons, std::slice::from_ref(&resource), &wrong_positions).is_err());

        let wrong_source = circuit(&[(0, 1, 0, 0)]);
        assert!(SettledRegisterBindings::from_specs(&wrong_source, &pistons, std::slice::from_ref(&resource), &contact_specs).is_err());
        let shared_resource = ResourceSpec {
            material: ProfileId::new(0),
            contacts: Box::new([far, ContactId::new(2)]),
        };
        assert!(SettledRegisterBindings::from_specs(&single_register, &pistons, &[resource, shared_resource], &contact_specs).is_err());
    }
}
