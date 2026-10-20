//! Interpreter versus the actual maximum compatible pipeline, excluding preparation and projection.
use super::*;
use std::time::Instant;

#[derive(Clone, Copy)]
enum Operation {
    Lever(BlockPos, bool),
    Button(BlockPos),
    Tick,
}

struct Episode {
    operations: Vec<Operation>,
    warmup: usize,
}

fn episodes(fixture: &Value) -> Vec<Episode> {
    fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| fixture["id"] != "adder_11bits" || case["expectation"].is_object())
        .map(|case| {
            let mut operations = Vec::new();
            for step in case.get("steps").or_else(|| case.get("actions")).unwrap().as_array().unwrap() {
                if step["diagnose"] == true {
                    continue;
                }
                if let Some(ticks) = step.get("advance").or_else(|| (step["op"] == "wait_ready").then_some(&step["ticks"])) {
                    operations.extend(std::iter::repeat_n(Operation::Tick, ticks.as_u64().unwrap() as usize));
                } else {
                    assert_eq!(step["op"], "lever");
                    operations.push(Operation::Lever(
                        local_pos(&step["pos"]) - BASE + origin(fixture),
                        step["powered"].as_bool().unwrap(),
                    ));
                }
            }
            operations.extend(std::iter::repeat_n(Operation::Tick, case["ticks"].as_u64().unwrap_or(0) as usize));
            Episode { operations, warmup: 0 }
        })
        .collect()
}

fn native_operation(world: &mut PlotWorld, operation: Operation) {
    match operation {
        Operation::Tick => world.tick_interpreted(),
        Operation::Lever(pos, value) => lever_action(world, pos, value),
        Operation::Button(pos) => cpus::click(world, pos - BlockPos::new(8, 8, 8)),
    }
}

