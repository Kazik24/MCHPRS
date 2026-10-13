//! Deterministic end-to-end interpreter benchmarks, with frozen-state assertions.
//! Run once by default; CPU initialization and verification are excluded from timing.
#[path = "support/cpus.rs"]
mod support;
use mchprs_blocks::BlockPos;
use mchprs_blocks::blocks::Block;
use mchprs_core::plot::PlotWorld;
#[cfg(feature = "graph-profile")]
use mchprs_core::redpiler::GraphExecutionProfile;
use mchprs_core::redpiler::{
    BackendKind, Compiler, CompilerOptions, GraphExecutionCounts, TaskMonitor,
};
use mchprs_core::world::World;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use support::*;

const SHORT_ACTIVE_TICKS: u32 = 100;
const TEST_GRAPH_COUNTS: GraphExecutionCounts = GraphExecutionCounts {
    ticks: 100,
    scheduled_ticks: 20_005,
    callbacks: 2_059_909,
    wire_walks: 32_505,
    dust_path_power_samples_elided: 0,
    piston_samples: 302_784,
    piston_actions: 27_884,
    deferred_publications: 27_884,
};

#[cfg(feature = "graph-profile")]
fn print_graph_profile(profile: GraphExecutionProfile) {
    assert_eq!(profile.tick_phases.calls, u64::from(SHORT_ACTIVE_TICKS));
    assert_eq!(
        profile.scheduled_drain.calls,
        u64::from(SHORT_ACTIVE_TICKS) * 2
    );
    assert_eq!(profile.piston_actions.calls, u64::from(SHORT_ACTIVE_TICKS));
    assert_eq!(
        profile.deferred_connections.calls,
        u64::from(SHORT_ACTIVE_TICKS)
    );
    assert!(profile.route_delivery.calls > 0);
    assert_eq!(profile.turbo_wire_walk.calls, 32_505);
    assert!(profile.piston_sample.calls > 0);
    println!("Graph operation timings (inclusive; nested buckets overlap):");
    for (name, metric) in [
        ("tick phases", profile.tick_phases),
        ("scheduled drain", profile.scheduled_drain),
        ("piston actions", profile.piston_actions),
        ("deferred connections", profile.deferred_connections),
        ("route delivery", profile.route_delivery),
        ("Turbo wire walk", profile.turbo_wire_walk),
        ("piston sample", profile.piston_sample),
    ] {
        let total_ms = metric.elapsed_ns as f64 / 1_000_000.0;
        let average_us = if metric.calls == 0 {
            0.0
        } else {
            metric.elapsed_ns as f64 / metric.calls as f64 / 1_000.0
        };
        println!(
            "  {name}: {total_ms:.3} ms / {} calls ({average_us:.3} us per call)",
            metric.calls
        );
    }
}

