//! Stateful COUNTER check for the settled logical-device runtime.
use super::*;
use crate::plot::PlotWorld;
use crate::redpiler::CompilerOptions;
use crate::world::World;
use mchprs_blocks::blocks::{Block, LeverFace};
use std::sync::Arc;

const ORIGIN: BlockPos = BlockPos::new(40, 30, 40);

fn manifest() -> serde_json::Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/instant-pistons-io/fixtures/counter_basic.json");
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn positions(value: &serde_json::Value) -> Vec<BlockPos> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            ORIGIN
                + BlockPos::new(
                    entry[0].as_i64().unwrap() as i32,
                    entry[1].as_i64().unwrap() as i32,
                    entry[2].as_i64().unwrap() as i32,
                )
        })
        .collect()
}

fn memory_q(world: &PlotWorld, positions: &[BlockPos]) -> Option<u16> {
    positions.iter().enumerate().try_fold(0u16, |word, (bit, &position)| {
        let Block::Piston { piston } = world.get_block(position) else {
            return None;
        };
        Some(word | (u16::from(piston.extended) << bit))
    })
}

#[test]
fn one_counter_register_and_public_bit_match_native_cadence() {
    use crate::redpiler::analysis::AnalysisLimits;
    use crate::redpiler::analysis::instant::layers::logical::compile_public_boundary;
    use crate::redpiler::passes;
    use crate::redpiler::program_graph::{InputPort, LayerInputSource, PowerTarget, SettledTarget, SiteSource, StateTest};
    use crate::world::World;

    let (mut world, bounds) = super::adder_tests::counter_world();
    let manifest = manifest();
    let trigger = ORIGIN
        + BlockPos::new(
            manifest["ports"]["inputs"]["trigger"][0].as_i64().unwrap() as i32,
            manifest["ports"]["inputs"]["trigger"][1].as_i64().unwrap() as i32,
            manifest["ports"]["inputs"]["trigger"][2].as_i64().unwrap() as i32,
        );
    let memory_position = positions(&manifest["ports"]["observations"]["memory"])[0];
    let output_position = positions(&manifest["ports"]["observations"]["repeater"])[0];

    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let mut prepared = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::for_budget(8)).unwrap();
    let options = CompilerOptions {
        optimization_level: Some(6),
        budget_multiplier: 8,
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
    let graph = prepared.graph;
    let read = *graph
        .plans
        .bud_memory_reads
        .iter()
        .find(|read| graph.pistons[read.storage.index()].position == memory_position)
        .expect("the first manifest memory piston has a BUD register read");
    let output_node = graph
        .nodes
        .iter()
        .position(|node| node.position == Some(output_position))
        .expect("the first manifest output repeater is in the graph");
    let output_target = PowerTarget::Input {
        node: NodeId::new(output_node as u32),
        port: InputPort::Main,
    };
    let circuit = compile_public_boundary(&graph, &[output_target], &[read], &monitor).unwrap();
    assert!(circuit.declines.is_empty());
    assert_eq!(circuit.outputs.len(), 2, "one requested output and one matching register");

    let output_index = circuit
        .outputs
        .iter()
        .position(|output| output.target == SettledTarget::Power(output_target))
        .unwrap();
    let register_index = circuit
        .outputs
        .iter()
        .position(|output| output.target == SettledTarget::Register(read))
        .unwrap();
    let initial_q = match world.get_block(memory_position) {
        Block::Piston { piston } => piston.extended,
        other => panic!("expected stationary memory piston, found {other:?}"),
    };
    let sample_source = |source: LayerInputSource| -> bool {
        let LayerInputSource::SourceAbove { site, .. } = source else {
            panic!("unexpected non-source input in the one-bit counter kernel: {source:?}");
        };
        match graph.sites[site.index()].source {
            SiteSource::Component(node) if graph.nodes[node.index()].position == Some(trigger) => true,
            SiteSource::Component(node)
                if matches!(
                    graph.nodes[node.index()].kind,
                    CircuitNodeKind::Electrical(crate::redpiler::components::NodeType::Torch)
                        | CircuitNodeKind::Observer { .. }
                        | CircuitNodeKind::Electrical(crate::redpiler::components::NodeType::Observer { .. })
                ) =>
            {
                false
            }
            other => panic!("unexpected counter stable-frame source: {other:?}"),
        }
    };
    let mut inputs = Vec::with_capacity(circuit.program.inputs.len());
    for input in circuit.program.inputs.iter() {
        inputs.push(match input.source {
            LayerInputSource::SourceAbove { .. } => sample_source(input.source),
            LayerInputSource::State(StateTest::ResourceAt { resource, contact }) if resource == read.resource => {
                let q = initial_q;
                let storage = graph.pistons[read.storage.index()];
                let other_contact = match storage.lane {
                    crate::redpiler::program_graph::PistonLane::SingleResource { far } if read.source == storage.near => far,
                    crate::redpiler::program_graph::PistonLane::SingleResource { far } if read.source == far => storage.near,
                    _ => panic!("selected BUD read is outside a single-resource lane"),
                };
                if contact == read.source {
                    q
                } else if contact == other_contact {
                    !q
                } else {
                    panic!("register input addresses an unrelated contact: {contact:?}")
                }
            }
            other => panic!("unexpected state dependency in one-bit register/output cone: {other:?}"),
        });
    }
    let mut machine = crate::redpiler::program_graph::SettledMachine::new(&circuit, &inputs).unwrap();
    let external = machine
        .external_inputs()
        .iter()
        .map(|input| sample_source(input.source))
        .collect::<Vec<_>>();

    set_native_lever(&mut world, trigger, true);
    let mut stable_updates = 0;
    let mut publications = 0;
    for tick in 1..=1_560 {
        world.tick_interpreted();
        if tick % 6 == 0 {
            machine.step(&external).unwrap();
            let Block::Piston { piston } = world.get_block(memory_position) else {
                panic!("memory piston is not stationary at computation tick {tick}");
            };
            let native_q = piston.extended;
            let logical_q = machine.outputs()[register_index] != 0;
            assert_eq!(logical_q, native_q, "single register bit at computation tick {tick}");
            stable_updates += 1;
        }
        if tick % 6 == 5 && tick >= 11 {
            machine.observe(&external).unwrap();
            let Block::RedstoneRepeater { repeater } = world.get_block(output_position) else {
                panic!("public repeater is missing at publication tick {tick}");
            };
            let native_bit = !repeater.powered;
            let logical_bit = machine.outputs()[output_index] == 0;
            assert_eq!(logical_bit, native_bit, "single public output bit at publication tick {tick}");
            publications += 1;
        }
    }
    assert_eq!(stable_updates, 260);
    assert_eq!(publications, 259);
}

