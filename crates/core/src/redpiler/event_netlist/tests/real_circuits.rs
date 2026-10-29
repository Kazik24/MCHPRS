//! Offline executable gates for the pinned adder and counter fixtures.
use super::*;
use crate::plot::{
    PLOT_WIDTH, PlotWorld,
    worldedit::{load_schematic, paste_clipboard},
};
use crate::redpiler::components::NodeType;
use crate::redpiler::event_netlist::executor::EventState;
use crate::redpiler::physical_signal_network::{EntryWork, PhysicalSignalNetwork};
use crate::redpiler::program_graph::{
    CircuitNodeKind, ComponentState, ContactId, InputPort, LoweredCircuit, PistonId, PistonLane, PowerPort, PowerTarget, ResourceId,
};
use crate::redpiler::{CompilerOptions, TaskMonitor, analysis::AnalysisLimits, passes};
use crate::world::storage::Chunk;
use mchprs_blocks::BlockPos;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::PathBuf};

const ORIGIN: BlockPos = BlockPos::new(40, 30, 40);

fn fixture(id: &str) -> (PlotWorld, (BlockPos, BlockPos), Value) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root.join(format!("test_data/instant-pistons-io/fixtures/{id}.json"))).unwrap()).unwrap();
    let bytes = std::fs::read(root.join(manifest["fixture"].as_str().unwrap())).unwrap();
    assert_eq!(format!("{:x}", Sha256::digest(&bytes)), manifest["sha256"].as_str().unwrap());
    let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    let size = BlockPos::new(clipboard.size_x as i32, clipboard.size_y as i32, clipboard.size_z as i32);
    let mut world = PlotWorld::from_chunks(
        0,
        0,
        (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
        Default::default(),
    );
    paste_clipboard(
        &mut world,
        &clipboard,
        ORIGIN + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z),
        false,
    );
    (world, (ORIGIN - BlockPos::new(1, 1, 1), ORIGIN + size), manifest)
}

fn lower(world: &PlotWorld, bounds: (BlockPos, BlockPos)) -> Result<LoweredCircuit, String> {
    let monitor = TaskMonitor::default();
    let options = CompilerOptions::default();
    let prepared = passes::rich::prepare(
        world,
        bounds,
        world.scheduler().iter_entries().collect(),
        &monitor,
        AnalysisLimits::default(),
    )
    .map_err(|error| error.to_string())?;
    prepared
        .run_passes(&options, &passes::default_graph_pipeline(&options), &monitor)
        .map_err(|error| error.to_string())?
        .lower(&monitor)
        .map_err(|error| error.to_string())
}

#[test]
fn real_fixture_inventory_reports_executable_admission() {
    for id in ["adder_1bit", "adder_11bits", "counter_basic"] {
        let (world, bounds, manifest) = fixture(id);
        let lowered = lower(&world, bounds).unwrap_or_else(|error| panic!("{id} preparation/lowering failed: {error}"));
        let mut kinds = BTreeMap::new();
        for node in lowered.program.nodes() {
            let kind = match &node.kind {
                CircuitNodeKind::Electrical(kind) => format!("{kind:?}"),
                CircuitNodeKind::Wire(_) => "Wire".into(),
                CircuitNodeKind::Piston(_) => "Piston".into(),
                CircuitNodeKind::Contact(_) => "Contact".into(),
                CircuitNodeKind::Observer { .. } => "Observer".into(),
                other => format!("{other:?}"),
            };
            *kinds.entry(kind).or_insert(0usize) += 1;
        }
        let mut lanes = BTreeMap::new();
        for piston in lowered.program.pistons() {
            let kind = match piston.lane {
                PistonLane::Empty => "empty",
                PistonLane::SingleResource { .. } => "single-resource",
            };
            *lanes.entry((piston.sticky, kind)).or_insert(0usize) += 1;
        }
        let entry = EntryWork::capture(&world, bounds);
        let signal = PhysicalSignalNetwork::compile(&lowered.program, &lowered.state, &entry).err();
        let actors = CompiledPistonActors::compile(&lowered.program, &lowered.state, &entry).err();
        println!(
            "{id}: dimensions={} nodes={} wires={} pistons={} resources={} contacts={} routes={} power_links={} scheduled={} kinds={kinds:?} lanes={lanes:?}",
            manifest["dimensions"],
            lowered.program.nodes().len(),
            lowered.program.wires().len(),
            lowered.program.pistons().len(),
            lowered.program.resources().len(),
            lowered.program.contacts().len(),
            lowered.program.routes().len(),
            lowered.program.power_links().len(),
            world.scheduler().iter_entries().count()
        );
        println!("{id}: signal admission={signal:?}; compiled actor admission={actors:?}");
        assert!(!lowered.program.pistons().is_empty(), "fixture must contain real piston actors");
    }
}

#[test]
fn event_compiler_rejects_unimplemented_dynamic_components_at_their_position() {
    let (world, bounds, _) = fixture("counter_basic");
    let lowered = lower(&world, bounds).unwrap();
    let torch = lowered
        .program
        .nodes()
        .iter()
        .find(|node| matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Torch)))
        .expect("counter fixture contains a torch");

    let error = match crate::redpiler::event_netlist::compiler::compile(&lowered) {
        Ok(_) => panic!("torch behavior was silently compiled"),
        Err(error) => error,
    };
    assert_eq!(error.position, torch.position);
    assert!(error.reason.contains("Torch"), "{error}");
}

#[test]
#[ignore = "real one-bit adder compiled electrical path acceptance"]
fn offline_adder1_compiled_electrical_arithmetic() {
    let (world, bounds, manifest) = fixture("adder_1bit");
    let lowered = lower(&world, bounds).unwrap();
    let mut runtime =
        CompiledPistonActors::compile(&lowered.program, &lowered.state, &EntryWork::capture(&world, bounds)).expect("one-bit adder actor admission");
    runtime
        .signals_mut()
        .enable_dust_corridor()
        .expect("one-bit adder compiled electrical path admission");
    let trigger = position(&manifest["ports"]["inputs"]["trigger"]);
    for case in manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["expectation"].is_object())
    {
        let (world, bounds, _) = fixture("adder_1bit");
        let lowered = lower(&world, bounds).unwrap();
        let mut runtime = CompiledPistonActors::compile(&lowered.program, &lowered.state, &EntryWork::capture(&world, bounds)).unwrap();
        runtime.signals_mut().enable_dust_corridor().unwrap();
        runtime.begin_computation_cycle();
        for action in case["actions"].as_array().unwrap() {
            match action["op"].as_str().unwrap() {
                "lever" => {
                    let target = position(&action["pos"]);
                    if target == trigger {
                        runtime.begin_computation_cycle();
                    }
                    runtime.set_lever(target, action["powered"].as_bool().unwrap()).unwrap();
                }
                "wait" | "wait_ready" => {
                    for _ in 0..action["ticks"].as_u64().unwrap() {
                        runtime.advance_tick().unwrap();
                    }
                }
                op => panic!("unexpected action {op}"),
            }
        }
        let positions = lowered.program.position_index();
        for tick in 1..=case["ticks"].as_u64().unwrap() {
            runtime.advance_tick().unwrap();
            for (port, window, expected) in [("sum_repeater", 3..=7, "sum"), ("carry_repeater", 5..=9, "carry")] {
                if window.contains(&tick) {
                    let node = positions[&position(&manifest["ports"]["observations"][port])].index();
                    let ComponentState::Electrical(state) = &runtime.signals.state().components[node] else {
                        panic!("missing {port}")
                    };
                    assert_eq!(
                        u64::from(!state.powered),
                        case["expectation"][expected].as_u64().unwrap(),
                        "{} {port} tick {tick}",
                        case["id"]
                    );
                }
            }
        }
        let (links, settles) = runtime.signals().dust_corridor_work();
        assert!(
            links > 0 && runtime.movement_count() > 0,
            "{} compiled work and movements required: links={links}, movements={}, pending={}, actions={}",
            case["id"],
            runtime.movement_count(),
            runtime.pending.len(),
            runtime.successful_actions.iter().sum::<u64>()
        );
        assert_eq!(settles, 0, "owned corridor must not settle physically");
        println!("{} moves={} link_work={links} settles={settles}", case["id"], runtime.movement_count());
    }
}

