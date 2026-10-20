//! Diagnostic comparison of final-state dust and exact Turbo on pinned adders.
use super::*;
use crate::plot::worldedit::{load_schematic, paste_clipboard};
use crate::redpiler::CompilerOptions;
use serde_json::json;
use sha2::{Digest, Sha256};

#[test]
#[ignore = "research: export explicit piston nodes, callback edges, wire-corridor boundaries, and shared resources"]
fn nanotick_piston_graph_preserves_stateful_actors_and_typed_wire_boundaries() {
    let manifest_path = root().join("test_data/instant-pistons/fixtures/nanotick_example.json");
    let manifest: Value = serde_json::from_slice(&std::fs::read(manifest_path).unwrap()).unwrap();
    let bytes = std::fs::read(root().join(manifest["fixture"].as_str().unwrap())).unwrap();
    assert_eq!(manifest["sha256"], format!("{:x}", Sha256::digest(&bytes)));
    let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    let anchor = ORIGIN + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z);
    let mut world = empty_world();
    paste_clipboard(&mut world, &clipboard, anchor, false);
    let last = ORIGIN + BlockPos::new(clipboard.size_x as i32 - 1, clipboard.size_y as i32 - 1, clipboard.size_z as i32 - 1);
    let monitor = TaskMonitor::default();
    let mut prepared = passes::rich::prepare(&world, (ORIGIN, last), vec![], &monitor, AnalysisLimits::default()).unwrap();
    let options = CompilerOptions {
        optimization_level: Some(5),
        stable_instant_inputs: true,
        ..Default::default()
    };
    let pipeline = passes::default_graph_pipeline(&options);
    passes::run_graph_pipeline(&mut prepared.graph, &options, &prepared.entry, &pipeline, &monitor).unwrap();
    let graph = &prepared.graph;
    let catalog = graph.plans.instant.boundaries.as_ref().unwrap();
    let strict_census = crate::redpiler::program_graph::census_closed_instant_nets(graph, catalog, &monitor).unwrap();
    let relaxed_census = crate::redpiler::program_graph::census_instant_nets(graph, catalog, &monitor, true).unwrap();
    let resource_for_actor = |actor: PistonId| {
        let piston = graph.pistons[actor.index()];
        matches!(piston.lane, PistonLane::SingleResource { .. }).then(|| {
            graph
                .resources
                .iter()
                .position(|resource| resource.contacts.contains(&piston.far))
                .map(|id| ResourceId::new(id as u32))
        })?
    };
    let actors: Vec<_> = graph
        .pistons
        .iter()
        .enumerate()
        .map(|(id, piston)| {
            let actor = PistonId::new(id as u32);
            json!({"actor":id,"position":piston.position,"fixture_local":piston.position-ORIGIN,"lane":format!("{:?}",piston.lane),
                "resource":resource_for_actor(actor).map(|resource|resource.index()),
                "near_contact":piston.near.index(),"far_contact":piston.far.index()})
        })
        .collect();
    let callback_edges: Vec<_> = graph
        .routes
        .iter()
        .enumerate()
        .filter_map(|(route_id, route)| {
            let CircuitNodeKind::Piston(source) = graph.nodes[route.source.index()].kind else {
                return None;
            };
            let deliveries: Vec<_> = route
                .deliveries
                .iter()
                .enumerate()
                .filter_map(|(ordinal, delivery)| {
                    let receiver = delivery.receiver?;
                    let target = match receiver {
                        CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor) => Some((actor.index(), None)),
                        CallbackReceiver::Component(node) => match graph.nodes[node.index()].kind {
                            CircuitNodeKind::Piston(actor) => Some((actor.index(), Some(node.index()))),
                            _ => None,
                        },
                        _ => None,
                    }?;
                    Some(json!({"ordinal":ordinal,"target_actor":target.0,"receiver_node":target.1,
                        "kind":format!("{:?}",delivery.kind),"guard":delivery.guard.index(),"sequence":delivery.sequence}))
                })
                .collect();
            (!deliveries.is_empty()).then(|| json!({"route":route_id,"source_actor":source.index(),"deliveries":deliveries}))
        })
        .collect();
    let corridors: Vec<_> = catalog
        .corridors
        .iter()
        .enumerate()
        .map(|(id, corridor)| {
            let ports: Vec<_> = catalog
                .ports
                .iter()
                .filter(|port| port.owner == BoundaryOwner::Corridor(id as u32))
                .map(|port| json!({"direction":format!("{:?}",port.direction),"kind":format!("{:?}",port.kind)}))
                .collect();
            json!({"corridor":id,"wire_nodes":corridor.wires.iter().map(|node|node.index()).collect::<Vec<_>>(),
                "wire_count":corridor.wires.len(),"boundary_ports":ports})
        })
        .collect();
    let shared_resources: Vec<_> = graph
        .resources
        .iter()
        .enumerate()
        .map(|(id, _)| {
            let resource = ResourceId::new(id as u32);
            json!({"resource":id,"owners":graph.pistons.iter().enumerate().filter_map(|(actor,_)|
                (resource_for_actor(PistonId::new(actor as u32))==Some(resource)).then_some(actor)).collect::<Vec<_>>()})
        })
        .collect();
    assert_eq!(actors.len(), graph.pistons.len(), "every piston remains a distinct stateful node");
    assert_eq!(
        corridors
            .iter()
            .map(|corridor| corridor["wire_count"].as_u64().unwrap() as usize)
            .sum::<usize>(),
        catalog.corridors.iter().map(|corridor| corridor.wires.len()).sum::<usize>()
    );
    let report = json!({"fixture":"NANOTICK_EXAMPLE","fixture_sha256":manifest["sha256"],"case":"falling","executor":"inventory only; no production rewrite",
        "causal_witness":"Preserve operation-4 downstream retraction before operation-5 inhibition; this is the native supplied behavior, even though the resulting output is unwanted by the schematic author.",
        "ports":manifest["ports"],
        "actor_nodes":actors,"piston_callback_edges":callback_edges,"wire_corridors":corridors,"shared_resource_owners":shared_resources,
        "boundary_ports":catalog.ports.len(),
        "wire_census":{"strict_multi_wire_nets":strict_census.closed.iter().filter(|net|net.wires>1).count(),
            "strict_multi_wire_members":strict_census.closed.iter().filter(|net|net.wires>1).map(|net|net.wires).sum::<usize>(),
            "strict_declines":strict_census.declines,"relaxed_electrical_consumer_multi_wire_nets":relaxed_census.closed.iter().filter(|net|net.wires>1).count(),
            "relaxed_electrical_consumer_multi_wire_members":relaxed_census.closed.iter().filter(|net|net.wires>1).map(|net|net.wires).sum::<usize>(),
            "relaxed_declines":relaxed_census.declines},
        "boundary_port_policy":"retain every typed Power, Callback, Guard, Contact, and geometry dependency at its original owner edge"});
    let directory = root().join("target/instant-wire-nodes/piston-graph");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("nanotick-example.json"), serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!(
        "NANOTICK piston graph: {} actors, {} callback routes, {} wire corridors, {} typed boundary ports, {} resources",
        graph.pistons.len(),
        callback_edges.len(),
        catalog.corridors.len(),
        catalog.ports.len(),
        graph.resources.len()
    );
}

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
    let mut template = GraphBackend::new(program.clone(), state.clone(), 0).unwrap();
    let tapes = super::super::unchecked_classic_anpu::TapeProgram::attach(&mut template);
    let positions = circuit.positions();
    for repetition in 0..3 {
        let mut native = circuit.build();
        let mut projection = circuit.build();
        let mut exact = GraphBackend::new(program.clone(), state.clone(), 0).unwrap();
        let mut replay = GraphBackend::new(program.clone(), state.clone(), 0).unwrap();
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
            super::super::fixed_point::with_mode(false, || graph_step(&mut exact, step, toggle)).unwrap();
            super::super::fixed_point::with_mode(false, || graph_step(&mut replay, step, toggle)).unwrap();
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
