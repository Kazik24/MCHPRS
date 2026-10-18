//! Paired Rust Graph and optional generated C execution on the real BubbleSort schematic.
#![allow(dead_code)]
#[path = "support/cpus.rs"]
mod support;

use anyhow::{Context, Result, bail, ensure};
use mchprs_blocks::{BlockPos, blocks::Block};
use mchprs_core::{
    plot::PlotWorld,
    redpiler::{Compiler, CompilerOptions},
    world::World,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

struct Run {
    report: Value,
    active_ram: Vec<Vec<Option<u16>>>,
    continuation_ram: Vec<Vec<Option<u16>>>,
}

fn main() -> Result<()> {
    let mut iterations = 3usize;
    let mut ticks = None;
    let mut native_demo = false;
    let mut whole_engine = false;
    let mut graph_tier = None;
    let mut classic_redstone = false;
    let mut verify_native = false;
    let mut clang = std::env::var_os("MCHPRS_NATIVE_CLANG").map(PathBuf::from);
    let mut linker = std::env::var_os("MCHPRS_NATIVE_LINKER").map(PathBuf::from);
    let mut output = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/native-query-demo/cpu-bubblesort.json");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bench" => continue,
            "--native-demo" => native_demo = true,
            "--native-engine" => {
                native_demo = true;
                whole_engine = true;
            }
            "--verify-native" => verify_native = true,
            "--classic-redstone" => classic_redstone = true,
            "--help" => {
                println!(
                    "cpu_bubblesort [--native-demo | --native-engine] [--verify-native] [--graph-tier 0..3] [--classic-redstone] [--iterations N] [--ticks N] [--clang path] [--linker path] [--output path]\n--native-demo pairs O3 Graph with generated C queries. --native-engine pairs the complete C executor with Graph (default O3, retaining inferred operations). --verify-native adds a separate validation run: query-by-query comparison or complete state/queue/effect comparison after every input and tick. Windows needs Clang and lld-link; set MCHPRS_NATIVE_CLANG and MCHPRS_NATIVE_LINKER or pass paths. Default ticks run through the 10,035-tick sort plus one settling tick."
                );
                return Ok(());
            }
            _ => {
                let value = args.next().with_context(|| format!("{arg} requires a value"))?;
                match arg.as_str() {
                    "--iterations" => iterations = value.parse()?,
                    "--graph-tier" => graph_tier = Some(value.parse::<usize>()?),
                    "--ticks" => ticks = Some(value.parse::<u32>()?),
                    "--clang" => clang = Some(value.into()),
                    "--linker" => linker = Some(value.into()),
                    "--output" => output = value.into(),
                    _ => bail!("unknown argument {arg}"),
                }
            }
        }
    }
    ensure!(iterations > 0, "--iterations must be positive");
    ensure!(!verify_native || native_demo, "--verify-native requires --native-demo");
    let graph_tier = graph_tier.unwrap_or(3);
    ensure!(graph_tier <= 3, "--graph-tier must be in 0..=3");
    ensure!(!classic_redstone || graph_tier >= 2, "--classic-redstone requires O2 or O3");
    #[cfg(not(feature = "native-demo"))]
    ensure!(!native_demo, "rebuild with --features native-demo to run the C query experiment");
    #[cfg(feature = "native-demo")]
    if native_demo {
        ensure!(clang.is_some(), "pass --clang or set MCHPRS_NATIVE_CLANG");
        #[cfg(windows)]
        ensure!(linker.is_some(), "pass --linker or set MCHPRS_NATIVE_LINKER");
    }

    let cpu = support::CPUS.iter().copied().find(|cpu| cpu.name == "cpu_bubblesort").unwrap();
    let ticks = ticks.unwrap_or(cpu.active_ticks + 1);
    ensure!(ticks > 0 && ticks <= 50_000, "--ticks must be in 1..=50000");
    let reference_ram = frozen_ram_trace(cpu, ticks)?;
    let verification = if verify_native {
        let started = Instant::now();
        let checked = execute(
            cpu,
            ticks,
            &reference_ram,
            true,
            true,
            whole_engine,
            graph_tier,
            classic_redstone,
            clang.as_deref(),
            linker.as_deref(),
        )?;
        let elapsed = started.elapsed().as_secs_f64();
        let native = &checked.report["native_query"];
        ensure!(
            whole_engine || native["verified_queries"] == native["hits"],
            "not every native query was checked against Graph"
        );
        json!({"kind":if whole_engine { "full_state_future_effects" } else { "power_queries" },"checked_ticks":ticks,
            "native_engine":checked.report["native_engine"],"elapsed_seconds":elapsed,"tick_seconds_with_verification":checked.report["tick_seconds"],
            "hits":native["hits"],"empty_root_hits":native["empty_root_hits"],"fallbacks":native["fallbacks"],"checks":native["verified_queries"],
            "active_ram_sha256":checked.report["active_ram_sha256"],"reset_checkpoint":checked.report["reset_checkpoint"],
            "continuation_checkpoint":checked.report["continuation_checkpoint"]})
    } else {
        Value::Null
    };
    let modes: &[bool] = if native_demo { &[false, true] } else { &[false] };
    let mut samples = Vec::new();
    let mut reference_runs: Vec<Option<(Vec<Vec<Option<u16>>>, Vec<Vec<Option<u16>>>, Value)>> = vec![None; modes.len()];
    for iteration in 0..iterations {
        for &use_native in modes {
            // Reverse pair order on alternate iterations to reduce fixed-order bias.
            let selected = if native_demo && iteration % 2 != 0 { !use_native } else { use_native };
            let run = execute(
                cpu,
                ticks,
                &reference_ram,
                selected,
                false,
                whole_engine,
                graph_tier,
                classic_redstone,
                clang.as_deref(),
                linker.as_deref(),
            )?;
            let report_index = usize::from(selected);
            if let Some((active, continuation, checkpoint)) = &reference_runs[report_index] {
                ensure!(
                    &run.active_ram == active,
                    "{} per-tick RAM changed between repeat samples",
                    run.report["executor"]
                );
                ensure!(
                    &run.continuation_ram == continuation,
                    "{} continuation RAM changed between repeat samples",
                    run.report["executor"]
                );
                ensure!(
                    run.report["reset_checkpoint"] == *checkpoint,
                    "{} reset checkpoint changed between repeat samples",
                    run.report["executor"]
                );
            } else {
                reference_runs[report_index] = Some((
                    run.active_ram.clone(),
                    run.continuation_ram.clone(),
                    run.report["reset_checkpoint"].clone(),
                ));
            }
            println!(
                "sample={} {} ticks={} seconds={:.6} compile={:.3}s",
                iteration + 1,
                run.report["executor"].as_str().unwrap(),
                ticks,
                run.report["tick_seconds"].as_f64().unwrap(),
                run.report["graph_compile_seconds"].as_f64().unwrap()
            );
            samples.push(run.report);
        }
    }
    if native_demo {
        for iteration in 0..iterations {
            let graph_index = iteration * 2 + usize::from(iteration % 2 != 0);
            let native_index = iteration * 2 + usize::from(iteration % 2 == 0);
            let graph = &samples[graph_index];
            let native = &samples[native_index];
            ensure!(
                graph["active_ram_sha256"] == native["active_ram_sha256"],
                "per-tick active RAM trace differs between Graph and native queries"
            );
            ensure!(
                graph["active_checkpoint"] == native["active_checkpoint"],
                "active checkpoint differs between Graph and native queries"
            );
            ensure!(
                graph["continuation_ram_sha256"] == native["continuation_ram_sha256"],
                "reset continuation RAM trace differs between Graph and native queries"
            );
            ensure!(
                graph["reset_checkpoint"] == native["reset_checkpoint"],
                "reset checkpoint differs between Graph and native queries"
            );
            ensure!(
                graph["continuation_checkpoint"] == native["continuation_checkpoint"],
                "continuation checkpoint differs between Graph and native queries"
            );
        }
    }
    let report = json!({
        "schema": 1,
        "fixture": "CPU_BubbleSort",
        "schematic_sha256": cpu.sha256,
        "optimization_level": graph_tier,
        "classic_redstone": classic_redstone,
        "whole_native_engine": whole_engine,
        "ticks": ticks,
        "iterations": iterations,
        "measurement": "tick_seconds measures tick_with_world only; flush_seconds includes native state projection and World presentation. Compilation, activation, RAM probes, reset and continuation are excluded. Verification is a separate run.",
        "native_feature": native_demo,
        "native_verification": verification,
        "samples": samples,
    });
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&output, serde_json::to_vec_pretty(&report)?)?;
    println!("report: {}", output.display());
    Ok(())
}

