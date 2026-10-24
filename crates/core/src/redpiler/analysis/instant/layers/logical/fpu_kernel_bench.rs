//! Release-only throughput probe for the compiled FPU settled computation.
use super::*;
use crate::redpiler::analysis::AnalysisLimits;
use crate::redpiler::program_graph::{LayerInputSource, SettledEvaluation, SettledMachine, SettledTarget, StateTest};
use crate::redpiler::{CompilerOptions, passes};
use crate::world::World;
use mchprs_blocks::BlockPos;
use mchprs_blocks::blocks::Block;
use rustc_hash::FxHashMap;
use std::hint::black_box;
use std::time::Instant;

const SELECTOR_PATTERNS: usize = 8;
const SAMPLE_COUNT: usize = 3;
const OPERANDS_PER_PATTERN: usize = 4_096;
const MAX_FIXED_POINT_PASSES: usize = 128;

pub(super) fn run_frame(
    evaluation: &mut SettledEvaluation,
    inputs: &mut [bool],
    external_frame: &[bool],
    state_slots: &[(usize, usize, bool)],
    output_slots: &[usize],
) -> (u16, usize, usize) {
    inputs.copy_from_slice(external_frame);
    for &(slot, _, polarity) in state_slots {
        inputs[slot] = polarity;
    }
    let mut operations = 0;
    for pass in 1..=MAX_FIXED_POINT_PASSES {
        operations += evaluation.evaluate(black_box(&*inputs)).unwrap();
        let mut changed = false;
        for &(slot, output, polarity) in state_slots {
            let next = (evaluation.outputs()[output] != 0) == polarity;
            changed |= inputs[slot] != next;
            inputs[slot] = next;
        }
        if !changed {
            let output = output_slots
                .iter()
                .fold(0u16, |word, &index| (word << 1) | u16::from(evaluation.outputs()[index] != 0));
            return (output, pass, operations);
        }
    }
    panic!("FPU feedback did not converge within {MAX_FIXED_POINT_PASSES} passes");
}

