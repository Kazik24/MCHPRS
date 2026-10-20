//! Complete-state transport validation and isolated before/after inference timing.
use super::*;
use std::time::Instant;

fn prepare_cpu(world: &PlotWorld, bounds: (BlockPos, BlockPos)) -> (LoweredCircuit, CircuitProgram, Vec<(PistonId, BlockPos)>) {
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let options = CompilerOptions {
        optimization_level: Some(5),
        budget_multiplier: 8,
        ..Default::default()
    };
    let mut prepared = passes::rich::prepare(
        world,
        bounds,
        world.scheduler().iter_entries().collect(),
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
    let plan = &prepared.graph.plans.instant_transport;
    let covered = plan.nets.iter().map(|net| net.wires.len()).sum::<usize>();
    println!(
        "BubbleSort dense transport: {} programs, {covered}/{} wires",
        plan.nets.len(),
        prepared.graph.wires.len()
    );
    assert!(covered > 0);
    let memories = memory_only::memory_probes(&prepared.graph, "cpu_bubblesort");
    assert_eq!(memories.len(), 768);
    let mut baseline = prepared.graph.clone();
    baseline.plans.instant_transport = Default::default();
    let (baseline, _) = baseline.lower_with_monitor(&monitor).unwrap();
    (prepared.lower(&monitor).unwrap(), baseline, memories)
}

fn instance(lowered: &LoweredCircuit, program: CircuitProgram) -> GraphBackend {
    let mut backend = GraphBackend::from_lowered(
        LoweredCircuit {
            program,
            state: lowered.state.clone(),
            logical_tick: lowered.logical_tick,
            positions: lowered.positions.clone(),
            imported_ticks: lowered.imported_ticks.clone(),
        },
        &TaskMonitor::default(),
    )
    .unwrap();
    backend.set_test_recording(false);
    backend
}

fn sorted_ram(backend: &GraphBackend) {
    let cpu = cpus::CPUS[2];
    let mut projected = empty();
    project(backend, &mut projected);
    for row in 0..16 {
        let word = (0..12).fold(0u16, |word, bit| {
            let Block::Piston { piston } = projected.get_block(cpu.origin + BlockPos::new(175 + 4 * bit, 37, 62 - 4 * row)) else {
                panic!("RAM unavailable")
            };
            word | (u16::from(!piston.extended) << bit)
        });
        assert_eq!(word, row as u16, "complete sort RAM row {row}");
    }
}

#[test]
#[ignore = "active BubbleSort smoke check: every component, piston, resource and pending item"]
fn bubblesort_dense_transport_first_128_ticks_preserve_exact_state() {
    validate_cpu(128);
}

#[test]
#[ignore = "full native BubbleSort: every graph state/future boundary and all 768 native RAM bits"]
fn bubblesort_dense_transport_complete_sort_matches_native_and_future() {
    validate_cpu(10036);
}

fn validate_cpu(ticks: usize) {
    let cpu = cpus::CPUS[2];
    let (mut native, bounds, _) = large_builds::load_cpu(cpu);
    native.disable_command_output_limits_for_replay();
    let (lowered, reference, memories) = prepare_cpu(&native, bounds);
    let mut baseline = instance(&lowered, reference);
    let mut candidate = instance(&lowered, lowered.program.clone());
    let mut baseline_host = empty();
    let mut candidate_host = empty();
    baseline_host.disable_command_output_limits_for_replay();
    candidate_host.disable_command_output_limits_for_replay();
    cpus::click_cpu(&mut native, cpu, cpu.start);
    baseline.use_block(cpu.origin + cpu.start).unwrap();
    candidate.use_block(cpu.origin + cpu.start).unwrap();
    candidate.assert_same_execution_state(&baseline);
    for tick in 1..=ticks {
        baseline.tick_with_world(&mut baseline_host).unwrap();
        candidate.tick_with_world(&mut candidate_host).unwrap();
        candidate.assert_same_execution_state(&baseline);
        native.tick_interpreted();
        for &(actor, position) in &memories {
            assert_eq!(
                memory_only::graph_storage_bit(&candidate, actor),
                memory_only::native_storage_bit(&native, position),
                "RAM {position:?}, tick {tick}"
            );
        }
        if tick % 1024 == 0 {
            println!("dense transport: {tick} exact CPU boundaries");
        }
    }
    assert!(candidate.execution_counts().instant_transport_invocations > 0);
    assert_eq!(
        baseline_host.command_output().collect::<Vec<_>>(),
        candidate_host.command_output().collect::<Vec<_>>()
    );
    if ticks == 10036 {
        sorted_ram(&candidate);
    }
    println!(
        "dense transport baseline {:?}\ncandidate {:?}",
        baseline.execution_counts(),
        candidate.execution_counts()
    );
}

#[test]
#[ignore = "five alternating full-sort O5 versus dense transport inference pairs"]
fn bubblesort_dense_transport_paired_inference() {
    let cpu = cpus::CPUS[2];
    let (world, bounds, _) = large_builds::load_cpu(cpu);
    let (lowered, reference, _) = prepare_cpu(&world, bounds);
    let mut samples = Vec::new();
    let directory = root().join("target/instant-transport");
    std::fs::create_dir_all(&directory).unwrap();
    for pair in 0..5 {
        let mut runs = Vec::new();
        for enabled in if pair % 2 == 0 { [false, true] } else { [true, false] } {
            let mut backend = instance(&lowered, if enabled { lowered.program.clone() } else { reference.clone() });
            let mut host = empty();
            host.disable_command_output_limits_for_replay();
            let start = Instant::now();
            backend.use_block(cpu.origin + cpu.start).unwrap();
            for _ in 0..10036 {
                backend.tick_with_world(&mut host).unwrap();
            }
            let seconds = start.elapsed().as_secs_f64();
            runs.push((enabled, seconds, backend, host));
        }
        runs.sort_by_key(|run| run.0);
        runs[1].2.assert_same_execution_state(&runs[0].2);
        assert_eq!(
            runs[0].3.command_output().collect::<Vec<_>>(),
            runs[1].3.command_output().collect::<Vec<_>>()
        );
        sorted_ram(&runs[1].2);
        let row = json!({"pair":pair+1,"ticks":10036,"baseline_seconds":runs[0].1,"candidate_seconds":runs[1].1,
            "baseline_tps":10036.0/runs[0].1,"candidate_tps":10036.0/runs[1].1,
            "baseline_counts":format!("{:?}",runs[0].2.execution_counts()),"candidate_counts":format!("{:?}",runs[1].2.execution_counts())});
        println!("dense transport pair {row}");
        samples.push(row);
        std::fs::write(
            directory.join("bubblesort-pairs.json"),
            serde_json::to_vec_pretty(&json!({
                "build":"CPU_BUBBLESORT","source_hash":cpu.sha256,"profile":if cfg!(debug_assertions) {"debug"} else {"release"},
                "baseline":"O5 with only compile-instant-transport removed","candidate":"O5 dense transport",
                "ticks_per_run":10036,"exact_state_future_and_final_sort":true,"original_world_used_for_graph_inference":false,"samples":samples
            }))
            .unwrap(),
        )
        .unwrap();
    }
}
