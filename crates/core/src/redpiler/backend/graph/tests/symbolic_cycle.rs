//! Recurrence requires identical state and ordered future functions for every input valuation.
use super::*;
use crate::redpiler::analysis::instant_regions::compiler::{
    Compiler as SymbolicCompiler,
    cycle::{Payload, Publication, Scheduled, Snapshot, unique_word_proves_equal},
    entry::Entry,
    execution::Execution,
    work::Guarded,
};
use mchprs_redpiler_opt::{Limits, symbolic::Formula};
use symbolic_observation_tests::Observation;

fn limits() -> Limits {
    Limits {
        max_work: 256 * 1024 * 1024,
        ..Limits::default()
    }
}

#[test]
fn guarded_word_proof_preserves_order_and_declines_unproven_duplicate_identities() {
    let (graph, _) = stable_region_tests::stable_graph_at_level("adder_1bit", 1);
    let mut compiler = SymbolicCompiler::new(&graph, limits()).unwrap();
    let a = compiler.logic.input(0).unwrap();
    let b = compiler.logic.input(1).unwrap();
    let inverse = compiler.logic.not(a).unwrap();
    let both = compiler.logic.and(a, b).unwrap();
    let guards = [Formula::FALSE, Formula::TRUE, a, b, inverse, both];
    let mut random = 0x4f65_81d9u32;
    let mut next = || {
        random = random.wrapping_mul(1664525).wrapping_add(1013904223);
        random as usize
    };
    let mut pairs = Vec::new();
    for _ in 0..256 {
        let mut word = || {
            (0..next() % 6)
                .map(|_| Guarded {
                    when: guards[next() % guards.len()],
                    value: (next() >> 16) % 3,
                })
                .collect::<Vec<_>>()
        };
        let left = word();
        pairs.push((left.clone(), left.clone()));
        pairs.push((left, word()));
    }
    // Splitting one identity across mutually exclusive guards must not create a duplicate event.
    pairs.push((
        vec![Guarded { when: a, value: 0 }, Guarded { when: inverse, value: 0 }],
        vec![Guarded {
            when: Formula::TRUE,
            value: 0,
        }],
    ));
    // These words differ in static order but never select both identities at once.
    pairs.push((
        vec![Guarded { when: a, value: 0 }, Guarded { when: inverse, value: 1 }],
        vec![Guarded { when: inverse, value: 1 }, Guarded { when: a, value: 0 }],
    ));
    let proofs: Vec<_> = pairs
        .iter()
        .map(|(a, b)| unique_word_proves_equal(&mut compiler, a, b, |value| *value, |_, _, _| Ok(Formula::TRUE)).unwrap())
        .collect();
    assert!(proofs[proofs.len() - 2..].iter().all(|&proof| proof));
    let plan = mchprs_redpiler_opt::optimize(&compiler.logic.circuit(&guards).unwrap(), limits(), || false).unwrap();
    let mut equal = vec![true; pairs.len()];
    let mut unique = vec![true; pairs.len()];
    for inputs in [[false, false], [false, true], [true, false], [true, true]] {
        let mut values = vec![false; guards.len()];
        plan.evaluate(&inputs, &mut vec![false; plan.scratch_len()], &mut values).unwrap();
        let select = |word: &[Guarded<usize>]| {
            word.iter()
                .filter_map(|event| {
                    let index = guards.iter().position(|&guard| guard == event.when).unwrap();
                    values[index].then_some(event.value)
                })
                .collect::<Vec<_>>()
        };
        for (index, (left, right)) in pairs.iter().enumerate() {
            let a = select(left);
            let b = select(right);
            let same = a == b;
            equal[index] &= same;
            unique[index] &=
                a.iter().enumerate().all(|(i, value)| !a[..i].contains(value)) && b.iter().enumerate().all(|(i, value)| !b[..i].contains(value));
            assert!(!proofs[index] || same, "unsound proof for {inputs:?}: {left:?} versus {right:?}");
        }
    }
    for index in 0..pairs.len() {
        if unique[index] {
            assert_eq!(proofs[index], equal[index], "complete for unique event identities: {:?}", pairs[index]);
        }
    }
}

