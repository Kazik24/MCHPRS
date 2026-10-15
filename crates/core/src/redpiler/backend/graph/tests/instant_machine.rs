//! Real input histories compare complete memory, deadlines and interpreter boundary outputs.
use super::*;

#[test]
fn closed_adder_machine_covers_all_falling_edge_responses_without_turbo() {
    let circuit = load_adder("adder_1bit", ORIGIN, None);
    let (graph, _) = stable_region_tests::stable_graph_at_level("adder_1bit", 4);
    let machine = graph.instant_machine.as_ref().expect("the closed one-bit family must compile");
    eprintln!(
        "compile certificate: {} states, {} edges, {} bytes incl. kernel; runtime kernel: {} phases, {} expressions, {} bytes",
        machine.states.len(),
        machine.transitions.iter().flat_map(|r| r.iter()).flatten().count(),
        machine.estimated_bytes(),
        machine.kernel.phases.len(),
        machine.kernel.expressions.len(),
        machine.kernel_bytes()
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
            if index > activation {
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
fn compact_kernel_phases_preserve_revision_origins_and_future_work() {
    let circuit = load_adder("adder_1bit", ORIGIN, None);
    let (graph, _) = stable_region_tests::stable_graph_at_level("adder_1bit", 4);
    let mut baseline = graph.clone();
    baseline.instant_machine = None;
    let (base_program, base_state) = baseline.lower().unwrap();
    let (program, state) = graph.lower().unwrap();
    let mut exact = GraphBackend::new(base_program, base_state, 0).unwrap();
    let mut compiled = GraphBackend::new(program, state, 0).unwrap();
    let mut native = circuit.build();
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root().join("test_data/instant-pistons-io/fixtures/adder_1bit.json")).unwrap()).unwrap();
    let case = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["expectation"]["carry"] == 1)
        .unwrap()
        .clone();
    let steps = case_steps(&circuit, &case, false, true);
    for (index, &step) in steps.iter().enumerate() {
        if index + 1 == steps.len() {
            for backend in [&mut exact, &mut compiled] {
                for (actor, state) in backend.state.pistons.iter_mut().enumerate() {
                    state.revision = u64::MAX - actor as u64;
                }
                for (resource, state) in backend.state.resources.iter_mut().enumerate() {
                    state.revision = u64::MAX - 2 * resource as u64;
                }
            }
        }
        let toggle = match step {
            Step::SetLever { position, powered } => matches!(native.get_block(position), Block::Lever { lever } if lever.powered != powered),
            Step::Tick => false,
        };
        native_step(&mut native, step);
        graph_step(&mut exact, step, toggle).unwrap();
        graph_step(&mut compiled, step, toggle).unwrap();
        assert_memory_and_effects(&exact, &compiled);
    }
    for _ in 0..64 {
        exact.tick().unwrap();
        compiled.tick().unwrap();
        assert_memory_and_effects(&exact, &compiled);
    }
    assert!(compiled.execution_counts().instant_transitions >= 64);
}

#[test]
fn stale_entry_work_prevents_compact_kernel_admission() {
    let (graph, _) = stable_region_tests::stable_graph_at_level("adder_1bit", 4);
    let mut baseline = graph.clone();
    baseline.instant_machine = None;
    let (base_program, base_state) = baseline.lower().unwrap();
    let (program, state) = graph.lower().unwrap();
    let mut exact = GraphBackend::new(base_program, base_state, 0).unwrap();
    let mut compiled = GraphBackend::new(program, state, 0).unwrap();
    let node = compiled.program.instant_kernel().unwrap().data_controls[0];
    let circuit = load_adder("adder_1bit", ORIGIN, None);
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root().join("test_data/instant-pistons-io/fixtures/adder_1bit.json")).unwrap()).unwrap();
    let case = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["expectation"].is_object())
        .unwrap();
    let mut native = circuit.build();
    let steps = case_steps(&circuit, case, false, true);
    let activation_index = steps.len() - 1;
    for (index, step) in steps.into_iter().enumerate() {
        if index == activation_index {
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
        }
        let toggle = match step {
            Step::SetLever { position, powered } => matches!(native.get_block(position), Block::Lever { lever } if lever.powered != powered),
            Step::Tick => false,
        };
        native_step(&mut native, step);
        graph_step(&mut exact, step, toggle).unwrap();
        graph_step(&mut compiled, step, toggle).unwrap();
        assert_memory_and_effects(&exact, &compiled);
    }
    assert_eq!(
        compiled.execution_counts().instant_transitions,
        0,
        "unrepresentable work prevents kernel admission"
    );
    for _ in 0..12 {
        exact.tick().unwrap();
        compiled.tick().unwrap();
        assert_memory_and_effects(&exact, &compiled);
    }
    assert!(
        compiled.execution_counts().instant_transitions == 0,
        "a missed activation stays on the exact Graph executor"
    );
}

