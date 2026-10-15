//! Independent checks of display projection and deoptimization against exact execution.
use super::*;
use crate::redpiler::CompilerOptions;

fn manifest_cases() -> Vec<Value> {
    let path = root().join("test_data/instant-pistons-io/fixtures/adder_1bit.json");
    let manifest: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["expectation"].is_object())
        .cloned()
        .collect()
}

fn backend(circuit: &Adder, compact: bool) -> GraphBackend {
    GraphBackend::compile_with_options(
        &circuit.build(),
        circuit.bounds(),
        vec![],
        &CompilerOptions {
            optimization_level: Some(4),
            stable_instant_inputs: compact,
            ..Default::default()
        },
        Arc::default(),
    )
    .unwrap()
}

fn apply_step(native: &mut PlotWorld, exact: &mut GraphBackend, compact: &mut GraphBackend, step: Step) {
    let toggle = match step {
        Step::Tick => false,
        Step::SetLever { position, powered } => matches!(native.get_block(position), Block::Lever { lever } if lever.powered != powered),
    };
    native_step(native, step);
    graph_step(exact, step, toggle).unwrap();
    graph_step(compact, step, toggle).unwrap();
    // Check the live output storage too: cold projection must not hide a missed hot write.
    for (index, node) in compact.program.nodes().iter().enumerate() {
        if matches!(node.kind, CircuitNodeKind::Electrical(_)) {
            assert_eq!(
                compact.state.components[index], exact.state.components[index],
                "live electrical output {index}"
            );
        }
    }
}

fn assert_rendered(exact: &PlotWorld, compact: &PlotWorld, positions: &[BlockPos], label: &str) {
    let expected = future(exact, positions);
    let actual = future(compact, positions);
    assert_eq!(actual.cells, expected.cells, "{label}: rendered cells");
    assert_eq!(actual.entities, expected.entities, "{label}: rendered entities");
}

#[test]
fn compact_kernel_flush_modes_preserve_projection_and_pending_continuation() {
    let circuit = load_adder("adder_1bit", ORIGIN, None);
    let positions = circuit.positions();
    for case in manifest_cases() {
        let mut native = circuit.build();
        let mut exact_world = circuit.build();
        let mut compact_world = circuit.build();
        let mut exact = backend(&circuit, false);
        let mut compact = backend(&circuit, true);
        for (index, step) in case_steps(&circuit, &case, false, true).into_iter().enumerate() {
            apply_step(&mut native, &mut exact, &mut compact, step);
            // Flush history differs from compile-time preparation and cannot select different logic.
            let io_only = index % 3 != 0;
            exact.flush(&mut exact_world, io_only);
            compact.flush(&mut compact_world, io_only);
            assert_rendered(&exact_world, &compact_world, &positions, &format!("{} preparation {index}", case["id"]));
        }
        let before = compact.execution_counts().instant_transitions;
        for tick in 0..64 {
            apply_step(&mut native, &mut exact, &mut compact, Step::Tick);
            let io_only = tick % 4 != 0;
            exact.flush(&mut exact_world, io_only);
            compact.flush(&mut compact_world, io_only);
            // Repeating a flush must neither rearm reset nor duplicate queued presentation.
            exact.flush(&mut exact_world, io_only);
            compact.flush(&mut compact_world, io_only);
            assert_rendered(&exact_world, &compact_world, &positions, &format!("{} response {tick}", case["id"]));
        }
        assert!(
            compact.execution_counts().instant_transitions > before,
            "the check must exercise a running kernel"
        );
        exact.reset(&mut exact_world);
        compact.reset(&mut compact_world);
        for tick in 0..17 {
            let expected = future(&native, &positions);
            assert_eq!(
                first_difference(&expected, &future(&compact_world, &positions)),
                None,
                "{} native continuation {tick}",
                case["id"]
            );
            assert_eq!(
                first_difference(&future(&exact_world, &positions), &future(&compact_world, &positions)),
                None
            );
            native.tick_interpreted();
            exact_world.tick_interpreted();
            compact_world.tick_interpreted();
        }
    }
}