#[test]
#[ignore = "five alternating interpreter/max pairs for five devices, including full BubbleSort"]
fn maximum_pipeline_interpreter_tps_for_all_requested_builds() {
    assert!(
        !cfg!(feature = "unsafe-abstract-instant-nets") && !cfg!(feature = "unsafe-turbo-fixed-point"),
        "known incorrect experiments cannot enter the maximum-correctness comparison"
    );
    let directory = root().join("target/cpu-kernels");
    std::fs::create_dir_all(&directory).unwrap();
    let mut reports = Vec::new();
    for name in ["adder11", "counter", "divider", "fpu", "cpu_bubblesort"] {
        let fixture = match name {
            "adder11" | "counter" => serde_json::from_slice(
                &std::fs::read(root().join("test_data/instant-pistons-io/fixtures").join(if name == "adder11" {
                    "adder_11bits.json"
                } else {
                    "counter_basic.json"
                }))
                .unwrap(),
            )
            .unwrap(),
            "divider" => manifest("fpu_divider"),
            "fpu" => manifest("fpu_legal"),
            _ => Value::Null,
        };
        let load_world = || {
            if name == "cpu_bubblesort" {
                let (world, bounds, _) = large_builds::load_cpu(cpus::CPUS[2]);
                (world, bounds)
            } else {
                load(&fixture)
            }
        };
        let (world, bounds) = load_world();
        let episodes = if name == "fpu" {
            let (ops, warmup, _) = random_fpu::operations(&fixture);
            vec![Episode {
                operations: ops
                    .into_iter()
                    .map(|op| match op {
                        random_fpu::Operation::Tick => Operation::Tick,
                        random_fpu::Operation::Lever(p, v) => Operation::Lever(p, v),
                    })
                    .collect(),
                warmup,
            }]
        } else if name == "cpu_bubblesort" {
            let mut ops = vec![Operation::Button(cpus::CPUS[2].origin + cpus::CPUS[2].start)];
            ops.extend(std::iter::repeat_n(Operation::Tick, 10036));
            vec![Episode { operations: ops, warmup: 0 }]
        } else {
            episodes(&fixture)
        };
        let ticks = episodes
            .iter()
            .map(|e| e.operations[e.warmup..].iter().filter(|op| matches!(op, Operation::Tick)).count())
            .sum::<usize>();
        let monitor = TaskMonitor::default();
        monitor.set_budget_multiplier(8);
        let options = CompilerOptions {
            optimization_level: Some(5),
            stable_instant_inputs: true,
            budget_multiplier: 8,
            ..Default::default()
        };
        let start = Instant::now();
        let mut prepared = passes::rich::prepare(
            &world,
            bounds,
            world.scheduler().iter_entries().collect(),
            &monitor,
            AnalysisLimits::for_budget(8),
        )
        .unwrap();
        let pipeline = passes::default_graph_pipeline(&options);
        let selected_passes: Vec<_> = pipeline
            .entries
            .iter()
            .filter(|e| e.enabled)
            .map(|e| match &e.pass {
                passes::Pass::Build(p) => p.name(),
                passes::Pass::Rewrite(p) => p.name(),
            })
            .collect();
        passes::run_graph_pipeline(&mut prepared.graph, &options, &prepared.entry, &pipeline, &monitor).unwrap();
        let kernel_count = prepared.graph.plans.instant_events.len();
        let cpu_entries = prepared.graph.plans.cpu_kernels.entries.len();
        assert_eq!(kernel_count, 1, "{name} must remain one connected kernel");
        assert!(cpu_entries > 0);
        if name == "counter" {
            assert_eq!(prepared.graph.plans.bud_memory_reads.len(), 16);
        }
        let compile_seconds = start.elapsed().as_secs_f64();
        let lowered = prepared.lower(&monitor).unwrap();
        let mut samples = Vec::new();
        for pair in 0..5 {
            let mut native_seconds = 0.0;
            let mut graph_seconds = 0.0;
            for episode in &episodes {
                let (mut native, _) = load_world();
                native.disable_command_output_limits_for_replay();
                let mut host = empty();
                host.disable_command_output_limits_for_replay();
                let mut controls = std::collections::HashMap::new();
                for operation in &episode.operations {
                    if let Operation::Lever(p, _) = operation {
                        let Block::Lever { lever } = native.get_block(*p) else {
                            panic!("missing lever")
                        };
                        controls.insert(*p, lever.powered);
                    }
                }
                let mut backend = GraphBackend::from_lowered(
                    LoweredCircuit {
                        program: lowered.program.clone(),
                        state: lowered.state.clone(),
                        logical_tick: lowered.logical_tick,
                        positions: lowered.positions.clone(),
                        imported_ticks: lowered.imported_ticks.clone(),
                    },
                    &monitor,
                )
                .unwrap();
                backend.set_test_recording(false);
                for graph in if pair % 2 == 0 { [false, true] } else { [true, false] } {
                    let mut duration = 0.0;
                    for (index, operations) in [
                        episode.operations[..episode.warmup].as_ref(),
                        episode.operations[episode.warmup..].as_ref(),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let start = Instant::now();
                        for &operation in operations {
                            if graph {
                                match operation {
                                    Operation::Tick => backend.tick_with_world(&mut host).unwrap(),
                                    Operation::Lever(p, value) => {
                                        if controls.insert(p, value) != Some(value) {
                                            backend.use_block(p).unwrap();
                                        }
                                    }
                                    Operation::Button(p) => backend.use_block(p).unwrap(),
                                }
                            } else {
                                native_operation(&mut native, operation);
                            }
                        }
                        if index == 1 {
                            duration = start.elapsed().as_secs_f64();
                        }
                    }
                    if graph {
                        graph_seconds += duration;
                    } else {
                        native_seconds += duration;
                    }
                }
                let mut projected = load_world().0;
                project(&backend, &mut projected);
                assert_future_work(&native, &projected, name);
                pc_counter_contract::compare_checkpoint(&native, &projected, bounds, name);
                assert_eq!(
                    native.command_output().collect::<Vec<_>>(),
                    host.command_output().collect::<Vec<_>>(),
                    "{name} command outputs"
                );
                if name == "cpu_bubblesort" {
                    for row in 0..16 {
                        let word = (0..12).fold(0u16, |word, bit| {
                            let Block::Piston { piston } = native.get_block(cpus::CPUS[2].origin + BlockPos::new(175 + 4 * bit, 37, 62 - 4 * row))
                            else {
                                panic!("RAM cell")
                            };
                            word | (u16::from(!piston.extended) << bit)
                        });
                        assert_eq!(word, row as u16, "complete sorted RAM");
                    }
                }
            }
            println!(
                "TPS {name} pair {}: interpreter {:.2} TPS, max {:.2} TPS ({native_seconds:.6}s -> {graph_seconds:.6}s, {ticks} ticks)",
                pair + 1,
                ticks as f64 / native_seconds,
                ticks as f64 / graph_seconds
            );
            samples.push(json!({"pair":pair+1,"ticks":ticks,"interpreter_seconds":native_seconds,"maximum_seconds":graph_seconds,"interpreter_tps":ticks as f64/native_seconds,"maximum_tps":ticks as f64/graph_seconds}));
        }
        reports.push(json!({"build":name,"fixture":if name=="cpu_bubblesort" {json!(cpus::CPUS[2].schematic)} else {fixture["fixture"].clone()},"source_hash":if name=="cpu_bubblesort" {json!(cpus::CPUS[2].sha256)} else {fixture["sha256"].clone()},
            "pipeline":selected_passes,"optimization_level":5,"stable_instant_inputs":true,"kernel_count":kernel_count,"cpu_entries":cpu_entries,"compile_seconds":compile_seconds,"episodes":episodes.len(),"samples":samples,
            "scope":if name=="fpu" {"random switching diagnostic; native outputs remain 0xffff"} else if name=="cpu_bubblesort" {"complete sort; RAM 0..15 verified; frozen loader and documented in-memory wall-sign removal"} else {"pinned manifest histories"},
            "original_world_used_for_graph_inference":false}));
        std::fs::write(
            directory.join("maximum-vs-interpreter-tps.json"),
            serde_json::to_vec_pretty(&reports).unwrap(),
        )
        .unwrap();
    }
}