#[test]
#[ignore = "one-bit resource observer notification execution regression"]
fn offline_adder1_resource_departure_schedules_compiled_observer() {
    let (world, bounds, _) = fixture("adder_1bit");
    let lowered = lower(&world, bounds).unwrap();
    let mut runtime = CompiledPistonActors::compile(&lowered.program, &lowered.state, &EntryWork::capture(&world, bounds)).unwrap();
    runtime.signals_mut().enable_dust_corridor().expect("compiled observer relationships");
    let observer_count = runtime.signals().dust_corridor_resource_observers();
    assert!(observer_count > 0, "ADDER1BIT must compile resource-to-observer notifications");
    let observer_nodes: Vec<_> = lowered
        .program
        .nodes()
        .iter()
        .enumerate()
        .filter_map(|(id, node)| matches!(node.kind, CircuitNodeKind::Observer { .. }).then_some(id))
        .collect();
    let resource = lowered.program.resources().iter().enumerate().find(|(_, spec)| spec.contacts.iter().any(|contact| observer_nodes.iter().any(|&node| matches!(lowered.program.nodes()[node].kind, CircuitNodeKind::Observer { watched, .. } if lowered.program.sites()[watched.index()].source == crate::redpiler::program_graph::SiteSource::Contact(*contact))))).map(|(id, _)| id).expect("resource watched by observer");
    let contact = lowered.state.resources[resource].at;
    runtime.signals_mut().notify_resource_observers(resource, contact).unwrap();
    assert!(
        runtime.signals().has_pending_observer_work(),
        "compiled movement notification must schedule observer work"
    );
    for _ in 0..2 {
        runtime.advance_tick().unwrap();
    }
    assert!(
        runtime
            .signals()
            .state()
            .components
            .iter()
            .any(|state| matches!(state, ComponentState::Electrical(node) if node.output_strength > 0 && node.pending_tick))
            || runtime.signals().has_pending_observer_work(),
        "observer pulse should run through the existing scheduler"
    );
    println!("compiled resource observer records={observer_count}");
}

#[test]
#[ignore = "compare the one-bit adder physical fallback with Graph"]
fn offline_adder1_preparation_graph_comparison() {
    use crate::redpiler::backend::graph::GraphBackend;
    use crate::world::World;
    use mchprs_blocks::blocks::Block;

    let (mut world, bounds, manifest) = fixture("adder_1bit");
    let lowered = lower(&world, bounds).unwrap();
    let mut graph = GraphBackend::compile(&world, bounds, world.scheduler().iter_entries().collect(), Default::default()).unwrap();
    graph.set_test_recording(true);
    let mut event = CompiledPistonActors::compile(&lowered.program, &lowered.state, &EntryWork::capture(&world, bounds)).unwrap();
    let case = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "prepared-0-0-0")
        .unwrap();
    let trigger = position(&manifest["ports"]["inputs"]["trigger"]);
    let positions = lowered.program.position_index();
    let carry = positions[&position(&manifest["ports"]["observations"]["carry_repeater"])];
    let carry_link = lowered.program.power_links_to(PowerTarget::Input {
        node: carry,
        port: InputPort::Main,
    })[0];
    let carry_wire = match lowered.program.sites()[carry_link.from.index()].source {
        crate::redpiler::program_graph::SiteSource::Component(node) => node,
        source => panic!("carry link source is not a graph component: {source:?}"),
    };
    let resource1_owners = lowered
        .program
        .pistons()
        .iter()
        .enumerate()
        .filter_map(|(index, piston)| {
            let id = PistonId::new(index as u32);
            (lowered.program.resource_for_piston(id) == Some(ResourceId::new(1))).then_some((id, piston.position))
        })
        .collect::<Vec<_>>();
    let report = |label: &str, graph: &GraphBackend, event: &CompiledPistonActors, world: &mut PlotWorld| {
        world.clear_scheduled_ticks();
        world.piston_state_mut().motions.clear();
        graph.project_future(world);
        for port in ["sum_repeater", "carry_repeater"] {
            let pos = position(&manifest["ports"]["observations"][port]);
            let Block::RedstoneRepeater { repeater } = world.get_block(pos) else {
                panic!("missing {port}")
            };
            let ComponentState::Electrical(state) = &event.signals.state().components[positions[&pos].index()] else {
                panic!("missing event output")
            };
            println!("{label} {port}: Graph={} event={}", u8::from(!repeater.powered), u8::from(!state.powered));
            if port == "carry_repeater" {
                println!(
                    "{label} carry Graph input={} pending={:?} wire={:?} wire_updated={} carry_updated={} resource1={:?} owners={:?}",
                    graph.input_strength_for_test(carry, InputPort::Main),
                    graph.scheduled_delay_for_test(carry),
                    graph.circuit_state().components[carry_wire.index()],
                    graph.component_updated_for_test(carry_wire),
                    graph.component_updated_for_test(carry),
                    graph.circuit_state().resources[1],
                    resource1_owners
                        .iter()
                        .map(|&(piston, position)| {
                            let input = lowered.program.pistons()[piston.index()].node;
                            (
                                piston,
                                position,
                                graph.input_strength_for_test(input, InputPort::Main),
                                graph.circuit_state().pistons[piston.index()],
                            )
                        })
                        .collect::<Vec<_>>()
                );
            }
        }
        println!(
            "{label}: movements={} pending={} deferred={}",
            event.movement_count(),
            event.pending.len(),
            event.deferred.len()
        );
    };
    report("loaded", &graph, &event, &mut world);
    event.begin_computation_cycle();
    for action in case["actions"].as_array().unwrap() {
        match action["op"].as_str().unwrap() {
            "lever" => {
                let pos = position(&action["pos"]);
                let value = action["powered"].as_bool().unwrap();
                if pos == trigger {
                    report("prepared", &graph, &event, &mut world);
                    event.begin_computation_cycle();
                }
                let Block::Lever { lever } = world.get_block(pos) else {
                    panic!("missing lever")
                };
                if lever.powered != value {
                    graph.use_block(pos).unwrap();
                }
                if pos == trigger {
                    let piston = resource1_owners[0].0;
                    let piston_input = lowered.program.pistons()[piston.index()].node;
                    println!(
                        "after trigger input before Graph tick: P0 input={} carry input={} resource1={:?} P0={:?}",
                        graph.input_strength_for_test(piston_input, InputPort::Main),
                        graph.input_strength_for_test(carry, InputPort::Main),
                        graph.circuit_state().resources[1],
                        graph.circuit_state().pistons[piston.index()]
                    );
                }
                event.set_lever(pos, value).unwrap();
                graph.project_future(&mut world);
            }
            "wait" | "wait_ready" => {
                for tick in 1..=action["ticks"].as_u64().unwrap() {
                    graph.tick().unwrap();
                    event.advance_tick().unwrap();
                    if tick <= 3 || tick == 32 {
                        report(&format!("preparation tick {tick}"), &graph, &event, &mut world);
                    }
                }
            }
            op => panic!("unexpected action {op}"),
        }
    }
    for tick in 1..=9 {
        graph.clear_test_traces();
        graph.tick().unwrap();
        event.advance_tick().unwrap();
        println!(
            "Graph P0 tick={tick} input={} state={:?} resource0={:?} resource1={:?} operations={:?}",
            graph.input_strength_for_test(lowered.program.pistons()[resource1_owners[0].0.index()].node, InputPort::Main),
            graph.circuit_state().pistons[resource1_owners[0].0.index()],
            graph.circuit_state().resources[0],
            graph.circuit_state().resources[1],
            graph.piston_operations_for_test(resource1_owners[0].1)
        );
        for (index, spec) in lowered.program.pistons().iter().enumerate() {
            let input = graph.input_strength_for_test(spec.node, InputPort::Main);
            let operations = graph.piston_operations_for_test(spec.position);
            if !operations.is_empty() || index == 0 || index == 9 {
                println!(
                    "Graph piston {:?} tick={tick} input={input} state={:?} operations={operations:?}",
                    PistonId::new(index as u32),
                    graph.circuit_state().pistons[index]
                );
            }
        }
        report(&format!("trigger tick {tick}"), &graph, &event, &mut world);
    }
}