struct ShortSample {
    initial: Checkpoint,
    final_ram: Vec<Option<u16>>,
    elapsed: Duration,
    graph_counts: Option<GraphExecutionCounts>,
    /// This records the lowered table sizes produced by the selected pipeline.
    graph_tables: Option<GraphTableCounts>,
    /// This records candidate coverage and decline reasons for the experimental pass.
    dust_path_tables: Vec<(String, usize)>,
    /// This retains the parser's estimated table bytes after callback construction.
    retained_callback_bytes: usize,
    /// This retains the lowered program's estimated table bytes.
    retained_lowering_bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
struct GraphTableCounts {
    /// This counts runtime guard expressions after lowering.
    guards: usize,
    /// This counts ordered route deliveries retained after lowering.
    deliveries: usize,
    /// This counts callback links retained in the post-pass draft.
    callback_links: usize,
    /// This counts power queries retained after lowering.
    power_links: usize,
}

fn sign_free_world(cpu: Cpu) -> PlotWorld {
    let mut world = load_cpu(cpu);
    let sign = cpu.origin + BlockPos::new(186, 2, 184);
    assert!(matches!(world.get_block(sign), Block::WallSign { .. }));
    assert!(world.set_block(sign, Block::Air));
    world
}

fn current_ram(world: &PlotWorld, cpu: Cpu) -> Vec<Option<u16>> {
    let mut trace = Vec::new();
    collect_ram(world, cpu, SHORT_ACTIVE_TICKS, &mut trace);
    trace.pop().expect("BubbleSort RAM should be present").1
}

fn interpreter_sample(cpu: Cpu) -> ShortSample {
    let mut world = sign_free_world(cpu);
    let initial = checkpoint(&world, 0, &[]);
    click_cpu(&mut world, cpu, cpu.start);
    let started = Instant::now();
    for _ in 0..SHORT_ACTIVE_TICKS {
        world.tick_interpreted();
    }
    let elapsed = started.elapsed();
    ShortSample {
        initial,
        final_ram: current_ram(&world, cpu),
        elapsed,
        graph_counts: None,
        graph_tables: None,
        dust_path_tables: vec![],
        retained_callback_bytes: 0,
        retained_lowering_bytes: 0,
    }
}

fn graph_sample(
    cpu: Cpu,
    bounds: (BlockPos, BlockPos),
    optimize: bool,
    dust_paths: bool,
) -> ShortSample {
    let mut world = sign_free_world(cpu);
    let initial = checkpoint(&world, 0, &[]);
    let ticks = world.scheduler().iter_entries().collect();
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &world,
            bounds,
            CompilerOptions {
                optimization_level: if optimize { None } else { Some(0) },
                experimental_dust_paths: dust_paths,
                ..Default::default()
            },
            ticks,
            Arc::new(TaskMonitor::default()),
        )
        .expect("CPU Graph compilation must succeed");
    assert_eq!(compiler.backend_kind(), Some(BackendKind::Graph));
    assert_eq!(compiler.stats().unwrap().backend, BackendKind::Graph);
    let statistics = compiler.stats().unwrap();
    let table_entries = |stage: &str, name: &str| {
        statistics
            .graph
            .preparation_tables
            .iter()
            .find(|table| table.stage == stage && table.table == name)
            .unwrap_or_else(|| panic!("missing {stage} {name} statistics"))
            .entries
    };
    let graph_tables = GraphTableCounts {
        guards: table_entries("lowering", "guards"),
        deliveries: table_entries("lowering", "deliveries"),
        callback_links: table_entries("lowering input", "callback links"),
        power_links: table_entries("lowering", "power links"),
    };
    let dust_path_tables: Vec<(String, usize)> = statistics
        .graph
        .preparation_tables
        .iter()
        .filter(|table| table.stage == "dust contraction")
        .map(|table| (table.table.to_owned(), table.entries))
        .collect();
    assert_eq!(dust_path_tables.is_empty(), !dust_paths);
    let retained_bytes = |stage| {
        statistics
            .graph
            .preparation_tables
            .iter()
            .filter(|table| table.stage == stage)
            .map(|table| table.estimated_bytes)
            .sum()
    };
    let retained_callback_bytes = retained_bytes("callbacks");
    let retained_lowering_bytes = retained_bytes("lowering");

    compiler.on_use_block(cpu.origin + cpu.start);
    let started = Instant::now();
    for _ in 0..SHORT_ACTIVE_TICKS {
        compiler.tick_with_world(&mut world).unwrap();
    }
    let elapsed = started.elapsed();
    #[cfg(feature = "graph-profile")]
    print_graph_profile(
        compiler
            .graph_execution_profile()
            .expect("Graph profile must be available in the profiling build"),
    );
    let graph_counts = compiler
        .graph_execution_counts()
        .expect("Graph backend must report work counters");
    compiler.flush(&mut world);
    ShortSample {
        initial,
        final_ram: current_ram(&world, cpu),
        elapsed,
        graph_counts: Some(graph_counts),
        graph_tables: Some(graph_tables),
        dust_path_tables,
        retained_callback_bytes,
        retained_lowering_bytes,
    }
}

fn short_bounds(cpu: Cpu) -> (BlockPos, BlockPos) {
    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test_data/piston-research/fixtures/cpu_bubblesort.json");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(manifest_path).unwrap()).unwrap();
    let dimensions = manifest["dimensions"].as_array().unwrap();
    let size = BlockPos::new(
        dimensions[0].as_i64().unwrap() as i32,
        dimensions[1].as_i64().unwrap() as i32,
        dimensions[2].as_i64().unwrap() as i32,
    );
    (cpu.origin, cpu.origin + size - BlockPos::new(1, 1, 1))
}

