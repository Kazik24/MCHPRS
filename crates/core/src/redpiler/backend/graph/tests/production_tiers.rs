//! O4–O5 retain live inputs through reset; O6 checks the stable-input device contract.
use super::*;
use crate::redpiler::CompilerOptions;

fn first_stable_difference(mut expected: Future, mut actual: Future) -> Option<String> {
    // O6's stable-output contract excludes internal FIFO order. Canonicalize both
    // work queues but preserve every event/tick field and compare all other state.
    let event_key = |event: &PistonEvent| {
        (
            event.pos.x,
            event.pos.y,
            event.pos.z,
            event.sticky,
            event.facing as u8,
            event.action as u8,
        )
    };
    expected.events.sort_by_key(event_key);
    actual.events.sort_by_key(event_key);
    let tick_key = |tick: &TickEntry| (tick.ticks_left, tick.tick_priority, tick.pos.x, tick.pos.y, tick.pos.z, tick.block_type);
    expected.scheduled.sort_by_key(tick_key);
    actual.scheduled.sort_by_key(tick_key);
    let motion_key = |motion: &(BlockPos, f32, f32, u64)| (motion.0.x, motion.0.y, motion.0.z, motion.1.to_bits(), motion.2.to_bits(), motion.3);
    expected.motions.sort_by_key(motion_key);
    actual.motions.sort_by_key(motion_key);
    first_difference(&expected, &actual)
}

fn check_stable_boundary(
    backend: &GraphBackend,
    native: &PlotWorld,
    scratch: &mut PlotWorld,
    positions: &[BlockPos],
    label: &str,
) -> Result<(), String> {
    project(backend, scratch);
    let expected = future(native, positions);
    let actual = future(scratch, positions);
    first_stable_difference(expected, actual).map_or(Ok(()), |difference| Err(format!("{label}: {difference}")))
}

