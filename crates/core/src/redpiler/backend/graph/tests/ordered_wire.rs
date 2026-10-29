//! Exact ordered-wire execution on pinned adders.
use super::*;

#[test]
fn ordered_wire_tapes_preserve_adder_fifo_and_native_response() {
    let circuit = load_adder("adder_1bit", ORIGIN, None);
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root().join("test_data/instant-pistons-io/fixtures/adder_1bit.json")).unwrap()).unwrap();
    let case = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "prepared-0-1-1")
        .unwrap();
    let initial = circuit.build();
    let prepared = passes::rich::prepare(&initial, circuit.bounds(), vec![], &TaskMonitor::default(), AnalysisLimits::default()).unwrap();
    let (program, state) = prepared.graph.lower().unwrap();
    let mut template = GraphBackend::new_unactivated(program.clone(), state.clone(), 0).unwrap();
    let tapes = super::super::unchecked_classic_anpu::TapeProgram::attach(&mut template);
    let positions = circuit.positions();
    for repetition in 0..3 {
        let mut native = circuit.build();
        let mut projection = circuit.build();
        let mut exact = GraphBackend::new_unactivated(program.clone(), state.clone(), 0).unwrap();
        let mut replay = GraphBackend::new_unactivated(program.clone(), state.clone(), 0).unwrap();
        replay.test_recorder.wire_tapes = Some(tapes.clone());
        exact.set_test_recording(true);
        replay.set_test_recording(true);
        let mut steps = case_steps(&circuit, case, false, true);
        steps.extend(std::iter::repeat_n(Step::Tick, 24));
        for (index, step) in steps.into_iter().enumerate() {
            let toggle = match step {
                Step::SetLever { position, powered } => matches!(native.get_block(position), Block::Lever { lever } if lever.powered != powered),
                Step::Tick => false,
            };
            native_step(&mut native, step);
            graph_step(&mut exact, step, toggle).unwrap();
            graph_step(&mut replay, step, toggle).unwrap();
            let label = format!("repeat {repetition}, step {index} {step:?}");
            assert_eq!(replay.state, exact.state, "{label}: full circuit state");
            assert_eq!(replay.scheduled_ticks(), exact.scheduled_ticks(), "{label}: scheduler FIFO");
            assert_eq!(replay.future.pending, exact.future.pending, "{label}: pending membership");
            let piston_future = |backend: &GraphBackend| {
                format!(
                    "{:?}|{:?}|{:?}",
                    backend.future.clock, backend.future.piston_requests, backend.future.deferred_pistons
                )
            };
            assert_eq!(piston_future(&replay), piston_future(&exact), "{label}: piston future");
            assert_eq!(replay.observed_operations, exact.observed_operations, "{label}: semantic operations");
            check_boundary(&replay, &native, &mut projection, &positions, &label).unwrap();
        }
        let mut resumed = circuit.build();
        replay.reset(&mut resumed);
        for tick in 0..=8 {
            assert_eq!(
                first_difference(&future(&native, &positions), &future(&resumed, &positions)),
                None,
                "repeat {repetition}: interpreter continuation tick {tick}"
            );
            if tick < 8 {
                native.tick_interpreted();
                resumed.tick_interpreted();
            }
        }
    }
    println!("ordered adder tape work {}", tapes.counts());
}