#[test]
fn recurrence_rejects_changed_deadlines_priorities_requests_and_independent_revision_witnesses() {
    let (graph, _) = stable_region_tests::stable_graph_at_level("adder_1bit", 1);
    let mut compiler = SymbolicCompiler::new(&graph, limits()).unwrap();
    let execution = Execution::empty(&graph);
    let mut original = Snapshot::capture(&mut compiler, &execution, 0).unwrap();
    original.ticks.push(Guarded {
        when: Formula::TRUE,
        value: Scheduled {
            node: NodeId::new(0),
            delay: 2,
            priority: mchprs_world::TickPriority::High,
        },
    });
    original.publications.push(Guarded {
        when: Formula::TRUE,
        value: Publication {
            actor: PistonId::new(0),
            revision_age: vec![],
            resource: Some(Guarded {
                when: Formula::TRUE,
                value: Payload {
                    resource: ResourceId::new(0),
                    revision_age: vec![],
                    contact: graph.resources[0].contacts[0],
                },
            }),
            delay: 2,
            extending: true,
        },
    });
    assert!(original.proves_same(&original, &mut compiler).unwrap());
    for change in 0..7 {
        let mut changed = original.clone();
        match change {
            0 => changed.ticks[0].value.delay += 1,
            1 => changed.ticks[0].value.priority = mchprs_world::TickPriority::Normal,
            2 => changed.publications[0].value.revision_age = vec![Formula::TRUE],
            3 => changed.publications[0].value.resource.as_mut().unwrap().value.revision_age = vec![Formula::TRUE],
            4 => changed.publications[0].value.resource.as_mut().unwrap().value.resource = ResourceId::new(1),
            5 => changed.scalars[0] = compiler.logic.not(changed.scalars[0]).unwrap(),
            6 => changed.requests.push(Guarded {
                when: Formula::TRUE,
                value: crate::redpiler::analysis::instant_regions::compiler::work::Request {
                    actor: PistonId::new(0),
                    action: PistonAction::Retract,
                },
            }),
            _ => unreachable!(),
        }
        assert!(!original.proves_same(&changed, &mut compiler).unwrap(), "change {change}");
    }
    let present = compiler.logic.input(0).unwrap();
    let mut changed = original.clone();
    original.publications[0].value.resource.as_mut().unwrap().when = present;
    changed.publications[0].value.resource.as_mut().unwrap().when = present;
    original.publications[0].value.resource.as_mut().unwrap().value.revision_age = vec![present];
    changed.publications[0].value.resource.as_mut().unwrap().value.revision_age = vec![Formula::TRUE];
    assert!(
        original.proves_same(&changed, &mut compiler).unwrap(),
        "absent payload values must not constrain a present actor"
    );
}

