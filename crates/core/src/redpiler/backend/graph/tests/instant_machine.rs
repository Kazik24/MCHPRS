//! Real input histories compare complete memory, deadlines and interpreter boundary outputs.
use super::*;
use crate::redpiler::analysis::instant_regions::symbolic::{SymbolicBuilder, SymbolicLimits, SymbolicValue, ValueKind};
use crate::redpiler::passes::Pass;
use crate::redpiler::passes::planning::PlanBuilder;
use crate::redpiler::program_graph::{CircuitNodeKind, KernelBoundary, KernelInput, SiteSource};

#[test]
fn lowered_kernel_effects_match_every_frozen_assignment_and_preserve_occurrences() {
    let (graph, _) = stable_region_tests::stable_graph_at_level("adder_1bit", 4);
    let (program, _) = graph.lower().unwrap();
    let kernel = program.instant_kernel().unwrap();
    let execution = program.instant_execution().unwrap();
    let mut candidates = 0;
    let mut retained = 0;
    for mask in 0..(1u8 << kernel.data_controls.len()) {
        for (index, phase) in kernel.phases.iter().enumerate() {
            let actual = execution.phase(mask, index).unwrap();
            let writes: Vec<_> = phase
                .components
                .iter()
                .filter(|delta| kernel.evaluate(delta.changed, mask) != 0)
                .map(|delta| (delta.node, kernel.evaluate(delta.value, mask) as u16))
                .collect();
            let dirty: Vec<_> = phase
                .dirty
                .iter()
                .filter(|&&(_, changed)| kernel.evaluate(changed, mask) != 0)
                .map(|&(node, _)| node)
                .collect();
            let effects: Vec<_> = phase
                .effects
                .iter()
                .filter(|effect| kernel.evaluate(effect.enabled, mask) != 0)
                .map(|effect| (effect.piston, effect.action, effect.extended))
                .collect();
            assert_eq!(actual.writes.as_ref(), writes);
            assert_eq!(actual.dirty.as_ref(), dirty);
            assert_eq!(actual.effects.as_ref(), effects);
            assert_eq!(actual.scheduled, kernel.evaluate(phase.scheduled, mask));
            assert_eq!(actual.published, kernel.evaluate(phase.published, mask));
            candidates += phase.components.len() + phase.dirty.len() + phase.effects.len();
            retained += actual.writes.len() + actual.dirty.len() + actual.effects.len();
        }
    }
    assert!(
        retained < candidates,
        "lowering must remove inactive operations, not just move their checks"
    );
    assert!(execution.phase(8, 0).is_none());
    assert!(execution.phase(0, kernel.phases.len()).is_none());
    eprintln!(
        "kernel candidate operations={candidates}, active operations={retained}, execution bytes={}",
        execution.estimated_bytes()
    );
}

#[test]
fn adder_symbolic_ir_uses_graph_ports_without_compiling_finite_machine() {
    let circuit = load_adder("adder_1bit", ORIGIN, None);
    let (graph, _) = stable_region_tests::stable_graph("adder_1bit");
    assert!(
        graph.plans.instant.machine.is_none(),
        "O3 symbolic tracing must not invoke finite compilation"
    );
    let boundary = KernelBoundary::from_region(&graph.plans.instant.regions[0]);
    let mut data_ports = [None; 3];
    for (port, input) in boundary.inputs.iter().enumerate() {
        let KernelInput::Power(link) = input else { continue };
        let link = graph.power_links[*link as usize];
        let SiteSource::Component(source) = graph.sites[link.from.index()].source else {
            continue;
        };
        if graph.nodes[source.index()].kind != CircuitNodeKind::Electrical(crate::redpiler::components::NodeType::Lever) {
            continue;
        }
        if let Some(input_index) = ["A", "B", "Cin"].iter().position(|name| {
            let position = circuit.port_positions("inputs", name)[0];
            graph.nodes[source.index()].position == Some(position)
        }) {
            data_ports[input_index] = Some(port);
        }
    }
    assert!(data_ports.iter().all(Option::is_some), "graph boundary must expose A, B, and Cin");

    let mut builder = SymbolicBuilder::new(
        boundary.clone(),
        vec![ValueKind::Boolean; boundary.state.len()],
        SymbolicLimits::default(),
    )
    .unwrap();
    let inputs: Vec<_> = data_ports
        .iter()
        .map(|port| builder.input(port.unwrap(), ValueKind::Boolean).unwrap())
        .collect();
    let ab = builder.xor(inputs[0], inputs[1]).unwrap();
    let sum = builder.xor(ab, inputs[2]).unwrap();
    let carry_ab = builder.and(inputs[0], inputs[1]).unwrap();
    let carry_ac = builder.and(inputs[0], inputs[2]).unwrap();
    let carry_bc = builder.and(inputs[1], inputs[2]).unwrap();
    let carry_bc_or_ac = builder.or(carry_ac, carry_bc).unwrap();
    let carry = builder.or(carry_ab, carry_bc_or_ac).unwrap();
    builder.hot_root(sum).unwrap();
    builder.hot_root(carry).unwrap();
    let program = builder.finish();
    let mut values = vec![SymbolicValue::Boolean(false); program.expressions.len()];
    let mut port_values = vec![SymbolicValue::Boolean(false); boundary.inputs.len()];

    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root().join("test_data/instant-pistons-io/fixtures/adder_1bit.json")).unwrap()).unwrap();
    let cases = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["expectation"].is_object());
    let mut count = 0;
    for case in cases {
        let bits = [
            case["inputs"][0].as_u64().unwrap() != 0,
            case["inputs"][1].as_u64().unwrap() != 0,
            case["inputs"][2].as_u64().unwrap() != 0,
        ];
        for (index, port) in data_ports.iter().enumerate() {
            port_values[port.unwrap()] = SymbolicValue::Boolean(bits[index]);
        }
        program.evaluate(&port_values, &[], &mut values).unwrap();
        let actual = (
            u16::from(values[sum.index()] == SymbolicValue::Boolean(true)),
            u16::from(values[carry.index()] == SymbolicValue::Boolean(true)),
        );
        assert_eq!(actual, expectation(&circuit, case), "symbolic IR case {}", case["id"]);
        let mut steps = case_steps(&circuit, case, false, true);
        let response_start = steps.len();
        steps.extend(std::iter::repeat_n(Step::Tick, case["ticks"].as_u64().unwrap() as usize));
        replay(&circuit, &steps, Some(response_start), Some(actual), None)
            .unwrap_or_else(|error| panic!("symbolic IR Graph/interpreter comparison for {}: {error}", case["id"]));
        count += 1;
    }
    assert_eq!(count, 8);
}

