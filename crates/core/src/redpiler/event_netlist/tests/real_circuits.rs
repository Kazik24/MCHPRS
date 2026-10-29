//! Offline executable gates for the pinned adder and counter fixtures.
use super::*;
use crate::plot::{
    PLOT_WIDTH, PlotWorld,
    worldedit::{load_schematic, paste_clipboard},
};
use crate::redpiler::physical_signal_network::{EntryWork, PhysicalSignalNetwork};
use crate::redpiler::program_graph::{CircuitNodeKind, ComponentState, LoweredCircuit, PistonLane};
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
        assert!(links > 0 && runtime.movement_count() > 0, "compiled work and movements required");
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
    let mut event = CompiledPistonActors::compile(&lowered.program, &lowered.state, &EntryWork::capture(&world, bounds)).unwrap();
    let case = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "prepared-0-0-0")
        .unwrap();
    let trigger = position(&manifest["ports"]["inputs"]["trigger"]);
    let positions = lowered.program.position_index();
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
        graph.tick().unwrap();
        event.advance_tick().unwrap();
        report(&format!("trigger tick {tick}"), &graph, &event, &mut world);
    }
}

fn position(value: &Value) -> BlockPos {
    ORIGIN
        + BlockPos::new(
            value[0].as_i64().unwrap() as i32,
            value[1].as_i64().unwrap() as i32,
            value[2].as_i64().unwrap() as i32,
        )
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
