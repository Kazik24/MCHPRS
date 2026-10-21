//! Native tick-by-tick diagnostic for the FPU computation cone.
use super::*;
use crate::plot::PlotWorld;
use crate::redpiler::analysis::AnalysisLimits;
use crate::redpiler::components::NodeType;
use crate::redpiler::program_graph::{
    CellReset, ContactId, InputPort, LayerInputSource, PistonId, PowerPort, PowerTarget, ResourceId, SettledTarget, SiteSource, StateTest,
};
use crate::redpiler::{CompilerOptions, passes};
use crate::world::World;
use mchprs_blocks::blocks::{Block, LeverFace};
use mchprs_blocks::{BlockFace, BlockPos};
use std::collections::BTreeMap;

const ORIGIN: BlockPos = BlockPos::new(40, 30, 40);
const SEED: u64 = 28_192_451_288_813;
const INPUT_TICKS: usize = 64;
const INITIAL_TICKS: usize = 1024;

pub(super) fn fixture() -> (serde_json::Value, PlotWorld, (BlockPos, BlockPos)) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/piston-research/fixtures/fpu_legal.json");
    let mut descriptor: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    descriptor["origin"] = serde_json::json!([ORIGIN.x, ORIGIN.y, ORIGIN.z]);
    let world = super::cpus::load_manifest(&descriptor).unwrap();
    let dimensions = &descriptor["dimensions"];
    let bounds = (
        ORIGIN,
        ORIGIN
            + BlockPos::new(
                dimensions[0].as_i64().unwrap() as i32 - 1,
                dimensions[1].as_i64().unwrap() as i32 - 1,
                dimensions[2].as_i64().unwrap() as i32 - 1,
            ),
    );
    (descriptor, world, bounds)
}

pub(super) fn pos(value: &serde_json::Value) -> BlockPos {
    ORIGIN
        + BlockPos::new(
            value[0].as_i64().unwrap() as i32,
            value[1].as_i64().unwrap() as i32,
            value[2].as_i64().unwrap() as i32,
        )
}

pub(super) fn set_lever(world: &mut PlotWorld, position: BlockPos, powered: bool) {
    let Block::Lever { mut lever } = world.get_block(position) else {
        panic!("FPU input is not a lever at {position:?}");
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

pub(super) fn stimuli(initial: u64, count: usize) -> Vec<u64> {
    let mask = (1u64 << 35) - 1;
    let mut state = SEED;
    let mut previous = initial & mask;
    (0..count)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let word = state & mask;
            let word = if word == previous { word ^ 1 } else { word };
            previous = word;
            word
        })
        .collect()
}

pub(super) fn source_strength(graph: &CompileGraph, world: &PlotWorld, source: LayerInputSource) -> Option<bool> {
    let LayerInputSource::SourceAbove { site, port, threshold } = source else {
        return None;
    };
    let site = &graph.sites[site.index()];
    if matches!(site.source, SiteSource::Constant(_)) {
        return Some(site.source_strength > threshold);
    }
    let SiteSource::Component(node) = site.source else {
        return None;
    };
    if port != PowerPort::Output
        || !matches!(
            graph.nodes[node.index()].kind,
            CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::PressurePlate)
        )
    {
        return None;
    }
    let position = graph.nodes[node.index()].position?;
    let block = world.get_block(position);
    let strength = crate::redstone::source_strength(block, world, position);
    Some(strength > threshold)
}

fn native_output_word(world: &PlotWorld, outputs: &[BlockPos]) -> u16 {
    outputs.iter().fold(0u16, |word, &position| {
        let powered = matches!(world.get_block(position), Block::RedstoneWire { wire } if wire.power > 0);
        (word << 1) | u16::from(powered)
    })
}

fn clock_candidate_value(world: &PlotWorld, position: BlockPos) -> serde_json::Value {
    match world.get_block(position) {
        Block::Lever { lever } => serde_json::json!({"kind":"lever","powered":lever.powered}),
        Block::RedstoneRepeater { repeater } => serde_json::json!({
            "kind":"repeater","powered":repeater.powered,"locked":repeater.locked,"delay":repeater.delay
        }),
        Block::Piston { piston } => serde_json::json!({"kind":"piston","extended":piston.extended}),
        block => serde_json::json!({"kind":"other","block_id":block.get_id(),"debug":format!("{block:?}")}),
    }
}