#[test]
fn closed_adder_machine_covers_all_falling_edge_responses_without_turbo() {
    let circuit = load_adder("adder_1bit", ORIGIN, None);
    let (graph, _) = stable_region_tests::stable_graph_at_level("adder_1bit", 4);
    let machine = graph.plans.instant.machine.as_ref().expect("the closed one-bit family must compile");
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
    baseline.plans.instant.machine = None;
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
    baseline.plans.instant.machine = None;
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
    baseline.plans.instant.machine = None;
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
                backend.future.scheduler.schedule_half_tick(tick, 3, TickPriority::Normal);
                backend.future.pending.insert(tick.into(), 1);
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
    let original = graph.plans.instant.machine.as_ref().unwrap();
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
        let machine = Arc::make_mut(bad.plans.instant.machine.as_mut().unwrap());
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
    let machine = Arc::make_mut(bad.plans.instant.machine.as_mut().unwrap());
    let state = machine.states.iter_mut().find(|state| !state.deferred.is_empty()).unwrap();
    state.deferred[0].delay = 32;
    assert!(matches!(bad.validate(), Err(GraphBuildError::InvalidInstantMachine { .. })));
}

#[test]
fn compact_kernel_validation_rejects_malformed_programs_before_lowering() {
    let (graph, _) = stable_region_tests::stable_graph_at_level("adder_1bit", 4);
    let valid = graph.plans.instant.machine.as_ref().unwrap();
    let select = valid
        .kernel
        .expressions
        .iter()
        .position(|expression| matches!(expression, KernelExpr::Select { .. }))
        .unwrap();
    for corruption in 0..6 {
        let mut bad = graph.clone();
        let machine = Arc::make_mut(bad.plans.instant.machine.as_mut().unwrap());
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
    let kernel = &graph.plans.instant.machine.as_ref().unwrap().kernel;
    let other_root = |original: KernelExprId| {
        (0..kernel.expressions.len())
            .map(|index| KernelExprId(index as u32))
            .find(|&candidate| candidate != original)
            .unwrap()
    };
    let mut hot = graph.clone();
    let machine = Arc::make_mut(hot.plans.instant.machine.as_mut().unwrap());
    let delta = machine.kernel.phases.iter_mut().find_map(|phase| phase.components.first_mut()).unwrap();
    delta.value = other_root(delta.value);
    assert!(matches!(hot.validate(), Err(GraphBuildError::InvalidInstantMachine { .. })));

    let mut future = graph.clone();
    let machine = Arc::make_mut(future.plans.instant.machine.as_mut().unwrap());
    let phase = machine
        .kernel
        .phases
        .iter_mut()
        .find(|phase| !phase.state.future.ticks.is_empty())
        .unwrap();
    phase.state.future.tick_len = other_root(phase.state.future.tick_len);
    assert!(matches!(future.validate(), Err(GraphBuildError::InvalidInstantMachine { .. })));

    let mut effects = graph.clone();
    let machine = Arc::make_mut(effects.plans.instant.machine.as_mut().unwrap());
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
    let controls = graph.plans.instant.machine.as_ref().unwrap().controls.clone();
    let mut baseline = graph.clone();
    baseline.plans.instant.machine = None;
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
    let original = cancelled.plans.instant.machine.clone().unwrap();
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
        Arc::ptr_eq(&original, cancelled.plans.instant.machine.as_ref().unwrap()),
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
    assert!(ordinary.plans.instant.machine.is_none());
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
                pass: Pass::rewrite(UnknownRewrite),
                enabled,
            }]),
            &monitor,
        )
        .unwrap();
        assert_eq!(output.plans.instant.machine.is_some(), !enabled);
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
    assert_eq!(
        rebuilt.plans.instant.machine, graph.plans.instant.machine,
        "deterministic rebuild is idempotent"
    );
}
