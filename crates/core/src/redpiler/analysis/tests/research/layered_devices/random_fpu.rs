//! Random FPU inference from graph state; an empty host receives command effects.
use super::*;
use crate::redpiler::passes::planning::PlanBuilder;
use std::time::Instant;

#[derive(Clone, Copy)]
pub(super) enum Operation {
    Lever(BlockPos, bool),
    Tick,
}

pub(super) fn operations(fixture: &Value) -> (Vec<Operation>, usize, Vec<u64>) {
    let (world, _) = load(fixture);
    let position = |value: &Value| local_pos(value) - BASE + origin(fixture);
    let trigger = position(&fixture["ports"]["trigger"]);
    let mut inputs = Vec::new();
    for key in ["light_blue_operand_inputs", "red_operand_inputs", "opcode_levers_by_sign_weight_4_2_1"] {
        inputs.extend(fixture["ports"][key].as_array().unwrap().iter().map(position));
    }
    assert_eq!(inputs.len(), 35);
    let mut current = 0u64;
    for (bit, &pos) in inputs.iter().enumerate() {
        let Block::Lever { lever } = world.get_block(pos) else {
            panic!("missing input")
        };
        current |= u64::from(lever.powered) << bit;
    }
    let mut ops = vec![Operation::Lever(trigger, true)];
    ops.extend(std::iter::repeat_n(Operation::Tick, 64));
    let mut random = 0x19a4_110b_5eedu64;
    let mut vectors = Vec::new();
    let mut measured_start = 0;
    for episode in 0..36 {
        if episode == 4 {
            measured_start = ops.len();
        }
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        let word = random & ((1u64 << 35) - 1);
        vectors.push(word);
        ops.push(Operation::Lever(trigger, false));
        for (bit, &pos) in inputs.iter().enumerate() {
            if (current ^ word) & (1u64 << bit) != 0 {
                ops.push(Operation::Lever(pos, word & (1u64 << bit) != 0));
            }
        }
        current = word;
        ops.extend(std::iter::repeat_n(Operation::Tick, 64));
        ops.push(Operation::Lever(trigger, true));
        ops.extend(std::iter::repeat_n(Operation::Tick, 64));
    }
    (ops, measured_start, vectors)
}

fn execute(backend: &mut GraphBackend, operation: Operation, controls: &mut std::collections::HashMap<BlockPos, bool>, host: &mut PlotWorld) {
    match operation {
        Operation::Tick => backend.tick_with_world(host).unwrap(),
        Operation::Lever(pos, powered) => {
            if controls.insert(pos, powered) != Some(powered) {
                backend.use_block(pos).unwrap();
            }
        }
    }
}

#[test]
#[ignore = "explicit one-kernel FPU, random native histories, and five alternating timing pairs"]
fn fpu_random_inputs_one_kernel_and_paired_inference() {
    random_fpu(true, CpuComparison::Off);
}

#[test]
#[ignore = "explicit random switching-cost diagnostic; constant outputs do not establish arithmetic throughput"]
fn fpu_random_inputs_one_kernel_diagnostic_measurement() {
    random_fpu(false, CpuComparison::Off);
}

#[test]
#[ignore = "explicit isolated CPU packing versus event kernels: native replay and five alternating timing pairs"]
fn fpu_cpu_kernels_paired_inference() {
    random_fpu(false, CpuComparison::Packing);
}

#[test]
#[ignore = "explicit scalar predicates versus mask packing on the same FPU graph and executable"]
fn fpu_cpu_scalar_paired_inference() {
    random_fpu(false, CpuComparison::Scalar);
}