#[test]
#[ignore = "lockstep Graph/EventProgram piston-order diagnostic"]
fn offline_adder1_event_program_graph_piston_order() {
    use crate::redpiler::backend::graph::GraphBackend;
    use crate::redpiler::program_graph::PistonId;
    let (world, bounds, manifest) = fixture("adder_1bit");
    let lowered = lower(&world, bounds).unwrap();
    let compiled = crate::redpiler::event_netlist::compiler::compile(&lowered).unwrap();
    let mut event = EventState::new(compiled.program.clone(), lowered.logical_tick);
    let mut graph = GraphBackend::compile(&world, bounds, world.scheduler().iter_entries().collect(), Default::default()).unwrap();
    graph.set_test_recording(true);
    let case = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "prepared-0-0-0")
        .unwrap();
    let trigger = position(&manifest["ports"]["inputs"]["trigger"]);

    let report = |label: &str, graph: &GraphBackend, event: &EventState| {
        for index in [0usize, 6, 9, 11] {
            let piston = PistonId::new(index as u32);
            let graph_spec = lowered.program.pistons()[index];
            let event_spec = compiled.program.pistons[index];
            let event_links = compiled.program.input_links[compiled.program.input_ranges[event_spec.input.index()].clone()]
                .iter()
                .map(|&link_id| {
                    let link = &compiled.program.links[link_id];
                    (
                        link_id,
                        link.source,
                        lowered.program.nodes().get(link.source.index()).and_then(|node| node.position),
                        event.link_state_for_test(link_id),
                    )
                })
                .collect::<Vec<_>>();
            println!(
                "{label} piston={piston:?} Graph main={} quasi={} state={:?} actions={:?}; Event main={:?} state={:?} links={event_links:?}",
                graph.input_strength_for_test(graph_spec.node, InputPort::Main),
                graph.input_strength_for_test(graph_spec.node, InputPort::Quasi),
                graph.circuit_state().pistons[index],
                graph.piston_operations_for_test(graph_spec.position),
                event.input_strength(event_spec.input),
                event.piston(piston)
            );
        }
        println!(
            "{label} Graph resource8={:?} resource0={:?}; Event resource8={:?} resource0={:?}",
            graph.circuit_state().resources[8],
            graph.circuit_state().resources[0],
            event.resource(ResourceId::new(8)),
            event.resource(ResourceId::new(0))
        );
    };

    for index in [0usize, 6, 9, 11] {
        let node = lowered.program.pistons()[index].node;
        for port in [InputPort::Main, InputPort::Quasi] {
            let target = PowerTarget::Input { node, port };
            println!(
                "Piston {:?} original {port:?}: fixed={:?} links={:?}",
                PistonId::new(index as u32),
                lowered.program.fixed_signal_query(target),
                lowered
                    .program
                    .power_links_to(target)
                    .iter()
                    .map(|link| {
                        let site = &lowered.program.sites()[link.from.index()];
                        (
                            site.position,
                            site.source,
                            link.from_port,
                            link.attenuation,
                            &lowered.program.guards()[link.guard.index()],
                        )
                    })
                    .collect::<Vec<_>>()
            );
        }
    }

    for action in case["actions"].as_array().unwrap() {
        match action["op"].as_str().unwrap() {
            "lever" => {
                let pos = position(&action["pos"]);
                let value = action["powered"].as_bool().unwrap();
                let node = compiled.positions[&pos];
                if pos == trigger {
                    graph.clear_test_traces();
                }
                let graph_powered = matches!(&graph.circuit_state().components[node.index()], ComponentState::Electrical(state) if state.powered);
                if graph_powered != value {
                    graph.use_block(pos).unwrap();
                }
                if pos == trigger {
                    report("before trigger tick", &graph, &event);
                }
                event.set_input(node, if value { 15 } else { 0 }).unwrap();
                if pos == trigger {
                    report("after trigger input", &graph, &event);
                }
            }
            "wait" | "wait_ready" => {
                for tick in 0..action["ticks"].as_u64().unwrap() {
                    graph.tick().unwrap();
                    event.tick().unwrap();
                    if tick < 3 || tick == 31 {
                        report(&format!("preparation tick {}", tick + 1), &graph, &event);
                    }
                }
            }
            op => panic!("unexpected manifest action {op}"),
        }
    }

    for tick in 1..=3 {
        graph.clear_test_traces();
        graph.tick().unwrap();
        event.tick().unwrap();
        report(&format!("trigger tick {tick}"), &graph, &event);
    }
}