fn output_count(world: &PlotWorld, positions: &[BlockPos]) -> Option<u16> {
    positions.iter().enumerate().try_fold(0u16, |word, (bit, &position)| {
        let Block::RedstoneRepeater { repeater } = world.get_block(position) else {
            return None;
        };
        // The fixture's public output bank is active low.
        Some(word | (u16::from(!repeater.powered) << bit))
    })
}

fn set_native_lever(world: &mut PlotWorld, position: BlockPos, powered: bool) {
    let Block::Lever { mut lever } = world.get_block(position) else {
        panic!("counter trigger is not a lever at {position:?}");
    };
    if lever.powered == powered {
        return;
    }
    lever.powered = powered;
    world.set_block(position, Block::Lever { lever });
    crate::redstone::update_surrounding_blocks(world, position);
    let support = match lever.face {
        LeverFace::Floor => BlockFace::Bottom,
        LeverFace::Ceiling => BlockFace::Top,
        LeverFace::Wall => lever.facing.opposite().block_face(),
    };
    crate::redstone::update_surrounding_blocks(world, position.offset(support));
}

fn tick_pair(backend: &mut GraphBackend, graph_world: &mut PlotWorld, native: &mut PlotWorld) {
    backend.tick().unwrap();
    backend.flush(graph_world, false);
    native.tick_interpreted();
}

