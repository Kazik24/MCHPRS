//! Diagnostic comparison of final-state dust and exact Turbo on pinned adders.
use super::*;

#[test]
#[ignore = "research: prints first fixed-point divergence across pinned adder histories"]
fn fixed_point_adders_report_first_state_and_event_divergence() {
    for id in FAMILY {
        let circuit = load_adder(id, ORIGIN, None);
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(root().join("test_data/instant-pistons-io/fixtures").join(format!("{id}.json"))).unwrap()).unwrap();
        for case in manifest["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|case| case["expectation"].is_object())
        {
            let world = circuit.build();
            let mut exact = compile_with_instant_pass(&circuit, &world, vec![]).unwrap();
            let mut stable = compile_with_instant_pass(&circuit, &world, vec![]).unwrap();
            exact.set_test_recording(true);
            stable.set_test_recording(true);
            let mut native = circuit.build();
            let mut exact_projection = circuit.build();
            let mut stable_projection = circuit.build();
            let positions = circuit.positions();
            let mut steps = case_steps(&circuit, case, false, true);
            steps.extend(std::iter::repeat_n(Step::Tick, 16));
            let mut first_events = false;
            let mut first_state = false;
            let mut first_cells = false;
            for (index, step) in steps.into_iter().enumerate() {
                let toggle = match step {
                    Step::SetLever { position, powered } => matches!(native.get_block(position), Block::Lever { lever } if lever.powered != powered),
                    Step::Tick => false,
                };
                native_step(&mut native, step);
                let exact_start = exact.observed_operations.len();
                let stable_start = stable.observed_operations.len();
                super::super::fixed_point::with_mode(false, || graph_step(&mut exact, step, toggle)).unwrap();
                super::super::fixed_point::with_mode(true, || graph_step(&mut stable, step, toggle)).unwrap();
                let a = graph_operations_since(&exact, exact_start, &positions);
                let b = graph_operations_since(&stable, stable_start, &positions);
                if !first_events && a != b {
                    let at = a.iter().zip(&b).position(|(a, b)| a != b).unwrap_or(a.len().min(b.len()));
                    println!(
                        "FIRST EVENTS {id}/{} step={index} {step:?} at={at} exact={:?} fixed={:?}",
                        case["id"],
                        &a[at.saturating_sub(2)..(at + 8).min(a.len())],
                        &b[at.saturating_sub(2)..(at + 8).min(b.len())]
                    );
                    first_events = true;
                }
                project(&exact, &mut exact_projection);
                project(&stable, &mut stable_projection);
                let changed: Vec<_> = positions
                    .iter()
                    .filter_map(|&p| {
                        let a = exact_projection.get_block(p);
                        let b = stable_projection.get_block(p);
                        (a != b).then_some((p, a, b))
                    })
                    .collect();
                let a = future(&exact_projection, &positions);
                let b = future(&stable_projection, &positions);
                if !first_state && (a != b) {
                    println!(
                        "FIRST STATE {id}/{} step={index} {step:?} changed={changed:?} scheduled exact={:?} fixed={:?} events exact={:?} fixed={:?}",
                        case["id"], a.scheduled, b.scheduled, a.events, b.events
                    );

                    first_state = true;
                }
                if !first_cells && !changed.is_empty() {
                    println!("FIRST CELLS {id}/{} step={index} {step:?} changed={changed:?}", case["id"]);
                    println!("EXACT TRACE {:?}", exact.trace.iter().rev().take(45).collect::<Vec<_>>());
                    println!("FIXED TRACE {:?}", stable.trace.iter().rev().take(45).collect::<Vec<_>>());
                    first_cells = true;
                }
            }
            println!(
                "END {id}/{} exact={:?} fixed={:?}",
                case["id"],
                exact.execution_counts(),
                stable.execution_counts()
            );
        }
    }
}

