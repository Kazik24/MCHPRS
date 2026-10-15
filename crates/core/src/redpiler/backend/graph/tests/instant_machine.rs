//! Real input histories compare complete memory, deadlines and interpreter boundary outputs.
use super::*;

#[test]
fn closed_adder_machine_covers_all_falling_edge_responses_without_turbo() {
    let circuit = load_adder("adder_1bit", ORIGIN, None);
    let (graph, _) = stable_region_tests::stable_graph("adder_1bit");
    let machine = graph.instant_machine.as_ref().expect("the closed one-bit family must compile");
    eprintln!(
        "machine: {} states, {} edges, {} estimated bytes",
        machine.states.len(),
        machine.transitions.iter().flat_map(|r| r.iter()).flatten().count(),
        machine.estimated_bytes()
    );
    let mut baseline = graph.clone();
    baseline.instant_machine = None;
    let (base_program, base_state) = baseline.lower().unwrap();
    let (program, state) = graph.lower().unwrap();
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root().join("test_data/instant-pistons-io/fixtures/adder_1bit.json")).unwrap()).unwrap();
    let positions = circuit.positions();
    let mut cases = 0;
    for case in manifest["cases"].as_array().unwrap().iter().filter(|c| c["expectation"].is_object()) {
        let mut native = circuit.build();
        let mut scratch = circuit.build();
        let mut exact = GraphBackend::new(base_program.clone(), base_state.clone(), 0).unwrap();
        let mut compiled = GraphBackend::new(program.clone(), state.clone(), 0).unwrap();
        let mut steps = case_steps(&circuit, case, false, true);
        let activation = steps.len() - 1;
        steps.extend(std::iter::repeat_n(Step::Tick, 16));
        for (index, step) in steps.into_iter().enumerate() {
            let toggle = match step {
                Step::SetLever { position, powered } => matches!(native.get_block(position), Block::Lever { lever } if lever.powered != powered),
                Step::Tick => false,
            };
            native_step(&mut native, step);
            let before = compiled.execution_counts();
            graph_step(&mut exact, step, toggle).unwrap();
            graph_step(&mut compiled, step, toggle).unwrap();
            assert_memory_and_effects(&exact, &compiled);
            let a = exact.execution_counts();
            let b = compiled.execution_counts();
            assert_eq!(
                (a.ticks, a.scheduled_ticks, a.piston_actions, a.deferred_publications),
                (b.ticks, b.scheduled_ticks, b.piston_actions, b.deferred_publications)
            );
            check_boundary(&compiled, &native, &mut scratch, &positions, &format!("{} step {index}", case["id"])).unwrap();
            if index >= activation {
                assert_eq!(
                    b.instant_transitions,
                    before.instant_transitions + 1,
                    "{} step {index} must use a compiled transition",
                    case["id"]
                );
                assert_eq!(
                    (b.wire_walks, b.callbacks, b.piston_samples),
                    (before.wire_walks, before.callbacks, before.piston_samples)
                );
            }
        }
        cases += 1;
    }
    assert_eq!(cases, 8);
}