#[test]
fn o6_counter_output_matches_native_each_frame_through_control_changes() {
    let (mut native, bounds) = super::adder_tests::counter_world();
    let (mut graph_world, _) = super::adder_tests::counter_world();
    let manifest = manifest();
    let trigger = ORIGIN
        + BlockPos::new(
            manifest["ports"]["inputs"]["trigger"][0].as_i64().unwrap() as i32,
            manifest["ports"]["inputs"]["trigger"][1].as_i64().unwrap() as i32,
            manifest["ports"]["inputs"]["trigger"][2].as_i64().unwrap() as i32,
        );
    let output_positions = positions(&manifest["ports"]["observations"]["repeater"]);
    let ticks = graph_world.scheduler().iter_entries().collect();
    let options = CompilerOptions {
        optimization_level: Some(6),
        budget_multiplier: 8,
        ..Default::default()
    };
    let mut backend = GraphBackend::compile_with_options(&graph_world, bounds, ticks, &options, Arc::new(TaskMonitor::default())).unwrap();
    assert!(!backend.logical_devices.is_empty(), "COUNTER must use the logical device");

    let mut powered = false;
    let mut changed_at = 0;
    let mut frames = 0;
    let mut saw_byte_max = false;
    let mut saw_byte_carry = false;
    for tick in 1..=1_700 {
        if [7, 40, 55, 90, 109, 145, 168].contains(&tick) {
            powered = !powered;
            changed_at = tick;
            set_native_lever(&mut native, trigger, powered);
            backend.use_block(trigger).unwrap();
            backend.flush(&mut graph_world, false);
        }
        tick_pair(&mut backend, &mut graph_world, &mut native);
        let elapsed = tick - changed_at + 1;
        // Transient output values may differ between settled publication frames.
        if (powered && elapsed % 6 == 5) || (!powered && elapsed >= 12) {
            let word = output_count(&native, &output_positions).expect("native output repeater bank");
            saw_byte_max |= word == 255;
            saw_byte_carry |= word == 256;
            for &position in &output_positions {
                assert_eq!(
                    graph_world.get_block(position),
                    native.get_block(position),
                    "output at {position:?}, tick {tick}"
                );
            }
            frames += 1;
        }
    }
    assert!(backend.execution_counts().logical_kernel_evaluations > 0);
    assert!(
        frames > 256 && saw_byte_max && saw_byte_carry,
        "frames={frames}, saw 255={saw_byte_max}, saw 256={saw_byte_carry}"
    );
}