#[test]
#[ignore = "five complete BubbleSort pairs: packed masks/Source reads versus scalar predicates/typed wires"]
fn cpu_scalar_wire_bubblesort_paired_inference() {
    let cpu = cpus::CPUS[2];
    let (world, bounds, _) = large_builds::load_cpu(cpu);
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let options = CompilerOptions {
        optimization_level: Some(5),
        budget_multiplier: 8,
        ..Default::default()
    };
    let mut prepared = passes::rich::prepare(
        &world,
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
    assert_eq!(prepared.graph.plans.instant_events.len(), 1);
    let lowered = prepared.lower(&monitor).unwrap();
    let reference = lowered.program.clone().with_general_cpu_predicates().with_general_cpu_wire_reads();
    let directory = root().join("target/cpu-scalar-wire-bubblesort");
    std::fs::create_dir_all(&directory).unwrap();
    let mut samples = Vec::new();
    for pair in 0..5 {
        let mut runs = Vec::new();
        for enabled in if pair % 2 == 0 { [false, true] } else { [true, false] } {
            let mut backend = GraphBackend::from_lowered(
                LoweredCircuit {
                    program: if enabled { lowered.program.clone() } else { reference.clone() },
                    state: lowered.state.clone(),
                    logical_tick: lowered.logical_tick,
                    positions: lowered.positions.clone(),
                    imported_ticks: lowered.imported_ticks.clone(),
                },
                &monitor,
            )
            .unwrap();
            backend.set_test_recording(false);
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
        runs[0].2.assert_same_execution_state(&runs[1].2);
        assert_eq!(runs[0].2.execution_counts(), runs[1].2.execution_counts());
        assert_eq!(
            runs[0].3.command_output().collect::<Vec<_>>(),
            runs[1].3.command_output().collect::<Vec<_>>()
        );
        let mut projected = large_builds::load_cpu(cpu).0;
        project(&runs[1].2, &mut projected);
        for row in 0..16 {
            let word = (0..12).fold(0u16, |word, bit| {
                let Block::Piston { piston } = projected.get_block(cpu.origin + BlockPos::new(175 + 4 * bit, 37, 62 - 4 * row)) else {
                    panic!("RAM cell")
                };
                word | (u16::from(!piston.extended) << bit)
            });
            assert_eq!(word, row as u16, "complete sorted RAM");
        }
        println!(
            "BubbleSort scalar/wire pair {}: {:.6}s -> {:.6}s ({:.2} -> {:.2} TPS)",
            pair + 1,
            runs[0].1,
            runs[1].1,
            10036.0 / runs[0].1,
            10036.0 / runs[1].1
        );
        samples.push(json!({"pair":pair+1,"ticks":10036,"baseline_seconds":runs[0].1,"enabled_seconds":runs[1].1,"baseline_tps":10036.0/runs[0].1,"enabled_tps":10036.0/runs[1].1,"work_counts":format!("{:?}",runs[1].2.execution_counts())}));
        std::fs::write(
            directory.join("paired.json"),
            serde_json::to_vec_pretty(&json!({
                "build":"cpu_bubblesort","source_hash":cpu.sha256,"optimization_level":5,"stable_instant_inputs":false,
                "baseline":"same production graph with general packed-mask predicates and Source wire reads",
                "candidate":"scalar predicate specialization and typed wire emission reads",
                "original_world_used_for_graph_inference":false,"sorted_ram":true,"exact_state_future_work_counts":true,"samples":samples
            }))
            .unwrap(),
        )
        .unwrap();
    }
}