#[test]
#[ignore = "lockstep 0+1 sum investigation"]
fn offline_adder1_event_program_sum_lockstep_0_1_0() {
    use crate::redpiler::backend::graph::GraphBackend;

    let (world, bounds, manifest) = fixture("adder_1bit");
    let lowered = lower(&world, bounds).unwrap();
    let compiled = crate::redpiler::event_netlist::compiler::compile(&lowered).unwrap();
    let mut event = EventState::new(compiled.program.clone(), lowered.logical_tick);
    let mut graph = GraphBackend::compile(&world, bounds, world.scheduler().iter_entries().collect(), Default::default()).unwrap();
    let case = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "prepared-0-1-0")
        .unwrap();
    let trigger = position(&manifest["ports"]["inputs"]["trigger"]);
    let sum = compiled.positions[&position(&manifest["ports"]["observations"]["sum_repeater"])];
    let report = |label: &str, graph: &GraphBackend, event: &EventState| {
        let graph_state = match &graph.circuit_state().components[sum.index()] {
            ComponentState::Electrical(state) => state,
            other => panic!("sum repeater has unexpected Graph state {other:?}"),
        };
        println!(
            "{label}: Graph raw={} input={} pending={:?}; Event raw={} input={:?} pending={:?} actions={} rejected={} moves={} completions={}",
            graph_state.powered,
            graph.input_strength_for_test(sum, InputPort::Main),
            graph.scheduled_delay_for_test(sum),
            event.component(sum).unwrap().powered,
            event.input_strength(sum),
            event.scheduled_delay_for_test(sum),
            event.counts().piston_actions,
            event.counts().rejected_piston_actions,
            event.counts().resource_movements,
            event.counts().motion_completions
        );
        println!(
            "{label}: Graph resources={:?}; Event resources={:?}",
            graph.circuit_state().resources,
            (0..compiled.program.resources.len())
                .map(|index| event.resource(ResourceId::new(index as u32)).unwrap())
                .collect::<Vec<_>>()
        );
        for (index, graph_spec) in lowered.program.pistons().iter().enumerate() {
            let actions = graph.piston_operations_for_test(graph_spec.position);
            if actions
                .iter()
                .any(|action| action.contains("Request") || action.contains("Accepted") || action.contains("AcceptanceAttempt"))
            {
                let event_spec = compiled.program.pistons[index];
                println!(
                    "{label}: piston={index} pos={:?} Graph main/quasi={}/{} state={:?} actions={actions:?}; Event input={:?} state={:?}",
                    graph_spec.position,
                    graph.input_strength_for_test(graph_spec.node, InputPort::Main),
                    graph.input_strength_for_test(graph_spec.node, InputPort::Quasi),
                    graph.circuit_state().pistons[index],
                    event.input_strength(event_spec.input),
                    event.piston(PistonId::new(index as u32))
                );
                let event_input = event_spec.input;
                let event_links = compiled.program.input_links[compiled.program.input_ranges[event_input.index()].clone()]
                    .iter()
                    .map(|&id| {
                        let link = &compiled.program.links[id];
                        (id, link.source, link.port, event.link_state_for_test(id), &link.condition)
                    })
                    .collect::<Vec<_>>();
                let mut graph_links = Vec::new();
                for port in [InputPort::Main, InputPort::Quasi] {
                    for link in lowered
                        .program
                        .power_links_to(crate::redpiler::program_graph::PowerTarget::Input { node: graph_spec.node, port })
                    {
                        graph_links.push((
                            port,
                            link.from,
                            lowered.program.sites()[link.from.index()],
                            link.from_port,
                            link.attenuation,
                            &lowered.program.guards()[link.guard.index()],
                        ));
                    }
                }
                println!("{label}: piston={index} Event links={event_links:?}; Graph links={graph_links:?}");
            }
        }
        if label == "after trigger" {
            for route_id in [25usize, 27, 35, 59] {
                let route = &lowered.program.routes()[route_id];
                println!(
                    "trigger request route {route_id}: source={:?} source_node={:?} deliveries={:?}",
                    route.source,
                    lowered.program.nodes()[route.source.index()],
                    route
                        .deliveries
                        .iter()
                        .map(|delivery| (
                            delivery.sequence,
                            lowered.program.sites()[delivery.target.index()],
                            delivery.kind,
                            delivery.receiver,
                            &lowered.program.guards()[delivery.guard.index()]
                        ))
                        .collect::<Vec<_>>()
                );
            }
        }
        let sum_links = compiled.program.input_links[compiled.program.input_ranges[sum.index()].clone()].to_vec();
        if event.input_strength(sum) != Some(graph.input_strength_for_test(sum, InputPort::Main)) {
            println!(
                "{label}: Event sum links={:?}",
                sum_links
                    .iter()
                    .map(|&link| {
                        let item = &compiled.program.links[link];
                        (link, item.source, item.port, event.link_state_for_test(link), &item.condition)
                    })
                    .collect::<Vec<_>>()
            );
            println!(
                "{label}: Graph sum links={:?}",
                lowered
                    .program
                    .power_links_to(crate::redpiler::program_graph::PowerTarget::Input {
                        node: sum,
                        port: InputPort::Main
                    })
                    .iter()
                    .map(|link| (
                        link.from,
                        lowered.program.sites()[link.from.index()],
                        link.from_port,
                        link.attenuation,
                        &lowered.program.guards()[link.guard.index()]
                    ))
                    .collect::<Vec<_>>()
            );
        }
    };

    for action in case["actions"].as_array().unwrap() {
        match action["op"].as_str().unwrap() {
            "lever" => {
                let pos = position(&action["pos"]);
                let value = action["powered"].as_bool().unwrap();
                let node = compiled.positions[&pos];
                if pos == trigger {
                    graph.set_test_recording(true);
                }
                let graph_powered = matches!(&graph.circuit_state().components[node.index()], ComponentState::Electrical(state) if state.powered);
                if graph_powered != value {
                    graph.use_block(pos).unwrap();
                }
                event.set_input(node, if value { 15 } else { 0 }).unwrap();
                if pos == trigger {
                    report("after trigger", &graph, &event);
                }
            }
            "wait" | "wait_ready" => {
                for _ in 0..action["ticks"].as_u64().unwrap() {
                    graph.tick().unwrap();
                    event.tick().unwrap();
                }
            }
            op => panic!("unexpected manifest action {op}"),
        }
    }

    for tick in 1..=case["ticks"].as_u64().unwrap() {
        graph.clear_test_traces();
        graph.tick().unwrap();
        event.tick().unwrap();
        report(&format!("tick {tick}"), &graph, &event);
        if (3..=7).contains(&tick) {
            let graph_raw = match &graph.circuit_state().components[sum.index()] {
                ComponentState::Electrical(state) => state.powered,
                _ => unreachable!(),
            };
            assert_eq!(event.component(sum).unwrap().powered, graph_raw, "sum repeater lockstep tick {tick}");
        }
    }
}