#[test]
fn o6_counter_matches_native_at_stable_storage_and_publication_cuts() {
    let (mut native, bounds) = super::adder_tests::counter_world();
    let (mut graph_world, graph_bounds) = super::adder_tests::counter_world();
    assert_eq!(graph_bounds, bounds);
    let manifest = manifest();
    let trigger = ORIGIN
        + BlockPos::new(
            manifest["ports"]["inputs"]["trigger"][0].as_i64().unwrap() as i32,
            manifest["ports"]["inputs"]["trigger"][1].as_i64().unwrap() as i32,
            manifest["ports"]["inputs"]["trigger"][2].as_i64().unwrap() as i32,
        );
    let memory_positions = positions(&manifest["ports"]["observations"]["memory"]);
    let output_positions = positions(&manifest["ports"]["observations"]["repeater"]);
    let ticks = graph_world.scheduler().iter_entries().collect();
    let options = CompilerOptions {
        optimization_level: Some(6),
        budget_multiplier: 8,
        ..Default::default()
    };
    let monitor = Arc::new(TaskMonitor::default());
    let mut backend = GraphBackend::compile_with_options(&graph_world, bounds, ticks, &options, monitor).unwrap();
    assert!(
        !backend.logical_devices.is_empty(),
        "the O6 COUNTER must install its settled logical device"
    );

    let initial = memory_q(&native, &memory_positions).expect("stationary initial memory bank");
    assert_eq!(initial, memory_q(&graph_world, &memory_positions).unwrap());

    // An OFF interval must leave the registered word unchanged. Then the ordinary
    // manifest trigger starts the native cadence and the logical runtime together.
    for _ in 0..6 {
        tick_pair(&mut backend, &mut graph_world, &mut native);
    }
    assert_eq!(memory_q(&native, &memory_positions), Some(initial));
    assert_eq!(memory_q(&graph_world, &memory_positions), Some(initial));
    set_native_lever(&mut native, trigger, true);
    backend.use_block(trigger).unwrap();
    backend.flush(&mut graph_world, false);

    let mut stable_updates = 0;
    let mut publications = 0;
    for tick in 1..=1_560 {
        tick_pair(&mut backend, &mut graph_world, &mut native);

        if tick % 6 == 0 {
            let native_q = memory_q(&native, &memory_positions).expect("native word at the six-tick stationary cut");
            let graph_q = memory_q(&graph_world, &memory_positions).expect("projected O6 word at the stationary cut");
            assert_eq!(graph_q, native_q, "logical q bits at computation cut {tick}");
            assert_eq!(
                native_q,
                initial.wrapping_sub((tick / 6) as u16),
                "native q-register progression at tick {tick}"
            );
            stable_updates += 1;
        }

        if tick % 6 == 5 && tick >= 11 {
            let native_output = output_count(&native, &output_positions).expect("native publication bank");
            let graph_output = output_count(&graph_world, &output_positions).expect("projected O6 publication bank");
            assert_eq!(graph_output, native_output, "public repeater bank at phase-five tick {tick}");
            assert_eq!(
                native_output,
                (!initial).wrapping_add((tick / 6) as u16),
                "valid output word at tick {tick}"
            );
            publications += 1;
        }
    }
    assert_eq!(stable_updates, 260);
    assert_eq!(publications, 259);

    let counts = backend.execution_counts();
    assert!(
        counts.logical_kernel_evaluations >= 260,
        "every native computation cut is evaluated: {counts:?}"
    );
    assert!(counts.logical_kernel_operations > 0, "the logical DAG must perform work: {counts:?}");
    assert_eq!(counts.callbacks, 0, "the logical device must not run physical callbacks: {counts:?}");
    assert_eq!(counts.wire_walks, 0, "the logical device must not walk physical wires: {counts:?}");
    assert_eq!(
        counts.piston_samples, 0,
        "the logical device must not sample physical pistons: {counts:?}"
    );
    assert_eq!(counts.piston_actions, 0);
    assert_eq!(counts.deferred_publications, 0);
    assert_eq!(counts.scheduled_ticks, 0);

    // Stop and restart once more to catch an adapter that advances on logical
    // evaluations alone instead of respecting the fixture's clock enable.
    set_native_lever(&mut native, trigger, false);
    backend.use_block(trigger).unwrap();
    backend.flush(&mut graph_world, false);
    let stopped = memory_q(&native, &memory_positions).unwrap();
    for _ in 0..12 {
        tick_pair(&mut backend, &mut graph_world, &mut native);
    }
    assert_eq!(memory_q(&native, &memory_positions), Some(stopped));
    assert_eq!(memory_q(&graph_world, &memory_positions), Some(stopped));
    set_native_lever(&mut native, trigger, true);
    backend.use_block(trigger).unwrap();
    for tick in 1..=30 {
        tick_pair(&mut backend, &mut graph_world, &mut native);
        if tick % 6 == 0 {
            assert_eq!(memory_q(&graph_world, &memory_positions), memory_q(&native, &memory_positions));
        }
        if tick % 6 == 5 {
            assert_eq!(output_count(&graph_world, &output_positions), output_count(&native, &output_positions));
        }
    }
}

