//! Sparse one-shot latch updates over the existing reduced Boolean evaluator.
use super::*;

/// Incremental fired-state inputs for one immutable firing-aware settled program.
///
/// The graph still owns requests, FIFO acceptance, reset delivery, and publication timing.
/// This adapter only updates the proof-bound latch input and reads its eligibility root.
#[derive(Debug)]
pub(crate) struct LogicalFiringEvaluation {
    evaluator: LogicalValues,
    actor_to_use: Box<[Option<usize>]>,
    eligibility_roots: Box<[usize]>,
    fired_actor_by_input: Box<[Option<PistonId>]>,
    fired_input_slots: Box<[LayerInputId]>,
    input_count: usize,
}

impl LogicalFiringEvaluation {
    pub(crate) fn new(circuit: &SettledCircuit, actor_count: usize, initial_inputs: &[bool]) -> Result<Self, String> {
        circuit.validate()?;
        if !circuit.declines.is_empty() {
            return Err("a partial settled circuit cannot execute a logical firing evaluation".into());
        }
        if initial_inputs.len() != circuit.program.inputs.len() {
            return Err("logical input count does not match the firing program".into());
        }
        let uses = circuit.firing.as_ref().map_or(&[][..], |plan| plan.uses.as_ref());
        if let Some(use_) = uses.iter().find(|use_| use_.actor.index() >= actor_count) {
            return Err(format!("firing actor {:?} is outside the circuit actor table", use_.actor));
        }
        let mut actor_to_use = vec![None; actor_count];
        let mut eligibility_roots = Vec::with_capacity(uses.len());
        let mut fired_actor_by_input = vec![None; circuit.program.inputs.len()];
        let mut fired_input_slots = Vec::with_capacity(uses.len());
        for (index, use_) in uses.iter().enumerate() {
            let Some(slot) = actor_to_use.get_mut(use_.actor.index()) else {
                return Err(format!("firing actor {:?} is outside the circuit actor table", use_.actor));
            };
            if slot.replace(index).is_some() {
                return Err(format!("firing plan repeats actor {:?}", use_.actor));
            }
            let output = &circuit.outputs[use_.power_eligible_output as usize];
            eligibility_roots.push(output.roots[0].0 as usize);
            fired_actor_by_input[use_.fired_input.0 as usize] = Some(use_.actor);
            fired_input_slots.push(use_.fired_input);
        }
        let mut evaluator = LogicalValues::new(&circuit.program)?;
        evaluator.evaluate(initial_inputs).map_err(|error| error.to_owned())?;
        Ok(Self {
            evaluator,
            actor_to_use: actor_to_use.into_boxed_slice(),
            eligibility_roots: eligibility_roots.into_boxed_slice(),
            fired_actor_by_input: fired_actor_by_input.into_boxed_slice(),
            fired_input_slots: fired_input_slots.into_boxed_slice(),
            input_count: initial_inputs.len(),
        })
    }

    /// Mark a use fired only after Graph accepts its action; repeated acceptance is a no-op.
    pub(crate) fn accept_notify(&mut self, actor: PistonId) -> Result<usize, String> {
        self.set_fired(actor, true)
    }

    /// Clear only the named use when its source-certified reset event completes.
    pub(crate) fn reset_notify(&mut self, actor: PistonId) -> Result<usize, String> {
        self.set_fired(actor, false)
    }

    pub(crate) fn power_eligible(&self, actor: PistonId) -> Result<bool, String> {
        let use_index = self
            .use_index(actor)
            .ok_or_else(|| format!("actor {actor:?} is not in the firing plan"))?;
        self.evaluator
            .values()
            .get(self.eligibility_roots[use_index])
            .copied()
            .ok_or_else(|| format!("actor {actor:?} has a missing eligibility expression"))
    }

    pub(crate) fn expression_value(&self, expression: usize) -> Result<bool, String> {
        self.evaluator
            .values()
            .get(expression)
            .copied()
            .ok_or_else(|| "logical expression root is out of range".to_owned())
    }

    /// Apply caller-sampled external changes without scanning/copying every input slot.
    pub(crate) fn update_inputs(&mut self, changes: &[(LayerInputId, bool)]) -> Result<usize, String> {
        for &(input, _) in changes {
            let slot = input.0 as usize;
            if slot >= self.input_count {
                return Err("logical input change is out of range".to_owned());
            }
            if let Some(actor) = self.fired_actor_by_input[slot] {
                return Err(format!("fired input for {actor:?} must change through a firing event"));
            }
        }
        self.evaluator.evaluate_changes(changes).map_err(|error| error.to_owned())
    }