#[test]
fn compiled_event_pistons_keep_quasi_power_during_trigger_departures() {
    use crate::redpiler::backend::graph::GraphBackend;
    use crate::redstone::piston::trace::{self, Stage};

    let (mut scratch, bounds, manifest) = fixture("adder_1bit");
    let lowered = lower(&scratch, bounds).unwrap();
    let compiled = crate::redpiler::event_netlist::compiler::compile(&lowered).unwrap();
    let mut event = EventState::new(compiled.program.clone(), lowered.logical_tick);
    let mut graph = GraphBackend::compile(&scratch, bounds, scratch.scheduler().iter_entries().collect(), Default::default()).unwrap();
    let case = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "prepared-0-0-0")
        .unwrap();
    let trigger = position(&manifest["ports"]["inputs"]["trigger"]);
    let quasi_powered = [6usize, 11];
    let piston_positions = [lowered.program.pistons()[11].position, lowered.program.pistons()[9].position];

    for action in case["actions"].as_array().unwrap() {
        match action["op"].as_str().unwrap() {
            "lever" => {
                let pos = position(&action["pos"]);
                let value = action["powered"].as_bool().unwrap();
                let node = compiled.positions[&pos];
                let native_trace = if pos == trigger {
                    Some(trace::capture_detailed(&piston_positions, || {
                        set_native_lever(&mut scratch, pos, value);
                        scratch.tick_interpreted();
                    }))
                } else {
                    set_native_lever(&mut scratch, pos, value);
                    None
                };
                let graph_powered = matches!(&graph.circuit_state().components[node.index()], ComponentState::Electrical(state) if state.powered);
                if graph_powered != value {
                    graph.use_block(pos).unwrap();
                }
                event.set_input(node, if value { 15 } else { 0 }).unwrap();
                if pos == trigger {
                    let native_trace = native_trace.unwrap();
                    assert!(
                        native_trace
                            .iter()
                            .any(|entry| entry.pos == piston_positions[0] && entry.stage == Stage::Sample && entry.powered)
                    );
                    assert!(
                        !native_trace
                            .iter()
                            .any(|entry| entry.pos == piston_positions[0] && entry.stage == Stage::Request)
                    );
                    assert!(
                        !native_trace.iter().any(|entry| entry.pos == piston_positions[1]),
                        "Piston 9 must not update without resource 8 departure"
                    );
                    for index in quasi_powered {
                        let piston = PistonId::new(index as u32);
                        let graph_input = lowered.program.pistons()[index].node;
                        let event_input = compiled.program.pistons[index].input;
                        assert_eq!(graph.input_strength_for_test(graph_input, InputPort::Main), 0);
                        assert!(graph.input_strength_for_test(graph_input, InputPort::Quasi) > 0);
                        assert!(event.input_strength(event_input).unwrap() > 0, "{piston:?} must include quasi power");
                    }
                    graph.tick().unwrap();
                    event.tick().unwrap();
                    for index in quasi_powered {
                        assert!(event.piston(PistonId::new(index as u32)).unwrap().extended);
                    }
                    assert_eq!(event.resource(ResourceId::new(8)).unwrap().at, ContactId::new(32));
                    assert!(event.resource(ResourceId::new(8)).unwrap().enabled);
                    let graph_resources = graph.circuit_state().resources.clone();
                    let mut native_applied = Vec::new();
                    let piston_positions = lowered.program.pistons().iter().map(|piston| piston.position).collect::<Vec<_>>();
                    for _ in 1..case["ticks"].as_u64().unwrap() {
                        graph.tick().unwrap();
                        event.tick().unwrap();
                        native_applied.extend(
                            trace::capture_detailed(&piston_positions, || scratch.tick_interpreted())
                                .into_iter()
                                .filter(|entry| entry.stage == Stage::Applied),
                        );
                    }
                    assert_eq!(
                        graph.circuit_state().resources,
                        graph_resources,
                        "Graph resource arrangement over 0+0 output window"
                    );
                    assert!(
                        native_applied.is_empty(),
                        "native piston movement over 0+0 output window: {native_applied:?}"
                    );
                    assert_eq!(event.counts().resource_movements, 0, "EventProgram movement count for 0+0");
                    return;
                }
            }
            "wait" | "wait_ready" => {
                for _ in 0..action["ticks"].as_u64().unwrap() {
                    graph.tick().unwrap();
                    event.tick().unwrap();
                    scratch.tick_interpreted();
                }
            }
            op => panic!("unexpected manifest action {op}"),
        }
    }
    panic!("fixture case did not reach its trigger");
}

fn set_native_lever(world: &mut PlotWorld, pos: BlockPos, powered: bool) {
    use crate::world::World;
    use mchprs_blocks::BlockFace;
    use mchprs_blocks::blocks::{Block, LeverFace};

    let Block::Lever { mut lever } = world.get_block(pos) else {
        panic!("expected lever at {pos:?}");
    };
    if lever.powered == powered {
        return;
    }
    lever.powered = powered;
    world.set_block(pos, Block::Lever { lever });
    crate::redstone::update_surrounding_blocks(world, pos);
    let support = match lever.face {
        LeverFace::Floor => BlockFace::Bottom,
        LeverFace::Ceiling => BlockFace::Top,
        LeverFace::Wall => lever.facing.opposite().block_face(),
    };
    crate::redstone::update_surrounding_blocks(world, pos.offset(support));
}

fn position(value: &Value) -> BlockPos {
    ORIGIN
        + BlockPos::new(
            value[0].as_i64().unwrap() as i32,
            value[1].as_i64().unwrap() as i32,
            value[2].as_i64().unwrap() as i32,
        )
}

