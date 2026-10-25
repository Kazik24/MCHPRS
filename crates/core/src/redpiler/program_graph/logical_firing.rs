//! Cold, source-derived bindings for actors that may fire once before their reset.
//!
//! The firing bits are logical evaluator inputs. They do not replace actor or resource
//! state and do not define a global reset epoch.
use super::*;
use rustc_hash::FxHashSet;

#[path = "logical_firing/evaluation.rs"]
#[cfg(test)]
mod evaluation;
#[cfg(test)]
pub(crate) use evaluation::LogicalFiringEvaluation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LogicalFiringPlan {
    /// These rows bind each actor's fired bit to its original reset witness.
    pub(crate) uses: Box<[LogicalFireUse]>,
    /// These map shared payload-source presence to the fired actors that can pull it.
    pub(crate) resources: Box<[LogicalFireResource]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LogicalFireUse {
    /// This is the piston actor guarded by a one-shot latch.
    pub(crate) actor: PistonId,
    /// This is the dense typed input slot for the actor's current fired state.
    pub(crate) fired_input: LayerInputId,
    /// This indexes `!fired && Main == 0 && Quasi == 0`; it is not a route/request/accept result.
    pub(crate) power_eligible_output: u32,
    /// This exact per-actor witness determines which existing source can rearm the latch.
    pub(crate) reset: CellReset,
    /// These are the original callback occurrences that can sample the actor.
    pub(crate) activation_routes: Box<[RouteOccurrence]>,
    /// This reuses the catalog's source-derived retraction and deferred-publication contract.
    pub(crate) effect: CellRetractionEffect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LogicalFireResource {
    /// This is the shared resource whose source presence is affected by its owners' fired bits.
    pub(crate) resource: ResourceId,
    /// This is the source contact used by the compiled feedback cut.
    pub(crate) source: ContactId,
    /// These indices refer to rows in `LogicalFiringPlan::uses`, not piston IDs or bit positions.
    pub(crate) owners: Box<[u32]>,
}

impl LogicalFiringPlan {
    pub(crate) fn validate(&self, program: &ReducedLayerProgram, outputs: &[SettledOutput]) -> Result<(), String> {
        if self.uses.is_empty() {
            return Err("logical firing plan has no actor uses".into());
        }
        let mut actors = FxHashSet::default();
        let mut inputs = FxHashSet::default();
        let mut use_by_actor = rustc_hash::FxHashMap::default();
        for (use_index, use_) in self.uses.iter().enumerate() {
            if !actors.insert(use_.actor) {
                return Err(format!("logical firing plan repeats actor {:?}", use_.actor));
            }
            use_by_actor.insert(use_.actor, use_);
            if !inputs.insert(use_.fired_input) {
                return Err(format!("logical firing plan reuses fired input {:?}", use_.fired_input));
            }
            let input = program
                .inputs
                .get(use_.fired_input.0 as usize)
                .ok_or_else(|| format!("logical firing plan references missing input {:?}", use_.fired_input))?;
            if input.source != (LayerInputSource::InstantFired { actor: use_.actor }) {
                return Err(format!(
                    "logical firing plan input {:?} is not bound to actor {:?}",
                    use_.fired_input, use_.actor
                ));
            }
            let output = outputs.get(use_.power_eligible_output as usize).ok_or_else(|| {
                format!(
                    "logical firing actor {:?} references missing output {}",
                    use_.actor, use_.power_eligible_output
                )
            })?;
            if output.target != SettledTarget::InstantPowerEligible(use_.actor) || output.kind != SettledValueKind::Bool || output.roots.len() != 1 {
                return Err(format!("logical firing actor {:?} has an invalid fire output binding", use_.actor));
            }
            if use_.activation_routes.is_empty() {
                return Err(format!("logical firing actor {:?} has no activation route", use_.actor));
            }
            if use_.effect.actor_revision_increment != 1 || use_.effect.wake.ticks_after_acceptance == 0 {
                return Err(format!("logical firing actor {:?} has an invalid retraction revision/delay", use_.actor));
            }
            if let Some(transfer) = use_.effect.transfer {
                if transfer.resource_revision_increment != 1 {
                    return Err(format!(
                        "logical firing actor {:?} has an invalid resource revision increment",
                        use_.actor
                    ));
                }
                let Some(wake) = use_.effect.wake.resource else {
                    return Err(format!(
                        "logical firing actor {:?} transfers a resource without a deferred wake",
                        use_.actor
                    ));
                };
                if wake.resource != transfer.resource || wake.contact != transfer.destination {
                    return Err(format!("logical firing actor {:?} has mismatched transfer and resource wake", use_.actor));
                }
                if !self.resources.iter().any(|resource| {
                    resource.resource == transfer.resource && resource.source == transfer.source && resource.owners.contains(&(use_index as u32))
                }) {
                    return Err(format!(
                        "logical firing actor {:?} transfer is not bound to its source resource",
                        use_.actor
                    ));
                }
            } else if use_.effect.wake.resource.is_some() {
                return Err(format!("logical firing actor {:?} has a resource wake without a transfer", use_.actor));
            }
        }

        let mut eligible_outputs = FxHashSet::default();
        for use_ in &self.uses {
            if !eligible_outputs.insert(use_.power_eligible_output) {
                return Err(format!("logical firing plan reuses output {}", use_.power_eligible_output));
            }
        }

        for (index, input) in program.inputs.iter().enumerate() {
            if let LayerInputSource::InstantFired { actor } = input.source {
                let Some(use_) = use_by_actor.get(&actor) else {
                    return Err(format!("program has an unbound fired input for actor {actor:?}"));
                };
                if use_.fired_input.0 as usize != index {
                    return Err(format!("program fired input for actor {actor:?} is bound to the wrong slot"));
                }
            }
        }
        for (index, output) in outputs.iter().enumerate() {
            if let SettledTarget::InstantPowerEligible(actor) = output.target {
                let Some(use_) = use_by_actor.get(&actor) else {
                    return Err(format!("program has an unbound fire output for actor {actor:?}"));
                };
                if use_.power_eligible_output as usize != index {
                    return Err(format!("program fire output for actor {actor:?} is bound to the wrong slot"));
                }
            }
        }

        let mut resources = FxHashSet::default();
        for resource in &self.resources {
            if !resources.insert(resource.resource) {
                return Err(format!("logical firing plan repeats resource {:?}", resource.resource));
            }
            if resource.owners.is_empty() {
                return Err(format!("logical firing resource {:?} has no owners", resource.resource));
            }
            let mut owners = FxHashSet::default();
            for &owner in &resource.owners {
                let use_ = self
                    .uses
                    .get(owner as usize)
                    .ok_or_else(|| format!("logical firing resource {:?} references missing use {owner}", resource.resource))?;
                if !owners.insert(use_.actor) {
                    return Err(format!("logical firing resource {:?} repeats owner {:?}", resource.resource, use_.actor));
                }
            }
        }
        Ok(())
    }
}

impl SettledCircuit {
    pub(crate) fn validate_firing_binding(&self) -> Result<(), String> {
        match &self.firing {
            Some(firing) => firing.validate(&self.program, &self.outputs),
            None if self
                .program
                .inputs
                .iter()
                .any(|input| matches!(input.source, LayerInputSource::InstantFired { .. }))
                || self
                    .outputs
                    .iter()
                    .any(|output| matches!(output.target, SettledTarget::InstantPowerEligible(_))) =>
            {
                Err("settled circuit uses one-shot bindings without a firing plan".into())
            }
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cycle_fixture() -> SettledCircuit {
        let actors = [PistonId::new(10), PistonId::new(11)];
        let inputs = actors
            .iter()
            .map(|&actor| LayerInput {
                source: LayerInputSource::InstantFired { actor },
                role: LayerInputRole::AcceptanceRevalidation,
            })
            .collect::<Vec<_>>();
        // The shared resource is present only while both owners remain unfired.
        // Its absence blocks both actors until their source-derived reset events clear the latches.
        let expressions = vec![
            LayerExpr::Input(LayerInputId(0)),
            LayerExpr::Input(LayerInputId(1)),
            LayerExpr::Any(Box::new([LayerExprId(0), LayerExprId(1)])),
            LayerExpr::Not(LayerExprId(2)),
            LayerExpr::Constant(true),
            LayerExpr::All(Box::new([LayerExprId(3), LayerExprId(4)])),
        ];
        let outputs = actors
            .iter()
            .map(|&actor| SettledOutput {
                target: SettledTarget::InstantPowerEligible(actor),
                kind: SettledValueKind::Bool,
                roots: Box::new([LayerExprId(5)]),
            })
            .collect::<Vec<_>>();
        let uses = actors
            .iter()
            .enumerate()
            .map(|(index, &actor)| LogicalFireUse {
                actor,
                fired_input: LayerInputId(index as u32),
                power_eligible_output: index as u32,
                reset: CellReset::Observer {
                    node: NodeId::new(50 + index as u32),
                    wake: CellResetWake {
                        watched_actor: actor,
                        on_base_changed: Box::new([RouteOccurrence {
                            route: RouteId::new(100 + index as u32),
                            ordinal: 0,
                        }]),
                        on_base_restored: Box::new([RouteOccurrence {
                            route: RouteId::new(200 + index as u32),
                            ordinal: 1,
                        }]),
                    },
                },
                activation_routes: Box::new([RouteOccurrence {
                    route: RouteId::new(300 + index as u32),
                    ordinal: 0,
                }]),
                effect: CellRetractionEffect {
                    actor_revision_increment: 1,
                    transfer: None,
                    routes: CellRetractionRoutes {
                        base_changed: RouteId::new(400 + index as u32),
                        far_changed: None,
                        near_changed: RouteId::new(500 + index as u32),
                    },
                    wake: CellDeferredWake {
                        ticks_after_acceptance: 2,
                        base_restored: RouteId::new(600 + index as u32),
                        resource: None,
                    },
                },
            })
            .collect::<Vec<_>>();
        let plan = LogicalFiringPlan {
            uses: uses.into_boxed_slice(),
            resources: Box::new([LogicalFireResource {
                resource: ResourceId::new(30),
                source: ContactId::new(31),
                owners: Box::new([0, 1]),
            }]),
        };
        SettledCircuit {
            program: ReducedLayerProgram {
                inputs: inputs.into_boxed_slice(),
                expressions: expressions.into_boxed_slice(),
                roots: Box::new([(LayerRoot::Output(0), LayerExprId(5)), (LayerRoot::Output(1), LayerExprId(5))]),
            },
            outputs: outputs.into_boxed_slice(),
            firing: Some(plan),
            retained_cells: Box::new([]),
            retained_effects: Box::new([]),
            declines: Box::new([]),
            timing: None,
        }
    }

    #[test]
    fn two_owner_one_shot_cycle_validates_and_shared_resource_waits_for_reset() {
        let circuit = cycle_fixture();
        circuit.validate().unwrap();
        let mut values = LogicalValues::new(&circuit.program).unwrap();
        let inputs = [false, false];
        values.evaluate(&inputs).unwrap();
        assert!(values.values()[3], "resource source is present before either owner fires");
        let inputs = [true, false];
        values.evaluate(&inputs).unwrap();
        assert!(
            !values.values()[3],
            "one accepted firing suppresses the shared source for the rest of this reset epoch"
        );
        let inputs = [false, false];
        values.evaluate(&inputs).unwrap();
        assert!(values.values()[3], "only a reset clearing the fired latch rearms the shared source");
    }

    #[test]
    fn firing_plan_rejects_forged_input_output_and_owner_ids() {
        let mut circuit = cycle_fixture();
        circuit.firing.as_mut().unwrap().uses[0].fired_input = LayerInputId(99);
        assert!(circuit.validate().is_err(), "out-of-range fired input must decline");

        let mut circuit = cycle_fixture();
        circuit.firing.as_mut().unwrap().uses[0].power_eligible_output = 99;
        assert!(circuit.validate().is_err(), "out-of-range fire output must decline");

        let mut circuit = cycle_fixture();
        circuit.firing.as_mut().unwrap().resources[0].owners = Box::new([0, 2]);
        assert!(circuit.validate().is_err(), "out-of-range resource owner use must decline");

        let mut circuit = cycle_fixture();
        circuit.program.inputs[0].source = LayerInputSource::InstantFired { actor: PistonId::new(11) };
        assert!(circuit.validate().is_err(), "fired input bound to another actor must decline");
    }

    #[test]
    fn firing_inputs_and_outputs_require_a_plan() {
        let mut circuit = cycle_fixture();
        circuit.firing = None;
        assert!(circuit.validate().is_err(), "one-shot evaluator bindings cannot be unowned");
    }

    #[test]
    fn retraction_effect_transfer_must_match_the_shared_source_binding() {
        let mut circuit = cycle_fixture();
        let effect = &mut circuit.firing.as_mut().unwrap().uses[0].effect;
        effect.transfer = Some(CellResourceTransfer {
            resource: ResourceId::new(30),
            source: ContactId::new(31),
            destination: ContactId::new(32),
            resource_revision_increment: 1,
            when_at_source: CellPinId(7),
        });
        effect.wake.resource = Some(CellResourceWake {
            resource: ResourceId::new(30),
            contact: ContactId::new(32),
            route: RouteId::new(700),
        });
        circuit.validate().expect("source transfer and wake are bound to the owner row");

        let effect = &mut circuit.firing.as_mut().unwrap().uses[0].effect;
        effect.wake.resource.as_mut().unwrap().contact = ContactId::new(99);
        assert!(circuit.validate().is_err(), "forged deferred contact must decline");

        let mut circuit = cycle_fixture();
        circuit.firing.as_mut().unwrap().uses[0].effect.transfer = Some(CellResourceTransfer {
            resource: ResourceId::new(30),
            source: ContactId::new(99),
            destination: ContactId::new(32),
            resource_revision_increment: 1,
            when_at_source: CellPinId(7),
        });
        circuit.firing.as_mut().unwrap().uses[0].effect.wake.resource = Some(CellResourceWake {
            resource: ResourceId::new(30),
            contact: ContactId::new(32),
            route: RouteId::new(700),
        });
        assert!(circuit.validate().is_err(), "transfer cannot name a different source contact");
    }
}