#[test]
#[ignore = "bounded native FPU piston-cadence diagnostic; records events without asserting publication semantics"]
fn report_fpu_native_publication_events() {
    use crate::redstone::piston::trace::{self, Stage};

    let (descriptor, initial_world, bounds) = fixture();
    let input_positions = descriptor["ports"]["light_blue_operand_inputs"]
        .as_array()
        .unwrap()
        .iter()
        .chain(descriptor["ports"]["red_operand_inputs"].as_array().unwrap())
        .chain(descriptor["ports"]["opcode_levers_by_sign_weight_4_2_1"].as_array().unwrap())
        .map(pos)
        .collect::<Vec<_>>();
    assert_eq!(input_positions.len(), 35);
    let output_positions = descriptor["observations"]["output"]
        .as_array()
        .unwrap()
        .iter()
        .map(pos)
        .collect::<Vec<_>>();
    assert_eq!(output_positions.len(), 16);
    let initial_mask = input_positions.iter().enumerate().fold(0u64, |word, (bit, &position)| {
        let Block::Lever { lever } = initial_world.get_block(position) else {
            panic!("manifest input is not a lever at {position:?}");
        };
        word | (u64::from(lever.powered) << bit)
    });
    let vectors = stimuli(initial_mask, 16);
    let selected = [(0usize, "problematic"), (1usize, "stable")];

    let mut runs = Vec::new();
    let mut stage_activity = [false; 3];
    for (vector_index, label) in selected {
        // Reloading the fixture gives each vector the same observer and piston
        // scheduler state; no reset behavior is inherited from the prior run.
        let (_, mut world, run_bounds) = fixture();
        assert_eq!(run_bounds, bounds);
        let run_trigger = pos(&descriptor["ports"]["trigger"]);
        let expected_input = vectors[vector_index];

        let Block::Lever { lever } = world.get_block(run_trigger) else {
            panic!("green control is not a lever");
        };
        assert!(lever.powered, "fixture saved trigger is expected ON before replay");
        world.disable_command_output_limits_for_replay();
        set_lever(&mut world, run_trigger, false);
        for _ in 0..INITIAL_TICKS {
            world.tick_interpreted();
        }
        let start_tick = world.piston_state().logical_tick;
        let start_phase = world.piston_state().phase;
        let mut output_rows = Vec::with_capacity(INPUT_TICKS);
        let mut observer_positions = Vec::new();
        let mut piston_positions = Vec::new();
        let mut clock_candidates = Vec::new();
        crate::world::for_each_block_optimized(&world, bounds.0, bounds.1, |position| match world.get_block(position) {
            Block::Piston { .. } => {
                piston_positions.push(position);
                clock_candidates.push(position);
            }
            Block::Lever { .. } | Block::RedstoneRepeater { .. } => clock_candidates.push(position),
            Block::Observer { .. } => observer_positions.push(position),
            _ => {}
        });
        let mut previous_candidates = clock_candidates
            .iter()
            .map(|&position| clock_candidate_value(&world, position))
            .collect::<Vec<_>>();
        let start_candidates = clock_candidates
            .iter()
            .zip(&previous_candidates)
            .map(|(&position, state)| serde_json::json!({"position":position,"state":state}))
            .collect::<Vec<_>>();
        let start_powered_observers = observer_positions
            .iter()
            .filter_map(|&position| match world.get_block(position) {
                Block::Observer { observer } if observer.powered => Some(position),
                _ => None,
            })
            .collect::<Vec<_>>();

        let events = trace::capture_detailed(&piston_positions, || {
            for (bit, &position) in input_positions.iter().enumerate() {
                set_lever(&mut world, position, expected_input & (1 << bit) != 0);
            }
            let observed_input = input_positions.iter().enumerate().fold(0u64, |word, (bit, &position)| {
                let Block::Lever { lever } = world.get_block(position) else {
                    panic!("manifest input is not a lever at {position:?}");
                };
                word | (u64::from(lever.powered) << bit)
            });
            assert_eq!(observed_input, expected_input, "selected vector inputs applied");
            for offset in 0..INPUT_TICKS {
                world.tick_interpreted();
                let current_candidates = clock_candidates
                    .iter()
                    .map(|&position| clock_candidate_value(&world, position))
                    .collect::<Vec<_>>();
                let component_changes = clock_candidates
                    .iter()
                    .zip(previous_candidates.iter().zip(&current_candidates))
                    .filter_map(|(&position, (before, after))| {
                        (before != after).then(|| serde_json::json!({"position":position,"before":before,"after":after}))
                    })
                    .collect::<Vec<_>>();
                previous_candidates = current_candidates;
                let observer_powered = observer_positions
                    .iter()
                    .filter_map(|&position| match world.get_block(position) {
                        Block::Observer { observer } if observer.powered => Some(position),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                output_rows.push(serde_json::json!({
                    "tick":world.piston_state().logical_tick,
                    "offset":offset,
                    "phase":world.piston_state().phase,
                    "output_word":native_output_word(&world, &output_positions),
                    "powered_reset_observers":observer_powered,
                    "clock_candidate_changes":component_changes,
                }));
            }
        });
        stage_activity[0] |= events.iter().any(|entry| matches!(entry.stage, Stage::Request));
        stage_activity[1] |= events.iter().any(|entry| matches!(entry.stage, Stage::Accept));
        stage_activity[2] |= events.iter().any(|entry| matches!(entry.stage, Stage::Applied));
        let tail_words = output_rows
            .iter()
            .rev()
            .take(16)
            .map(|row| row["output_word"].as_u64().unwrap())
            .collect::<Vec<_>>();
        let distinct_tail_words = tail_words.iter().copied().collect::<std::collections::BTreeSet<_>>();
        runs.push(serde_json::json!({
            "label":label,
            "vector_index":vector_index,
            "input_word":expected_input,
            "start_tick":start_tick,
            "start_phase":start_phase,
            "observer_count":observer_positions.len(),
            "start_powered_reset_observers":start_powered_observers,
            "start_clock_candidates":start_candidates,
            "last_16_output_words":tail_words,
            "tail_classification":if distinct_tail_words.len() == 1 { "steady" } else { "varying" },
            "tick_outputs":output_rows,
            "piston_events":events,
        }));
    }
    let report = serde_json::json!({
        "contract":"Native diagnostic only. Tick-aligned output words and piston request/accept/applied events; no settled-frame parity or publication-cadence claim.",
        "warmup_ticks":INITIAL_TICKS,
        "ticks_per_vector":INPUT_TICKS,
        "vectors":runs,
    });
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/logical-inference/fpu-native-publication-events.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!("FPU native publication-event trace: {}", path.display());
    assert!(stage_activity[0], "native trace records request activity");
    assert!(stage_activity[1], "native trace records accept activity");
    assert!(stage_activity[2], "native trace records applied activity");
}

fn source_class(graph: &CompileGraph, node: NodeId) -> &'static str {
    match &graph.nodes[node.index()].kind {
        CircuitNodeKind::Wire(_) => "wire",
        CircuitNodeKind::BuiltinReset { .. } | CircuitNodeKind::Observer { .. } => "reset",
        CircuitNodeKind::Electrical(NodeType::Observer { .. }) => "reset",
        CircuitNodeKind::Electrical(NodeType::Torch) => "torch",
        CircuitNodeKind::Electrical(NodeType::Repeater { .. }) => "repeater",
        CircuitNodeKind::Electrical(NodeType::Comparator { .. }) => "comparator",
        CircuitNodeKind::Electrical(NodeType::Lever) => "lever",
        CircuitNodeKind::Electrical(NodeType::Button) => "button",
        CircuitNodeKind::Electrical(NodeType::PressurePlate) => "pressure_plate",
        _ => "other",
    }
}

fn feedback_geometry(graph: &CompileGraph, feedback: &[(ResourceId, ContactId, usize)]) -> Vec<String> {
    let Some(catalog) = graph.plans.instant.boundaries.as_ref() else {
        return vec!["missing cell catalog".into()];
    };
    feedback
        .iter()
        .map(|&(resource, source, _)| {
            let owners_and_resets = catalog
                .cells
                .iter()
                .find_map(|cell| {
                    let token = cell.resources.iter().find(|token| token.resource == resource)?;
                    let owners = token
                        .owners
                        .iter()
                        .map(|owner| {
                            let position = graph.pistons[owner.index()].position;
                            let reset = cell.uses.iter().find(|use_| use_.actor == *owner).map_or_else(
                                || "missing-use".to_owned(),
                                |use_| match &use_.reset {
                                    CellReset::Observer { node, .. } => {
                                        format!("observer:{:?}@{:?}", graph.nodes[node.index()].kind, graph.nodes[node.index()].position)
                                    }
                                    CellReset::Payload { resource, .. } => format!("payload:{resource:?}"),
                                    CellReset::InputRestoration { resource, contact, .. } => {
                                        format!("input-restoration:{resource:?}@{contact:?}")
                                    }
                                },
                            );
                            format!("{owner:?}@{position:?}/{reset}")
                        })
                        .collect::<Vec<_>>();
                    Some(owners.join(","))
                })
                .unwrap_or_else(|| "missing-contract".into());
            format!(
                "resource={resource:?} source={source:?}@{:?} owners=[{owners_and_resets}]",
                graph.contacts[source.index()].position
            )
        })
        .collect()
}

#[test]
#[ignore = "native FPU computation/state diagnostic; reports every-tick divergences without an arithmetic oracle"]
fn report_fpu_computation_native_tick_trace() {
    let (descriptor, mut world, bounds) = fixture();
    let input_positions = descriptor["ports"]["light_blue_operand_inputs"]
        .as_array()
        .unwrap()
        .iter()
        .chain(descriptor["ports"]["red_operand_inputs"].as_array().unwrap())
        .chain(descriptor["ports"]["opcode_levers_by_sign_weight_4_2_1"].as_array().unwrap())
        .map(pos)
        .collect::<Vec<_>>();
    assert_eq!(input_positions.len(), 35);
    let trigger = pos(&descriptor["ports"]["trigger"]);
    let output_positions = descriptor["observations"]["output"]
        .as_array()
        .unwrap()
        .iter()
        .map(pos)
        .collect::<Vec<_>>();
    assert_eq!(output_positions.len(), 16);

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
    let targets = output_positions
        .iter()
        .map(|&position| {
            let (index, node) = graph
                .nodes
                .iter()
                .enumerate()
                .find(|(_, node)| node.position == Some(position))
                .unwrap_or_else(|| panic!("manifest FPU dust output has no node at {position:?}"));
            assert!(matches!(node.kind, CircuitNodeKind::Wire(_)));
            PowerTarget::Input {
                node: NodeId::new(index as u32),
                port: InputPort::Main,
            }
        })
        .collect::<Vec<_>>();
    let circuit = match compile_computation(&graph, &targets, &monitor) {
        Ok(circuit) => circuit,
        Err(error) => {
            println!("FPU computation extraction declined before producing a closed circuit: {error}");
            panic!("FPU computation extraction is not closed: {error}");
        }
    };
    assert!(
        circuit.declines.is_empty(),
        "whole FPU query declined ({} entries, first 12: {:#?})",
        circuit.declines.len(),
        circuit.declines.iter().take(12).collect::<Vec<_>>()
    );
    assert_eq!(
        circuit
            .outputs
            .iter()
            .filter(|output| matches!(output.target, SettledTarget::Power(_)))
            .count(),
        16
    );

    let feedback_outputs = circuit
        .outputs
        .iter()
        .enumerate()
        .filter_map(|(index, output)| match output.target {
            SettledTarget::ResourceState { resource, source } => Some((resource, source, index)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let feedback_by_resource: rustc_hash::FxHashMap<_, _> = feedback_outputs
        .iter()
        .map(|&(resource, source, index)| (resource, (source, index)))
        .collect();
    for row in feedback_geometry(&graph, &feedback_outputs).iter().take(32) {
        println!("FPU feedback geometry: {row}");
    }
    let mut class_counts = BTreeMap::<&'static str, (usize, rustc_hash::FxHashSet<NodeId>, BTreeMap<String, usize>)>::new();
    let mut unclassified_count = 0usize;
    let mut unclassified_samples = Vec::new();
    let mut samples_by_class = BTreeMap::<&'static str, usize>::new();
    for input in circuit.program.inputs.iter() {
        match input.source {
            LayerInputSource::SourceAbove { site, port, threshold } => match graph.sites[site.index()].source {
                SiteSource::Component(node) => {
                    let class = source_class(&graph, node);
                    let (count, nodes, predicates) = class_counts.entry(class).or_default();
                    *count += 1;
                    nodes.insert(node);
                    *predicates.entry(format!("{port:?}/>{threshold}")).or_default() += 1;

                    let declared = matches!(class, "lever" | "button" | "pressure_plate") && port == PowerPort::Output && threshold == 0;
                    if !declared {
                        unclassified_count += 1;
                        let sampled = samples_by_class.entry(class).or_default();
                        if *sampled < 4 {
                            let component = &graph.nodes[node.index()];
                            unclassified_samples.push(format!(
                                "site={site:?} port={port:?} threshold={threshold} node={node:?} kind={:?} position={:?}",
                                component.kind, component.position
                            ));
                            *sampled += 1;
                        }
                    }
                }
                SiteSource::Constant(_) => {}
                source => {
                    unclassified_count += 1;
                    let sampled = samples_by_class.entry("source").or_default();
                    if *sampled < 4 {
                        unclassified_samples.push(format!("site={site:?} port={port:?} threshold={threshold} source={source:?}"));
                        *sampled += 1;
                    }
                }
            },
            LayerInputSource::State(StateTest::ResourceAt { resource, contact })
                if feedback_by_resource
                    .get(&resource)
                    .is_some_and(|(source, _)| *source == contact || graph.resources[resource.index()].contacts.contains(&contact)) => {}
            source => {
                unclassified_count += 1;
                let sampled = samples_by_class.entry("state").or_default();
                if *sampled < 4 {
                    unclassified_samples.push(format!("unsupported/unbound input {source:?}"));
                    *sampled += 1;
                }
            }
        }
    }
    let classes = class_counts
        .iter()
        .map(|(class, (predicates, nodes, kinds))| {
            format!("{class}: predicates={predicates} unique_nodes={} input_predicates={kinds:?}", nodes.len())
        })
        .collect::<Vec<_>>();
    let class_summary = classes.iter().take(12).cloned().collect::<Vec<_>>();
    println!(
        "FPU computation circuit: graph_nodes={} recognized_cells={} BUD_registers={} logical_inputs={} expressions={} power_outputs={} feedback_resources={} source_classes={class_summary:?} undeclared_input_count={unclassified_count} first_undeclared_inputs={unclassified_samples:#?}",
        graph.nodes.len(),
        graph.plans.instant.boundaries.as_ref().map_or(0, |catalog| catalog.cells.len()),
        graph.plans.bud_memory_reads.len(),
        circuit.program.inputs.len(),
        circuit.program.expressions.len(),
        targets.len(),
        feedback_outputs.len(),
    );
    assert!(
        unclassified_count == 0,
        "FPU computation has {unclassified_count} undeclared external/state inputs; first samples: {unclassified_samples:#?}"
    );

    let mut external_sources = rustc_hash::FxHashSet::default();
    let mut initial_inputs = Vec::with_capacity(circuit.program.inputs.len());
    let mut feedback_slots = Vec::<(usize, ResourceId, ContactId)>::new();
    for (slot, input) in circuit.program.inputs.iter().enumerate() {
        match input.source {
            LayerInputSource::SourceAbove { .. } => {
                let sampled = source_strength(&graph, &world, input.source).expect("declared FPU boundary can be sampled");
                external_sources.insert(input.source);
                initial_inputs.push(sampled);
            }
            LayerInputSource::State(StateTest::ResourceAt { resource, contact }) => {
                let (source, _) = feedback_by_resource[&resource];
                let at = graph.initial_resources[resource.index()].at;
                let present = at == source;
                initial_inputs.push(if contact == source { present } else { !present });
                feedback_slots.push((slot, resource, contact));
            }
            other => panic!("closure assertion should have rejected {other:?}"),
        }
    }
    let mut machine = crate::redpiler::program_graph::SettledMachine::new(&circuit, &initial_inputs).unwrap();
    let output_indices = targets
        .iter()
        .map(|&target| {
            circuit
                .outputs
                .iter()
                .position(|output| output.target == SettledTarget::Power(target))
                .unwrap()
        })
        .collect::<Vec<_>>();

    let Block::Lever { lever } = world.get_block(trigger) else {
        panic!("green control is not a lever");
    };
    assert!(lever.powered, "fixture saved trigger is expected ON before the replay protocol");
    world.disable_command_output_limits_for_replay();
    set_lever(&mut world, trigger, false);
    let mut current_mask = input_positions.iter().enumerate().fold(0u64, |word, (bit, &position)| {
        let Block::Lever { lever } = world.get_block(position) else {
            panic!("manifest input is not a lever at {position:?}");
        };
        word | (u64::from(lever.powered) << bit)
    });
    let vectors = stimuli(current_mask, 16);
    let mut mismatches = Vec::new();
    let mut mismatches_by_phase = BTreeMap::<(usize, usize), usize>::new();
    let mut vector_boundaries = Vec::new();
    let mut tick_number = 0usize;
    let mut tick_rows = Vec::new();
    let mut fresh_frames = Vec::new();

    for _ in 0..INITIAL_TICKS {
        tick_number += 1;
        world.tick_interpreted();
        let external = machine
            .external_inputs()
            .iter()
            .map(|input| source_strength(&graph, &world, input.source).expect("classified source remains sampleable"))
            .collect::<Vec<_>>();
        machine.step(&external).unwrap();
        compare_tick(
            &mut tick_rows,
            &mut mismatches,
            &mut mismatches_by_phase,
            tick_number,
            usize::MAX,
            tick_number,
            &machine,
            &output_indices,
            &world,
            &output_positions,
        );
    }

    for (vector_index, &vector) in vectors.iter().enumerate() {
        for (bit, &position) in input_positions.iter().enumerate() {
            let powered = vector & (1 << bit) != 0;
            set_lever(&mut world, position, powered);
        }
        current_mask = vector;
        for offset in 0..INPUT_TICKS {
            tick_number += 1;
            world.tick_interpreted();
            let external = machine
                .external_inputs()
                .iter()
                .map(|input| source_strength(&graph, &world, input.source).expect("classified source remains sampleable"))
                .collect::<Vec<_>>();
            machine.step(&external).unwrap();
            compare_tick(
                &mut tick_rows,
                &mut mismatches,
                &mut mismatches_by_phase,
                tick_number,
                vector_index,
                offset,
                &machine,
                &output_indices,
                &world,
                &output_positions,
            );
            if offset + 1 == INPUT_TICKS {
                let native = native_output_word(&world, &output_positions);
                let logical = output_indices
                    .iter()
                    .fold(0u16, |word, &index| (word << 1) | u16::from(machine.outputs()[index] != 0));
                vector_boundaries.push((vector_index, native, logical));
            }
        }
        // Diagnostic: an instant computation starts with reset Far sources,
        // rather than inheriting the preceding computation's feedback values.
        let frame_inputs = circuit
            .program
            .inputs
            .iter()
            .map(|input| match input.source {
                LayerInputSource::SourceAbove { .. } => source_strength(&graph, &world, input.source).unwrap(),
                LayerInputSource::State(StateTest::ResourceAt { resource, contact }) => contact == feedback_by_resource[&resource].0,
                other => panic!("unclassified frame input {other:?}"),
            })
            .collect::<Vec<_>>();
        let mut frame = crate::redpiler::program_graph::SettledMachine::new(&circuit, &frame_inputs).unwrap();
        let frame_external = frame
            .external_inputs()
            .iter()
            .map(|input| source_strength(&graph, &world, input.source).unwrap())
            .collect::<Vec<_>>();
        let mut converged = false;
        let mut steps = 0;
        for _ in 0..128 {
            let before = frame.input_values().to_vec();
            frame.step(&frame_external).unwrap();
            steps += 1;
            if frame.input_values() == before {
                converged = true;
                break;
            }
        }
        let word = output_indices
            .iter()
            .fold(0u16, |word, &index| (word << 1) | u16::from(frame.outputs()[index] != 0));
        fresh_frames.push(serde_json::json!({"vector":vector_index,"word":word,"steps":steps,"converged":converged}));
    }

    let mut bad_state_feedback = Vec::new();
    for &(slot, resource, contact) in &feedback_slots {
        let (source, output) = feedback_by_resource[&resource];
        let current = machine.input_values()[slot];
        let expected_next = machine.outputs()[output] != 0;
        if contact == source && current != expected_next {
            bad_state_feedback.push((resource, slot, current, expected_next));
        }
    }
    println!(
        "FPU native-vs-logical trace: ticks={} vectors={} ticks_per_vector={} feedback_resources={} feedback_input_slots={} external_sources={} mismatch_count={} first_mismatches={:?} first_vector_boundaries={:?} first_phase_counts={:?} first_feedback_snapshot_differences={:?}",
        tick_number,
        vectors.len(),
        INPUT_TICKS,
        feedback_outputs.len(),
        feedback_slots.len(),
        external_sources.len(),
        mismatches_by_phase.values().sum::<usize>(),
        mismatches.iter().take(12).collect::<Vec<_>>(),
        vector_boundaries.iter().take(12).collect::<Vec<_>>(),
        mismatches_by_phase.iter().take(12).collect::<Vec<_>>(),
        bad_state_feedback.iter().take(12).collect::<Vec<_>>(),
    );
    let report = serde_json::json!({
        "contract":"Candidate settled-frame values; raw tick differences include removable reset glitches. This trace alone does not certify publication timing.",
        "expressions":circuit.program.expressions.len(),
        "feedback_resources":feedback_outputs.len(),
        "external_sources":external_sources.len(),
        "input_vectors":vectors,
        "fresh_frames":fresh_frames,
        "ticks":tick_rows,
    });
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/logical-inference/fpu-computation-frame-trace.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!("FPU computation frame trace: {}", path.display());
    assert_eq!(current_mask, *vectors.last().unwrap());
}

#[test]
#[ignore = "focused O6 FPU production-admission diagnostic; reports graph-derived targets and compiler declines"]
fn report_fpu_production_admission() {
    let (_descriptor, _world, bounds) = fixture();
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let mut prepared = passes::rich::prepare(&_world, bounds, vec![], &monitor, AnalysisLimits::for_budget(8)).unwrap();
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
    let graph = &prepared.graph;
    let command_blocks = graph
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            matches!(node.kind, CircuitNodeKind::Electrical(NodeType::CommandBlock { .. })).then_some(NodeId::new(index as u32))
        })
        .collect::<rustc_hash::FxHashSet<_>>();
    let command_block_support = command_blocks
        .iter()
        .map(|&command_block| {
            let node = &graph.nodes[command_block.index()];
            let incoming = graph
                .power_links
                .iter()
                .filter(|link| matches!(link.to, PowerTarget::Input { node, .. } if node == command_block))
                .map(|link| (graph.sites[link.from.index()].source, link.from_port, link.to))
                .collect::<Vec<_>>();
            let outgoing = graph
                .power_links
                .iter()
                .filter(|link| graph.sites[link.from.index()].source == SiteSource::Component(command_block))
                .map(|link| (link.to, link.from_port))
                .collect::<Vec<_>>();
            (command_block, node.position, &node.kind, &node.state, incoming, outgoing)
        })
        .collect::<Vec<_>>();
    println!(
        "FPU command-block census: blocks={} input_links={} output_links={} detail={command_block_support:#?}",
        command_blocks.len(),
        command_block_support
            .iter()
            .map(|(_, _, _, _, incoming, _)| incoming.len())
            .sum::<usize>(),
        command_block_support
            .iter()
            .map(|(_, _, _, _, _, outgoing)| outgoing.len())
            .sum::<usize>(),
    );
    let known_clock = graph
        .pistons
        .iter()
        .enumerate()
        .find_map(|(index, piston)| (piston.position == BlockPos::new(46, 66, 112)).then_some(PistonId::new(index as u32)));
    if let Some(clock) = known_clock {
        println!(
            "FPU known direct-clock support: {:#?}",
            super::timing::diagnostic_candidate_support(graph, clock, &monitor)
        );
    } else {
        println!("FPU known direct-clock support: no piston at diagnostic position");
    }
    let targets = super::settled::terminal_wire_targets(graph);
    let timing = super::timing::infer(graph, &monitor);
    let computation = compile_computation(graph, &targets, &monitor);
    println!(
        "FPU production admission: graph_nodes={} pistons={} resources={} cell_contracts={} BUD_reads={} selected_terminal_wires={} selected_clock={:?}",
        graph.nodes.len(),
        graph.pistons.len(),
        graph.resources.len(),
        graph.plans.instant.boundaries.as_ref().map_or(0, |catalog| catalog.cells.len()),
        graph.plans.bud_memory_reads.len(),
        targets.len(),
        timing
            .as_ref()
            .map(|island| (island.clock, island.actors.len(), island.nodes.len(), island.trigger)),
    );
    println!("FPU production timing inference: {timing:#?}");
    match computation {
        Ok(circuit) => {
            println!(
                "FPU production unsupported frame inputs: {:#?}",
                super::settled::unsupported_computation_frame_inputs(graph, &circuit)
            );
            let mut sources = BTreeMap::<String, usize>::new();
            for input in &circuit.program.inputs {
                let kind = match input.source {
                    LayerInputSource::SourceAbove { site, .. } => match graph.sites[site.index()].source {
                        SiteSource::Component(node) => source_class(graph, node).to_owned(),
                        other => format!("{other:?}"),
                    },
                    LayerInputSource::State(StateTest::ResourceAt { .. }) => "feedback".to_owned(),
                    other => format!("{other:?}"),
                };
                *sources.entry(kind).or_default() += 1;
            }
            println!("FPU production input classes: {sources:?}");
            println!(
                "FPU production computation: inputs={} expressions={} outputs={} power_outputs={} resource_states={} declines={:#?}",
                circuit.program.inputs.len(),
                circuit.program.expressions.len(),
                circuit.outputs.len(),
                circuit
                    .outputs
                    .iter()
                    .filter(|output| matches!(output.target, SettledTarget::Power(_)))
                    .count(),
                circuit
                    .outputs
                    .iter()
                    .filter(|output| matches!(output.target, SettledTarget::ResourceState { .. }))
                    .count(),
                circuit.declines,
            );
        }
        Err(error) => println!("FPU production computation declined: {error}"),
    }
    match super::settled::compile_device(graph, &monitor) {
        Ok(circuit) => println!(
            "FPU selected device artifact: inputs={} expressions={} outputs={} timing={} declines={:#?}",
            circuit.program.inputs.len(),
            circuit.program.expressions.len(),
            circuit.outputs.len(),
            circuit.timing.is_some(),
            circuit.declines,
        ),
        Err(error) => println!("FPU selected device compiler error: {error}"),
    }
}

#[test]
#[ignore = "full FPU production adapter versus the accepted benchmark frame, including same-tick outputs"]
fn fpu_production_device_matches_benchmark_frames_at_clock_edges() {
    use crate::redpiler::backend::graph::GraphBackend;
    use crate::redpiler::program_graph::SettledEvaluation;
    use std::sync::Arc;

    let (descriptor, mut world, bounds) = fixture();
    let controls = descriptor["ports"]["light_blue_operand_inputs"]
        .as_array()
        .unwrap()
        .iter()
        .chain(descriptor["ports"]["red_operand_inputs"].as_array().unwrap())
        .chain(descriptor["ports"]["opcode_levers_by_sign_weight_4_2_1"].as_array().unwrap())
        .map(pos)
        .collect::<Vec<_>>();
    let outputs = descriptor["observations"]["output"]
        .as_array()
        .unwrap()
        .iter()
        .map(pos)
        .collect::<Vec<_>>();
    let trigger = pos(&descriptor["ports"]["trigger"]);
    let mut current = controls.iter().enumerate().fold(0u64, |word, (bit, &position)| {
        let Block::Lever { lever } = world.get_block(position) else {
            panic!("input is not a lever")
        };
        word | (u64::from(lever.powered) << bit)
    });
    let words = stimuli(current, 16);
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let options = CompilerOptions {
        optimization_level: Some(6),
        budget_multiplier: 8,
        ..Default::default()
    };
    let mut prepared = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::for_budget(8)).unwrap();
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::default_graph_pipeline(&options),
        &monitor,
    )
    .unwrap();
    let graph = &prepared.graph;
    let targets = outputs
        .iter()
        .map(|&position| {
            let index = graph.nodes.iter().position(|node| node.position == Some(position)).unwrap();
            PowerTarget::Input {
                node: NodeId::new(index as u32),
                port: InputPort::Main,
            }
        })
        .collect::<Vec<_>>();
    // This is the original benchmark's selected-output extraction, independent of automatic port selection.
    let reference = compile_computation(graph, &targets, &monitor).unwrap();
    let slots = targets
        .iter()
        .map(|target| {
            reference
                .outputs
                .iter()
                .position(|output| output.target == SettledTarget::Power(*target))
                .unwrap()
        })
        .collect::<Vec<_>>();
    let state_outputs = reference
        .outputs
        .iter()
        .enumerate()
        .filter_map(|(index, output)| match output.target {
            SettledTarget::ResourceState { resource, source } => Some((resource, (source, index))),
            _ => None,
        })
        .collect::<rustc_hash::FxHashMap<_, _>>();
    let feedback = reference
        .program
        .inputs
        .iter()
        .enumerate()
        .filter_map(|(slot, input)| {
            let LayerInputSource::State(StateTest::ResourceAt { resource, contact }) = input.source else {
                return None;
            };
            let &(source, output) = state_outputs.get(&resource).unwrap();
            Some((slot, output, contact == source))
        })
        .collect::<Vec<_>>();
    let mut evaluation = SettledEvaluation::new(&reference).unwrap();
    let mut reference_inputs = vec![false; reference.program.inputs.len()];
    let mut expected = Vec::new();
    let mut expected_strengths = Vec::new();
    for &word in &words {
        let frame = reference
            .program
            .inputs
            .iter()
            .map(|input| match input.source {
                LayerInputSource::SourceAbove { site, .. } => {
                    if let SiteSource::Component(node) = graph.sites[site.index()].source {
                        let position = graph.nodes[node.index()].position;
                        if position == Some(trigger) {
                            return false;
                        }
                        if let Some(bit) = controls.iter().position(|&p| position == Some(p)) {
                            return word & (1 << bit) != 0;
                        }
                    }
                    source_strength(graph, &world, input.source).expect("benchmark external seed")
                }
                LayerInputSource::State(StateTest::ResourceAt { .. }) => false,
                other => panic!("unsupported benchmark seed {other:?}"),
            })
            .collect::<Vec<_>>();
        expected.push(super::fpu_kernel_bench::run_frame(&mut evaluation, &mut reference_inputs, &frame, &feedback, &slots).0);
        expected_strengths.push(slots.iter().map(|&slot| evaluation.outputs()[slot]).collect::<Vec<_>>());
    }
    let monitor = Arc::new(TaskMonitor::default());
    let mut backend = GraphBackend::compile_with_options(&world, bounds, vec![], &options, monitor).unwrap();
    for (index, (&word, &expected)) in words.iter().zip(&expected).enumerate() {
        for (bit, &position) in controls.iter().enumerate() {
            if (word ^ current) & (1 << bit) != 0 {
                backend.use_block(position).unwrap();
            }
        }
        current = word;
        if index == 0 {
            backend.use_block(trigger).unwrap();
        }
        for phase in 0..6 {
            backend.tick().unwrap();
            backend.flush(&mut world, false);
            assert_eq!(
                native_output_word(&world, &outputs),
                expected,
                "production frame {index}, game-tick offset {phase}: output must be immediate and held through reset"
            );
            for (&position, &strength) in outputs.iter().zip(&expected_strengths[index]) {
                let Block::RedstoneWire { wire } = world.get_block(position) else {
                    panic!("FPU output is not a wire at {position:?}");
                };
                assert_eq!(wire.power, strength, "frame {index}, offset {phase}, output {position:?}");
            }
        }
        let work = backend.execution_counts();
        assert_eq!(work.callbacks, 0, "physical fallback must not satisfy the test");
        assert_eq!(work.piston_actions, 0);
        assert_eq!(work.logical_kernel_evaluations, index as u64 + 1);
    }
    assert!(expected.windows(2).any(|words| words[0] != words[1]), "changing output activity");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/logical-inference/fpu-production-frame-equivalence.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&serde_json::json!({"input_words":words,"benchmark_output_words":expected,"game_ticks_per_frame":6})).unwrap(),
    )
    .unwrap();
}

#[allow(clippy::too_many_arguments)]
fn compare_tick(
    rows: &mut Vec<serde_json::Value>,
    mismatches: &mut Vec<(usize, usize, usize, u16, u16)>,
    by_phase: &mut BTreeMap<(usize, usize), usize>,
    tick: usize,
    vector: usize,
    offset: usize,
    machine: &crate::redpiler::program_graph::SettledMachine,
    output_indices: &[usize],
    world: &PlotWorld,
    outputs: &[BlockPos],
) {
    let native = native_output_word(world, outputs);
    let logical = output_indices
        .iter()
        .fold(0u16, |word, &index| (word << 1) | u16::from(machine.outputs()[index] != 0));
    rows.push(serde_json::json!({
        "tick":tick,"vector":(vector != usize::MAX).then_some(vector),
        "offset":offset,"native":native,"logical":logical,
    }));
    if native != logical {
        *by_phase.entry((vector, offset)).or_default() += 1;
        if mismatches.len() < 12 {
            mismatches.push((tick, vector, offset, native, logical));
        }
    }
}