#[test]
fn offline_adder1_compiled_event_program_arithmetic() {
    let (_, _, manifest) = fixture("adder_1bit");
    let mut resource_movements = 0;
    for case in manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["expectation"].is_object())
    {
        let (world, bounds, _) = fixture("adder_1bit");
        let lowered = lower(&world, bounds).unwrap();
        let compiled = crate::redpiler::event_netlist::compiler::compile(&lowered)
            .unwrap_or_else(|error| panic!("{} event compilation failed: {error}", case["id"]));
        assert!(!compiled.program.links.is_empty(), "compiled electrical links required");
        let sum = compiled.positions[&position(&manifest["ports"]["observations"]["sum_repeater"])];
        let carry = compiled.positions[&position(&manifest["ports"]["observations"]["carry_repeater"])];
        let carry_links = compiled.program.input_links[compiled.program.input_ranges[carry.index()].clone()].to_vec();
        assert!(!carry_links.is_empty(), "compiled carry dependencies are required");
        let carry_target = PowerTarget::Input {
            node: carry,
            port: InputPort::Main,
        };
        let graph_links = lowered.program.power_links_to(carry_target);
        println!(
            "{} carry Graph links={} fixed query={:?} predicates={:?} event guard={:?}",
            case["id"],
            graph_links.len(),
            lowered.program.fixed_signal_query(carry_target),
            graph_links
                .iter()
                .map(|link| (link.from, link.from_port, link.attenuation, &lowered.program.guards()[link.guard.index()]))
                .collect::<Vec<_>>(),
            carry_links
                .iter()
                .map(|&link| compiled.program.links[link].condition.as_ref())
                .collect::<Vec<_>>()
        );
        for link in graph_links {
            let site = &lowered.program.sites()[link.from.index()];
            println!("{} carry source site {site:?}", case["id"]);
            if let crate::redpiler::program_graph::SiteSource::Component(node) = site.source {
                if matches!(lowered.program.nodes()[node.index()].kind, CircuitNodeKind::Wire(_)) {
                    println!(
                        "{} carry source wire {:?} query={:?} has links {:?}",
                        case["id"],
                        node,
                        lowered.program.fixed_signal_query(PowerTarget::Input { node, port: InputPort::Main }),
                        lowered.program.power_links_to(PowerTarget::Input { node, port: InputPort::Main })
                    );
                    for wire_link in lowered.program.power_links_to(PowerTarget::Input { node, port: InputPort::Main }) {
                        let wire_site = &lowered.program.sites()[wire_link.from.index()];
                        println!("{} carry wire link site={wire_site:?}", case["id"]);
                        if let PowerPort::Conducted { include_dust } = wire_link.from_port {
                            for input in lowered.program.power_links_to(PowerTarget::Conductor {
                                site: wire_link.from,
                                include_dust,
                            }) {
                                println!(
                                    "{} carry wire conductor input={input:?} site={:?} guard={:?}",
                                    case["id"],
                                    lowered.program.sites()[input.from.index()],
                                    lowered.program.guards()[input.guard.index()]
                                );
                            }
                        }
                    }
                }
            }
        }
        let mut event = EventState::new(compiled.program.clone(), lowered.logical_tick);
        let event_resource1_owners = compiled
            .program
            .pistons
            .iter()
            .enumerate()
            .filter_map(|(index, piston)| (piston.resource == Some(ResourceId::new(1))).then_some((PistonId::new(index as u32), piston.input)))
            .collect::<Vec<_>>();
        let p0_input = lowered.program.pistons()[event_resource1_owners[0].0.index()].node;
        let p0_event_links = compiled.program.input_links[compiled.program.input_ranges[p0_input.index()].clone()].to_vec();
        println!(
            "{} carry compiled sources: {:?}",
            case["id"],
            carry_links
                .iter()
                .map(|&link| {
                    let source = compiled.program.links[link].source;
                    (
                        link,
                        source,
                        lowered.program.nodes().get(source.index()).map(|node| &node.kind),
                        lowered.state.components.get(source.index()),
                        compiled.program.components[source.index()].kind,
                        event.component(source),
                        compiled.program.links[link].condition.as_ref(),
                    )
                })
                .collect::<Vec<_>>()
        );
        for action in case["actions"].as_array().unwrap() {
            match action["op"].as_str().unwrap() {
                "lever" => {
                    let node = compiled.positions[&position(&action["pos"])];
                    if node == compiled.positions[&position(&manifest["ports"]["inputs"]["trigger"])] {
                        println!(
                            "{} P0 before trigger: strength={:?} links={:?}",
                            case["id"],
                            event.input_strength(p0_input),
                            p0_event_links
                                .iter()
                                .map(|&link| {
                                    let source = compiled.program.links[link].source;
                                    (
                                        link,
                                        source,
                                        compiled.program.components[source.index()].kind,
                                        event.component(source),
                                        event.link_state_for_test(link),
                                        &compiled.program.links[link].condition,
                                    )
                                })
                                .collect::<Vec<_>>()
                        );
                    }
                    event.set_input(node, if action["powered"].as_bool().unwrap() { 15 } else { 0 }).unwrap();
                    if node == compiled.positions[&position(&manifest["ports"]["inputs"]["trigger"])] {
                        let wire = compiled.positions[&BlockPos::new(40, 34, 41)];
                        println!(
                            "{} wire89 after trigger={:?} input={:?} ops={:?} links={:?}",
                            case["id"],
                            event.component(wire),
                            event.input_strength(wire),
                            compiled.program.source_ranges[wire.index()]
                                .clone()
                                .map(|index| compiled.program.operations[index])
                                .collect::<Vec<_>>(),
                            compiled.program.input_links[compiled.program.input_ranges[wire.index()].clone()]
                                .iter()
                                .map(|&link| (link, &compiled.program.links[link], event.link_state_for_test(link)))
                                .collect::<Vec<_>>()
                        );
                        println!(
                            "{} P0 after trigger: strength={:?} links={:?}",
                            case["id"],
                            event.input_strength(p0_input),
                            p0_event_links.iter().map(|&link| event.link_state_for_test(link)).collect::<Vec<_>>()
                        );
                        println!(
                            "{} carry at event entry: links={:?}, pending deadline={:?}, resource1={:?}, owners={:?}, notifications={}",
                            case["id"],
                            carry_links.iter().map(|&link| event.link_state_for_test(link)).collect::<Vec<_>>(),
                            event.scheduled_delay_for_test(carry),
                            event.resource(ResourceId::new(1)),
                            event_resource1_owners
                                .iter()
                                .map(|&(piston, input)| {
                                    (
                                        piston,
                                        lowered.program.nodes()[input.index()].position,
                                        event.input_strength(input),
                                        event.piston(piston),
                                    )
                                })
                                .collect::<Vec<_>>(),
                            event.counts().notifications
                        );
                    }
                }
                "wait" | "wait_ready" => {
                    for _ in 0..action["ticks"].as_u64().unwrap() {
                        event.tick().unwrap();
                    }
                }
                op => panic!("unexpected manifest action {op}"),
            }
        }
        for tick in 1..=case["ticks"].as_u64().unwrap() {
            let before = event.counts();
            event.tick().unwrap();
            let after = event.counts();
            if after.resource_movements != before.resource_movements || after.motion_completions != before.motion_completions {
                println!(
                    "{} tick={tick} {} carry links={:?}, pending deadline={:?}, resource1={:?}, owners={:?}, notifications += {}",
                    case["id"],
                    if after.resource_movements != before.resource_movements {
                        "departure"
                    } else {
                        "arrival"
                    },
                    carry_links.iter().map(|&link| event.link_state_for_test(link)).collect::<Vec<_>>(),
                    event.scheduled_delay_for_test(carry),
                    event.resource(ResourceId::new(1)),
                    event_resource1_owners
                        .iter()
                        .map(|&(piston, input)| {
                            (
                                piston,
                                lowered.program.nodes()[input.index()].position,
                                event.input_strength(input),
                                event.piston(piston),
                            )
                        })
                        .collect::<Vec<_>>(),
                    after.notifications - before.notifications
                );
            }
            if (3..=7).contains(&tick) {
                assert_eq!(
                    u64::from(!event.component(sum).unwrap().powered),
                    case["expectation"]["sum"].as_u64().unwrap(),
                    "{} raw sum repeater tick {tick}",
                    case["id"]
                );
            }
            if (5..=9).contains(&tick) {
                assert_eq!(
                    u64::from(!event.component(carry).unwrap().powered),
                    case["expectation"]["carry"].as_u64().unwrap(),
                    "{} raw carry repeater tick {tick}",
                    case["id"]
                );
            }
        }
        let counts = event.counts();
        assert!(counts.notifications > 0, "{} compiled notifications", case["id"]);
        resource_movements += counts.resource_movements;
    }
    assert!(resource_movements > 0, "arithmetic vectors must exercise compiled resource movement");
}