#[test]
fn o6_adders_preserve_ordered_future_at_each_handoff_phase() {
    let mut source_checks = 0;
    for (id, case_id, previous) in [
        ("adder_1bit", "prepared-1-1-1", None),
        ("adder_11bits", "prepared-1-0-0", None),
        ("adder_11bits", "prepared-1023-1-0", None),
        ("adder_11bits", "prepared-2047-1-0", Some("prepared-1-0-0")),
    ] {
        let circuit = load_adder(id, ORIGIN, None);
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(root().join(format!("test_data/instant-pistons-io/fixtures/{id}.json"))).unwrap()).unwrap();
        let case = |name: &str| manifest["cases"].as_array().unwrap().iter().find(|case| case["id"] == name).unwrap();
        let mut preparation = Vec::new();
        if let Some(previous) = previous {
            preparation.extend(case_steps(&circuit, case(previous), false, true));
            preparation.extend(std::iter::repeat_n(Step::Tick, 32));
            preparation.push(Step::SetLever {
                position: circuit.trigger(),
                powered: true,
            });
        }
        preparation.extend(case_steps(&circuit, case(case_id), false, true));
        let positions = circuit.positions();
        for offset in (0..6).chain(24..30) {
            let mut native = circuit.build();
            let mut handed_back = circuit.build();
            let mut projection = circuit.build();
            let monitor = TaskMonitor::default();
            let options = CompilerOptions::parse("-O6").unwrap();
            let bounds = (ORIGIN, ORIGIN + circuit.size - EXTRA);
            let mut prepared = passes::rich::prepare(&native, bounds, vec![], &monitor, AnalysisLimits::default()).unwrap();
            passes::run_graph_pipeline(
                &mut prepared.graph,
                &options,
                &prepared.entry,
                &passes::default_graph_pipeline(&options),
                &monitor,
            )
            .unwrap();
            let mut candidate = GraphBackend::from_lowered(prepared.lower(&monitor).unwrap(), &monitor).unwrap();
            candidate.set_test_recording(true);
            for (index, step) in preparation.iter().copied().chain(std::iter::repeat_n(Step::Tick, offset)).enumerate() {
                let changed = match step {
                    Step::SetLever { position, powered } => matches!(native.get_block(position), Block::Lever { lever } if lever.powered != powered),
                    Step::Tick => false,
                };
                let entries = capture_at(&positions, || native_step(&mut native, step));
                let start = candidate.observed_operations.len();
                match step {
                    Step::Tick => candidate.tick_with_world(&mut handed_back).unwrap(),
                    _ => graph_step(&mut candidate, step, changed).unwrap(),
                }
                candidate.flush(&mut handed_back, false);
                let native_requests: Vec<_> = native_operations(&entries, &positions)
                    .into_iter()
                    .filter(|op| matches!(op, Operation::Request { .. }))
                    .collect();
                let graph_requests: Vec<_> = graph_operations_since(&candidate, start, &positions)
                    .into_iter()
                    .filter(|op| matches!(op, Operation::Request { .. }))
                    .collect();
                assert_eq!(
                    native_requests, graph_requests,
                    "{id} {case_id} offset {offset} step {index} {step:?}: ordered request occurrences"
                );
                for (event_index, entry) in entries.iter().enumerate().filter(|(_, entry)| entry["kind"] == "event_enqueue") {
                    let event: PistonEvent = serde_json::from_value(entry["data"].clone()).unwrap();
                    let Some(source) = entries[..event_index]
                        .iter()
                        .rev()
                        .find(|callback| callback["kind"] == "callback" && callback["data"]["pos"] == entry["data"]["pos"])
                        .and_then(|callback| serde_json::from_value::<BlockPos>(callback["data"]["source"].clone()).ok())
                    else {
                        continue;
                    };
                    let Some(request) = candidate.future.piston_requests.iter().find(|request| {
                        let spec = candidate.program.pistons()[request.piston.index()];
                        candidate.program.contacts()[spec.base.index()].position == event.pos && request.action == event.action
                    }) else {
                        continue;
                    };
                    let Some(route) = request.source else { continue };
                    let compiled_source = candidate.program.pistons().iter().find_map(|piston| {
                        let routes = piston.routes;
                        let contact = if route == routes.base_changed || route == routes.base_restored {
                            piston.base
                        } else if route == routes.near_changed || route == routes.head_restored || Some(route) == routes.resource_restored_near {
                            piston.near
                        } else if Some(route) == routes.far_changed
                            || Some(route) == routes.far_shape_changed
                            || Some(route) == routes.resource_restored_far
                        {
                            piston.far
                        } else {
                            return None;
                        };
                        Some(candidate.program.contacts()[contact.index()].position)
                    });
                    let Some(compiled_source) = compiled_source else { continue };
                    assert_eq!(
                        compiled_source, source,
                        "{id} {case_id} offset {offset} step {index}: source for {event:?}"
                    );
                    source_checks += 1;
                }
                check_boundary(
                    &candidate,
                    &native,
                    &mut projection,
                    &positions,
                    &format!("offset {offset} step {index} {step:?}"),
                )
                .unwrap_or_else(|error| {
                    let native_ops = native_operations(&entries, &positions);
                    let compiled_ops = &candidate.observed_operations;
                    let sources: Vec<_> = candidate
                        .future
                        .piston_requests
                        .iter()
                        .map(|request| {
                            let source = request
                                .source
                                .map(|route| candidate.program.nodes()[candidate.program.routes()[route.index()].source.index()].position);
                            (request, source)
                        })
                        .collect();
                    panic!(
                        "{error}; native recent callbacks {:?}; compiled recent callbacks {:?}; queued requests and source positions {sources:?}",
                        &native_ops[native_ops.len().saturating_sub(20)..],
                        &compiled_ops[compiled_ops.len().saturating_sub(20)..]
                    );
                });
            }
            let counts = candidate.execution_counts();
            assert!(
                counts.instant_transport_invocations > 0 || counts.instant_kernel_invocations > 0 || counts.logical_kernel_evaluations > 0,
                "O6 adder optimization not exercised: {counts:?}"
            );
            println!("O6 {id} {case_id} offset {offset}: {counts:?}");
            candidate.reset(&mut handed_back);
            for tick in 0..=32 {
                assert_eq!(
                    first_difference(&future(&native, &positions), &future(&handed_back, &positions)),
                    None,
                    "{id} {case_id} offset {offset} continuation {tick}"
                );
                let native_state = native.piston_state();
                let graph_state = handed_back.piston_state();
                assert_eq!(native_state.phase, AdvancePhase::BetweenTicks);
                assert_eq!(graph_state.phase, AdvancePhase::BetweenTicks);
                assert_eq!(native_state.movement_cursor, graph_state.movement_cursor);
                assert_eq!(native_state.movement_work.len(), graph_state.movement_work.len());
                let carried = |state: &mchprs_world::PistonState| {
                    state
                        .motions
                        .iter()
                        .map(|motion| format!("{:?}", motion.carried_entity))
                        .collect::<Vec<_>>()
                };
                assert_eq!(
                    carried(native_state),
                    carried(graph_state),
                    "{id} {case_id} offset {offset} continuation {tick}: carried entities"
                );
                native.tick_interpreted();
                handed_back.tick_interpreted();
            }
        }
    }
    assert!(source_checks > 0, "no pending requests had comparable native source positions");
}