#[test]
fn fixed_point_preserves_wire_callbacks_outside_the_electrical_network() {
    let mut world = empty_world();
    let source = BlockPos::new(42, 34, 42);
    let root_wire = source.offset(BlockFace::East);
    let separate_wire = root_wire.offset(BlockFace::East).offset(BlockFace::East);
    world.set_block(source, Block::Lever { lever: Default::default() });
    for position in [source, root_wire, separate_wire] {
        world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
    }
    world.set_block(root_wire, Block::RedstoneWire { wire: Default::default() });
    world.set_block(separate_wire, Block::RedstoneWire { wire: Default::default() });
    let graph = passes::rich::prepare(
        &world,
        (BlockPos::new(39, 30, 39), BlockPos::new(49, 39, 49)),
        vec![],
        &TaskMonitor::default(),
        AnalysisLimits::default(),
    )
    .unwrap()
    .graph;
    let build = || {
        let (program, state) = graph.clone().lower().unwrap();
        let mut backend = GraphBackend::new(program, state, 0).unwrap();
        let source_node = backend.positions[&source];
        let ComponentState::Electrical(state) = &mut backend.state.components[source_node.index()] else {
            panic!("lever state")
        };
        state.powered = true;
        state.output_strength = 15;
        let separate = backend.positions[&separate_wire];
        let mut wire = backend.wire_state(separate);
        wire.power = 15;
        backend.set_wire(separate, wire);
        backend
    };
    let mut exact = build();
    let mut stable = build();
    let root_node = exact.positions[&root_wire];
    super::super::fixed_point::with_mode(false, || exact.evaluate_seed(root_node, None)).unwrap();
    super::super::fixed_point::with_mode(true, || stable.evaluate_seed(root_node, None)).unwrap();
    let separate = exact.positions[&separate_wire];
    assert_eq!(
        exact.wire_state(separate).power,
        0,
        "native Turbo reaches a second-ring wire without an electrical edge"
    );
    assert_eq!(
        stable.wire_state(separate).power,
        0,
        "settling must retain notifications outside its electrical component"
    );
}