#[test]
fn compact_kernel_control_change_reconstructs_before_general_callbacks() {
    let circuit = load_adder("adder_1bit", ORIGIN, None);
    let positions = circuit.positions();
    let case = manifest_cases().into_iter().find(|case| case["expectation"]["carry"] == 1).unwrap();
    let input = circuit.port_positions("inputs", "A")[0];
    for cut in [1, 2, 4, 8, 17] {
        let mut native = circuit.build();
        let mut exact = backend(&circuit, false);
        let mut compact = backend(&circuit, true);
        for step in case_steps(&circuit, &case, false, true) {
            apply_step(&mut native, &mut exact, &mut compact, step);
        }
        let before = compact.execution_counts().instant_transitions;
        for _ in 0..cut {
            apply_step(&mut native, &mut exact, &mut compact, Step::Tick);
        }
        assert!(compact.execution_counts().instant_transitions > before);
        let Block::Lever { lever } = native.get_block(input) else {
            panic!("missing input")
        };
        // This deliberately exercises the implementation's deoptimization path while work is live.
        apply_step(
            &mut native,
            &mut exact,
            &mut compact,
            Step::SetLever {
                position: input,
                powered: !lever.powered,
            },
        );
        let mut exact_world = circuit.build();
        let mut compact_world = circuit.build();
        exact.reset(&mut exact_world);
        compact.reset(&mut compact_world);
        for tick in 0..25 {
            let expected = future(&native, &positions);
            assert_eq!(
                first_difference(&expected, &future(&compact_world, &positions)),
                None,
                "cut={cut}, continuation={tick}"
            );
            assert_eq!(
                first_difference(&future(&exact_world, &positions), &future(&compact_world, &positions)),
                None
            );
            native.tick_interpreted();
            exact_world.tick_interpreted();
            compact_world.tick_interpreted();
        }
    }
}

#[test]
fn compact_kernel_projection_preserves_independent_revision_origins_and_clock_epoch() {
    let circuit = load_adder("adder_1bit", ORIGIN, None);
    let positions = circuit.positions();
    for case in manifest_cases() {
        let mut native = circuit.build();
        let mut exact = backend(&circuit, false);
        let mut compact = backend(&circuit, true);
        let steps = case_steps(&circuit, &case, false, true);
        for &step in &steps[..steps.len() - 1] {
            apply_step(&mut native, &mut exact, &mut compact, step);
        }
        assert!(native.piston_state().motions.is_empty(), "origins move only at quiescent entry");
        native.piston_state_mut().logical_tick += 5_000;
        for backend in [&mut exact, &mut compact] {
            assert!(backend.scheduler.iter().next().is_none());
            assert!(backend.piston_requests.is_empty());
            assert!(backend.deferred_pistons.is_empty());
            backend.clock.logical_tick += 5_000;
            for (index, actor) in backend.state.pistons.iter_mut().enumerate() {
                actor.revision = u64::MAX - index as u64;
            }
            for (index, payload) in backend.state.resources.iter_mut().enumerate() {
                payload.revision = u64::MAX - 2 * index as u64;
            }
        }
        apply_step(&mut native, &mut exact, &mut compact, *steps.last().unwrap());
        let before = compact.execution_counts().instant_transitions;
        for _ in 0..36 {
            apply_step(&mut native, &mut exact, &mut compact, Step::Tick);
        }
        assert!(compact.execution_counts().instant_transitions > before);
        let mut exact_world = circuit.build();
        let mut compact_world = circuit.build();
        exact.reset(&mut exact_world);
        compact.reset(&mut compact_world);
        assert_eq!(compact.state, exact.state, "{} reconstructed state and revision counters", case["id"]);
        for tick in 0..17 {
            let expected = future(&native, &positions);
            assert_eq!(
                first_difference(&expected, &future(&compact_world, &positions)),
                None,
                "{} shifted/wrapping continuation {tick}",
                case["id"]
            );
            assert_eq!(
                first_difference(&future(&exact_world, &positions), &future(&compact_world, &positions)),
                None
            );
            native.tick_interpreted();
            exact_world.tick_interpreted();
            compact_world.tick_interpreted();
        }
    }
}
