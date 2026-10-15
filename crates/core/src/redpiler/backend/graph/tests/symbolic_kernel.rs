//! A real-fixture power-query slice; the full piston lifecycle stays on Graph.
use super::*;
use crate::redpiler::CompilerOptions;
use crate::redpiler::analysis::AnalysisLimits;
use crate::redpiler::analysis::instant_regions::symbolic::{SymbolicLimits, SymbolicValue, compile_power_query};
use crate::redpiler::passes::rich;
use crate::redpiler::program_graph::{CircuitNodeKind, InputPort, NodeId, PowerTarget};

pub(super) fn load_mechanism(id: &'static str) -> Adder {
    let directory = root().join("test_data/instant-pistons-io");
    let manifest: Value = serde_json::from_slice(&std::fs::read(directory.join("fixtures").join(format!("{id}.json"))).unwrap()).unwrap();
    let bytes = std::fs::read(root().join(manifest["fixture"].as_str().unwrap())).unwrap();
    let hash = format!("{:x}", Sha256::digest(&bytes));
    assert_eq!(manifest["sha256"], hash);

    let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    let size = BlockPos::new(clipboard.size_x as i32, clipboard.size_y as i32, clipboard.size_z as i32);
    let mut imported = empty_world();
    paste_clipboard(
        &mut imported,
        &clipboard,
        ORIGIN + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z),
        false,
    );
    let mut cells = Vec::new();
    for y in 0..size.y {
        for z in 0..size.z {
            for x in 0..size.x {
                let position = BlockPos::new(x, y, z);
                let block = imported.get_block(ORIGIN + position);
                let entity = imported.get_block_entity(ORIGIN + position).cloned();
                if block != Block::Air || entity.is_some() {
                    cells.push((position, block, entity));
                }
            }
        }
    }
    Adder {
        id,
        hash,
        size,
        anchor: ORIGIN,
        rotation: None,
        cells,
        ports: manifest["ports"].clone(),
    }
}

fn stable_graph(circuit: &Adder) -> (crate::redpiler::program_graph::CompileGraph, crate::redpiler::program_graph::EntryState) {
    stable_graph_at(circuit, 3)
}