#[test]
fn both_adders_have_a_symbolically_proven_cycle_with_exact_repeated_future_and_interpreter_outputs() {
    for id in FAMILY {
        let circuit = load_adder(id, ORIGIN, None);
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(root().join("test_data/instant-pistons-io/fixtures").join(format!("{id}.json"))).unwrap()).unwrap();
        let (graph, _) = stable_region_tests::stable_graph_at_level(id, 1);
        let trigger = NodeId::new(graph.nodes.iter().position(|node| node.position == Some(circuit.trigger())).unwrap() as u32);
        let mut compiler = SymbolicCompiler::new(&graph, limits()).unwrap();
        let entry = Entry::derive(&mut compiler, trigger).unwrap();
        let mut execution = Execution::empty(&graph);
        execution.set_control(&mut compiler, trigger, Formula::FALSE).unwrap();
        let mut snapshots = vec![Snapshot::capture(&mut compiler, &execution, execution.accepted.len()).unwrap()];
        let mut frames = Vec::new();
        let mut roots = Vec::new();
        let mut cycle = None;
        for tick in 1..=48 {
            let first_effect = execution.accepted.len();
            execution
                .run_tick(&mut compiler)
                .unwrap_or_else(|error| panic!("{id} tick {tick}: {error:?}"));
            frames.push(Observation::capture(&compiler, &execution, first_effect, &mut roots));
            let snapshot = Snapshot::capture(&mut compiler, &execution, first_effect).unwrap();
            for (prefix, earlier) in snapshots.iter().enumerate() {
                if earlier.proves_same(&snapshot, &mut compiler).unwrap_or_else(|error| {
                    panic!(
                        "{id} comparing tick {tick} to {prefix}: {error:?}, decisions={}, work={}",
                        compiler.logic.node_count(),
                        compiler.logic.work()
                    )
                }) {
                    cycle = Some((prefix, tick - prefix));
                    break;
                }
            }
            if cycle.is_some() {
                break;
            }
            snapshots.push(snapshot);
        }
        let (prefix, period) = cycle.unwrap_or_else(|| panic!("{id} no symbolic recurrence within the phase budget"));
        assert!(period > 0);
        assert_eq!(frames.len(), prefix + period);
        let plan = mchprs_redpiler_opt::optimize(&compiler.logic.circuit(&roots).unwrap(), limits(), || false).unwrap();
        let mut scratch = vec![false; plan.scratch_len()];
        let mut values = vec![false; roots.len()];
        eprintln!(
            "{id}: universally proven prefix={prefix}, period={period}, decisions={}, diagnostic_ops={}",
            compiler.logic.node_count(),
            plan.ops().len()
        );
        for case in manifest["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|case| case["expectation"].is_object())
        {
            let (program, state) = graph.clone().lower().unwrap();
            let mut exact = GraphBackend::new(program, state, 0).unwrap();
            let mut native = circuit.build();
            let mut scratch_world = circuit.build();
            let positions = circuit.positions();
            for step in case_steps(&circuit, case, false, false) {
                native_step(&mut native, step);
                match step {
                    Step::Tick => exact.tick().unwrap(),
                    Step::SetLever { position, powered } => {
                        if exact.node_state(exact.positions[&position]).powered != powered {
                            exact.use_block(position).unwrap();
                        }
                    }
                }
            }
            let inputs = entry.read_inputs(&graph, &exact.state).unwrap();
            let runtime_inputs: Vec<_> = entry
                .inputs
                .iter()
                .map(|input| input.read(&exact.program, &exact.state, &entry.shapes).unwrap())
                .collect();
            assert_eq!(runtime_inputs, inputs, "lowered runtime bindings for {}", case["id"]);
            plan.evaluate(&inputs, &mut scratch, &mut values).unwrap();
            let origins: Vec<_> = exact
                .state
                .pistons
                .iter()
                .map(|actor| actor.revision)
                .chain(exact.state.resources.iter().map(|resource| resource.revision))
                .collect();
            let prefix_revisions = if prefix == 0 {
                vec![0; origins.len()]
            } else {
                frames[prefix - 1].revision_deltas(&values)
            };
            let period_revisions: Vec<_> = frames
                .last()
                .unwrap()
                .revision_deltas(&values)
                .into_iter()
                .zip(prefix_revisions)
                .map(|(end, start)| end.wrapping_sub(start))
                .collect();
            exact.use_block(circuit.trigger()).unwrap();
            native_step(
                &mut native,
                Step::SetLever {
                    position: circuit.trigger(),
                    powered: false,
                },
            );
            for step in 0..prefix + period * 4 {
                let (phase, repetitions) = if step < prefix {
                    (step, 0)
                } else {
                    (prefix + (step - prefix) % period, (step - prefix) / period)
                };
                let effective_origins: Vec<_> = origins
                    .iter()
                    .zip(&period_revisions)
                    .map(|(&origin, &delta)| origin.wrapping_add(delta.wrapping_mul(repetitions as u64)))
                    .collect();
                let first_effect = exact.effects.len();
                exact.tick().unwrap();
                native_step(&mut native, Step::Tick);
                frames[phase].assert_matches(&values, &exact, &effective_origins, first_effect);
                check_boundary(
                    &exact,
                    &native,
                    &mut scratch_world,
                    &positions,
                    &format!("{id}/{} repeated phase {step}", case["id"]),
                )
                .unwrap();
                if step < 16 {
                    let (sum, carry) = expectation(&circuit, case);
                    assert_math_window(&circuit, &native, sum, carry, step + 1);
                }
            }
        }
    }
}

#[test]
fn one_bit_aot_kernel_preserves_memory_ordered_future_and_interpreter_handoff() {
    verify_production_kernel("adder_1bit");
}

#[test]
fn eleven_bit_aot_kernel_preserves_memory_ordered_future_and_interpreter_handoff() {
    verify_production_kernel("adder_11bits");
}

fn verify_production_kernel(id: &'static str) {
    let circuit = load_adder(id, ORIGIN, None);
    let (graph, _) = stable_region_tests::stable_graph_at_level(id, 4);
    let trigger = NodeId::new(graph.nodes.iter().position(|node| node.position == Some(circuit.trigger())).unwrap() as u32);
    let kernel = graph
        .plans
        .instant
        .symbolic_kernels
        .iter()
        .find(|kernel| kernel.trigger == trigger)
        .unwrap_or_else(|| panic!("{id} O4 did not publish the trigger's symbolic kernel"));
    assert_eq!(kernel.plan.inputs().len(), kernel.inputs.len());
    assert_eq!(kernel.phases.len(), kernel.prefix + kernel.period);
    assert!(kernel.plan.scratch_len() > 0);
    assert!(kernel.plan.roots().len() > kernel.entry.conditions.len());
    if id == "adder_11bits" {
        assert!(kernel.inputs.len() > 3, "the AOT plan must retain the full typed interface");
    }
    let (program, _) = graph.clone().lower().unwrap();
    let lowered = program
        .symbolic_kernels()
        .iter()
        .find(|kernel| kernel.trigger == trigger)
        .unwrap_or_else(|| panic!("{id} lowering dropped its symbolic kernel"));
    assert_eq!(lowered.as_ref(), kernel.as_ref());
    verify_symbolic_runtime(&circuit, graph);
}

