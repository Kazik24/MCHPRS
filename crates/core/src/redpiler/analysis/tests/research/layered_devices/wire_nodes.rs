//! Paired O5 measurement from identical prepared graphs, with strict native checks.
use super::*;
use crate::redpiler::passes::planning::PlanBuilder;
use std::time::Instant;

#[test]
#[ignore = "explicit paired BubbleSort/FPU instant-wire experiment and native checkpoints"]
fn instant_wire_nodes_cpu_fpu_paired_measurement() {
    paired_measurement(Experiment::WireNodes);
}

#[test]
#[ignore = "explicit five-pair O5 capture-dispatch benchmark with native checkpoints"]
fn wire_captures_cpu_fpu_paired_measurement() {
    paired_measurement(Experiment::Captures);
}

#[test]
#[ignore = "explicit five-pair FIFO-kernel CPU/FPU timing and native continuation"]
fn instant_events_cpu_fpu_paired_measurement() {
    paired_measurement(Experiment::Events);
}

enum Experiment {
    WireNodes,
    Captures,
    Events,
}

fn paired_measurement(experiment: Experiment) {
    let captures = matches!(experiment, Experiment::Captures);
    let events = matches!(experiment, Experiment::Events);
    for name in ["cpu_bubblesort", "fpu"] {
        let load_world = || {
            if name == "cpu_bubblesort" {
                let (world, bounds, _) = large_builds::load_cpu(cpus::CPUS[2]);
                (world, bounds)
            } else {
                load(&manifest("fpu_legal"))
            }
        };
        let (mut native, bounds) = load_world();
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
        let mut pipeline = passes::default_graph_pipeline(&options);
        pipeline
            .entries
            .retain(|entry| !matches!(&entry.pass, passes::Pass::Build(builder) if matches!(builder.name(), "experimental-compile-instant-wire-nodes" | "experimental-compile-instant-events" | "experimental-compile-bud-memory-reads" | "experimental-minimize-wire-captures") || captures && builder.name() == "compile-wire-captures"));
        for entry in &mut pipeline.entries {
            if matches!(&entry.pass, passes::Pass::Build(builder) if builder.name() == "compile-instant-layers") {
                entry.enabled = true;
            }
        }
        passes::run_graph_pipeline(&mut prepared.graph, &options, &prepared.entry, &pipeline, &monitor).unwrap();
        let mut candidate = prepared.graph.clone();
        let context = passes::PassContext::new(&monitor, &options, &prepared.entry);
        let start = Instant::now();
        if events {
            passes::minimize_wire_captures::MinimizeWireCaptures
                .run(&mut candidate, &context)
                .unwrap();
            passes::compile_bud_memory_reads::CompileBudMemoryReads
                .run(&mut candidate, &context)
                .unwrap();
            passes::compile_instant_events::CompileInstantEvents
                .run(&mut candidate, &context)
                .unwrap();
        } else if captures {
            passes::compile_wire_captures::CompileWireCaptures.run(&mut candidate, &context).unwrap();
        } else {
            passes::compile_instant_wire_nodes::CompileInstantWireNodes
                .run(&mut candidate, &context)
                .unwrap();
        }
        let builder_seconds = start.elapsed().as_secs_f64();
        let accepted = if events {
            candidate.plans.instant_events.iter().map(|kernel| kernel.actors.len()).sum()
        } else if captures {
            candidate.plans.wire_captures.len()
        } else {
            candidate.wires.iter().filter(|wire| wire.execution.instant_node.is_some()).count()
        };
        let wires = candidate.wires.len();
        let kernel_groups: Vec<_> = candidate.plans.instant_events.iter().map(|kernel| kernel.actors.len()).collect();
        let memory_reads = candidate.plans.bud_memory_reads.len();
        let capture_kinds = candidate
            .plans
            .wire_captures
            .iter()
            .fold(std::collections::BTreeMap::new(), |mut counts, capture| {
                *counts.entry(capture.kind()).or_insert(0usize) += 1;
                counts
            });
        println!(
            "{name}: {accepted} plan entries in {builder_seconds:.6}s; {} event kernels, {memory_reads} BUD reads",
            kernel_groups.len()
        );
        let baseline = prepared.lower(&monitor).unwrap();
        let (program, state) = candidate.lower_with_monitor(&monitor).unwrap();
        let candidate = crate::redpiler::program_graph::LoweredCircuit {
            program,
            state,
            positions: baseline.positions.clone(),
            logical_tick: baseline.logical_tick,
            imported_ticks: baseline.imported_ticks.clone(),
        };
        let copy = |lowered: &crate::redpiler::program_graph::LoweredCircuit| crate::redpiler::program_graph::LoweredCircuit {
            program: lowered.program.clone(),
            state: lowered.state.clone(),
            positions: lowered.positions.clone(),
            logical_tick: lowered.logical_tick,
            imported_ticks: lowered.imported_ticks.clone(),
        };
        let trigger = if name == "cpu_bubblesort" {
            cpus::CPUS[2].origin + cpus::CPUS[2].start
        } else {
            local_pos(&manifest("fpu_legal")["ports"]["trigger"]) - BASE + origin(&manifest("fpu_legal"))
        };
        let mut samples = Vec::new();
        for pair in 0..if captures || events { 5 } else { 3 } {
            let mut results = Vec::new();
            for enabled in if pair % 2 == 0 { [false, true] } else { [true, false] } {
                let (mut world, _) = load_world();
                let mut backend = GraphBackend::from_lowered(copy(if enabled { &candidate } else { &baseline }), &monitor).unwrap();
                backend.set_test_recording(false);
                let mut seconds = 0.0;
                let ticks = if name == "cpu_bubblesort" { 1000 } else { 256 };
                let start = Instant::now();
                backend.use_block(trigger).unwrap();
                seconds += start.elapsed().as_secs_f64();
                for tick in 0..ticks {
                    if name == "fpu" && tick != 0 && tick % 64 == 0 {
                        let start = Instant::now();
                        backend.use_block(trigger).unwrap();
                        seconds += start.elapsed().as_secs_f64();
                    }
                    let start = Instant::now();
                    backend.tick_with_world(&mut world).unwrap();
                    seconds += start.elapsed().as_secs_f64();
                }
                results.push((enabled, seconds, backend));
            }
            results.sort_by_key(|row| row.0);
            let general = &results[0].2;
            let optimized = &results[1].2;
            if events {
                general.assert_same_execution_state(optimized);
            }
            let (mut left, _) = load_world();
            let (mut right, _) = load_world();
            project(general, &mut left);
            project(optimized, &mut right);
            assert_future_work(&left, &right, name);
            pc_counter_contract::compare_checkpoint(&left, &right, bounds, name);
            if captures {
                assert_eq!(
                    general.execution_counts(),
                    optimized.execution_counts(),
                    "capture compilation preserves work counts"
                );
            }
            if events {
                let mut baseline = general.execution_counts();
                let mut enabled = optimized.execution_counts();
                baseline.instant_kernel_invocations = 0;
                enabled.instant_kernel_invocations = 0;
                baseline.callbacks = 0;
                enabled.callbacks = 0;
                assert_eq!(baseline, enabled, "event kernels preserve wire walks, actions and pending publications");
            }
            println!("{name} pair {}: {:.6}s -> {:.6}s", pair + 1, results[0].1, results[1].1);
            samples.push(json!({"pair":pair+1,"baseline_seconds":results[0].1,"enabled_seconds":results[1].1,
                "baseline_counts":format!("{:?}",general.execution_counts()),"enabled_counts":format!("{:?}",optimized.execution_counts())}));
        }
        // Independent native execution is separate from timing, including every future boundary.
        let mut backend = GraphBackend::from_lowered(copy(&candidate), &monitor).unwrap();
        backend.set_test_recording(false);
        let (mut projected, _) = load_world();
        native.disable_command_output_limits_for_replay();
        if name == "cpu_bubblesort" {
            cpus::click_cpu(&mut native, cpus::CPUS[2], cpus::CPUS[2].start);
        } else {
            native_action(&mut native, trigger);
        }
        backend.use_block(trigger).unwrap();
        let native_ticks = if name == "cpu_bubblesort" { 100 } else { 256 };
        for tick in 0..native_ticks {
            if name == "fpu" && tick != 0 && tick % 64 == 0 {
                native_action(&mut native, trigger);
                backend.use_block(trigger).unwrap();
            }
            native.tick_interpreted();
            backend.tick_with_world(&mut projected).unwrap();
            project(&backend, &mut projected);
            assert_future_work(&native, &projected, name);
            if [0, 31, 63, native_ticks - 1].contains(&tick) {
                pc_counter_contract::compare_checkpoint(&native, &projected, bounds, name);
            }
        }
        check_handoff(&mut native, &mut backend, &mut projected, bounds, name);
        let report = json!({"name":name,"tier":5,"pass":if events {"experimental-instant-events+bud-memory-reads"} else if captures {"compile-wire-captures"} else {"experimental-compile-instant-wire-nodes"},
            "plan_entries":accepted,"total_wires":wires,"site_capture_kinds":capture_kinds,
            "event_kernel_actor_groups":kernel_groups,"bud_memory_reads":memory_reads,
            "capture_table_bytes":if captures {accepted * std::mem::size_of::<WireCapture>()} else {0},
            "builder_seconds":builder_seconds,"samples":samples,"native_ticks":native_ticks,"handoff_ticks":16,
            "full_sort_verified":false,"fpu_numeric_result_verified":false});
        let directory = root().join(if events {
            "target/instant-event-kernels"
        } else if captures {
            "target/wire-captures"
        } else {
            "target/instant-wire-nodes"
        });
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join(format!("{name}-paired.json")), serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        println!("{name}: {accepted} plan entries, builder {builder_seconds:.6}s; {samples:?}");
    }
}

fn native_action(world: &mut PlotWorld, position: BlockPos) {
    let Block::Lever { mut lever } = world.get_block(position) else {
        panic!("missing lever")
    };
    lever.powered = !lever.powered;
    lever_action(world, position, lever.powered);
}