#[test]
#[ignore = "explicit typed wire emission versus original Source reads with identical scalar predicates"]
fn fpu_cpu_wire_emission_paired_inference() {
    random_fpu(false, CpuComparison::WireEmission);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CpuComparison {
    Off,
    Packing,
    Scalar,
    WireEmission,
}

fn random_fpu(require_activity: bool, comparison: CpuComparison) {
    let cpu = comparison != CpuComparison::Off;
    let fixture = manifest("fpu_legal");
    let (mut native, bounds) = load(&fixture);
    native.disable_command_output_limits_for_replay();
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let options = CompilerOptions {
        optimization_level: Some(5),
        stable_instant_inputs: true,
        budget_multiplier: 8,
        ..Default::default()
    };
    let start = Instant::now();
    let mut prepared = passes::rich::prepare(&native, bounds, vec![], &monitor, AnalysisLimits::for_budget(8)).unwrap();
    let mut pipeline = passes::default_graph_pipeline(&options);
    pipeline.entries.retain(|entry| !matches!(&entry.pass, passes::Pass::Build(builder) if matches!(builder.name(), "experimental-compile-instant-events" | "experimental-compile-bud-memory-reads" | "experimental-minimize-wire-captures" | "optimize-instant-kernels-cpu")));
    for entry in &mut pipeline.entries {
        if matches!(&entry.pass, passes::Pass::Build(builder) if builder.name() == "compile-instant-layers") {
            entry.enabled = true;
        }
    }
    passes::run_graph_pipeline(&mut prepared.graph, &options, &prepared.entry, &pipeline, &monitor).unwrap();
    let mut baseline_graph = prepared.graph.clone();
    let context = passes::PassContext::new(&monitor, &options, &prepared.entry);
    passes::minimize_wire_captures::MinimizeWireCaptures
        .run(&mut prepared.graph, &context)
        .unwrap();
    passes::compile_bud_memory_reads::CompileBudMemoryReads
        .run(&mut prepared.graph, &context)
        .unwrap();
    passes::compile_instant_events::CompileInstantEvents
        .run(&mut prepared.graph, &context)
        .unwrap();
    let packing_start = Instant::now();
    if cpu {
        baseline_graph = prepared.graph.clone();
        passes::optimize_instant_kernels_cpu::OptimizeInstantKernelsCpu
            .run(&mut prepared.graph, &context)
            .unwrap();
    }
    let packing_seconds = packing_start.elapsed().as_secs_f64();
    prepared.graph.validate().unwrap();
    let compile_seconds = start.elapsed().as_secs_f64();
    let kernels = &prepared.graph.plans.instant_events;
    assert_eq!(kernels.len(), 1, "FPU computation must form one connected kernel");
    let actors = kernels[0].actors.len();
    let KernelProgram::Events(entries) = &kernels[0].program else {
        unreachable!()
    };
    let pins = entries.iter().map(|entry| entry.pins.len()).sum::<usize>();
    let constant_entries = entries.iter().filter(|entry| entry.pins.len() == 1).count();
    let catalog_pins = prepared
        .graph
        .plans
        .instant
        .boundaries
        .as_ref()
        .unwrap()
        .cells
        .iter()
        .map(|cell| cell.pins.len())
        .sum::<usize>();
    let capture_operands = prepared
        .graph
        .wires
        .iter()
        .map(|wire| wire.execution.mutable_captures.unwrap().count_ones() as usize)
        .sum::<usize>();
    let directory = root().join(match comparison {
        CpuComparison::Off => "target/instant-event-kernels",
        CpuComparison::Packing => "target/cpu-kernels",
        CpuComparison::Scalar => "target/cpu-scalar",
        CpuComparison::WireEmission => "target/cpu-wire-emission",
    });
    std::fs::create_dir_all(&directory).unwrap();
    if cpu {
        std::fs::write(
            directory.join("fpu-packed-kernel.json"),
            serde_json::to_vec_pretty(&crate::redpiler::analysis::export_json::cpu_kernel_records(&prepared.graph)).unwrap(),
        )
        .unwrap();
    }
    let records = crate::redpiler::analysis::export_json::event_kernel_records(&prepared.graph);
    std::fs::write(
        directory.join("fpu-kernel.json"),
        serde_json::to_vec_pretty(&json!({
            "format":"redpiler-event-kernel-research", "executable_kernels":records,
            "wire_propagation_removed":false,"reference_scope":"actor and resource IDs refer to the source CompileGraph"
        }))
        .unwrap(),
    )
    .unwrap();
    println!("FPU: one kernel, {actors} actors, {pins} reduced pins, {constant_entries} single-pin entries; compile {compile_seconds:.3}s");
    let make = |graph: &CompileGraph| {
        let (program, state) = graph.clone().lower().unwrap();
        let mut backend = GraphBackend::new(program, state, 0).unwrap();
        backend.set_test_recording(false);
        backend
    };
    let (ops, measured_start, vectors) = operations(&fixture);
    let mut initial_controls = std::collections::HashMap::new();
    for operation in &ops {
        if let Operation::Lever(pos, _) = *operation {
            let Block::Lever { lever } = native.get_block(pos) else {
                panic!("missing lever")
            };
            initial_controls.insert(pos, lever.powered);
        }
    }
    let output_positions = fixture["observations"]["output_repeaters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| local_pos(value) - BASE + origin(&fixture))
        .collect::<Vec<_>>();
    let output_word = |world: &PlotWorld| {
        output_positions.iter().fold(0u16, |word, &pos| {
            (word << 1) | u16::from(matches!(world.get_block(pos), Block::RedstoneRepeater { repeater } if repeater.powered))
        })
    };
    let mut optimized = make(&prepared.graph);
    let mut host = empty();
    host.disable_command_output_limits_for_replay();
    let mut projected = load(&fixture).0;
    let mut controls = initial_controls.clone();
    let mut ticks = 0;
    let mut outputs = std::collections::BTreeSet::new();
    for &operation in &ops {
        match operation {
            Operation::Lever(pos, powered) => lever_action(&mut native, pos, powered),
            Operation::Tick => native.tick_interpreted(),
        }
        execute(&mut optimized, operation, &mut controls, &mut host);
        if matches!(operation, Operation::Tick) {
            ticks += 1;
            project(&optimized, &mut projected);
            assert_future_work(&native, &projected, "random FPU");
            assert_eq!(native.piston_state().events, projected.piston_state().events, "random FPU queued actions");
            let motions = |world: &PlotWorld| {
                world
                    .piston_state()
                    .motions
                    .iter()
                    .map(|motion| (motion.pos, motion.progress, motion.previous_progress, motion.last_tick))
                    .collect::<Vec<_>>()
            };
            assert_eq!(motions(&native), motions(&projected), "random FPU deferred publications");
            assert_eq!(output_word(&native), output_word(&projected), "random FPU tick {ticks}");
            outputs.insert(output_word(&native));
            if ticks % 64 == 0 {
                assert_eq!(
                    native.command_output().collect::<Vec<_>>(),
                    host.command_output().collect::<Vec<_>>(),
                    "random FPU command output"
                );
                pc_counter_contract::compare_checkpoint(&native, &projected, bounds, &format!("random FPU tick {ticks}"));
            }
            if ticks % 512 == 0 {
                println!("FPU random native validation: {ticks}/4672 ticks");
            }
        }
    }
    let activity_confirmed = outputs.len() > 1;
    if require_activity {
        assert!(activity_confirmed, "random workload must change FPU outputs");
    }
    assert!(
        optimized.execution_counts().piston_actions > 0,
        "diagnostic requires internal computation activity"
    );
    check_handoff(&mut native, &mut optimized, &mut projected, bounds, "random FPU");
    println!(
        "FPU: {ticks} random-input native ticks, {} distinct output words and native handoff passed",
        outputs.len()
    );
    let mut samples = Vec::new();
    for pair in 0..5 {
        let mut runs = Vec::new();
        for enabled in if pair % 2 == 0 { [false, true] } else { [true, false] } {
            let mut backend = make(if enabled || matches!(comparison, CpuComparison::Scalar | CpuComparison::WireEmission) {
                &prepared.graph
            } else {
                &baseline_graph
            });
            if !enabled && comparison == CpuComparison::Scalar {
                backend.use_general_cpu_predicates();
            }
            if !enabled && comparison == CpuComparison::WireEmission {
                backend.use_general_cpu_wire_reads();
            }
            let mut controls = initial_controls.clone();
            let mut host = empty();
            host.disable_command_output_limits_for_replay();
            for &operation in &ops[..measured_start] {
                execute(&mut backend, operation, &mut controls, &mut host);
            }
            let before = backend.execution_counts();
            let start = Instant::now();
            for &operation in &ops[measured_start..] {
                execute(&mut backend, operation, &mut controls, &mut host);
            }
            let seconds = start.elapsed().as_secs_f64();
            runs.push((enabled, seconds, backend, before));
        }
        runs.sort_by_key(|run| run.0);
        runs[0].2.assert_same_execution_state(&runs[1].2);
        if cpu {
            assert_eq!(runs[0].2.execution_counts(), runs[1].2.execution_counts(), "CPU packing preserves work");
        }
        let (mut left, _) = load(&fixture);
        let (mut right, _) = load(&fixture);
        project(&runs[0].2, &mut left);
        project(&runs[1].2, &mut right);
        assert_future_work(&left, &right, "paired random FPU");
        pc_counter_contract::compare_checkpoint(&left, &right, bounds, "paired random FPU");
        println!("FPU random pair {}: {:.6}s -> {:.6}s", pair + 1, runs[0].1, runs[1].1);
        samples.push(json!({"pair":pair+1,"baseline_seconds":runs[0].1,"enabled_seconds":runs[1].1,
            "baseline_measured_wire_walks":runs[0].2.execution_counts().wire_walks-runs[0].3.wire_walks,
            "enabled_measured_wire_walks":runs[1].2.execution_counts().wire_walks-runs[1].3.wire_walks,
            "baseline_measured_callbacks":runs[0].2.execution_counts().callbacks-runs[0].3.callbacks,
            "enabled_measured_callbacks":runs[1].2.execution_counts().callbacks-runs[1].3.callbacks,
            "baseline_counts":format!("{:?}",runs[0].2.execution_counts()),"enabled_counts":format!("{:?}",runs[1].2.execution_counts())}));
    }
    let packed = &prepared.graph.plans.cpu_kernels;
    let predicates: Vec<_> = packed.entries.iter().flat_map(|e| [e.main, e.quasi, e.valid]).collect();
    let report = json!({"fixture":"fpu_legal","cpu_packing":cpu,"scalar_comparison":comparison == CpuComparison::Scalar,"packing_seconds":packing_seconds,
        "wire_emission_comparison":comparison == CpuComparison::WireEmission,
        "predicate_kinds":predicates.iter().fold(std::collections::BTreeMap::<String,usize>::new(), |mut m,p| { *m.entry(if p.read_len == 0 { "Constant".into() } else { format!("{:?}",p.kind) }).or_default() += 1; m }),
        "cpu_entries":packed.entries.len(),"cpu_reads":packed.reads.len(),"cpu_clauses":packed.require_true.len(),
        "simd_predicates":predicates.iter().filter(|p| p.clause_len >= 4).count(),
        "cpu_read_kinds":format!("{:?}",packed.reads.iter().fold(std::collections::BTreeMap::<String,usize>::new(), |mut m,r| { *m.entry(format!("{:?}",r).split(['(', '{']).next().unwrap().trim().to_owned()).or_default()+=1; m })),
        "cpu_bytes":packed.entries.len()*std::mem::size_of::<CpuKernelEntry>()+packed.reads.len()*std::mem::size_of::<CpuRead>()+packed.require_true.len()*16,
        "seed":0x19a4_110b_5eedu64,"raw_vectors":vectors,"warmup_episodes":4,"measured_episodes":32,
        "ticks_per_phase":64,"kernel_count":1,"kernel_actors":actors,"reduced_pins":pins,"single_pin_entries":constant_entries,
        "source_catalog_pins":catalog_pins,"wire_capture_operands_before":prepared.graph.wires.len()*24,"wire_capture_operands_after":capture_operands,
        "physical_pistons":prepared.graph.pistons.len(),"wire_walks_removed":false,"original_world_used_for_inference":false,
        "command_host":"empty PlotWorld: command effects only, no circuit blocks",
        "activity_confirmed":activity_confirmed,"measurement_kind":if activity_confirmed {"raw_port_inference"} else {"constant_output_switching_diagnostic"},
        "useful_fpu_operations_per_second":null,
        "numeric_encoding_verified":false,"native_ticks":ticks,"native_handoff_ticks":16,"distinct_output_words":outputs.len(),
        "compile_seconds":compile_seconds,"samples":samples});
    std::fs::write(directory.join("fpu-random-paired.json"), serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}

#[test]
#[ignore = "native-only green-trigger protocol investigation; not an arithmetic correctness oracle"]
fn fpu_native_prepared_input_protocol_probe() {
    let fixture = manifest("fpu_legal");
    let position = |value: &Value| local_pos(value) - BASE + origin(&fixture);
    let trigger = position(&fixture["ports"]["trigger"]);
    let inputs: Vec<_> = ["light_blue_operand_inputs", "red_operand_inputs", "opcode_levers_by_sign_weight_4_2_1"]
        .iter()
        .flat_map(|key| fixture["ports"][key].as_array().unwrap().iter().map(position))
        .collect();
    let outputs: Vec<_> = fixture["observations"]["output_repeaters"]
        .as_array()
        .unwrap()
        .iter()
        .map(position)
        .collect();
    let vectors = operations(&fixture).2;
    let mut reports = Vec::new();
    for prepare_on in [true, false] {
        let (mut world, _) = load(&fixture);
        world.disable_command_output_limits_for_replay();
        assert_eq!(world.get_block(trigger.offset(BlockFace::North)), Block::from_name("green_wool").unwrap());
        let mut words = std::collections::BTreeSet::new();
        let mut ticks = 0;
        let advance = |world: &mut PlotWorld, count, words: &mut std::collections::BTreeSet<u16>, ticks: &mut usize| {
            for _ in 0..count {
                world.tick_interpreted();
                *ticks += 1;
                words.insert(outputs.iter().fold(0u16, |word, &pos| {
                    (word << 1) | u16::from(matches!(world.get_block(pos), Block::RedstoneRepeater { repeater } if repeater.powered))
                }));
            }
        };
        lever_action(&mut world, trigger, true);
        advance(&mut world, 64, &mut words, &mut ticks);
        for &vector in vectors.iter().take(8) {
            if !prepare_on {
                lever_action(&mut world, trigger, false);
                advance(&mut world, 128, &mut words, &mut ticks);
            }
            // Hold new operands before a separate clean activation edge.
            for (bit, &pos) in inputs.iter().enumerate() {
                lever_action(&mut world, pos, vector & (1u64 << bit) != 0);
            }
            advance(&mut world, 64, &mut words, &mut ticks);
            if !prepare_on {
                lever_action(&mut world, trigger, true);
                advance(&mut world, 64, &mut words, &mut ticks);
            }
            lever_action(&mut world, trigger, false);
            advance(&mut world, 128, &mut words, &mut ticks);
            lever_action(&mut world, trigger, true);
            advance(&mut world, 128, &mut words, &mut ticks);
        }
        println!("FPU green protocol prepare_on={prepare_on}: {ticks} native ticks, output words {words:?}");
        reports.push(json!({"prepare_on":prepare_on,"native_ticks":ticks,"vectors":&vectors[..8],"output_words":words}));
    }
    std::fs::write(
        root().join("target/instant-event-kernels/native-protocol-probe.json"),
        serde_json::to_vec_pretty(&reports).unwrap(),
    )
    .unwrap();
}