#[test]
#[ignore = "native COUNTER diagnostic for BUD request-versus-acceptance reads at OFF phase cuts"]
fn native_counter_bud_request_acceptance_reads_across_off_phases() {
    use crate::redstone::piston::trace::{self, Stage};

    let manifest = manifest();
    let trigger = ORIGIN
        + BlockPos::new(
            manifest["ports"]["inputs"]["trigger"][0].as_i64().unwrap() as i32,
            manifest["ports"]["inputs"]["trigger"][1].as_i64().unwrap() as i32,
            manifest["ports"]["inputs"]["trigger"][2].as_i64().unwrap() as i32,
        );
    let memory_positions = positions(&manifest["ports"]["observations"]["memory"]);
    let mut phase_traces = Vec::new();

    for off_phase in 0..6 {
        let (mut world, _) = super::adder_tests::counter_world();
        set_native_lever(&mut world, trigger, true);

        // Stop after three completed updates at each cadence offset. Capture the
        // actual BUD piston reads before and after the OFF transition.
        let mut entries = trace::capture_detailed(&memory_positions, || {
            for _ in 0..(18 + off_phase) {
                world.tick_interpreted();
            }
            // Capture synchronous neighbor updates caused by the OFF transition
            // alongside the subsequent tick-level request/acceptance records.
            set_native_lever(&mut world, trigger, false);
        });
        entries.extend(trace::capture_detailed(&memory_positions, || {
            for _ in 0..24 {
                world.tick_interpreted();
            }
        }));

        let mut paired = 0;
        let mut changed_live_read = Vec::new();
        let mut matched_accepts = vec![false; entries.len()];
        for (request_index, request) in entries.iter().enumerate().filter(|(_, entry)| entry.stage == Stage::Request) {
            let Some(event) = request.event else { continue };
            let acceptance_index = entries.iter().enumerate().find_map(|(index, entry)| {
                (!matched_accepts[index]
                    && index > request_index
                    && entry.stage == Stage::Accept
                    && entry.tick == request.tick
                    && entry.pos == request.pos
                    && entry.event == Some(event))
                .then_some(index)
            });
            let Some(acceptance_index) = acceptance_index else { continue };
            matched_accepts[acceptance_index] = true;
            let acceptance = &entries[acceptance_index];
            paired += 1;
            if request.powered != acceptance.powered {
                changed_live_read.push((request.tick, request.pos, request.powered, acceptance.powered, event));
            }
        }

        let requests = entries.iter().filter(|entry| entry.stage == Stage::Request).count();
        let accepts = entries.iter().filter(|entry| entry.stage == Stage::Accept).count();
        println!(
            "COUNTER native BUD OFF phase={off_phase}: requests={requests} accepts={accepts} paired={paired} changed_reads={changed_live_read:?}"
        );
        assert!(requests > 0, "the native clock should sample BUD memory during the diagnostic window");
        assert!(paired > 0, "at least one sampled event should reach live acceptance");
        phase_traces.push((off_phase, entries));
    }

    let diagnostic_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/logical-inference/counter-bud-off-phases.json");
    std::fs::create_dir_all(diagnostic_path.parent().unwrap()).unwrap();
    std::fs::write(diagnostic_path, serde_json::to_vec_pretty(&phase_traces).unwrap()).unwrap();
}