fn graph_short(cpu: Cpu, iterations: usize, output: Option<&str>, label: &str) {
    assert_eq!(cpu.name, "cpu_bubblesort");
    let bounds = short_bounds(cpu);
    let mut graph_times = Vec::with_capacity(iterations);
    let mut opt01_times = Vec::with_capacity(iterations);
    let mut dust_times = Vec::with_capacity(iterations);
    let mut interpreter_times = Vec::with_capacity(iterations);
    let mut graph_counts = None;
    let mut opt01_counts = None;
    let mut dust_counts = None;
    let mut graph_tables = None;
    let mut opt01_tables = None;
    let mut dust_graph_tables = None;
    let mut dust_path_tables = None;
    let mut retained_estimates = None;

    for sample in 0..iterations {
        let (graph, opt01, dust, interpreter) = if sample % 2 == 0 {
            let graph = graph_sample(cpu, bounds, false, false);
            let opt01 = graph_sample(cpu, bounds, true, false);
            let dust = graph_sample(cpu, bounds, true, true);
            let interpreter = interpreter_sample(cpu);
            (graph, opt01, dust, interpreter)
        } else {
            let interpreter = interpreter_sample(cpu);
            let dust = graph_sample(cpu, bounds, true, true);
            let opt01 = graph_sample(cpu, bounds, true, false);
            let graph = graph_sample(cpu, bounds, false, false);
            (graph, opt01, dust, interpreter)
        };
        assert_eq!(graph.initial, interpreter.initial, "sample {sample} inputs");
        assert_eq!(
            opt01.initial, interpreter.initial,
            "sample {sample} O1 inputs"
        );
        assert_eq!(
            dust.initial, interpreter.initial,
            "sample {sample} O1+dust inputs"
        );
        assert_eq!(
            graph.final_ram, interpreter.final_ram,
            "sample {sample} RAM"
        );
        assert_eq!(
            opt01.final_ram, interpreter.final_ram,
            "sample {sample} O1 RAM"
        );
        assert_eq!(
            dust.final_ram, interpreter.final_ram,
            "sample {sample} O1+dust RAM"
        );
        let counts = graph.graph_counts.unwrap();
        assert_eq!(counts, TEST_GRAPH_COUNTS, "sample {sample} Graph work");
        if let Some(previous) = graph_counts {
            assert_eq!(counts, previous, "Graph work changed between samples");
        }
        graph_counts = Some(counts);
        let opt01_counts_sample = opt01.graph_counts.unwrap();
        assert_eq!(opt01_counts_sample.ticks, TEST_GRAPH_COUNTS.ticks);
        assert_eq!(
            opt01_counts_sample.scheduled_ticks,
            TEST_GRAPH_COUNTS.scheduled_ticks
        );
        assert_eq!(opt01_counts_sample.wire_walks, TEST_GRAPH_COUNTS.wire_walks);
        assert_eq!(
            opt01_counts_sample.piston_samples,
            TEST_GRAPH_COUNTS.piston_samples
        );
        assert_eq!(
            opt01_counts_sample.piston_actions,
            TEST_GRAPH_COUNTS.piston_actions
        );
        assert_eq!(
            opt01_counts_sample.deferred_publications,
            TEST_GRAPH_COUNTS.deferred_publications
        );
        assert_eq!(opt01_counts_sample.dust_path_power_samples_elided, 0);
        assert!(
            opt01_counts_sample.callbacks < counts.callbacks,
            "O1 must remove the callbacks covered by opt-01"
        );
        if let Some(previous) = opt01_counts {
            assert_eq!(
                opt01_counts_sample, previous,
                "O1 Graph work changed between samples"
            );
        }
        opt01_counts = Some(opt01_counts_sample);

        let dust_counts_sample = dust.graph_counts.unwrap();
        let mut opt01_without_dust = opt01_counts_sample;
        opt01_without_dust.dust_path_power_samples_elided = 0;
        let mut dust_without_counter = dust_counts_sample;
        dust_without_counter.dust_path_power_samples_elided = 0;
        assert_eq!(
            dust_without_counter, opt01_without_dust,
            "dust changed unrelated Graph work"
        );
        if let Some(previous) = dust_counts {
            assert_eq!(
                dust_counts_sample, previous,
                "O1+dust Graph work changed between samples"
            );
        }
        dust_counts = Some(dust_counts_sample);

        let plain_tables = graph.graph_tables.unwrap();
        let opt01_tables_sample = opt01.graph_tables.unwrap();
        let dust_tables_sample = dust.graph_tables.unwrap();
        assert!(opt01_tables_sample.guards <= plain_tables.guards);
        assert!(opt01_tables_sample.deliveries <= plain_tables.deliveries);
        assert!(opt01_tables_sample.callback_links <= plain_tables.callback_links);
        assert!(opt01_tables_sample.power_links <= plain_tables.power_links);
        assert_eq!(
            dust_tables_sample, opt01_tables_sample,
            "dust pass changed graph tables"
        );
        if let Some(previous) = graph_tables {
            assert_eq!(plain_tables, previous, "unoptimized table counts changed");
        }
        if let Some(previous) = opt01_tables {
            assert_eq!(opt01_tables_sample, previous, "O1 table counts changed");
        }
        if let Some(previous) = dust_graph_tables {
            assert_eq!(dust_tables_sample, previous, "O1+dust table counts changed");
        }
        graph_tables = Some(plain_tables);
        opt01_tables = Some(opt01_tables_sample);
        dust_graph_tables = Some(dust_tables_sample);

        let dust_path_tables_sample = dust.dust_path_tables.clone();
        if let Some(previous) = &dust_path_tables {
            assert_eq!(
                &dust_path_tables_sample, previous,
                "dust candidate coverage changed"
            );
        }
        dust_path_tables = Some(dust_path_tables_sample);

        let estimates = (
            graph.retained_callback_bytes,
            graph.retained_lowering_bytes,
            opt01.retained_callback_bytes,
            opt01.retained_lowering_bytes,
            dust.retained_callback_bytes,
            dust.retained_lowering_bytes,
        );
        if let Some(previous) = retained_estimates {
            assert_eq!(estimates, previous, "retained table estimates changed");
        }
        retained_estimates = Some(estimates);
        println!(
            "cpu_bubblesort sample {}: Graph {:.6}s, O1 {:.6}s, O1+dust {:.6}s, interpreter {:.6}s; {} active ticks; RAM matches; callbacks {} -> {}; dust samples elided {}",
            sample + 1,
            graph.elapsed.as_secs_f64(),
            opt01.elapsed.as_secs_f64(),
            dust.elapsed.as_secs_f64(),
            interpreter.elapsed.as_secs_f64(),
            SHORT_ACTIVE_TICKS,
            counts.callbacks,
            opt01_counts_sample.callbacks,
            dust_counts_sample.dust_path_power_samples_elided,
        );
        graph_times.push(graph.elapsed);
        opt01_times.push(opt01.elapsed);
        dust_times.push(dust.elapsed);
        interpreter_times.push(interpreter.elapsed);
    }

    let summarize = |times: &mut Vec<Duration>| {
        times.sort_unstable();
        (times[times.len() / 2], times[0], times[times.len() - 1])
    };
    let (graph_median, graph_min, graph_max) = summarize(&mut graph_times);
    let (opt01_median, opt01_min, opt01_max) = summarize(&mut opt01_times);
    let (dust_median, dust_min, dust_max) = summarize(&mut dust_times);
    let (interpreter_median, interpreter_min, interpreter_max) = summarize(&mut interpreter_times);
    let counts = graph_counts.unwrap();
    let opt01_counts = opt01_counts.unwrap();
    let dust_counts = dust_counts.unwrap();
    println!(
        "Graph backend: {:?}; {} paired samples of {} ticks; Graph median {:.6}s [{:.6}, {:.6}], O1 median {:.6}s [{:.6}, {:.6}], O1+dust median {:.6}s [{:.6}, {:.6}], interpreter median {:.6}s [{:.6}, {:.6}]",
        BackendKind::Graph,
        iterations,
        SHORT_ACTIVE_TICKS,
        graph_median.as_secs_f64(),
        graph_min.as_secs_f64(),
        graph_max.as_secs_f64(),
        opt01_median.as_secs_f64(),
        opt01_min.as_secs_f64(),
        opt01_max.as_secs_f64(),
        dust_median.as_secs_f64(),
        dust_min.as_secs_f64(),
        dust_max.as_secs_f64(),
        interpreter_median.as_secs_f64(),
        interpreter_min.as_secs_f64(),
        interpreter_max.as_secs_f64()
    );
    println!("Unoptimized Graph work counters: {counts:?}");
    println!("O1 Graph work counters: {opt01_counts:?}");
    println!("O1+dust Graph work counters: {dust_counts:?}");
    println!(
        "Post-pass graph tables: general={:?}; O1={:?}; O1+dust={:?}",
        graph_tables.unwrap(),
        opt01_tables.unwrap(),
        dust_graph_tables.unwrap()
    );
    let (
        callback_bytes,
        lowering_bytes,
        opt01_callback_bytes,
        opt01_lowering_bytes,
        dust_callback_bytes,
        dust_lowering_bytes,
    ) = retained_estimates.unwrap();
    println!(
        "Estimated retained tables: general callbacks/lowering {:.2}/{:.2} MiB; O1 {:.2}/{:.2} MiB; O1+dust {:.2}/{:.2} MiB",
        callback_bytes as f64 / (1024.0 * 1024.0),
        lowering_bytes as f64 / (1024.0 * 1024.0),
        opt01_callback_bytes as f64 / (1024.0 * 1024.0),
        opt01_lowering_bytes as f64 / (1024.0 * 1024.0),
        dust_callback_bytes as f64 / (1024.0 * 1024.0),
        dust_lowering_bytes as f64 / (1024.0 * 1024.0),
    );
    let coverage = dust_path_tables.unwrap();
    println!("O1+dust preparation coverage tables: {coverage:?}");

    if let Some(output) = output {
        let report = serde_json::json!({
            "schema": 2,
            "label": label,
            "profile": "bench/release",
            "scenario": "cpu_bubblesort_sign_free_first_100_active_ticks",
            "schematic_sha256": cpu.sha256,
            "sign_removed_from_both_inputs_at": [188, 10, 186],
            "graph_backend": format!("{:?}", BackendKind::Graph),
            "iterations": iterations,
            "active_ticks": SHORT_ACTIVE_TICKS,
            "general_graph": {
                "median_seconds": graph_median.as_secs_f64(),
                "range_seconds": [graph_min.as_secs_f64(), graph_max.as_secs_f64()],
                "counters": graph_counts_json(counts),
                "tables": graph_tables.unwrap(),
                "estimated_retained_bytes_after_callbacks": callback_bytes,
                "estimated_retained_bytes_after_lowering": lowering_bytes,
            },
            "o1_only": {
                "median_seconds": opt01_median.as_secs_f64(),
                "range_seconds": [opt01_min.as_secs_f64(), opt01_max.as_secs_f64()],
                "counters": graph_counts_json(opt01_counts),
                "tables": opt01_tables.unwrap(),
                "estimated_retained_bytes_after_callbacks": opt01_callback_bytes,
                "estimated_retained_bytes_after_lowering": opt01_lowering_bytes,
            },
            "o1_plus_dust": {
                "median_seconds": dust_median.as_secs_f64(),
                "range_seconds": [dust_min.as_secs_f64(), dust_max.as_secs_f64()],
                "counters": graph_counts_json(dust_counts),
                "tables": dust_graph_tables.unwrap(),
                "candidate_coverage_tables": coverage,
                "estimated_retained_bytes_after_callbacks": dust_callback_bytes,
                "estimated_retained_bytes_after_lowering": dust_lowering_bytes,
            },
            "interpreter": {
                "median_seconds": interpreter_median.as_secs_f64(),
                "range_seconds": [interpreter_min.as_secs_f64(), interpreter_max.as_secs_f64()],
            },
            "general_counters_match_cfg_test_episode": counts == TEST_GRAPH_COUNTS,
            "ram_matches_interpreter_each_sample": true,
            "timed_operations": "100 calls to tick_with_world or tick_interpreted; compile, input setup, flush, RAM reads, and counter checks excluded",
        });
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(output);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
}

fn graph_counts_json(counts: GraphExecutionCounts) -> serde_json::Value {
    serde_json::json!({
        "ticks": counts.ticks,
        "scheduled_ticks": counts.scheduled_ticks,
        "callbacks": counts.callbacks,
        "wire_walks": counts.wire_walks,
        "dust_path_power_samples_elided": counts.dust_path_power_samples_elided,
        "piston_samples": counts.piston_samples,
        "piston_actions": counts.piston_actions,
        "deferred_publications": counts.deferred_publications,
    })
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let short_graph = args.iter().any(|arg| arg == "--graph-short");
    let iterations = args
        .windows(2)
        .find(|pair| pair[0] == "--iterations")
        .map_or(if short_graph { 5 } else { 1 }, |pair| {
            pair[1]
                .parse::<usize>()
                .expect("--iterations needs a positive integer")
        });
    assert!(iterations > 0);
    let selected = args
        .windows(2)
        .find(|pair| pair[0] == "--cpu")
        .map(|pair| pair[1].as_str());
    let output = args
        .windows(2)
        .find(|pair| pair[0] == "--output")
        .map(|pair| &pair[1]);
    let label = args
        .windows(2)
        .find(|pair| pair[0] == "--label")
        .map_or("current", |pair| pair[1].as_str());
    let mut reports = Vec::new();
    let screen_only = args.iter().any(|arg| arg == "--screen-only");
    let flush_every = args
        .windows(2)
        .find(|pair| pair[0] == "--flush-every")
        .map_or(0, |pair| {
            pair[1]
                .parse::<u32>()
                .expect("--flush-every needs a tick interval")
        });
    if let Some(name) = selected {
        assert!(
            CPUS.iter().any(|cpu| cpu.name == name),
            "unknown CPU {name}"
        );
    }
    if short_graph {
        assert!(
            selected.is_none_or(|name| name == "cpu_bubblesort"),
            "--graph-short only supports cpu_bubblesort"
        );
        graph_short(
            CPUS.iter()
                .copied()
                .find(|cpu| cpu.name == "cpu_bubblesort")
                .unwrap(),
            iterations,
            output.map(String::as_str),
            label,
        );
        return;
    }
    for cpu in CPUS {
        if selected.is_some_and(|name| name != cpu.name) {
            continue;
        }
        let expected = reference(cpu);
        let mut times = Vec::new();
        let mut active_times = Vec::new();
        let mut active_ticks = 0;
        let mut visual_counts = Vec::new();
        let mut instant_cache_stats = Vec::new();
        for sample in 1..=iterations {
            let timing = replay_with_visuals(cpu, &expected, screen_only, flush_every);
            println!(
                "{} sample {sample}: {:.6}s / 50,000 game ticks ({:.1} TPS); active window: {} ticks / {:.6}s ({:.1} TPS); all assertions passed",
                cpu.name,
                timing.total.as_secs_f64(),
                50_000.0 / timing.total.as_secs_f64(),
                timing.active_ticks,
                timing.active.as_secs_f64(),
                f64::from(timing.active_ticks) / timing.active.as_secs_f64()
            );
            times.push(timing.total);
            active_times.push(timing.active);
            active_ticks = timing.active_ticks;
            visual_counts.push(timing.visual_counts);
            instant_cache_stats.push(timing.instant_piston_cache_stats);
            if flush_every != 0 {
                println!(
                    "visual totals: {} flushes, {} sections, {} block records",
                    timing.visual_counts.0, timing.visual_counts.1, timing.visual_counts.2
                );
            }
        }
        times.sort();
        active_times.sort();
        println!(
            "{} median: {:.6}s",
            cpu.name,
            times[times.len() / 2].as_secs_f64()
        );
        reports.push(serde_json::json!({
            "cpu":cpu.name,"schematic_sha256":cpu.sha256,"game_ticks":50_000,
            "samples_seconds":times.iter().map(|time| time.as_secs_f64()).collect::<Vec<_>>(),
            "median_seconds":times[times.len()/2].as_secs_f64(),
            "median_tps":50_000.0/times[times.len()/2].as_secs_f64(),
            "active_window_game_ticks":active_ticks,
            "active_samples_seconds":active_times.iter().map(|time| time.as_secs_f64()).collect::<Vec<_>>(),
            "active_median_seconds":active_times[active_times.len()/2].as_secs_f64(),
            "active_median_tps":f64::from(active_ticks)/active_times[active_times.len()/2].as_secs_f64(),
            "assertions":"whole-world checkpoints, ordered chat, per-tick Pong screen and BubbleSort RAM, BubbleSort halt boundary",
            "visual_mode":if screen_only { "screen" } else { "all" },
            "visual_flush_every_game_ticks":flush_every,
            "visual_counts":visual_counts,
            "instant_piston_cache_stats":instant_cache_stats,
        }));
    }
    if let Some(path) = output {
        let report = serde_json::json!({"schema":1,"label":label,"profile":"bench/release","samples":reports});
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
}
