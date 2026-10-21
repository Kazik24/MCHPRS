//! Exact ordered-wire execution on pinned adders.
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