fn verify_symbolic_runtime(circuit: &Adder, mut graph: CompileGraph) {
    // Neither side may fall back to the old enumerated machine.
    graph.plans.instant.machine = None;
    let mut baseline = graph.clone();
    baseline.plans.instant.symbolic_kernels.clear();
    let (program, state) = graph.lower().unwrap();
    let (base_program, base_state) = baseline.lower().unwrap();
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root().join("test_data/instant-pistons-io/fixtures").join(format!("{}.json", circuit.id))).unwrap())
            .unwrap();
    let positions = circuit.positions();
    for case in manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["expectation"].is_object())
    {
        let mut native = circuit.build();
        let mut scratch = circuit.build();
        let mut exact = GraphBackend::new(base_program.clone(), base_state.clone(), 0).unwrap();
        let mut compiled = GraphBackend::new(program.clone(), state.clone(), 0).unwrap();
        let mut steps = case_steps(circuit, case, false, true);
        let activation = steps.len() - 1;
        steps.extend(std::iter::repeat_n(Step::Tick, 48));
        for (index, step) in steps.into_iter().enumerate() {
            if index == activation {
                for backend in [&mut exact, &mut compiled] {
                    if backend.has_active_kernel() {
                        assert!(backend.materialize_kernel_state());
                        backend.future.symbolic_cursor = None;
                        backend.future.kernel_cursor = None;
                    }
                    for (index, actor) in backend.state.pistons.iter_mut().enumerate() {
                        actor.revision = u64::MAX - index as u64;
                    }
                    for (index, resource) in backend.state.resources.iter_mut().enumerate() {
                        resource.revision = u64::MAX - index as u64;
                    }
                }
            }
            let toggle = match step {
                Step::SetLever { position, powered } => matches!(native.get_block(position), Block::Lever { lever } if lever.powered != powered),
                Step::Tick => false,
            };
            let before = compiled.execution_counts();
            native_step(&mut native, step);
            graph_step(&mut exact, step, toggle).unwrap();
            graph_step(&mut compiled, step, toggle).unwrap();
            let label = format!("{}/{} step {index}", circuit.id, case["id"]);
            if let Some(cursor) = &compiled.future.symbolic_cursor {
                for binding in &compiled.program.symbolic_kernels()[cursor.kernel].hot_components {
                    assert_eq!(
                        compiled.state.components[binding.node.index()],
                        exact.state.components[binding.node.index()],
                        "{label}: live electrical output {:?}",
                        binding.node
                    );
                }
            } else {
                assert_eq!(compiled.state, exact.state, "{label}: general execution state");
            }
            assert_eq!(compiled.semantic_revisions(), exact.semantic_revisions(), "{label}: full revision totals");
            let mut expected = MachineState::default();
            let mut actual = MachineState::default();
            assert!(exact.capture_graph_boundary(&mut expected), "{label}: reference boundary");
            assert!(compiled.capture_graph_boundary(&mut actual), "{label}: AOT boundary");
            assert_eq!(actual, expected, "{label}: ordered future");
            assert_eq!(piston_effects(&compiled), piston_effects(&exact), "{label}: ordered effects");
            check_boundary(&compiled, &native, &mut scratch, &positions, &label).unwrap();
            if index == activation {
                assert!(compiled.future.symbolic_cursor.is_some(), "{label}: AOT admission must succeed");
                let after = compiled.execution_counts();
                assert_eq!(after.instant_transitions, before.instant_transitions + 1, "{label}: compiled activation");
                assert_eq!(
                    (after.callbacks, after.wire_walks, after.piston_samples),
                    (before.callbacks, before.wire_walks, before.piston_samples),
                    "{label}: activation must not replay propagation"
                );
            } else if index > activation {
                let after = compiled.execution_counts();
                assert_eq!(after.instant_transitions, before.instant_transitions + 1, "{label}: AOT tick");
                assert_eq!(
                    (after.callbacks, after.wire_walks, after.piston_samples),
                    (before.callbacks, before.wire_walks, before.piston_samples)
                );
                let (sum, carry) = expectation(circuit, case);
                assert_math_window(circuit, &scratch, sum, carry, index - activation);
            }
        }
        let mut resumed = circuit.build();
        compiled.reset(&mut resumed);
        for tick in 0..8 {
            native.tick_interpreted();
            resumed.tick_interpreted();
            assert_eq!(
                future(&resumed, &positions),
                future(&native, &positions),
                "{}/{} handoff {tick}",
                circuit.id,
                case["id"]
            );
        }
    }
}

fn piston_effects(backend: &GraphBackend) -> Vec<(BlockPos, PistonAction, RedstonePiston)> {
    backend
        .effects
        .iter()
        .map(|effect| match effect {
            PresentationEffect::BlockAction {
                position,
                action: BlockAction::Piston { action, piston },
            } => (*position, *action, *piston),
            _ => panic!("adder fixture emitted an unexpected host effect"),
        })
        .collect()
}