fn stable_graph_at(circuit: &Adder, level: usize) -> (crate::redpiler::program_graph::CompileGraph, crate::redpiler::program_graph::EntryState) {
    let world = circuit.build();
    let monitor = TaskMonitor::default();
    let mut prepared = rich::prepare(&world, circuit.bounds(), vec![], &monitor, AnalysisLimits::default()).unwrap();
    let options = CompilerOptions {
        optimization_level: Some(level),
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
    (prepared.graph, prepared.entry)
}

fn stable_graph_in_bounds(
    circuit: &Adder,
    level: usize,
    bounds: (BlockPos, BlockPos),
) -> (crate::redpiler::program_graph::CompileGraph, crate::redpiler::program_graph::EntryState) {
    let world = circuit.build();
    let monitor = TaskMonitor::default();
    let mut prepared = rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let options = CompilerOptions {
        optimization_level: Some(level),
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
    (prepared.graph, prepared.entry)
}

#[test]
fn power_query_checks_attenuation_independent_channels_and_declines() {
    use crate::redpiler::analysis::instant_regions::symbolic::{SymbolicDecline, SymbolicError};
    use crate::redpiler::program_graph::{Guard, GuardId, InputPort, PowerTarget};
    let circuit = load_mechanism("instant_observer");
    let (mut graph, _) = stable_graph(&circuit);
    let mut region = graph.plans.instant.regions[0].clone();
    let actor = region.actors[0];
    let target = graph.pistons[actor.index()].node;
    let mut link = *graph
        .power_links
        .iter()
        .find(|link| matches!(link.to, PowerTarget::Input { node, .. } if node == target))
        .unwrap();
    let always = GuardId::new(graph.guards.len() as u32);
    graph.guards.push(Guard::Always);
    link.guard = always;
    link.to = PowerTarget::Input {
        node: target,
        port: InputPort::Main,
    };
    let mut quasi = link;
    quasi.to = PowerTarget::Input {
        node: target,
        port: InputPort::Quasi,
    };
    graph.power_links = vec![link, quasi];
    region.power_inputs = vec![0, 1];
    region.incoming.clear();
    for attenuation in [0, 1, 15] {
        graph.power_links[0].attenuation = attenuation;
        let query = compile_power_query(&graph, &region, target, SymbolicLimits::default()).unwrap().unwrap();
        let mut scratch = vec![SymbolicValue::Boolean(false); query.program.expressions.len()];
        for main in 0u8..=15 {
            for qc in 0u8..=15 {
                query
                    .program
                    .evaluate(&[SymbolicValue::Signal(main), SymbolicValue::Signal(qc)], &[], &mut scratch)
                    .unwrap();
                assert_eq!(
                    scratch[query.powered.index()],
                    SymbolicValue::Boolean(main.saturating_sub(attenuation).max(qc) > 0)
                );
            }
        }
    }
    region.power_inputs.pop();
    assert_eq!(
        compile_power_query(&graph, &region, target, SymbolicLimits::default()).unwrap(),
        Err(SymbolicDecline::InternalPowerLink)
    );
    region.power_inputs.push(1);
    graph.guards.push(Guard::Not(always));
    graph.power_links[0].guard = GuardId::new(graph.guards.len() as u32 - 1);
    assert_eq!(
        compile_power_query(&graph, &region, target, SymbolicLimits::default()).unwrap(),
        Err(SymbolicDecline::GuardedPowerLink)
    );
    let limits = SymbolicLimits {
        max_expressions: 0,
        ..Default::default()
    };
    assert_eq!(compile_power_query(&graph, &region, target, limits), Err(SymbolicError::BudgetExceeded));
    assert_eq!(
        compile_power_query(&graph, &region, NodeId::new(u32::MAX), SymbolicLimits::default()).unwrap(),
        Err(SymbolicDecline::TargetOutsideRegion)
    );
}

#[test]
fn symbolic_actor_runs_request_accept_reset_and_exact_handoff() {
    let circuit = load_mechanism("instant_observer");
    let positions = circuit.positions();
    let trigger = circuit.trigger();
    let (graph, _) = stable_graph_at(&circuit, 4);
    assert!(
        graph.plans.instant.symbolic_kernels.is_empty(),
        "regional execution does not publish the whole-selection AOT cursor"
    );
    assert_eq!(graph.plans.instant.symbolic_regions.len(), 1);
    assert!(graph.plans.instant.machine.is_none());
    let reset = graph.plans.instant.symbolic_regions[0].resets[0].node;
    let mut general_graph = graph.clone();
    general_graph.plans.instant.symbolic_regions.clear();
    general_graph.plans.instant.symbolic_region_declines.clear();
    let (program, state) = graph.lower().unwrap();
    let (general_program, general_state) = general_graph.lower().unwrap();
    let mut saw_request = false;
    let mut saw_extend = false;
    let mut saw_retract = false;
    let mut saw_reset_pending = false;
    let mut saw_reset_powered = false;
    for cut in [1, 2, 3, 8, 24] {
        let mut native = circuit.build();
        let mut actual_world = circuit.build();
        let mut general_world = circuit.build();
        let mut actual = GraphBackend::new(program.clone(), state.clone(), 0).unwrap();
        let mut general = GraphBackend::new(general_program.clone(), general_state.clone(), 0).unwrap();
        actual.set_test_recording(true);
        general.set_test_recording(true);
        let step = Step::SetLever {
            position: trigger,
            powered: true,
        };
        native_step(&mut native, step);
        graph_step(&mut actual, step, true).unwrap();
        graph_step(&mut general, step, true).unwrap();
        for tick in 0..cut {
            native.tick_interpreted();
            actual.tick().unwrap();
            general.tick().unwrap();
            check_boundary(
                &actual,
                &native,
                &mut actual_world,
                &positions,
                &format!("symbolic cut={cut} tick={tick}"),
            )
            .unwrap();
            project(&general, &mut general_world);
            assert_eq!(future(&actual_world, &positions), future(&general_world, &positions));
            assert_eq!(actual.callback_order, general.callback_order);
            assert_eq!(actual.observed_operations, general.observed_operations);
            saw_request |= actual
                .observed_operations
                .iter()
                .any(|op| matches!(op, GraphObservedOperation::Request { .. }));
            saw_extend |= actual.observed_operations.iter().any(|op| {
                matches!(
                    op,
                    GraphObservedOperation::Accepted {
                        action: PistonAction::Extend,
                        ..
                    }
                )
            });
            saw_retract |= actual.observed_operations.iter().any(|op| {
                matches!(
                    op,
                    GraphObservedOperation::Accepted {
                        action: PistonAction::Retract | PistonAction::RetractWithoutPull,
                        ..
                    }
                )
            });
            saw_reset_pending |= actual.is_pending(reset);
            saw_reset_powered |= actual.node_state(reset).powered;
        }
        if cut >= 3 {
            assert!(
                actual.execution_counts().symbolic_region_steps > 0,
                "the symbolic program must execute after the input-torch delay"
            );
        }
        for tick in 0..8 {
            native.tick_interpreted();
            actual_world.tick_interpreted();
            assert_eq!(
                future(&actual_world, &positions),
                future(&native, &positions),
                "handoff cut={cut} tick={tick}"
            );
        }
    }
    assert!(saw_request && saw_extend && saw_retract, "fixture must execute both accepted directions");
    assert!(
        saw_reset_pending && saw_reset_powered,
        "fixture must schedule and execute its reset clock"
    );
}

#[test]
fn external_component_callback_runs_region_without_selected_lever_or_global_tick_gate() {
    use crate::redpiler::components::NodeType;
    use crate::redpiler::program_graph::ComponentState;
    use mchprs_world::TickPriority;

    let circuit = load_mechanism("instant_observer");
    let (mut low, high) = circuit.bounds();
    low.z = circuit.trigger().z + 1;
    let (graph, _) = stable_graph_in_bounds(&circuit, 4, (low, high));
    assert!(
        graph
            .nodes
            .iter()
            .all(|node| !matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Lever)))
    );
    assert!(graph.plans.instant.machine.is_none());
    assert!(graph.plans.instant.symbolic_kernels.is_empty());
    assert_eq!(graph.plans.instant.symbolic_region_declines.len(), 0);
    assert!(!graph.plans.instant.symbolic_regions.is_empty());

    let region = &graph.plans.instant.regions[0];
    let event_source = region
        .incoming
        .iter()
        .map(|event| graph.routes[event.route.index()].source)
        .find(|&node| {
            matches!(
                graph.nodes[node.index()].kind,
                CircuitNodeKind::Electrical(NodeType::Torch | NodeType::Repeater { .. })
            )
        })
        .expect("fixture has an ordinary external source callback into the owned region");
    assert!(!region.actors.iter().any(|actor| graph.pistons[actor.index()].node == event_source));
    let unrelated = graph
        .nodes
        .iter()
        .enumerate()
        .find_map(|(index, node)| {
            (matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Repeater { .. }))
                && NodeId::new(index as u32) != event_source
                && !region.resets.contains(&NodeId::new(index as u32)))
            .then_some(NodeId::new(index as u32))
        })
        .expect("fixture has an unrelated repeater with ordinary scheduled work");

    let mut general_graph = graph.clone();
    general_graph.plans.instant.symbolic_regions.clear();
    general_graph.plans.instant.symbolic_region_declines.clear();
    let (program, state) = graph.lower().unwrap();
    let (general_program, general_state) = general_graph.lower().unwrap();
    let mut actual = GraphBackend::new(program, state, 0).unwrap();
    let mut general = GraphBackend::new(general_program, general_state, 0).unwrap();
    actual.set_test_recording(true);
    general.set_test_recording(true);
    actual.schedule(unrelated, 1, TickPriority::Normal);
    general.schedule(unrelated, 1, TickPriority::Normal);
    let next_power = match &actual.state.components[event_source.index()] {
        ComponentState::Electrical(source_state) => !source_state.powered,
        _ => panic!("ordinary callback source has electrical state"),
    };
    actual
        .set_node(event_source, next_power, if next_power { 15 } else { 0 }, None, true)
        .unwrap();
    general
        .set_node(event_source, next_power, if next_power { 15 } else { 0 }, None, true)
        .unwrap();

    assert!(
        actual.execution_counts().symbolic_region_steps > 0,
        "the callback must enter a compiled actor or reset routine"
    );
    assert_eq!(general.execution_counts().symbolic_region_steps, 0);
    assert!(actual.is_pending(unrelated), "an unrelated pending tick remains in the global scheduler");
    instant_proxy_tests::assert_same_memory(&general, &actual);
    assert_eq!(actual.callback_order, general.callback_order);
    assert_eq!(actual.observed_operations, general.observed_operations);

    let scheduled_before = actual.execution_counts().scheduled_ticks;
    actual.clear_test_traces();
    general.clear_test_traces();
    for _ in 0..2 {
        actual.tick().unwrap();
        general.tick().unwrap();
    }
    instant_proxy_tests::assert_same_memory(&general, &actual);
    assert!(
        actual.execution_counts().scheduled_ticks > scheduled_before,
        "the unrelated device's pending tick executes independently"
    );
    assert!(actual.trace().iter().any(|line| line == &format!("dispatch {unrelated:?}")));
    assert!(general.trace().iter().any(|line| line == &format!("dispatch {unrelated:?}")));
    assert!(actual.execution_counts().symbolic_region_steps > 0);
    assert_eq!(actual.observed_operations, general.observed_operations);
}