    fn set_fired(&mut self, actor: PistonId, fired: bool) -> Result<usize, String> {
        let use_index = self
            .use_index(actor)
            .ok_or_else(|| format!("actor {actor:?} is not in the firing plan"))?;
        let input = self.fired_input(use_index);
        self.evaluator.evaluate_changes(&[(input, fired)]).map_err(|error| error.to_owned())
    }

    fn use_index(&self, actor: PistonId) -> Option<usize> {
        self.actor_to_use.get(actor.index()).copied().flatten()
    }

    fn fired_input(&self, use_index: usize) -> LayerInputId {
        // The plan is immutable after construction; retain only the dense actor mapping and
        // derive the corresponding fired slot from the validated input table captured below.
        self.fired_input_slots[use_index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> SettledCircuit {
        let actors = [PistonId::new(0), PistonId::new(1)];
        let inputs = [
            LayerInputSource::InstantFired { actor: actors[0] },
            LayerInputSource::InstantFired { actor: actors[1] },
            LayerInputSource::SourceAbove {
                site: SiteId::new(0),
                port: PowerPort::Output,
                threshold: 1,
            },
            LayerInputSource::SourceAbove {
                site: SiteId::new(1),
                port: PowerPort::Output,
                threshold: 1,
            },
        ]
        .into_iter()
        .map(|source| LayerInput {
            source,
            role: LayerInputRole::AcceptanceRevalidation,
        })
        .collect::<Vec<_>>();
        let expressions = vec![
            LayerExpr::Input(LayerInputId(0)),
            LayerExpr::Input(LayerInputId(1)),
            LayerExpr::Any(Box::new([LayerExprId(0), LayerExprId(1)])),
            LayerExpr::Not(LayerExprId(2)),
            LayerExpr::Input(LayerInputId(2)),
            LayerExpr::Not(LayerExprId(0)),
            LayerExpr::All(Box::new([LayerExprId(3), LayerExprId(4), LayerExprId(5)])),
            LayerExpr::Not(LayerExprId(1)),
            LayerExpr::All(Box::new([LayerExprId(3), LayerExprId(4), LayerExprId(7)])),
            LayerExpr::Input(LayerInputId(3)),
            LayerExpr::Not(LayerExprId(9)),
        ];
        let outputs = actors
            .iter()
            .enumerate()
            .map(|(index, &actor)| SettledOutput {
                target: SettledTarget::InstantPowerEligible(actor),
                kind: SettledValueKind::Bool,
                roots: Box::new([LayerExprId(if index == 0 { 6 } else { 8 })]),
            })
            .collect::<Vec<_>>();
        let uses = actors
            .iter()
            .enumerate()
            .map(|(index, &actor)| LogicalFireUse {
                actor,
                fired_input: LayerInputId(index as u32),
                power_eligible_output: index as u32,
                reset: CellReset::Payload {
                    resource: ResourceId::new(index as u32),
                    notification: RouteOccurrence {
                        route: RouteId::new(index as u32),
                        ordinal: 0,
                    },
                    sample: RouteOccurrence {
                        route: RouteId::new(index as u32),
                        ordinal: 1,
                    },
                },
                activation_routes: Box::new([RouteOccurrence {
                    route: RouteId::new(10 + index as u32),
                    ordinal: 0,
                }]),
                effect: CellRetractionEffect {
                    actor_revision_increment: 1,
                    transfer: None,
                    routes: CellRetractionRoutes {
                        base_changed: RouteId::new(20 + index as u32),
                        far_changed: None,
                        near_changed: RouteId::new(30 + index as u32),
                    },
                    wake: CellDeferredWake {
                        ticks_after_acceptance: 2,
                        base_restored: RouteId::new(40 + index as u32),
                        resource: None,
                    },
                },
            })
            .collect::<Vec<_>>();
        SettledCircuit {
            program: ReducedLayerProgram {
                inputs: inputs.into_boxed_slice(),
                expressions: expressions.into_boxed_slice(),
                roots: Box::new([]),
            },
            outputs: outputs.into_boxed_slice(),
            firing: Some(LogicalFiringPlan {
                uses: uses.into_boxed_slice(),
                resources: Box::new([LogicalFireResource {
                    resource: ResourceId::new(9),
                    source: ContactId::new(9),
                    owners: Box::new([0, 1]),
                }]),
            }),
            retained_effects: Box::new([]),
            declines: Box::new([]),
            timing: None,
        }
    }

    #[test]
    fn accepted_fire_suppresses_shared_feedback_until_that_use_resets() {
        let circuit = fixture();
        let mut evaluation = LogicalFiringEvaluation::new(&circuit, 2, &[false, false, true, false]).unwrap();
        assert!(evaluation.power_eligible(PistonId::new(0)).unwrap());
        assert!(evaluation.power_eligible(PistonId::new(1)).unwrap());

        assert!(evaluation.accept_notify(PistonId::new(0)).unwrap() > 0);
        assert!(!evaluation.power_eligible(PistonId::new(0)).unwrap());
        assert!(
            !evaluation.power_eligible(PistonId::new(1)).unwrap(),
            "shared resource feedback is absent while either owner fired"
        );
        assert_eq!(
            evaluation.accept_notify(PistonId::new(0)).unwrap(),
            0,
            "one-shot acceptance is idempotent"
        );

        assert_eq!(
            evaluation.reset_notify(PistonId::new(1)).unwrap(),
            0,
            "another owner's reset cannot clear this latch"
        );
        assert!(!evaluation.power_eligible(PistonId::new(0)).unwrap());
        assert!(evaluation.reset_notify(PistonId::new(0)).unwrap() > 0);
        assert!(evaluation.power_eligible(PistonId::new(0)).unwrap());
        assert!(evaluation.power_eligible(PistonId::new(1)).unwrap());
    }

    #[test]
    fn sparse_external_updates_are_atomic_and_skip_unrelated_branches() {
        let circuit = fixture();
        let mut evaluation = LogicalFiringEvaluation::new(&circuit, 2, &[false, false, true, false]).unwrap();
        assert_eq!(evaluation.update_inputs(&[(LayerInputId(3), false)]).unwrap(), 0);
        assert_eq!(evaluation.update_inputs(&[(LayerInputId(3), true)]).unwrap(), 2);
        let before = evaluation.evaluator.values().to_vec();
        assert!(evaluation.update_inputs(&[(LayerInputId(2), false), (LayerInputId(99), true)]).is_err());
        assert_eq!(
            evaluation.evaluator.values(),
            before,
            "invalid sparse batch must not partially mutate cached values"
        );
        assert!(evaluation.update_inputs(&[(LayerInputId(2), false)]).unwrap() > 0);
        assert!(
            !evaluation.power_eligible(PistonId::new(0)).unwrap(),
            "a valid change after the rejected batch proves the cache remained synchronized"
        );
    }

    #[test]
    fn constructor_validates_actor_ids_before_dense_index_allocation() {
        let mut circuit = fixture();
        let invalid_actor = PistonId::new(u32::MAX);
        circuit.firing.as_mut().unwrap().uses[0].actor = invalid_actor;
        circuit.program.inputs[0].source = LayerInputSource::InstantFired { actor: invalid_actor };
        circuit.outputs[0].target = SettledTarget::InstantPowerEligible(invalid_actor);
        assert!(
            LogicalFiringEvaluation::new(&circuit, 2, &[false, false, true, false]).is_err(),
            "forged actor IDs must be rejected against the parent circuit's actor count"
        );
    }

    #[test]
    fn acyclic_circuit_without_firing_plan_evaluates_and_rejects_unknown_notifications() {
        let circuit = SettledCircuit {
            program: ReducedLayerProgram {
                inputs: Box::new([LayerInput {
                    source: LayerInputSource::SourceAbove {
                        site: SiteId::new(0),
                        port: PowerPort::Output,
                        threshold: 1,
                    },
                    role: LayerInputRole::RequestSample,
                }]),
                expressions: Box::new([LayerExpr::Input(LayerInputId(0)), LayerExpr::Not(LayerExprId(0))]),
                roots: Box::new([]),
            },
            outputs: Box::new([]),
            firing: None,
            retained_effects: Box::new([]),
            declines: Box::new([]),
            timing: None,
        };
        let mut evaluation = LogicalFiringEvaluation::new(&circuit, 3, &[false]).unwrap();
        assert_eq!(evaluation.expression_value(1).unwrap(), true);
        assert_eq!(evaluation.update_inputs(&[(LayerInputId(0), true)]).unwrap(), 2);
        assert_eq!(evaluation.expression_value(1).unwrap(), false);
        assert!(evaluation.accept_notify(PistonId::new(1)).is_err());
        assert!(evaluation.reset_notify(PistonId::new(1)).is_err());
    }
}