fn execute(
    cpu: support::Cpu,
    ticks: u32,
    reference_ram: &[Vec<Option<u16>>],
    use_native: bool,
    #[cfg_attr(not(feature = "native-demo"), allow(unused_variables))] verify_queries: bool,
    whole_engine: bool,
    graph_tier: usize,
    classic_redstone: bool,
    #[cfg_attr(not(feature = "native-demo"), allow(unused_variables))] clang: Option<&Path>,
    #[cfg_attr(not(feature = "native-demo"), allow(unused_variables))] linker: Option<&Path>,
) -> Result<Run> {
    let setup_started = Instant::now();
    let mut world = support::load_cpu(cpu);
    let sign = cpu.origin + BlockPos::new(186, 2, 184);
    ensure!(
        matches!(world.get_block(sign), Block::WallSign { .. }),
        "expected CPU normalization sign at {sign}"
    );
    ensure!(world.set_block(sign, Block::Air), "failed to remove CPU normalization sign");
    world.flush_block_changes();
    let setup_seconds = setup_started.elapsed().as_secs_f64();

    let compile_started = Instant::now();
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &world,
            world.get_corners(),
            CompilerOptions {
                optimization_level: Some(graph_tier),
                classic_redstone,
                budget_multiplier: 8,
                ..Default::default()
            },
            world.scheduler().iter_entries().collect(),
            Default::default(),
        )
        .with_context(|| format!("O{graph_tier} Graph compile failed"))?;
    let graph_compile_seconds = compile_started.elapsed().as_secs_f64();

    #[cfg(feature = "native-demo")]
    let native_stats = if use_native && !whole_engine {
        Some(
            compiler
                .enable_native_power_demo(
                    clang.context("missing Clang path")?,
                    linker,
                    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/native-query-demo").as_path(),
                    verify_queries,
                )
                .context("native power-query compilation failed")?,
        )
    } else {
        None
    };
    #[cfg(not(feature = "native-demo"))]
    let native_stats: Option<()> = None;
    #[cfg(feature = "native-demo")]
    let engine_stats = if use_native && whole_engine {
        Some(
            compiler
                .enable_native_graph_demo(
                    clang.context("missing Clang path")?,
                    linker,
                    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/native-query-demo").as_path(),
                )
                .context("whole-graph C compilation failed")?,
        )
    } else {
        None
    };
    #[cfg(feature = "native-demo")]
    if use_native && whole_engine && verify_queries {
        compiler.enable_native_graph_verification()?;
    }

    let activation_started = Instant::now();
    let start_position = cpu.origin + cpu.start;
    ensure!(
        matches!(world.get_block(start_position), Block::StoneButton { .. }),
        "CPU start is not a button at {start_position}"
    );
    compiler.on_use_block(start_position);
    let activation_seconds = activation_started.elapsed().as_secs_f64();

    let mut active_ram = Vec::with_capacity(ticks as usize);
    let mut chat = Vec::new();
    let mut tick_time = Duration::ZERO;
    let mut flush_time = Duration::ZERO;
    for tick in 1..=ticks {
        let started = Instant::now();
        compiler.tick_with_world(&mut world)?;
        tick_time += started.elapsed();
        let started = Instant::now();
        compiler.flush(&mut world);
        flush_time += started.elapsed();
        active_ram.push(ram(&world, cpu));
        support::collect_chat(&world, tick, &mut chat);
        if ticks >= 1_000 && tick % 1_000 == 0 {
            eprintln!(
                "O{graph_tier} {}{} tick {tick}/{ticks}",
                if use_native { "native" } else { "Graph" },
                if verify_queries { " verification" } else { "" }
            );
        }
    }
    ensure!(
        active_ram == reference_ram,
        "{} RAM differs from the frozen interpreter trace",
        if use_native { "native" } else { "Graph" }
    );
    if ticks > cpu.active_ticks {
        ensure!(
            active_ram
                .last()
                .is_some_and(|words| words[48..] == (0..16).map(Some).collect::<Vec<_>>()),
            "BubbleSort did not finish with sorted RAM: {:?}",
            active_ram.last()
        );
    }
    let active_checkpoint = support::checkpoint(&world, ticks, &chat);
    let execution_counts = compiler.graph_execution_counts().unwrap();
    ensure!(execution_counts.ticks == u64::from(ticks), "Graph tick count did not advance");
    ensure!(execution_counts.scheduled_ticks > 0, "CPU start produced no scheduled Graph work");
    ensure!(
        execution_counts.wire_walks + execution_counts.piston_samples + execution_counts.piston_actions > 0,
        "CPU start produced no wire or piston Graph work"
    );
    let active_ram_sha256 = hash(&active_ram)?;
    #[cfg(feature = "native-demo")]
    let native_runtime_stats = compiler.native_power_query_stats();
    #[cfg(feature = "native-demo")]
    let native_engine_stats = compiler.native_graph_stats();
    #[cfg(feature = "native-demo")]
    if use_native && whole_engine {
        ensure!(
            native_engine_stats.as_ref().is_some_and(|s| s.ticks == u64::from(ticks)),
            "whole-graph C engine did not execute every tick"
        );
    }
    #[cfg(feature = "native-demo")]
    if use_native && !whole_engine {
        ensure!(
            native_runtime_stats.as_ref().is_some_and(|stats| stats.hits > 0),
            "native query module received no hits"
        );
    }

    let bounds = world.get_corners();
    compiler.reset(&mut world, bounds);
    let reset_checkpoint = support::checkpoint(&world, ticks, &chat);
    let mut continuation_ram = Vec::with_capacity(32);
    for _ in 0..32 {
        world.tick_interpreted();
        continuation_ram.push(ram(&world, cpu));
    }
    let continuation_checkpoint = support::checkpoint(&world, ticks + 32, &chat);
    let native_report = {
        #[cfg(feature = "native-demo")]
        {
            native_stats.map(|stats| {
                json!({
                    "compile_seconds": stats.compile_time.as_secs_f64(),
                    "total_targets": stats.total_targets,
                    "compiled_targets": stats.compiled_targets,
                    "compiled_links": stats.compiled_links,
                    "empty_root_hits": native_runtime_stats.as_ref().map(|stats| stats.empty_root_hits).unwrap_or(0),
                    "hits": native_runtime_stats.as_ref().map(|stats| stats.hits).unwrap_or(0),
                    "fallbacks": native_runtime_stats.as_ref().map(|stats| stats.fallbacks).unwrap_or(0),
                    "verified_queries": native_runtime_stats.as_ref().map(|stats| stats.verified_queries).unwrap_or(0),
                    "generated_c": stats.source,
                    "library": stats.library,
                })
            })
        }
        #[cfg(not(feature = "native-demo"))]
        {
            let _ = (native_stats, verify_queries);
            Value::Null
        }
    };
    let engine_report = {
        #[cfg(feature = "native-demo")]
        {
            engine_stats.map(|s| {
                let runtime = native_engine_stats.as_ref().unwrap();
                json!({"compile_seconds": s.compile_time.as_secs_f64(), "nodes": s.nodes, "ticks": runtime.ticks,
                    "compile_cache_hit":s.cache_hit,
                    "generated_c_bytes":std::fs::metadata(&s.source).ok().map(|m|m.len()),
                    "library_bytes":std::fs::metadata(&s.library).ok().map(|m|m.len()),
                    "compiled_query_targets":s.compiled_query_targets,"total_query_targets":s.total_query_targets,
                    "compiled_guard_targets":s.compiled_guard_targets,"total_guard_targets":s.total_guard_targets,
                    "compiled_power_queries":runtime.compiled_power_queries,"classic_nets":s.classic_nets,
                    "compiled_guard_evaluations":runtime.compiled_guard_evaluations,
                    "generated_c": s.source, "library": s.library, "rust_graph_fallbacks": 0,
                    "shortcut_calls":{"wire_query_facts":runtime.wire_query_fact_evaluations,
                        "dust_path":runtime.wire_dust_path_evaluations,"isolated_replay":runtime.wire_isolated_replays,
                        "instant_reset_replay":runtime.wire_instant_reset_replays,
                        "instant_route":runtime.instant_route_invocations,"instant_route_steps":runtime.instant_route_steps}})
            })
        }
        #[cfg(not(feature = "native-demo"))]
        {
            Value::Null
        }
    };
    Ok(Run {
        report: json!({
            "executor": if use_native { if whole_engine { format!("O{graph_tier}-native-engine") } else { format!("O{graph_tier}-native-query") } } else { format!("O{graph_tier}-Graph") },
            "setup_seconds": setup_seconds,
            "graph_compile_seconds": graph_compile_seconds,
            "activation_seconds": activation_seconds,
            "tick_seconds": tick_time.as_secs_f64(),
            "flush_seconds": flush_time.as_secs_f64(),
            "average_tick_microseconds": tick_time.as_secs_f64() * 1_000_000.0 / ticks as f64,
            "active_ram_sha256": active_ram_sha256,
            "active_ram_samples": active_ram.len(),
            "active_ram_changes": active_ram.windows(2).filter(|pair| pair[0] != pair[1]).count(),
            "active_checkpoint": active_checkpoint,
            "reset_checkpoint": reset_checkpoint,
            "continuation_ram_sha256": hash(&continuation_ram)?,
            "continuation_checkpoint": continuation_checkpoint,
            "graph_execution_counts": json!({"ticks":execution_counts.ticks,"scheduled_ticks":execution_counts.scheduled_ticks,
                "callbacks":execution_counts.callbacks,"wire_walks":execution_counts.wire_walks,"piston_samples":execution_counts.piston_samples,
                "piston_actions":execution_counts.piston_actions,"classic_net_invocations":execution_counts.classic_net_invocations,
                "classic_receiver_ops":execution_counts.classic_receiver_ops}),
            "native_query": native_report,
            "native_engine": engine_report,
        }),
        active_ram,
        continuation_ram,
    })
}

fn ram(world: &PlotWorld, cpu: support::Cpu) -> Vec<Option<u16>> {
    let mut changed = Vec::new();
    support::collect_ram(world, cpu, 0, &mut changed);
    changed.pop().expect("RAM probe returned a sample").1
}

fn hash(value: &impl serde::Serialize) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}

fn frozen_ram_trace(cpu: support::Cpu, ticks: u32) -> Result<Vec<Vec<Option<u16>>>> {
    let transitions = support::reference(cpu).ram_trace;
    ensure!(
        transitions.first().is_some_and(|(tick, _)| *tick == 0),
        "frozen RAM trace has no initial state"
    );
    let mut transition_index = 0usize;
    let mut current = transitions[0].1.clone();
    let mut expanded = Vec::with_capacity(ticks as usize);
    for tick in 1..=ticks {
        while transitions
            .get(transition_index + 1)
            .is_some_and(|(transition_tick, _)| *transition_tick <= tick)
        {
            transition_index += 1;
            current.clone_from(&transitions[transition_index].1);
        }
        expanded.push(current.clone());
    }
    Ok(expanded)
}
