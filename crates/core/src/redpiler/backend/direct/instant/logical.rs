//! Cached decision programs over frozen Boolean inputs, with no physical work.
use super::{Decision, Input};
use crate::redpiler::backend::BackendError;
use crate::redpiler::instant::boolean::{Expr, TRUE};
use rustc_hash::FxHashMap;

#[derive(Clone, Copy)]
enum Condition {
    Input(usize),
    Response(Expr),
}

#[derive(Clone, Copy)]
struct BoundDecision {
    condition: Condition,
    low: Expr,
    high: Expr,
}

pub(super) struct Plan {
    roots: Vec<Expr>,
    inputs: Vec<(Input, u8)>,
    snapshot: Vec<bool>,
    decisions: Vec<BoundDecision>,
    users: Vec<Vec<usize>>,
    parents: Vec<Vec<usize>>,
    cached: Vec<Option<bool>>,
    stack: Vec<usize>,
    #[cfg(test)]
    evaluations: u64,
}

impl Plan {
    pub(super) fn geometry_inputs(
        &self,
    ) -> impl Iterator<Item = (usize, super::GeometryPart)> + '_ {
        self.inputs.iter().filter_map(|(input, _)| match input {
            Input::Geometry { actor, part } => Some((*actor, *part)),
            _ => None,
        })
    }

    pub(super) fn compile_counts(&self) -> (usize, usize) {
        (self.decisions.len(), self.inputs.len())
    }

    fn bind(
        decisions: &[Decision],
        roots: Vec<Expr>,
        response: bool,
        response_order: &[usize],
    ) -> Result<Self, BackendError> {
        let ordered = if response_order.is_empty() {
            roots.clone()
        } else {
            let mut seen = vec![false; roots.len()];
            if !response || response_order.len() != roots.len() {
                return Err(BackendError::InvalidInstantProgram);
            }
            for &actor in response_order {
                if actor >= roots.len() || std::mem::replace(&mut seen[actor], true) {
                    return Err(BackendError::InvalidInstantProgram);
                }
            }
            response_order.iter().map(|&actor| roots[actor]).collect()
        };
        let mut status = vec![0u8; decisions.len()];
        let mut postorder = Vec::new();
        let mut pending: Vec<_> = ordered
            .into_iter()
            .rev()
            .map(|root| (root, false))
            .collect();
        while let Some((root, complete)) = pending.pop() {
            if root <= TRUE {
                continue;
            }
            let id = (root - 2) as usize;
            let decision = decisions
                .get(id)
                .ok_or(BackendError::InvalidInstantProgram)?;
            if complete {
                status[id] = 2;
                postorder.push(id);
                continue;
            }
            if status[id] == 2 {
                continue;
            }
            if status[id] == 1 {
                return Err(BackendError::InvalidInstantProgram);
            }
            if [decision.low, decision.high]
                .into_iter()
                .any(|child| child > TRUE && (child - 2) as usize >= id)
            {
                return Err(BackendError::InvalidInstantProgram);
            }
            let condition = match decision.input {
                Input::Source(_) | Input::Memory(_) => None,
                Input::Geometry { .. } if !response => None,
                Input::Response(actor) if response && decision.threshold == 0 => Some(
                    *roots
                        .get(actor)
                        .ok_or(BackendError::InvalidInstantProgram)?,
                ),
                _ => return Err(BackendError::InvalidInstantProgram),
            };
            status[id] = 1;
            pending.push((root, true));
            pending.extend(
                [decision.high, decision.low]
                    .into_iter()
                    .map(|child| (child, false)),
            );
            if let Some(condition) = condition {
                pending.push((condition, false));
            }
        }
        let mut inputs = Vec::new();
        let mut input_ids = FxHashMap::default();
        let mut users: Vec<Vec<usize>> = Vec::new();
        let mut parents: Vec<Vec<usize>> = Vec::new();
        let mut bound = Vec::new();
        let mut remap = vec![None; decisions.len()];
        for id in postorder {
            let decision = &decisions[id];
            let next = bound.len();
            let mapped = |child: Expr| {
                if child <= TRUE {
                    child
                } else {
                    remap[(child - 2) as usize].unwrap()
                }
            };
            let (low, high) = (mapped(decision.low), mapped(decision.high));
            let condition = if let Input::Response(actor) = decision.input {
                Condition::Response(mapped(roots[actor]))
            } else {
                let key = (decision.input, decision.threshold);
                let input = *input_ids.entry(key).or_insert_with(|| {
                    let next = inputs.len();
                    inputs.push(key);
                    users.push(Vec::new());
                    next
                });
                users[input].push(next);
                Condition::Input(input)
            };
            parents.push(Vec::new());
            let condition_child = if let Condition::Response(child) = condition {
                child
            } else {
                0
            };
            for child in [low, high, condition_child] {
                if child > TRUE {
                    parents[(child - 2) as usize].push(next);
                }
            }
            bound.push(BoundDecision {
                condition,
                low,
                high,
            });
            remap[id] = Some((next + 2) as Expr);
        }
        let roots = roots
            .into_iter()
            .map(|root| {
                if root <= TRUE {
                    root
                } else {
                    remap[(root - 2) as usize].unwrap()
                }
            })
            .collect();
        let count = bound.len();
        Ok(Self {
            roots,
            snapshot: vec![false; inputs.len()],
            inputs,
            decisions: bound,
            users,
            parents,
            cached: vec![None; count],
            stack: Vec::with_capacity(count),
            #[cfg(test)]
            evaluations: 0,
        })
    }

    pub(super) fn capture(&mut self, mut read: impl FnMut(Input, u8) -> bool) {
        for input in 0..self.inputs.len() {
            let (binding, threshold) = self.inputs[input];
            let value = read(binding, threshold);
            if self.snapshot[input] == value {
                continue;
            }
            self.snapshot[input] = value;
            for &user in &self.users[input] {
                if self.cached[user].take().is_some() {
                    self.stack.push(user);
                }
            }
            while let Some(child) = self.stack.pop() {
                for &parent in &self.parents[child] {
                    let decision = &self.decisions[parent];
                    let child = (child + 2) as Expr;
                    let condition_changed =
                        matches!(decision.condition, Condition::Response(root) if root == child);
                    let selected = self.condition(decision.condition).map(|high| {
                        if high {
                            decision.high
                        } else {
                            decision.low
                        }
                    });
                    if !condition_changed && selected.is_some_and(|selected| selected != child) {
                        continue;
                    }
                    if self.cached[parent].take().is_some() {
                        self.stack.push(parent);
                    }
                }
            }
        }
    }

    fn value(&self, expression: Expr) -> Option<bool> {
        if expression <= TRUE {
            Some(expression == TRUE)
        } else {
            self.cached[(expression - 2) as usize]
        }
    }

    fn condition(&self, condition: Condition) -> Option<bool> {
        match condition {
            Condition::Input(input) => Some(self.snapshot[input]),
            Condition::Response(root) => self.value(root),
        }
    }

    pub(super) fn evaluate(&mut self, root: usize) -> bool {
        let expression = self.roots[root];
        if let Some(value) = self.value(expression) {
            return value;
        }
        self.stack.push((expression - 2) as usize);
        while let Some(&id) = self.stack.last() {
            let decision = self.decisions[id];
            let Some(high) = self.condition(decision.condition) else {
                let Condition::Response(condition) = decision.condition else {
                    unreachable!()
                };
                self.stack.push((condition - 2) as usize);
                continue;
            };
            let child = if high { decision.high } else { decision.low };
            if let Some(value) = self.value(child) {
                self.cached[id] = Some(value);
                self.stack.pop();
                #[cfg(test)]
                {
                    self.evaluations += 1;
                }
            } else {
                self.stack.push((child - 2) as usize);
            }
        }
        self.value(expression).unwrap()
    }

    #[cfg(test)]
    pub(super) fn evaluations(&self) -> u64 {
        self.evaluations
    }
}