#[test]
fn fixed_point_reordered_adder_requests_change_the_sum_payload() {
    let circuit = load_adder("adder_1bit", ORIGIN, None);
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root().join("test_data/instant-pistons-io/fixtures/adder_1bit.json")).unwrap()).unwrap();
    let case = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "prepared-0-1-1")
        .unwrap();
    let mut native = circuit.build();
    let mut exact = compile_with_instant_pass(&circuit, &native, vec![]).unwrap();
    let mut stable = compile_with_instant_pass(&circuit, &native, vec![]).unwrap();
    let mut reordered = compile_with_instant_pass(&circuit, &native, vec![]).unwrap();
    exact.set_test_recording(true);
    stable.set_test_recording(true);
    reordered.set_test_recording(true);
    for step in case_steps(&circuit, case, false, true) {
        let toggle = match step {
            Step::SetLever { position, powered } => matches!(native.get_block(position),Block::Lever {lever} if lever.powered!=powered),
            Step::Tick => false,
        };
        native_step(&mut native, step);
        super::super::fixed_point::with_mode(false, || graph_step(&mut exact, step, toggle)).unwrap();
        super::super::fixed_point::with_mode(true, || graph_step(&mut stable, step, toggle)).unwrap();
        super::super::fixed_point::with_mode(true, || graph_step(&mut reordered, step, toggle)).unwrap();
    }
    println!(
        "initial exact requests {:?}; fixed {:?}",
        exact.future.piston_requests, stable.future.piston_requests
    );
    for (label, backend) in [("exact", &exact), ("fixed", &stable)] {
        let requests: Vec<_> = backend
            .future
            .piston_requests
            .iter()
            .map(|request| {
                let spec = backend.program.pistons()[request.piston.index()];
                (request.piston, backend.program.nodes()[spec.node.index()].position, request.action)
            })
            .collect();
        println!("{label} initial requests with positions {requests:?}");
        let seeds: Vec<_> = backend
            .trace
            .iter()
            .filter(|line| line.starts_with("wire seed") || line.starts_with("wire turbo begin"))
            .collect();
        println!("{label} trigger wire roots {seeds:?}");
    }

    // Counterfactual: change only the initial FIFO order, retaining fixed-point propagation.
    reordered.future.piston_requests = exact.future.piston_requests.clone();
    exact.observed_operations.clear();
    stable.observed_operations.clear();
    reordered.observed_operations.clear();
    native_step(&mut native, Step::Tick);
    super::super::fixed_point::with_mode(false, || exact.tick()).unwrap();
    super::super::fixed_point::with_mode(true, || stable.tick()).unwrap();
    super::super::fixed_point::with_mode(true, || reordered.tick()).unwrap();
    let output = circuit.port_positions("observations", "sum_payload")[0];
    let mut a = circuit.build();
    let mut b = circuit.build();
    let mut c = circuit.build();
    project(&exact, &mut a);
    project(&stable, &mut b);
    project(&reordered, &mut c);
    println!(
        "sum output {output:?}: exact={:?} fixed={:?} initial-FIFO-corrected={:?}",
        a.get_block(output),
        b.get_block(output),
        c.get_block(output)
    );
    assert_eq!(native.get_block(output), Block::RedstoneBlock, "native arithmetic response window");
    assert_eq!(a.get_block(output), native.get_block(output));
    let actors = [BlockPos::new(41, 33, 41), BlockPos::new(49, 33, 45), BlockPos::new(56, 32, 41)];
    for (label, backend) in [("exact", &exact), ("fixed", &stable), ("FIFO-corrected", &reordered)] {
        let operations: Vec<_> = graph_operations_since(backend, 0, &circuit.positions())
            .into_iter()
            .filter(|operation| match operation {
                Operation::Callback { .. } => false,
                Operation::Sample { actor, .. }
                | Operation::Request { actor, .. }
                | Operation::AcceptanceAttempt { actor, .. }
                | Operation::Accepted { actor, .. } => actors.contains(actor),
            })
            .collect();
        println!("{label} causal piston operations {operations:?}");
    }
    assert_eq!(b.get_block(output), Block::Air);
    assert_eq!(
        c.get_block(output),
        a.get_block(output),
        "restoring only the initial FIFO should repair this counterexample"
    );

    // A payload discrepancy alone does not prove that the real repeater output is wrong.
    let sum = circuit.port_positions("observations", "sum_repeater")[0];
    let carry = circuit.port_positions("observations", "carry_repeater")[0];
    let mut different_sum_ticks = Vec::new();
    let positions = circuit.positions();
    for tick in 1..=24 {
        if tick > 1 {
            native.tick_interpreted();
            super::super::fixed_point::with_mode(false, || exact.tick()).unwrap();
            super::super::fixed_point::with_mode(true, || stable.tick()).unwrap();
            project(&exact, &mut a);
            project(&stable, &mut b);
        }
        let real = (repeater_bit(&native, sum), repeater_bit(&native, carry));
        let ordered = (repeater_bit(&a, sum), repeater_bit(&a, carry));
        let unordered = (repeater_bit(&b, sum), repeater_bit(&b, carry));
        println!(
            "REAL OUTPUT tick={tick}: interpreter={real:?}, ordered={ordered:?}, unordered={unordered:?}; sum blocks native={:?}, unordered={:?}",
            native.get_block(sum),
            b.get_block(sum)
        );
        check_boundary(&exact, &native, &mut a, &positions, &format!("real outputs tick {tick}")).unwrap();
        assert_eq!(ordered, real);
        assert_math_window(&circuit, &native, 0, 1, tick);
        if (3..=7).contains(&tick) {
            assert_eq!(real.0, 0, "the interpreter's actual sum repeater encodes the expected zero");
            assert_eq!(unordered.0, 1, "the unordered experiment gives the wrong timed sum");
        }
        if real.0 != unordered.0 {
            different_sum_ticks.push(tick);
        }
    }
    println!("UNORDERED SUM CONSUMER DIFFERS at ticks {different_sum_ticks:?}");
}