#[test]
fn production_kernels_preserve_supported_input_histories_and_native_handoff() {
    for level in [4, 5, 6] {
        for id in ["adder_1bit", "adder_11bits"] {
            let circuit = load_adder(id, ORIGIN, None);
            let mut native = circuit.build();
            let mut projected = circuit.build();
            let monitor = TaskMonitor::default();
            let options = CompilerOptions::parse(&format!("-O{level}")).unwrap();
            assert!(!options.stable_instant_inputs);
            assert_eq!(options.assumes_stable_instant_inputs(), level == 6);
            let mut prepared = passes::rich::prepare(&native, circuit.bounds(), vec![], &monitor, AnalysisLimits::default()).unwrap();
            passes::run_graph_pipeline(
                &mut prepared.graph,
                &options,
                &prepared.entry,
                &passes::default_graph_pipeline(&options),
                &monitor,
            )
            .unwrap();
            assert_eq!(prepared.graph.plans.instant_events.len(), 1);
            assert_eq!(prepared.graph.plans.cpu_kernels.entries.is_empty(), level == 4);
            assert_eq!(prepared.graph.plans.cpu_power_queries.entries.is_empty(), level < 6);
            if level == 6 {
                assert_eq!(prepared.graph.plans.cpu_power_queries.entries.len(), prepared.graph.pistons.len());
            }
            let mut candidate = GraphBackend::from_lowered(prepared.lower(&monitor).unwrap(), &monitor).unwrap();
            candidate.set_test_recording(true);
            let controls: Vec<_> = circuit
                .cells
                .iter()
                .filter(|(_, block, _)| matches!(block, Block::Lever { .. }))
                .map(|(pos, _, _)| circuit.place(*pos))
                .collect();
            let manifest: Value =
                serde_json::from_slice(&std::fs::read(root().join(format!("test_data/instant-pistons-io/fixtures/{id}.json"))).unwrap()).unwrap();
            let case = manifest["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["expectation"].is_object() && (level < 6 || c["expectation"]["sum"].as_u64() == Some(1)))
                .unwrap();
            let mut steps = case_steps(&circuit, case, false, true);
            let mut seed = 0x19a4_110bu32;
            // O6 assumes stable inputs through reset; lower tiers also cover overlapping writes.
            for _ in 0..if level < 6 { 96 } else { 0 } {
                // Several control writes per tick deliberately overlap unfinished reset work.
                for _ in 0..3 {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    steps.push(Step::SetLever {
                        position: controls[(seed as usize >> 8) % controls.len()],
                        powered: seed >> 31 != 0,
                    });
                }
                steps.push(Step::Tick);
            }
            steps.extend(std::iter::repeat_n(Step::Tick, 16));
            let positions = circuit.positions();
            for (index, step) in steps.into_iter().enumerate() {
                let changed = match step {
                    Step::SetLever { position, powered } => matches!(native.get_block(position),Block::Lever{lever} if lever.powered!=powered),
                    Step::Tick => false,
                };
                native_step(&mut native, step);
                graph_step(&mut candidate, step, changed).unwrap();
                let label = format!("production O{level} {id} step {index}");
                if level == 6 {
                    check_stable_boundary(&candidate, &native, &mut projected, &positions, &label).unwrap();
                } else {
                    check_boundary(&candidate, &native, &mut projected, &positions, &label).unwrap();
                }
            }
            let work = candidate.execution_counts();
            assert!(
                work.instant_kernel_invocations > 0 || work.logical_kernel_evaluations > 0,
                "O{level} {id} must execute a physical event kernel or its logical replacement: {work:?}"
            );
            // Boundary inspection already materialized the future into this scratch world.
            projected.clear_scheduled_ticks();
            candidate.reset(&mut projected);
            for tick in 0..17 {
                let expected = future(&native, &positions);
                let actual = future(&projected, &positions);
                let difference = if level == 6 {
                    first_stable_difference(expected, actual)
                } else {
                    first_difference(&expected, &actual)
                };
                assert_eq!(difference, None, "O{level} {id} handoff {tick}");
                native.tick_interpreted();
                projected.tick_interpreted();
            }
        }
    }
}