fn assert_memory_and_effects(exact: &GraphBackend, compiled: &GraphBackend) {
    instant_proxy_tests::assert_same_memory(exact, compiled);
    let effects = |backend: &GraphBackend| {
        backend
            .effects
            .iter()
            .map(|effect| match effect {
                PresentationEffect::BlockAction {
                    position,
                    action: BlockAction::Piston { action, piston },
                } => format!("{position:?} {action:?} {piston:?}"),
                _ => panic!("closed instant family cannot emit other host effects"),
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(effects(exact), effects(compiled), "presentation order remains observable at flush");
}

#[test]
fn machine_transitions_preserve_revision_wrap_time_origins_and_future_work() {
    let (graph, _) = stable_region_tests::stable_graph("adder_1bit");
    let machine = graph.instant_machine.clone().unwrap();
    let mut baseline = graph.clone();
    baseline.instant_machine = None;
    let (base_program, base_state) = baseline.lower().unwrap();
    let (program, state) = graph.lower().unwrap();
    let mut exact = GraphBackend::new(base_program, base_state, 0).unwrap();
    let mut compiled = GraphBackend::new(program, state, 0).unwrap();
    // Include every admitted pending/deferred shape and every operation, with independent origins.
    for (source, row) in machine.transitions.iter().enumerate() {
        for (action, edge) in row.iter().enumerate() {
            if edge.is_none() {
                continue;
            }
            for backend in [&mut exact, &mut compiled] {
                backend.restore_machine_state(&machine.states[source]).unwrap();
                backend.clock.logical_tick = 10_000;
                for (i, actor) in backend.state.pistons.iter_mut().enumerate() {
                    actor.revision = u64::MAX - i as u64;
                }
                for (i, resource) in backend.state.resources.iter_mut().enumerate() {
                    resource.revision = u64::MAX - 2 * i as u64;
                }
                for event in &mut backend.deferred_pistons {
                    event.due_tick += 10_000 - super::super::machine_state::MACHINE_EPOCH;
                    event.revision = event.revision.wrapping_add(backend.state.pistons[event.piston.index()].revision);
                    if let Some(resource) = &mut event.resource {
                        resource.revision = resource
                            .revision
                            .wrapping_add(backend.state.resources[resource.resource.index()].revision);
                    }
                }
                // Dirty entries and unflushed presentation from previous calls are append-only.
                backend.dirty.fill(true);
            }
            if action == 0 {
                exact.tick().unwrap();
            } else {
                exact.use_block(exact.position(machine.controls[action - 1])).unwrap();
            }
            assert!(compiled.try_machine_step(action));
            assert_memory_and_effects(&exact, &compiled);
        }
    }
}

#[test]
fn exact_state_lookup_and_missing_edges_fall_back_before_effects() {
    let (graph, _) = stable_region_tests::stable_graph("adder_1bit");
    let mut baseline = graph.clone();
    baseline.instant_machine = None;
    let (base_program, base_state) = baseline.lower().unwrap();
    let (program, state) = graph.lower().unwrap();
    let mut exact = GraphBackend::new(base_program, base_state, 0).unwrap();
    let mut compiled = GraphBackend::new(program, state, 0).unwrap();
    // A stale external obligation cannot be normalized as a compiled receiver.
    let node = compiled.program.instant_machine().unwrap().controls[0];
    for backend in [&mut exact, &mut compiled] {
        let registry = backend.registry_type(node);
        let tick = ScheduledNodeTick {
            node: Some(node),
            position: backend.position(node),
            block_type: Some(registry + 1),
            registry_type: registry,
            current_registry_type: registry,
        };
        backend.scheduler.schedule_half_tick(tick, 3, TickPriority::Normal);
        backend.pending.insert(tick.into(), 1);
    }
    assert!(!compiled.try_machine_step(0));
    assert_memory_and_effects(&exact, &compiled);
    for _ in 0..12 {
        exact.tick().unwrap();
        compiled.tick().unwrap();
        assert_memory_and_effects(&exact, &compiled);
    }
    assert!(
        compiled.execution_counts().instant_transitions > 0,
        "exact execution can rejoin the table after stale work expires"
    );
}

#[test]
fn transition_validation_rejects_wrong_writes_deadlines_effects_and_targets() {
    let (graph, _) = stable_region_tests::stable_graph("adder_1bit");
    let original = graph.instant_machine.as_ref().unwrap();
    let (source, action) = original
        .transitions
        .iter()
        .enumerate()
        .find_map(|(source, row)| {
            row.iter()
                .enumerate()
                .find(|(_, edge)| edge.as_ref().is_some_and(|edge| !edge.components.is_empty() && !edge.effects.is_empty()))
                .map(|(action, _)| (source, action))
        })
        .unwrap();
    for corrupt in 0..4 {
        let mut bad = graph.clone();
        let machine = Arc::make_mut(bad.instant_machine.as_mut().unwrap());
        let edge = machine.transitions[source][action].as_mut().unwrap();
        match corrupt {
            0 => edge.components[0].1 ^= 1,
            1 => edge.next = u32::MAX,
            2 => edge.effects.reverse(),
            _ => edge.scheduled += 1,
        }
        if corrupt == 2 {
            edge.effects[0].2 = !edge.effects[0].2;
        }
        assert!(matches!(bad.validate(), Err(GraphBuildError::InvalidInstantMachine { .. })));
    }
    let mut bad = graph.clone();
    let machine = Arc::make_mut(bad.instant_machine.as_mut().unwrap());
    let state = machine.states.iter_mut().find(|state| !state.deferred.is_empty()).unwrap();
    state.deferred[0].delay = 32;
    assert!(matches!(bad.validate(), Err(GraphBuildError::InvalidInstantMachine { .. })));
}

#[test]
fn arbitrary_control_histories_keep_complete_state_even_when_table_misses() {
    let (graph, _) = stable_region_tests::stable_graph("adder_1bit");
    let controls = graph.instant_machine.as_ref().unwrap().controls.clone();
    let mut baseline = graph.clone();
    baseline.instant_machine = None;
    let (base_program, base_state) = baseline.lower().unwrap();
    let (program, state) = graph.lower().unwrap();
    let mut exact = GraphBackend::new(base_program, base_state, 0).unwrap();
    let mut compiled = GraphBackend::new(program, state, 0).unwrap();
    let mut random = 0x27182818u32;
    for _ in 0..1000 {
        random = random.wrapping_mul(1664525).wrapping_add(1013904223);
        let action = (random >> 16) as usize % (controls.len() + 2);
        if action < controls.len() {
            let position = exact.position(controls[action]);
            exact.use_block(position).unwrap();
            compiled.use_block(position).unwrap();
        } else {
            exact.tick().unwrap();
            compiled.tick().unwrap();
        }
        assert_memory_and_effects(&exact, &compiled);
    }
}

#[test]
fn machine_compilation_is_opt_in_cancellable_and_invalidated_by_graph_changes() {
    use crate::redpiler::passes::compile_instant_machine::CompileInstantMachine;
    use crate::redpiler::passes::{GraphPass, PassContext, PassEntry, PassPipeline};
    let (graph, entry) = stable_region_tests::stable_graph("adder_1bit");
    let options = crate::redpiler::CompilerOptions {
        optimization_level: Some(3),
        stable_instant_inputs: true,
        ..Default::default()
    };
    let monitor = TaskMonitor::default();
    monitor.cancel();
    let mut cancelled = graph.clone();
    let original = cancelled.instant_machine.clone().unwrap();
    assert!(
        CompileInstantMachine
            .run(
                &mut cancelled,
                &PassContext {
                    monitor: &monitor,
                    options: &options,
                    entry: &entry
                }
            )
            .is_err()
    );
    assert!(
        Arc::ptr_eq(&original, cancelled.instant_machine.as_ref().unwrap()),
        "cancellation publishes no partial artifact"
    );
    let monitor = TaskMonitor::default();
    let mut ordinary = graph.clone();
    CompileInstantMachine
        .run(
            &mut ordinary,
            &PassContext {
                monitor: &monitor,
                options: &Default::default(),
                entry: &entry,
            },
        )
        .unwrap();
    assert!(ordinary.instant_machine.is_none());
    struct UnknownRewrite;
    impl GraphPass for UnknownRewrite {
        fn name(&self) -> &'static str {
            "test-unknown-rewrite"
        }
        fn run(&self, _: &mut CompileGraph, _: &PassContext<'_>) -> Result<(), String> {
            Ok(())
        }
    }
    for enabled in [false, true] {
        let mut output = graph.clone();
        passes::run_graph_pipeline(
            &mut output,
            &options,
            &entry,
            &PassPipeline::new(vec![PassEntry {
                pass: Box::new(UnknownRewrite),
                enabled,
            }]),
            &monitor,
        )
        .unwrap();
        assert_eq!(output.instant_machine.is_some(), !enabled);
    }
    let mut rebuilt = graph.clone();
    CompileInstantMachine
        .run(
            &mut rebuilt,
            &PassContext {
                monitor: &monitor,
                options: &options,
                entry: &entry,
            },
        )
        .unwrap();
    assert_eq!(rebuilt.instant_machine, graph.instant_machine, "deterministic rebuild is idempotent");
}