#[test]
#[ignore = "release-only FPU logical-kernel throughput measurement"]
fn benchmark_fpu_logical_kernel() {
    assert!(!cfg!(debug_assertions), "run this throughput probe in release mode");

    let fixture_started = Instant::now();
    let (descriptor, mut world, bounds) = super::fpu_computation_tests::fixture();
    let fixture_load_ms = fixture_started.elapsed().as_secs_f64() * 1_000.0;
    let input_positions = descriptor["ports"]["light_blue_operand_inputs"]
        .as_array()
        .unwrap()
        .iter()
        .chain(descriptor["ports"]["red_operand_inputs"].as_array().unwrap())
        .chain(descriptor["ports"]["opcode_levers_by_sign_weight_4_2_1"].as_array().unwrap())
        .map(super::fpu_computation_tests::pos)
        .collect::<Vec<_>>();
    assert_eq!(input_positions.len(), 35);
    let input_bits: FxHashMap<BlockPos, usize> = input_positions.iter().copied().enumerate().map(|(bit, pos)| (pos, bit)).collect();
    let trigger = super::fpu_computation_tests::pos(&descriptor["ports"]["trigger"]);

    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let prepare_started = Instant::now();
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
    let prepare_ms = prepare_started.elapsed().as_secs_f64() * 1_000.0;
    let graph = prepared.graph;
    let output_positions = descriptor["observations"]["output"]
        .as_array()
        .unwrap()
        .iter()
        .map(super::fpu_computation_tests::pos)
        .collect::<Vec<_>>();
    let targets = output_positions
        .iter()
        .map(|&position| {
            let (index, node) = graph.nodes.iter().enumerate().find(|(_, node)| node.position == Some(position)).unwrap();
            assert!(matches!(node.kind, CircuitNodeKind::Wire(_)));
            PowerTarget::Input {
                node: NodeId::new(index as u32),
                port: InputPort::Main,
            }
        })
        .collect::<Vec<_>>();
    let compile_started = Instant::now();
    let circuit = compile_computation(&graph, &targets, &monitor).unwrap();
    let compile_ms = compile_started.elapsed().as_secs_f64() * 1_000.0;
    super::fpu_computation_tests::set_lever(&mut world, trigger, false);
    assert!(circuit.declines.is_empty(), "incomplete FPU kernel: {:?}", circuit.declines);
    assert_eq!(circuit.program.expressions.len(), 22_193, "unexpected FPU kernel expression count");
    assert_eq!(targets.len(), 16);

    let output_slots = targets
        .iter()
        .map(|target| {
            circuit
                .outputs
                .iter()
                .position(|output| output.target == SettledTarget::Power(*target))
                .unwrap()
        })
        .collect::<Vec<_>>();
    let state_outputs = circuit
        .outputs
        .iter()
        .enumerate()
        .filter_map(|(index, output)| match output.target {
            SettledTarget::ResourceState { resource, source } => Some((resource, source, index)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(state_outputs.len(), 15, "expected fifteen feedback state outputs");
    let state_sources: FxHashMap<_, _> = state_outputs
        .iter()
        .map(|&(resource, source, output)| (resource, (source, output)))
        .collect();
    let mut state_slots = Vec::new();
    for (slot, input) in circuit.program.inputs.iter().enumerate() {
        let LayerInputSource::State(StateTest::ResourceAt { resource, contact }) = input.source else {
            continue;
        };
        let (source, output) = state_sources[&resource];
        let polarity = contact == source;
        assert!(polarity || graph.resources[resource.index()].contacts.contains(&contact));
        state_slots.push((slot, output, polarity));
    }
    assert_eq!(state_slots.len(), 15, "each feedback output should bind one input slot");

    let mut initial_word = 0u64;
    for (bit, &position) in input_positions.iter().enumerate() {
        let Block::Lever { lever } = world.get_block(position) else {
            panic!("FPU input is not a lever at {position:?}");
        };
        initial_word |= u64::from(lever.powered) << bit;
    }
    let mut sample_inputs = Vec::<Vec<Box<[bool]>>>::with_capacity(SELECTOR_PATTERNS);
    let operand_mask = (1u64 << 32) - 1;
    for selector in 0..SELECTOR_PATTERNS {
        let operands = super::fpu_computation_tests::stimuli(initial_word & operand_mask, OPERANDS_PER_PATTERN);
        let words = operands
            .into_iter()
            .map(|word| (word & operand_mask) | ((selector as u64) << 32))
            .collect::<Vec<_>>();
        let mut frames = Vec::with_capacity(words.len());
        for &word in &words {
            let mut frame = vec![false; circuit.program.inputs.len()];
            for (slot, input) in circuit.program.inputs.iter().enumerate() {
                match input.source {
                    LayerInputSource::SourceAbove { site, .. } => {
                        frame[slot] = match graph.sites[site.index()].source {
                            SiteSource::Component(node) => {
                                if let Some(position) = graph.nodes[node.index()].position
                                    && let Some(&bit) = input_bits.get(&position)
                                {
                                    word & (1 << bit) != 0
                                } else {
                                    super::fpu_computation_tests::source_strength(&graph, &world, input.source)
                                        .expect("stable external source should be sampleable")
                                }
                            }
                            SiteSource::Constant(_) => super::fpu_computation_tests::source_strength(&graph, &world, input.source).unwrap(),
                            SiteSource::Contact(_) => panic!("contact source unexpectedly escaped feedback binding"),
                        };
                    }
                    LayerInputSource::State(StateTest::ResourceAt { .. }) => {}
                    other => panic!("unsupported FPU kernel input: {other:?}"),
                }
            }
            frames.push(frame.into_boxed_slice());
        }
        sample_inputs.push(frames);
    }

    // Untimed differential smoke: this reusable evaluation path must match the
    // existing machine's simultaneous feedback commit for 16 changing words.
    let differential_words = super::fpu_computation_tests::stimuli(initial_word, 16);
    let mut evaluation = SettledEvaluation::new(&circuit).unwrap();
    let mut candidate_inputs = vec![false; circuit.program.inputs.len()];
    for &word in &differential_words {
        let mut frame = vec![false; circuit.program.inputs.len()];
        for (slot, input) in circuit.program.inputs.iter().enumerate() {
            match input.source {
                LayerInputSource::SourceAbove { site, .. } => {
                    frame[slot] = match graph.sites[site.index()].source {
                        SiteSource::Component(node) => graph.nodes[node.index()]
                            .position
                            .and_then(|position| input_bits.get(&position).copied())
                            .map_or_else(
                                || super::fpu_computation_tests::source_strength(&graph, &world, input.source).unwrap(),
                                |bit| word & (1 << bit) != 0,
                            ),
                        SiteSource::Constant(_) => super::fpu_computation_tests::source_strength(&graph, &world, input.source).unwrap(),
                        SiteSource::Contact(_) => panic!("unexpected contact input"),
                    };
                }
                LayerInputSource::State(StateTest::ResourceAt { .. }) => {}
                other => panic!("unsupported FPU differential input: {other:?}"),
            }
        }
        let (candidate_word, _, _) = run_frame(&mut evaluation, &mut candidate_inputs, &frame, &state_slots, &output_slots);
        let mut machine_inputs = frame.to_vec();
        for &(slot, _, polarity) in &state_slots {
            machine_inputs[slot] = polarity;
        }
        let mut machine = SettledMachine::new(&circuit, &machine_inputs).unwrap();
        let external = machine
            .external_inputs()
            .iter()
            .map(|input| {
                let slot = circuit.program.inputs.iter().position(|candidate| candidate == input).unwrap();
                frame[slot]
            })
            .collect::<Vec<_>>();
        let mut machine_converged = false;
        for _ in 0..MAX_FIXED_POINT_PASSES {
            let previous = machine.input_values().to_vec();
            machine.step(&external).unwrap();
            if machine.input_values() == previous {
                machine_converged = true;
                break;
            }
        }
        assert!(machine_converged, "reference SettledMachine did not converge");
        let machine_word = output_slots
            .iter()
            .fold(0u16, |word, &index| (word << 1) | u16::from(machine.outputs()[index] != 0));
        assert_eq!(candidate_word, machine_word, "reused evaluator differs from SettledMachine for {word:#x}");
    }

    let mut rows = Vec::with_capacity(SELECTOR_PATTERNS * SAMPLE_COUNT);
    let mut total_checksum = 0u64;
    for selector in 0..SELECTOR_PATTERNS {
        for frame in sample_inputs[selector].iter().take(64) {
            black_box(run_frame(&mut evaluation, &mut candidate_inputs, frame, &state_slots, &output_slots));
        }
        for sample in 0..SAMPLE_COUNT {
            let mut output_seen = vec![false; 1 << 16];
            let mut checksum = 0u64;
            let mut passes = 0usize;
            let mut operations = 0usize;
            let started = Instant::now();
            for (index, frame) in sample_inputs[selector].iter().enumerate() {
                let (word, frame_passes, frame_operations) = run_frame(&mut evaluation, &mut candidate_inputs, frame, &state_slots, &output_slots);
                output_seen[word as usize] = true;
                passes += frame_passes;
                operations += frame_operations;
                checksum = checksum.rotate_left(5) ^ u64::from(word) ^ (index as u64 + 1);
            }
            let elapsed = started.elapsed();
            let unique_outputs = output_seen.iter().filter(|&&seen| seen).count();
            assert_ne!(checksum, 0, "selector pattern {selector:03b} must yield an observable checksum");
            rows.push(serde_json::json!({
                "raw_selector_pattern": format!("{selector:03b}"),
                "sample": sample,
                "computations": OPERANDS_PER_PATTERN,
                "elapsed_ms": elapsed.as_secs_f64() * 1_000.0,
                "computations_per_second": OPERANDS_PER_PATTERN as f64 / elapsed.as_secs_f64(),
                "ns_per_computation": elapsed.as_nanos() as f64 / OPERANDS_PER_PATTERN as f64,
                "mean_fixed_point_passes": passes as f64 / OPERANDS_PER_PATTERN as f64,
                "mean_expression_operations": operations as f64 / OPERANDS_PER_PATTERN as f64,
                "unique_outputs": unique_outputs,
                "checksum": checksum,
            }));
            total_checksum ^= checksum.rotate_left((selector * 3 + sample) as u32);
        }
    }
    assert_ne!(black_box(total_checksum), 0);
    let artifact = serde_json::json!({
        "benchmark": "fpu_settled_logical_kernel",
        "build": "release",
        "fixture_load_ms": fixture_load_ms,
        "graph_prepare_ms": prepare_ms,
        "logical_compile_ms": compile_ms,
        "input_count": circuit.program.inputs.len(),
        "expression_count": circuit.program.expressions.len(),
        "public_outputs": targets.len(),
        "feedback_state_bits": state_outputs.len(),
        "selector_patterns": SELECTOR_PATTERNS,
        "selector_bit_order": "numeric bit 0 drives the first manifest selector lever; these are raw control patterns, not numeric opcodes",
        "operands_per_pattern": OPERANDS_PER_PATTERN,
        "samples_per_pattern": SAMPLE_COUNT,
        "differential_vectors": differential_words.len(),
        "checksum": total_checksum,
        "cadence_conversion": "none; output timing is not validated",
        "rows": rows,
    });
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/logical-inference/fpu-kernel-benchmark.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_vec_pretty(&artifact).unwrap()).unwrap();
    println!("FPU logical kernel benchmark: {}", serde_json::to_string(&artifact).unwrap());
}