#[test]
#[ignore = "O6 COUNTER differential at all six OFF cuts and restart"]
fn o6_counter_off_phase_cuts_match_native_and_restart() {
    let mut observations = Vec::new();
    let mut cut_mismatches = Vec::new();
    for off_phase in 0..6 {
        let (mut native, bounds) = super::adder_tests::counter_world();
        let (mut graph_world, graph_bounds) = super::adder_tests::counter_world();
        assert_eq!(graph_bounds, bounds);
        let manifest = manifest();
        let trigger = ORIGIN
            + BlockPos::new(
                manifest["ports"]["inputs"]["trigger"][0].as_i64().unwrap() as i32,
                manifest["ports"]["inputs"]["trigger"][1].as_i64().unwrap() as i32,
                manifest["ports"]["inputs"]["trigger"][2].as_i64().unwrap() as i32,
            );
        let memory_positions = positions(&manifest["ports"]["observations"]["memory"]);
        let output_positions = positions(&manifest["ports"]["observations"]["repeater"]);
        let ticks = graph_world.scheduler().iter_entries().collect();
        let options = CompilerOptions {
            optimization_level: Some(6),
            budget_multiplier: 8,
            ..Default::default()
        };
        let mut backend = GraphBackend::compile_with_options(&graph_world, bounds, ticks, &options, Arc::new(TaskMonitor::default())).unwrap();
        assert!(!backend.logical_devices.is_empty());

        set_native_lever(&mut native, trigger, true);
        backend.use_block(trigger).unwrap();
        backend.flush(&mut graph_world, false);
        for relative_tick in 1..=(18 + off_phase) {
            tick_pair(&mut backend, &mut graph_world, &mut native);
            let native_tick = native.piston_state().logical_tick;
            let native_memory = memory_q(&native, &memory_positions);
            let logical_memory = memory_q(&graph_world, &memory_positions);
            let native_output = output_count(&native, &output_positions);
            let logical_output = output_count(&graph_world, &output_positions);
            observations.push(serde_json::json!({
                "off_phase": off_phase, "segment": "on", "relative_tick": relative_tick,
                "native_tick": native_tick, "native_memory": native_memory,
                "logical_memory": logical_memory, "native_output": native_output,
                "logical_output": logical_output,
            }));
            if relative_tick % 6 == 0
                && let (Some(native_word), Some(logical_word)) = (native_memory, logical_memory)
                && native_word != logical_word
            {
                cut_mismatches.push(serde_json::json!({
                    "off_phase": off_phase, "segment": "on", "relative_tick": relative_tick,
                    "native_tick": native_tick, "kind": "memory_phase0",
                    "native": native_word, "logical": logical_word,
                }));
            }
            if relative_tick % 6 == 5
                && let (Some(native_word), Some(logical_word)) = (native_output, logical_output)
                && native_word != logical_word
            {
                cut_mismatches.push(serde_json::json!({
                    "off_phase": off_phase, "segment": "on", "relative_tick": relative_tick,
                    "native_tick": native_tick, "kind": "output_phase5",
                    "native": native_word, "logical": logical_word,
                }));
            }
        }

        set_native_lever(&mut native, trigger, false);
        backend.use_block(trigger).unwrap();
        backend.flush(&mut graph_world, false);
        for tick in 1..=24 {
            tick_pair(&mut backend, &mut graph_world, &mut native);
            let native_tick = native.piston_state().logical_tick;
            let native_memory = memory_q(&native, &memory_positions);
            let logical_memory = memory_q(&graph_world, &memory_positions);
            let native_output = output_count(&native, &output_positions);
            let logical_output = output_count(&graph_world, &output_positions);
            observations.push(serde_json::json!({
                "off_phase": off_phase, "segment": "off", "relative_tick": tick,
                "native_tick": native_tick, "native_memory": native_memory,
                "logical_memory": logical_memory, "native_output": native_output,
                "logical_output": logical_output,
                "native_phase5_cut": native_tick % 6 == 5,
            }));
            if native_tick % 6 == 0
                && let (Some(native_word), Some(logical_word)) = (native_memory, logical_memory)
                && native_word != logical_word
            {
                cut_mismatches.push(serde_json::json!({
                    "off_phase": off_phase, "segment": "off", "relative_tick": tick,
                    "native_tick": native_tick, "kind": "memory_phase0",
                    "native": native_word, "logical": logical_word,
                }));
            }
            // OFF-period output samples are retained in the timeline, but are
            // not asserted as valid publications: the native controller can
            // expose its reset/settling state while its clock input is stopped.
        }
        let stopped = memory_q(&native, &memory_positions).expect("native memory settles after OFF");
        if memory_q(&graph_world, &memory_positions) != Some(stopped) {
            cut_mismatches.push(serde_json::json!({
                "off_phase": off_phase, "segment": "off_final", "kind": "memory_after_drain",
                "native": stopped, "logical": memory_q(&graph_world, &memory_positions),
            }));
        }
        let native_output = output_count(&native, &output_positions);
        let logical_output = output_count(&graph_world, &output_positions);
        if native_output != logical_output {
            cut_mismatches.push(serde_json::json!({
                "off_phase": off_phase, "segment": "off_final", "kind": "output_after_drain",
                "native": native_output, "logical": logical_output,
            }));
        }

        set_native_lever(&mut native, trigger, true);
        backend.use_block(trigger).unwrap();
        backend.flush(&mut graph_world, false);
        for tick in 1..=36 {
            tick_pair(&mut backend, &mut graph_world, &mut native);
            let native_tick = native.piston_state().logical_tick;
            let native_memory = memory_q(&native, &memory_positions);
            let logical_memory = memory_q(&graph_world, &memory_positions);
            let native_output = output_count(&native, &output_positions);
            let logical_output = output_count(&graph_world, &output_positions);
            observations.push(serde_json::json!({
                "off_phase": off_phase, "segment": "restart", "relative_tick": tick,
                "native_tick": native_tick, "native_memory": native_memory,
                "logical_memory": logical_memory, "native_output": native_output,
                "logical_output": logical_output,
                "memory_publication_cut": tick % 6 == 0,
                "output_publication_cut": tick % 6 == 5,
            }));
            if tick % 6 == 0
                && let (Some(native_word), Some(logical_word)) = (native_memory, logical_memory)
                && native_word != logical_word
            {
                cut_mismatches.push(serde_json::json!({
                    "off_phase": off_phase, "segment": "restart", "relative_tick": tick,
                    "native_tick": native_tick, "kind": "memory_phase0",
                    "native": native_word, "logical": logical_word,
                }));
            }
            if tick % 6 == 5
                && let (Some(native_word), Some(logical_word)) = (native_output, logical_output)
                && native_word != logical_word
            {
                cut_mismatches.push(serde_json::json!({
                    "off_phase": off_phase, "segment": "restart", "relative_tick": tick,
                    "native_tick": native_tick, "kind": "output_phase5",
                    "native": native_word, "logical": logical_word,
                }));
            }
        }
    }
    let diagnostic_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/logical-inference/counter-o6-off-phase-differential.json");
    std::fs::create_dir_all(diagnostic_path.parent().unwrap()).unwrap();
    std::fs::write(
        &diagnostic_path,
        serde_json::to_vec_pretty(&serde_json::json!({ "observations": &observations, "cut_mismatches": &cut_mismatches })).unwrap(),
    )
    .unwrap();
    assert!(
        cut_mismatches.is_empty(),
        "COUNTER O6 OFF-phase differential mismatches recorded at {diagnostic_path:?}"
    );
}