#[test]
fn transition_validation_rejects_wrong_writes_deadlines_effects_and_targets() {
    let (graph, _) = stable_region_tests::stable_graph_at_level("adder_1bit", 4);
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
fn compact_kernel_validation_rejects_malformed_programs_before_lowering() {
    let (graph, _) = stable_region_tests::stable_graph_at_level("adder_1bit", 4);
    let valid = graph.instant_machine.as_ref().unwrap();
    let select = valid
        .kernel
        .expressions
        .iter()
        .position(|expression| matches!(expression, KernelExpr::Select { .. }))
        .unwrap();
    for corruption in 0..6 {
        let mut bad = graph.clone();
        let machine = Arc::make_mut(bad.instant_machine.as_mut().unwrap());
        match corruption {
            0 => {
                if let KernelExpr::Select { low, .. } = &mut machine.kernel.expressions[select] {
                    *low = KernelExprId(select as u32);
                }
            }
            1 => {
                if let KernelExpr::Select { high, .. } = &mut machine.kernel.expressions[select] {
                    *high = KernelExprId(u32::MAX);
                }
            }
            2 => {
                if let KernelExpr::Select { control, .. } = &mut machine.kernel.expressions[select] {
                    *control = u8::MAX;
                }
            }
            3 => machine.kernel.entry.components[0] = KernelExprId(u32::MAX),
            4 => machine.kernel.period = 0,
            _ => {
                machine.kernel.prefix = u16::MAX;
                machine.kernel.period = 1;
            }
        }
        assert!(
            matches!(bad.validate(), Err(GraphBuildError::InvalidInstantMachine { .. })),
            "malformed kernel case {corruption} must be rejected before DAG evaluation"
        );
    }
}

#[test]
fn compact_kernel_validation_rederives_hot_effect_order_and_cold_future_roots() {
    let (graph, _) = stable_region_tests::stable_graph_at_level("adder_1bit", 4);
    let kernel = &graph.instant_machine.as_ref().unwrap().kernel;
    let other_root = |original: KernelExprId| {
        (0..kernel.expressions.len())
            .map(|index| KernelExprId(index as u32))
            .find(|&candidate| candidate != original)
            .unwrap()
    };
    let mut hot = graph.clone();
    let machine = Arc::make_mut(hot.instant_machine.as_mut().unwrap());
    let delta = machine.kernel.phases.iter_mut().find_map(|phase| phase.components.first_mut()).unwrap();
    delta.value = other_root(delta.value);
    assert!(matches!(hot.validate(), Err(GraphBuildError::InvalidInstantMachine { .. })));

    let mut future = graph.clone();
    let machine = Arc::make_mut(future.instant_machine.as_mut().unwrap());
    let phase = machine
        .kernel
        .phases
        .iter_mut()
        .find(|phase| !phase.state.future.ticks.is_empty())
        .unwrap();
    phase.state.future.tick_len = other_root(phase.state.future.tick_len);
    assert!(matches!(future.validate(), Err(GraphBuildError::InvalidInstantMachine { .. })));

    let mut effects = graph.clone();
    let machine = Arc::make_mut(effects.instant_machine.as_mut().unwrap());
    let phase = machine
        .kernel
        .phases
        .iter_mut()
        .find(|phase| phase.effects.windows(2).any(|pair| pair[0] != pair[1]))
        .unwrap();
    let index = phase.effects.windows(2).position(|pair| pair[0] != pair[1]).unwrap();
    phase.effects.swap(index, index + 1);
    assert!(matches!(effects.validate(), Err(GraphBuildError::InvalidInstantMachine { .. })));
}

#[test]
fn arbitrary_control_histories_deopt_and_keep_complete_state() {
    let (graph, _) = stable_region_tests::stable_graph_at_level("adder_1bit", 4);
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
    let (graph, entry) = stable_region_tests::stable_graph_at_level("adder_1bit", 4);
    let options = crate::redpiler::CompilerOptions {
        optimization_level: Some(4),
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