#[test]
fn instant_torch_graph_interpreter_reset_pending_work_and_handoff_match() {
    let circuit = load_mechanism("instant_torch");
    let trigger = circuit.trigger();
    let positions = circuit.positions();
    let mut saw_pending_work = false;
    for powered in [false, true] {
        let mut native = circuit.build();
        let mut scratch = circuit.build();
        let bounds = circuit.bounds();
        let mut backend = GraphBackend::compile(&native, bounds, vec![], Arc::default()).unwrap();
        let toggle = matches!(native.get_block(trigger), Block::Lever { lever } if lever.powered != powered);
        let step = Step::SetLever { position: trigger, powered };
        native_step(&mut native, step);
        graph_step(&mut backend, step, toggle).unwrap();
        for tick in 0..24 {
            native.tick_interpreted();
            backend.tick().unwrap();
            check_boundary(
                &backend,
                &native,
                &mut scratch,
                &positions,
                &format!("instant_torch power={powered} tick={tick}"),
            )
            .unwrap();
            let projected = future(&scratch, &positions);
            saw_pending_work |= !projected.scheduled.is_empty() || !projected.events.is_empty() || !projected.motions.is_empty();
        }
        let mut resumed = circuit.build();
        project(&backend, &mut resumed);
        for tick in 0..8 {
            native.tick_interpreted();
            resumed.tick_interpreted();
            assert_eq!(
                future(&native, &positions),
                future(&resumed, &positions),
                "interpreter handoff power={powered} tick={tick}"
            );
        }
    }
    assert!(saw_pending_work, "fixture activation must exercise retained work before reset");
}