#[test]
fn o6_counter_falls_back_to_graph_when_an_imported_tick_is_pending() {
    use crate::redpiler::analysis::AnalysisLimits;
    use crate::redpiler::passes;
    use crate::redpiler::program_graph::{CircuitNodeKind, ImportedTick, NodeId};
    use mchprs_world::TickPriority;

    let (world, bounds) = super::adder_tests::counter_world();
    let monitor = Arc::new(TaskMonitor::default());
    monitor.set_budget_multiplier(8);
    let ticks = world.scheduler().iter_entries().collect();
    let mut prepared = passes::rich::prepare(&world, bounds, ticks, &monitor, AnalysisLimits::for_budget(8)).unwrap();
    let options = CompilerOptions {
        optimization_level: Some(6),
        budget_multiplier: 8,
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
    let mut lowered = prepared.lower(&monitor).unwrap();
    assert!(
        lowered.program.settled_circuit().and_then(|circuit| circuit.timing.as_ref()).is_some(),
        "COUNTER must qualify for the timing-backed device path before imported work is added"
    );
    assert!(!lowered.program.logical_devices().is_empty(), "a candidate must reach runtime admission");
    let (index, repeater) = lowered
        .program
        .nodes()
        .iter()
        .enumerate()
        .find(|(_, node)| {
            matches!(
                &node.kind,
                CircuitNodeKind::Electrical(crate::redpiler::components::NodeType::Repeater { .. })
            )
        })
        .expect("COUNTER has a repeater for a valid imported scheduler entry");
    let position = repeater.position.unwrap();
    let block = repeater.block.unwrap();
    let registry_type = block.registry_id();
    lowered.imported_ticks.push(ImportedTick {
        target: Some(NodeId::new(index as u32)),
        position,
        block_type: Some(registry_type),
        delay: 1,
        priority: TickPriority::Normal,
        current_registry_type: Some(registry_type),
    });

    let mut backend = GraphBackend::from_lowered(lowered, &monitor).unwrap();
    assert!(backend.logical_devices.is_empty(), "entry work must select the ordinary Graph path");
    assert!(
        monitor
            .warnings()
            .iter()
            .any(|warning| warning.contains("could not start") && warning.contains("pending")),
        "runtime activation failure must be reported even when Graph remains available"
    );
    assert!(!backend.future.pending.is_empty());
    backend.tick().unwrap();
    assert!(backend.counts.scheduled_ticks > 0, "the imported repeater tick ran in Graph");
}