/// This gate intentionally fails when the real circuit cannot enter the event executor.
#[test]
#[ignore = "real adder event admission and arithmetic gate; run explicitly"]
fn offline_adder11_event_arithmetic() {
    for id in ["prepared-0-0-0", "prepared-1023-1-0", "prepared-2047-1-0", "prepared-2047-2047-0"] {
        let (world, bounds, manifest) = fixture("adder_11bits");
        let lowered = lower(&world, bounds).unwrap_or_else(|error| panic!("{id} lowering: {error}"));
        let entry = EntryWork::capture(&world, bounds);
        let mut runtime =
            CompiledPistonActors::compile(&lowered.program, &lowered.state, &entry).unwrap_or_else(|error| panic!("{id} event admission: {error}"));
        let case = manifest["cases"].as_array().unwrap().iter().find(|case| case["id"] == id).unwrap();
        let trigger = position(&manifest["ports"]["inputs"]["trigger"]);
        runtime.begin_computation_cycle();
        for action in case["actions"].as_array().unwrap() {
            match action["op"].as_str().unwrap() {
                "lever" => {
                    let target = position(&action["pos"]);
                    if target == trigger {
                        runtime.begin_computation_cycle();
                    }
                    runtime.set_lever(target, action["powered"].as_bool().unwrap()).unwrap();
                }
                "wait" | "wait_ready" => {
                    for _ in 0..action["ticks"].as_u64().unwrap() {
                        runtime.advance_tick().unwrap();
                    }
                }
                other => panic!("unsupported manifest action {other}"),
            }
        }
        for tick in 1..=case["ticks"].as_u64().unwrap() {
            runtime.advance_tick().unwrap();
            if (3..=7).contains(&tick) {
                let positions = lowered.program.position_index();
                let sum = manifest["ports"]["observations"]["sum_repeater"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .enumerate()
                    .fold(0u16, |sum, (bit, local)| {
                        let node = positions[&position(local)].index();
                        let ComponentState::Electrical(state) = &runtime.signals.state().components[node] else {
                            panic!("output repeater state missing")
                        };
                        sum | (u16::from(!state.powered) << bit)
                    });
                let inputs = case["inputs"].as_array().unwrap();
                let expected = ((inputs[0].as_u64().unwrap() + inputs[1].as_u64().unwrap()) & 0x7ff) as u16;
                assert_eq!(sum, expected, "{id} arithmetic output at tick {tick}");
            }
        }
        let movements = runtime.successful_moves.iter().sum::<u64>();
        let actions = runtime.successful_actions.iter().sum::<u64>();
        eprintln!(
            "{id} event work: movements={movements}, actions={actions}, pending={}, deferred={}, consumed={}",
            runtime.pending.len(),
            runtime.deferred.len(),
            runtime.consumed.iter().filter(|value| **value).count()
        );
        assert!(movements > 0, "{id} must execute piston movements");
    }
}

#[test]
#[ignore = "real adder event and physical Graph timeline"]
fn offline_adder11_graph_timeline() {
    use crate::redpiler::backend::graph::GraphBackend;
    use crate::world::World;
    use mchprs_blocks::blocks::Block;

    let (mut world, bounds, manifest) = fixture("adder_11bits");
    let lowered = lower(&world, bounds).unwrap();
    let mut graph = GraphBackend::compile(&world, bounds, world.scheduler().iter_entries().collect(), Default::default()).unwrap();
    let mut runtime = CompiledPistonActors::compile(&lowered.program, &lowered.state, &EntryWork::capture(&world, bounds)).unwrap();
    // Diagnostic only: clearing each tick is not a declared computation-cycle policy.
    let mut unlatching = runtime.clone();
    let trigger = position(&manifest["ports"]["inputs"]["trigger"]);
    let mut controls = std::collections::HashMap::new();
    for group in ["A", "B"] {
        let mut value = 0u16;
        for (bit, local) in manifest["ports"]["inputs"][group].as_array().unwrap().iter().enumerate() {
            let pos = position(local);
            let Block::Lever { lever } = world.get_block(pos) else {
                panic!("missing input lever")
            };
            controls.insert(pos, lever.powered);
            value |= u16::from(!lever.powered) << bit;
        }
        println!("loaded {group}={value}");
    }
    let Block::Lever { lever } = world.get_block(trigger) else {
        panic!("missing trigger")
    };
    controls.insert(trigger, lever.powered);
    println!("loaded trigger={}", lever.powered);
    let report = |label: &str, graph: &GraphBackend, runtime: &CompiledPistonActors, world: &mut PlotWorld| {
        world.clear_scheduled_ticks();
        world.piston_state_mut().motions.clear();
        graph.project_future(world);
        let positions = lowered.program.position_index();
        let mut event = 0u16;
        let mut physical = 0u16;
        for (bit, local) in manifest["ports"]["observations"]["sum_repeater"].as_array().unwrap().iter().enumerate() {
            let pos = position(local);
            let Block::RedstoneRepeater { repeater } = world.get_block(pos) else {
                panic!("missing output")
            };
            physical |= u16::from(!repeater.powered) << bit;
            let ComponentState::Electrical(state) = &runtime.signals.state().components[positions[&pos].index()] else {
                panic!("missing event output")
            };
            event |= u16::from(!state.powered) << bit;
        }
        let flags = runtime
            .signals
            .state()
            .components
            .iter()
            .filter(|state| matches!(state, ComponentState::Electrical(value) if value.pending_tick))
            .count();
        let piston = world.piston_state();
        println!(
            "{label}: Graph={physical:03x} event={event:03x}; Graph work component={} piston={} motion={}; event work component={flags} piston={} deferred={}; moves={} actions={} consumed={}",
            world.scheduler().iter_entries().count(),
            piston.events.len(),
            piston.motions.len(),
            runtime.pending.len(),
            runtime.deferred.len(),
            runtime.movement_count(),
            runtime.successful_actions.iter().sum::<u64>(),
            runtime.consumed.iter().filter(|value| **value).count()
        );
    };
    report("load", &graph, &runtime, &mut world);
    runtime.begin_computation_cycle();
    let case = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "prepared-0-0-0")
        .unwrap();
    for action in case["actions"].as_array().unwrap() {
        match action["op"].as_str().unwrap() {
            "lever" => {
                let pos = position(&action["pos"]);
                let value = action["powered"].as_bool().unwrap();
                if pos == trigger {
                    report("prepared", &graph, &runtime, &mut world);
                    report("prepared (diagnostic latch cleared each tick)", &graph, &unlatching, &mut world);
                    runtime.begin_computation_cycle();
                }
                if controls[&pos] != value {
                    graph.use_block(pos).unwrap();
                    controls.insert(pos, value);
                }
                runtime.set_lever(pos, value).unwrap();
                unlatching.set_lever(pos, value).unwrap();
            }
            "wait" | "wait_ready" => {
                for tick in 1..=action["ticks"].as_u64().unwrap() {
                    graph.tick().unwrap();
                    runtime.advance_tick().unwrap();
                    unlatching.begin_computation_cycle();
                    unlatching.advance_tick().unwrap();
                    report(&format!("preparation tick {tick}"), &graph, &runtime, &mut world);
                }
            }
            other => panic!("unexpected action {other}"),
        }
    }
    for tick in 1..=128 {
        graph.tick().unwrap();
        runtime.advance_tick().unwrap();
        unlatching.begin_computation_cycle();
        unlatching.advance_tick().unwrap();
        if tick <= 24 || tick % 16 == 0 {
            report(&format!("tick {tick}"), &graph, &runtime, &mut world);
            report(
                &format!("tick {tick} (diagnostic latch cleared each tick)"),
                &graph,
                &unlatching,
                &mut world,
            );
        }
    }
}