#[test]
fn symbolic_requests_are_deduplicated_and_revalidated_against_live_reset_power() {
    use crate::redpiler::program_graph::ComponentState;
    let circuit = load_mechanism("instant_observer");
    let (graph, _) = stable_graph_at(&circuit, 4);
    let actor = graph.plans.instant.symbolic_regions[0].actors[0].actor;
    let reset = graph.plans.instant.symbolic_regions[0].resets[0].node;
    let node = graph.pistons[actor.index()].node;
    let mut general_graph = graph.clone();
    general_graph.plans.instant.symbolic_regions.clear();
    general_graph.plans.instant.symbolic_region_declines.clear();
    let (program, state) = graph.lower().unwrap();
    let (general_program, general_state) = general_graph.lower().unwrap();
    let mut actual = GraphBackend::new(program, state, 0).unwrap();
    let mut general = GraphBackend::new(general_program, general_state, 0).unwrap();
    for backend in [&mut actual, &mut general] {
        for component in &mut backend.state.components {
            match component {
                ComponentState::Electrical(state) => {
                    state.powered = false;
                    state.output_strength = 0;
                }
                ComponentState::Wire(wire) => wire.power = 0,
                ComponentState::Specialized => {}
            }
        }
        for resource in &mut backend.state.resources {
            resource.enabled = false;
        }
        assert!(backend.state.pistons[actor.index()].extended);
        assert_eq!(backend.input_strength(node, InputPort::Main), 0);
        assert_eq!(backend.input_strength(node, InputPort::Quasi), 0);
        backend.sample_piston(actor).unwrap();
        backend.sample_piston(actor).unwrap();
        assert_eq!(backend.future.piston_requests.len(), 1);
        let ComponentState::Electrical(reset_state) = &mut backend.state.components[reset.index()] else {
            panic!("reset has electrical state")
        };
        reset_state.powered = true;
        reset_state.output_strength = 15;
        assert!(backend.input_strength(node, InputPort::Quasi) > 0);
        backend.drain_piston_actions().unwrap();
        assert_eq!(
            backend.execution_counts().piston_actions,
            0,
            "queued retraction must be cancelled after reset power rises"
        );
        assert!(backend.future.piston_requests.is_empty());
        assert!(backend.future.deferred_pistons.is_empty());
    }
    assert_eq!(actual.state, general.state);
    assert!(actual.execution_counts().symbolic_region_steps > 0);
}

#[test]
fn symbolic_region_build_budgets_cancellation_and_stale_plans_are_checked() {
    let circuit = load_mechanism("instant_observer");
    let (mut graph, _) = stable_graph_at(&circuit, 4);
    let actor = graph.plans.instant.symbolic_regions[0].actors[0].actor;
    let node = graph.pistons[actor.index()].node;
    let cancelled = TaskMonitor::default();
    cancelled.cancel();
    assert!(crate::redpiler::analysis::instant_regions::symbolic_region::compile(&graph, &cancelled).is_err());
    let mut stale = graph.clone();
    std::sync::Arc::make_mut(&mut stale.plans.instant.symbolic_regions[0].actors[0]).reads = Box::new([]);
    assert!(stale.validate().is_err());
    let link = *graph
        .power_links
        .iter()
        .find(|link| matches!(link.to, PowerTarget::Input { node: target, .. } if target == node))
        .unwrap();
    graph.power_links.extend(std::iter::repeat_n(link, 1024));
    let plans = crate::redpiler::analysis::instant_regions::symbolic_region::compile(&graph, &TaskMonitor::default()).unwrap();
    assert!(
        plans.programs.is_empty(),
        "construction must decline before exceeding bounded scratch storage"
    );
    assert_eq!(plans.declines.len(), 1);
    assert_eq!(plans.declines[0].reason, "actor_event_plan_unsupported");
}