pub(super) struct State {
    pub(super) responses: Plan,
    pub(super) outputs: Plan,
    pub(super) sampling: Plan,
    pub(super) initialized: bool,
    pub(super) next_sample: Option<u64>,
    #[cfg(test)]
    pub(super) samples: u64,
}

impl State {
    pub(super) fn source_nodes(&self) -> impl Iterator<Item = super::NodeId> + '_ {
        self.responses
            .inputs
            .iter()
            .chain(&self.outputs.inputs)
            .chain(&self.sampling.inputs)
            .filter_map(|&(input, _)| {
                if let Input::Source(node) = input {
                    Some(node)
                } else {
                    None
                }
            })
    }

    pub(super) fn bind(
        decisions: &[Decision],
        responses: Vec<Expr>,
        response_order: &[usize],
        outputs: Vec<Expr>,
        sampling: Vec<Expr>,
    ) -> Result<Self, BackendError> {
        Ok(Self {
            responses: Plan::bind(decisions, responses, true, response_order)?,
            outputs: Plan::bind(decisions, outputs, false, &[])?,
            sampling: Plan::bind(decisions, sampling, false, &[])?,
            initialized: false,
            next_sample: None,
            #[cfg(test)]
            samples: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::backend::direct::node::NodeId;

    fn source(id: usize) -> Input {
        Input::Source(unsafe { NodeId::from_index(id) })
    }

    #[test]
    fn shares_roots_and_invalidates_only_affected_paths() {
        let decisions = [
            Decision {
                input: source(0),
                threshold: 0,
                low: 0,
                high: 1,
            },
            Decision {
                input: source(1),
                threshold: 0,
                low: 0,
                high: 2,
            },
            Decision {
                input: source(2),
                threshold: 0,
                low: 0,
                high: 1,
            },
        ];
        let mut plan = Plan::bind(&decisions, vec![3, 3, 4, 0, 1], true, &[]).unwrap();
        plan.capture(|_, _| true);
        assert!(plan.evaluate(0));
        assert!(plan.evaluate(1));
        assert!(plan.evaluate(2));
        assert!(!plan.evaluate(3));
        assert!(plan.evaluate(4));
        assert_eq!(plan.evaluations(), 3);
        plan.capture(|_, _| true);
        assert!(plan.evaluate(0));
        assert_eq!(plan.evaluations(), 3);
        plan.capture(|input, _| input != source(0));
        assert!(plan.evaluate(2));
        assert_eq!(plan.evaluations(), 3);
        assert!(!plan.evaluate(0));
        assert!(!plan.evaluate(1));
        assert_eq!(plan.evaluations(), 5);
    }

    #[test]
    fn independent_snapshots_hold_old_bank_until_explicit_capture() {
        let decisions = [Decision {
            input: Input::Memory(0),
            threshold: 0,
            low: 0,
            high: 1,
        }];
        let mut responses = Plan::bind(&decisions, vec![2, 2], true, &[]).unwrap();
        let mut outputs = Plan::bind(&decisions, vec![2], false, &[]).unwrap();
        responses.capture(|_, _| false);
        assert!(!responses.evaluate(0));
        outputs.capture(|_, _| true);
        assert!(outputs.evaluate(0));
        assert!(!responses.evaluate(1));
        assert_eq!(responses.evaluations(), 1);
        responses.capture(|_, _| true);
        assert!(responses.evaluate(0));
        assert_eq!(responses.evaluations(), 2);
    }

    #[test]
    fn rejects_invalid_geometry_and_feedback_inputs_and_preserves_thresholds() {
        let malformed = [Decision {
            input: source(0),
            threshold: 0,
            low: 2,
            high: 1,
        }];
        assert!(Plan::bind(&malformed, vec![2], true, &[]).is_err());
        for input in [Input::Geometry {
            actor: 0,
            part: super::super::GeometryPart::Head,
        }] {
            let decisions = [Decision {
                input,
                threshold: 0,
                low: 0,
                high: 1,
            }];
            assert!(Plan::bind(&decisions, vec![2], true, &[]).is_err());
        }
        let decisions = [
            Decision {
                input: source(0),
                threshold: 3,
                low: 0,
                high: 1,
            },
            Decision {
                input: source(0),
                threshold: 7,
                low: 0,
                high: 1,
            },
        ];
        let mut plan = Plan::bind(&decisions, vec![2, 3], true, &[]).unwrap();
        plan.capture(|_, threshold| 5 > threshold);
        assert!(plan.evaluate(0));
        assert!(!plan.evaluate(1));
    }

    #[test]
    fn ignores_dead_handoff_decisions_and_unselected_branches() {
        let decisions = [
            Decision {
                input: source(0),
                threshold: 0,
                low: 0,
                high: 1,
            },
            Decision {
                input: source(1),
                threshold: 0,
                low: 0,
                high: 1,
            },
            Decision {
                input: source(2),
                threshold: 0,
                low: 2,
                high: 3,
            },
            Decision {
                input: Input::Geometry {
                    actor: 999,
                    part: super::super::GeometryPart::Head,
                },
                threshold: 0,
                low: 0,
                high: 1,
            },
        ];
        let mut plan = Plan::bind(&decisions, vec![4], true, &[]).unwrap();
        assert_eq!(plan.decisions.len(), 3);
        assert_eq!(plan.inputs.len(), 3);
        plan.capture(|input, _| input == source(1));
        assert!(!plan.evaluate(0));
        assert_eq!(plan.evaluations(), 2);
        plan.capture(|_, _| false);
        assert!(!plan.evaluate(0));
        assert_eq!(
            plan.evaluations(),
            2,
            "unselected uncached branch stays asleep"
        );
        plan.capture(|input, _| input == source(2));
        assert!(!plan.evaluate(0));
        assert_eq!(plan.evaluations(), 4);
        plan.capture(|input, _| input != source(1));
        assert!(!plan.evaluate(0));
        assert_eq!(
            plan.evaluations(),
            4,
            "unselected formerly cached branch stays asleep"
        );
        plan.capture(|input, _| input == source(0));
        assert!(plan.evaluate(0));
        assert_eq!(plan.evaluations(), 6);
    }

    #[test]
    fn cached_canonical_program_matches_plain_arena_across_all_inputs() {
        use crate::redpiler::instant::boolean::{BooleanArena, Variable, FALSE};
        use mchprs_blocks::BlockPos;

        let mut arena = BooleanArena::with_budget(1);
        let inputs: Vec<_> = (0..6)
            .map(|id| {
                arena.variable(Variable::Signal {
                    pos: BlockPos::new(id, 0, 0),
                    threshold: 0,
                    order: id as usize,
                })
            })
            .collect();
        let mut parity = inputs[0];
        for &input in &inputs[1..] {
            let inverse = arena.not(input);
            parity = arena.select(parity, inverse, input);
        }
        let interior = arena.select(inputs[2], inputs[3], inputs[4]);
        let left = arena.and(interior, inputs[5]);
        let right = arena.or(interior, inputs[1]);
        let mux = arena.select(inputs[0], left, right);
        let inverse = arena.not(mux);
        let mut roots = vec![mux, parity, left, right, mux, inverse, FALSE, TRUE];
        let arena = arena.compact(&mut roots);
        let decisions: Vec<_> = arena
            .nodes
            .iter()
            .map(|decision| {
                let Variable::Signal { pos, threshold, .. } = decision.variable else {
                    unreachable!()
                };
                Decision {
                    input: source(pos.x as usize),
                    threshold,
                    low: decision.low,
                    high: decision.high,
                }
            })
            .collect();
        let mut plan = Plan::bind(&decisions, roots.clone(), true, &[]).unwrap();
        for round in 0..3 {
            for input in 0usize..64 {
                let bits = match round {
                    0 => input ^ (input >> 1),
                    1 => (input * 13 + 7) & 63,
                    _ => 63 ^ (input ^ (input >> 1)),
                };
                plan.capture(|binding, threshold| {
                    let Input::Source(node) = binding else {
                        unreachable!()
                    };
                    (if bits & (1 << node.index()) != 0 {
                        15
                    } else {
                        0
                    }) > threshold
                });
                for (root, &expression) in roots.iter().enumerate() {
                    let expected = arena.evaluate(expression, |variable| {
                        let Variable::Signal { pos, threshold, .. } = variable else {
                            unreachable!()
                        };
                        (if bits & (1 << pos.x) != 0 { 15 } else { 0 }) > threshold
                    });
                    assert_eq!(
                        plan.evaluate(root),
                        expected,
                        "round {round}, assignment {bits}, root {root}"
                    );
                }
                let evaluations = plan.evaluations();
                for root in 0..roots.len() {
                    plan.evaluate(root);
                }
                assert_eq!(
                    plan.evaluations(),
                    evaluations,
                    "unchanged roots stay cached"
                );
            }
        }
    }

    #[test]
    fn wire_display_binding_is_rejected_for_logical_execution() {
        use super::super::{Nodes, Runtime};
        use crate::redpiler::analysis::ports::ConsumerInput;
        use crate::redpiler::backend::direct::node::{Node, NodeType};
        use crate::redpiler::instant::boolean::{BooleanArena, Variable};
        use crate::redpiler::instant::logic::WaveLogic;
        use crate::redpiler::instant::outputs::{OutputPort, PowerTerm};
        use crate::redpiler::instant::program::PreparedInstant;
        use mchprs_blocks::BlockPos;

        let pos = BlockPos::new(1, 2, 3);
        let node = |ty| Node {
            ty,
            default_inputs: Default::default(),
            side_inputs: Default::default(),
            updates: Default::default(),
            is_io: false,
            powered: false,
            locked: false,
            output_power: 0,
            changed: false,
            pending_tick: false,
        };
        let nodes = Nodes::new(
            vec![node(NodeType::Wire), node(NodeType::InstantSource)].into_boxed_slice(),
        );
        let bindings = [(pos, nodes.get(0))].into_iter().collect();
        let outputs = [(0, nodes.get(1))].into_iter().collect();
        let program = || {
            let mut arena = BooleanArena::with_budget(1);
            let guard = arena.variable(Variable::Signal {
                pos,
                threshold: 0,
                order: 0,
            });
            PreparedInstant {
                pistons: Vec::new(),
                output_offset: 0,
                clocked: None,
                independent_memory: Vec::new(),
                sampling: Vec::new(),
                reset_groups: Vec::new(),
                payloads: Vec::new(),
                controls: Vec::new(),
                groups: Vec::new(),
                aliases: Vec::new(),
                owned: Default::default(),
                propagation_wires: Default::default(),
                observable_wires: Default::default(),
                sampling_wires: Default::default(),
                candidate_wire_count: 0,
                wire_retention_reasons: [0; 7],
                template: Vec::new(),
                logical_tick: 0,
                logic: WaveLogic {
                    arena,
                    responses: Vec::new(),
                    response_order: Vec::new(),
                    sources: vec![pos],
                    response_sources: Vec::new(),
                    wires: Default::default(),
                    wire_links: Default::default(),
                    unprojected_consumer_wires: Default::default(),
                    context: Default::default(),
                    follows_payload: Vec::new(),
                    handoff_wires: Vec::new(),
                    outputs: vec![OutputPort {
                        consumer: pos,
                        input: ConsumerInput::Main,
                        initial_strength: 0,
                        terms: vec![PowerTerm {
                            guard,
                            source: None,
                            attenuation: 0,
                        }],
                    }],
                },
            }
        };
        assert!(
            matches!(Runtime::bind(program(), &bindings, &outputs, &nodes),
            Err(BackendError::LogicalWireInput { pos: actual }) if actual == pos)
        );
    }

    #[test]
    fn local_response_conditions_share_a_dag_and_invalidate_affected_ancestors() {
        let decisions = [
            Decision {
                input: source(0),
                threshold: 0,
                low: 0,
                high: 1,
            },
            Decision {
                input: source(1),
                threshold: 0,
                low: 0,
                high: 1,
            },
            Decision {
                input: source(2),
                threshold: 0,
                low: 0,
                high: 1,
            },
            Decision {
                input: Input::Response(3),
                threshold: 0,
                low: 3,
                high: 4,
            },
            Decision {
                input: Input::Response(2),
                threshold: 0,
                low: 2,
                high: 3,
            },
            // Dead physical inspection decisions are still excluded.
            Decision {
                input: Input::Geometry {
                    actor: 99,
                    part: super::super::GeometryPart::Head,
                },
                threshold: 0,
                low: 0,
                high: 1,
            },
        ];
        let mut plan = Plan::bind(
            &decisions,
            vec![6, 3, 5, 2, 6, 0, 1],
            true,
            &[3, 1, 2, 0, 4, 5, 6],
        )
        .unwrap();
        assert_eq!(plan.decisions.len(), 5);
        assert_eq!(plan.inputs.len(), 3);
        for round in 0..3 {
            for assignment in 0..8usize {
                let bits = (assignment * 5 + round) & 7;
                plan.capture(|binding, _| {
                    let Input::Source(id) = binding else {
                        panic!("responses are computed, not sampled inputs")
                    };
                    bits & (1 << id.index()) != 0
                });
                let (a, b, c) = (bits & 1 != 0, bits & 2 != 0, bits & 4 != 0);
                let interior = if a { c } else { b };
                let output = if interior { b } else { a };
                for (root, expected) in [output, b, interior, a, output, false, true]
                    .into_iter()
                    .enumerate()
                {
                    assert_eq!(
                        plan.evaluate(root),
                        expected,
                        "round {round}, inputs {bits}, root {root}"
                    );
                }
                let evaluations = plan.evaluations();
                for root in 0..7 {
                    plan.evaluate(root);
                }
                assert_eq!(plan.evaluations(), evaluations);
            }
        }
        // Both branches were cached during the sweep. An inactive branch
        // changing must not invalidate a parent with a known computed condition.
        plan.capture(|binding, _| binding == source(0));
        assert!(plan.evaluate(0));
        let evaluations = plan.evaluations();
        plan.capture(|binding, _| binding != source(2));
        assert!(plan.evaluate(0));
        assert_eq!(plan.evaluations(), evaluations);
    }

    #[test]
    fn rejects_computed_condition_cycles_and_malformed_response_roots() {
        let self_cycle = [Decision {
            input: Input::Response(0),
            threshold: 0,
            low: 0,
            high: 1,
        }];
        assert!(Plan::bind(&self_cycle, vec![2], true, &[0]).is_err());
        let cycle = [
            Decision {
                input: Input::Response(1),
                threshold: 0,
                low: 0,
                high: 1,
            },
            Decision {
                input: Input::Response(0),
                threshold: 0,
                low: 0,
                high: 1,
            },
        ];
        assert!(Plan::bind(&cycle, vec![2, 3], true, &[0, 1]).is_err());
        let missing = [Decision {
            input: Input::Response(9),
            threshold: 0,
            low: 0,
            high: 1,
        }];
        assert!(Plan::bind(&missing, vec![2], true, &[0]).is_err());
        assert!(Plan::bind(&self_cycle, vec![0], false, &[]).is_ok());
        let constant = [Decision {
            input: Input::Response(1),
            threshold: 0,
            low: 0,
            high: 1,
        }];
        let mut plan = Plan::bind(&constant, vec![2, 1], true, &[1, 0]).unwrap();
        assert!(plan.evaluate(0));
        assert!(plan.inputs.is_empty());
        assert!(Plan::bind(&constant, vec![2, 1], true, &[1, 1]).is_err());
        assert!(Plan::bind(&constant, vec![2, 1], true, &[1]).is_err());
        assert!(Plan::bind(&constant, vec![2, 1], false, &[]).is_err());
    }
}