#[test]
#[ignore = "focused bit-zero preparation diagnostic"]
fn offline_adder11_bit_zero_preparation_cone() {
    use crate::redpiler::backend::graph::GraphBackend;
    use crate::redpiler::program_graph::{InputPort, PistonId, PowerTarget};
    use crate::world::World;
    use mchprs_blocks::blocks::Block;

    let (mut world, bounds, manifest) = fixture("adder_11bits");
    let lowered = lower(&world, bounds).unwrap();
    let program = &lowered.program;
    let mut graph = GraphBackend::compile(&world, bounds, world.scheduler().iter_entries().collect(), Default::default()).unwrap();
    let mut event = CompiledPistonActors::compile(program, &lowered.state, &EntryWork::capture(&world, bounds)).unwrap();
    let output = position(&manifest["ports"]["observations"]["sum_repeater"][0]);
    let payload = position(&manifest["ports"]["observations"]["sum_payload"][0]);
    let node = program.position_index()[&output];
    // Route discovered by the diagnostic trace; this is not an execution rule.
    let source_wire = program.routes()[334].source;
    let contact = program.contacts().iter().position(|contact| contact.position == payload).unwrap();
    let resources: Vec<_> = program
        .resources()
        .iter()
        .enumerate()
        .filter(|(_, r)| r.contacts.iter().any(|c| c.index() == contact))
        .map(|(i, _)| i)
        .collect();
    let actors: Vec<_> = program
        .pistons()
        .iter()
        .enumerate()
        .filter(|(i, _)| {
            program
                .resource_for_piston(PistonId::new(*i as u32))
                .is_some_and(|id| resources.contains(&id.index()))
        })
        .map(|(i, _)| i)
        .collect();
    println!("bit0 output={output:?} node={node:?} payload={payload:?} contact={contact} resources={resources:?} actors={actors:?}");
    for link in program.power_links_to(PowerTarget::Input { node, port: InputPort::Main }) {
        println!("input link {link:?} source={:?}", program.sites()[link.from.index()]);
    }
    for &actor in &actors {
        println!("actor {actor}: {:?}", program.pistons()[actor]);
    }
    let report = |label: &str, graph: &GraphBackend, event: &CompiledPistonActors, world: &mut PlotWorld| {
        world.clear_scheduled_ticks();
        world.piston_state_mut().motions.clear();
        graph.project_future(world);
        println!(
            "{label}: output graph={:?} event={:?} event_input={:?} graph_schedule={:?} event_schedule={:?}",
            world.get_block(output),
            event.signals.state().components[node.index()],
            event.signals.diagnostic_input_power(node.index(), InputPort::Main),
            world.scheduler().iter_entries().filter(|e| e.pos == output).collect::<Vec<_>>(),
            event.signals.diagnostic_scheduled(node.index())
        );
        println!(
            "  triggering wire {source_wire:?}: graph={:?} event={:?}",
            graph.circuit_state().components[source_wire.index()],
            event.signals.state().components[source_wire.index()]
        );
        for &resource in &resources {
            println!(
                "  resource {resource}: graph={:?} event={:?} consumed={}",
                graph.circuit_state().resources[resource],
                event.signals.state().resources[resource],
                event.consumed[resource]
            );
        }
        for &actor in &actors {
            println!(
                "  actor {actor}: graph={:?} event={:?} moves={} actions={} pending={:?} deferred={:?}",
                graph.circuit_state().pistons[actor],
                event.signals.state().pistons[actor],
                event.successful_moves[actor],
                event.successful_actions[actor],
                event.pending.iter().filter(|e| e.piston.index() == actor).collect::<Vec<_>>(),
                event
                    .deferred
                    .iter()
                    .filter(|e| e.0 == actor)
                    .map(|e| (
                        e.1.due_tick,
                        e.1.revision,
                        e.1.extending,
                        e.1.resource_revision,
                        e.1.resource_contact,
                        e.2
                    ))
                    .collect::<Vec<_>>()
            );
        }
    };
    report("load", &graph, &event, &mut world);
    event.begin_computation_cycle();
    let case = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "prepared-0-0-0")
        .unwrap();
    for action in case["actions"].as_array().unwrap() {
        if action["op"] != "lever" {
            break;
        }
        let pos = position(&action["pos"]);
        let value = action["powered"].as_bool().unwrap();
        let Block::Lever { lever } = world.get_block(pos) else {
            panic!("missing lever")
        };
        if lever.powered != value {
            graph.use_block(pos).unwrap();
        }
        event.set_lever(pos, value).unwrap();
        report(&format!("input {pos:?}={value}"), &graph, &event, &mut world);
    }
    for tick in 1..=3 {
        graph.tick().unwrap();
        event.advance_tick().unwrap();
        report(&format!("tick {tick}"), &graph, &event, &mut world);
    }
}
