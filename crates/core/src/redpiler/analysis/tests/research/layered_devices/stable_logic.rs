//! Stable-input compilation is checked at the RAM/control/output boundary, not physical wire queues.
use super::*;
use std::time::Instant;

fn sorted_words(backend: &GraphBackend) -> Vec<Option<u16>> {
    let mut world = empty();
    project(backend, &mut world);
    let cpu = cpus::CPUS[2];
    (0..16)
        .map(|row| {
            (0..12).try_fold(0u16, |word, bit| {
                let value = memory_only::native_storage_bit(&world, cpu.origin + BlockPos::new(175 + 4 * bit, 37, 62 - 4 * row))?;
                Some(word | (u16::from(value) << bit))
            })
        })
        .collect()
}

#[test]
#[ignore = "stable-input logical compilation: 128 CPU ticks, native RAM/readiness and interpreter handoff"]
fn bubblesort_stable_logic_memory_smoke() {
    validate(128);
}

#[test]
#[ignore = "stable-input logical compilation: full10036 CPU ticks, native RAM/readiness and interpreter handoff"]
fn bubblesort_stable_logic_complete_sort_and_continuation() {
    validate(10036);
}

fn validate(ticks: usize) {
    let cpu = cpus::CPUS[2];
    let (mut native, bounds, _) = large_builds::load_cpu(cpu);
    native.disable_command_output_limits_for_replay();
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let options = CompilerOptions {
        optimization_level: Some(5),
        stable_instant_inputs: true,
        budget_multiplier: 8,
        ..Default::default()
    };
    let mut prepared = passes::rich::prepare(
        &native,
        bounds,
        native.scheduler().iter_entries().collect(),
        &monitor,
        AnalysisLimits::for_budget(8),
    )
    .unwrap();
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::default_graph_pipeline(&options),
        &monitor,
    )
    .unwrap();
    let stats = prepared.graph.plans.stable_instant_logic.stats;
    println!(
        "CPU stable logic: {} nets, {} wires, {} shared inputs, {} terms, {} outputs",
        stats.admitted, stats.wires, stats.inputs, stats.terms, stats.outputs
    );
    assert!(stats.wires > 0);
    let memories = memory_only::memory_probes(&prepared.graph, "cpu_bubblesort");
    assert_eq!(memories.len(), 768);
    let mut reference = prepared.graph.clone();
    reference.plans.stable_instant_logic = Default::default();
    reference.plans.guard_predicates = Default::default();
    let (program, _) = reference.lower_with_monitor(&monitor).unwrap();
    // Import the same scheduled work and logical clock in both executions.
    let lowered = prepared.lower(&monitor).unwrap();
    let mut baseline = GraphBackend::from_lowered(
        LoweredCircuit {
            program,
            state: lowered.state.clone(),
            logical_tick: lowered.logical_tick,
            positions: lowered.positions.clone(),
            imported_ticks: lowered.imported_ticks.clone(),
        },
        &monitor,
    )
    .unwrap();
    let mut candidate = GraphBackend::from_lowered(lowered, &monitor).unwrap();
    baseline.set_test_recording(false);
    candidate.set_test_recording(false);
    let mut baseline_host = empty();
    let mut candidate_host = empty();
    baseline_host.disable_command_output_limits_for_replay();
    candidate_host.disable_command_output_limits_for_replay();
    cpus::click_cpu(&mut native, cpu, cpu.start);
    baseline.use_block(cpu.origin + cpu.start).unwrap();
    candidate.use_block(cpu.origin + cpu.start).unwrap();
    let mut baseline_seconds = 0.0;
    let mut candidate_seconds = 0.0;
    for tick in 1..=ticks {
        let started = Instant::now();
        baseline.tick_with_world(&mut baseline_host).unwrap();
        baseline_seconds += started.elapsed().as_secs_f64();
        let started = Instant::now();
        candidate.tick_with_world(&mut candidate_host).unwrap();
        candidate_seconds += started.elapsed().as_secs_f64();
        native.tick_interpreted();
        for &(actor, position) in &memories {
            let expected = memory_only::native_storage_bit(&native, position);
            assert_eq!(
                memory_only::graph_storage_bit(&baseline, actor),
                expected,
                "baseline RAM {position:?}, tick{tick}"
            );
            assert_eq!(
                memory_only::graph_storage_bit(&candidate, actor),
                expected,
                "compiled RAM {position:?}, tick{tick}"
            );
        }
        if tick % 1024 == 0 {
            println!("stable logic: {tick} native RAM/readiness boundaries");
        }
    }
    assert_eq!(
        baseline_host.command_output().collect::<Vec<_>>(),
        candidate_host.command_output().collect::<Vec<_>>()
    );
    if ticks == 10036 {
        assert_eq!(sorted_words(&candidate), (0..16).map(Some).collect::<Vec<_>>());
    }
    let (mut resumed, _, _) = large_builds::load_cpu(cpu);
    resumed.disable_command_output_limits_for_replay();
    candidate.reset(&mut resumed);
    for tick in 1..=16 {
        native.tick_interpreted();
        resumed.tick_interpreted();
        for &(_, position) in &memories {
            assert_eq!(
                memory_only::native_storage_bit(&resumed, position),
                memory_only::native_storage_bit(&native, position),
                "native handoff RAM {position:?}, tick{tick}"
            );
        }
    }
    let report = json!({ "ticks":ticks,"stable_instant_inputs":true,"nets":stats.admitted,"wires":stats.wires,"shared_inputs":stats.inputs,"terms":stats.terms,"outputs":stats.outputs,
        "all_768_native_ram_bits_and_readiness":true,"command_outputs_equal":true,"interpreter_handoff_ticks":16,
        "diagnostic_timing_only":true,"baseline_seconds":baseline_seconds,"candidate_seconds":candidate_seconds,
        "baseline_counts":format!("{:?}",baseline.execution_counts()),"candidate_counts":format!("{:?}",candidate.execution_counts()) });
    println!("stable logic result {report}");
    let directory = root().join("target/stable-instant-logic");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join(format!("cpu-{ticks}-boundaries.json")),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}
