//! Fixed active windows through the production Graph backend, with native handoff checks.
//! Each sample starts from the same saved machine; projection is outside the core timer.
#[allow(dead_code)]
#[path = "support/cpus.rs"]
mod cpu_support;

use anyhow::{Context, Result, ensure};
use cpu_support::{CPUS, ScreenFrame, checkpoint, click, load_cpu, screen};
use mchprs_blocks::BlockPos;
use mchprs_core::{
    plot::{PLOT_WIDTH, PlotWorld},
    redpiler::{BackendKind, Compiler, CompilerOptions},
    world::{World, storage::Chunk},
};
use mchprs_save_data::plot_data::PlotData;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, process::Command, time::Instant};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn fixture(name: &str) -> PathBuf {
    root().join(if name == "chungus" {
        "crates/core/benches/chungus_mandelbrot_plot"
    } else {
        "test_data/Q2CK@Q2CK_Anpu1_Pong_KBTV.schem"
    })
}

fn load(name: &str) -> Result<PlotWorld> {
    let mut world = if name == "chungus" {
        let data = PlotData::load_from_file(fixture(name), false)?;
        let chunks = data
            .chunk_data
            .into_iter()
            .enumerate()
            .map(|(index, chunk)| Chunk::load(index as i32 / PLOT_WIDTH, index as i32 % PLOT_WIDTH, chunk))
            .collect();
        PlotWorld::from_chunks(0, 0, chunks, data.pending_ticks.into_iter().collect())
    } else {
        load_cpu(CPUS.into_iter().find(|cpu| cpu.name == "anpu_pong").unwrap())
    };
    world.disable_command_output_limits_for_replay();
    world.set_screen_only(false);
    world.flush_block_changes();
    Ok(world)
}

fn start(name: &str) -> BlockPos {
    if name == "chungus" {
        BlockPos::new(187, 99, 115)
    } else {
        let cpu = CPUS.into_iter().find(|cpu| cpu.name == "anpu_pong").unwrap();
        cpu.origin + cpu.start
    }
}

fn snapshot(world: &PlotWorld, tick: u32) -> Value {
    let checkpoint = checkpoint(world, tick, &[]);
    let state = world.piston_state();
    // Physical piston serialization differs during Graph execution; reset exports
    // its continuation. Compare the same motion projection as graph_short instead.
    json!({"blocks":checkpoint.blocks,"entities":checkpoint.entities,
        "chat":world.command_output().collect::<Vec<_>>(),
        "future":{"logical_tick":state.logical_tick,"phase":state.phase,
            "scheduled_ticks":world.scheduler().iter_entries().collect::<Vec<_>>(),
            "piston_events":state.events,"motions":state.motions.iter().map(|motion|
                json!([motion.pos,motion.progress,motion.previous_progress,motion.last_tick])).collect::<Vec<_>>()}})
}

fn native_reference(name: &str, ticks: u32) -> Result<(Value, Value, usize)> {
    let mut world = load(name)?;
    click(&mut world, start(name) - BlockPos::new(8, 8, 8));
    let frames: Vec<ScreenFrame> = if name == "anpu" {
        serde_json::from_slice(&std::fs::read(root().join("test_data/cpu-references/anpu_screen.json"))?)?
    } else {
        Vec::new()
    };
    let mut checked_frames = 0;
    for tick in 0..=ticks {
        if tick > 0 {
            world.tick_interpreted();
        }
        if let Some(frame) = frames.iter().find(|frame| frame.tick == tick) {
            ensure!(screen(&world) == frame.pixels, "native frozen screen mismatch at {tick}");
            checked_frames += 1;
        }
    }
    let final_state = snapshot(&world, ticks);
    for _ in 0..32 {
        world.tick_interpreted();
    }
    Ok((final_state, snapshot(&world, ticks + 32), checked_frames))
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let argument = |name: &str| args.windows(2).find(|pair| pair[0] == name).map(|pair| pair[1].as_str());
    let name = argument("--cpu").context("--cpu needs chungus or anpu")?;
    ensure!(matches!(name, "chungus" | "anpu"), "unknown CPU");
    let ticks = argument("--ticks")
        .map(str::parse::<u32>)
        .transpose()?
        .unwrap_or(if name == "chungus" { 16_384 } else { 5_000 });
    let iterations = argument("--iterations").unwrap_or("5").parse::<usize>()?;
    let tier = argument("--graph-tier").unwrap_or("2").parse::<usize>()?;
    ensure!(ticks > 0 && ticks <= u32::MAX - 32 && iterations > 0, "invalid window or sample count");
    ensure!((2..=6).contains(&tier), "baseline supports O2 through O6");
    let git = |args: &[&str]| -> Result<String> {
        let output = Command::new("git").args(args).current_dir(root()).output()?;
        ensure!(output.status.success(), "git metadata failed");
        Ok(String::from_utf8(output.stdout)?)
    };
    let mut report = json!({"schema":1,"cpu":name,"ticks":ticks,"iterations":iterations,"optimization_level":tier,
        "profile":if cfg!(debug_assertions) {"dev/debug"}else{"bench/release"},
        "classic_redstone_auto":tier >= 2,"stable_instant_inputs":false,"budget_multiplier":8,
        "fixture_sha256":hash(&std::fs::read(fixture(name))?),
        "binary_sha256":hash(&std::fs::read(std::env::current_exe()?)?),
        "harness_source_sha256":hash(include_bytes!("classic_baseline.rs")),
        "cpu_loader_source_sha256":hash(include_bytes!("support/cpus.rs")),
        "git_head":git(&["rev-parse","HEAD"])?.trim(),
        "working_tree_diff":git(&["diff","HEAD","--"] )?,"working_tree_status":git(&["status","--short"] )?,
        "measurement":"one contiguous active tick_with_world loop per fresh machine; no intermediate flush; load, compile, start, final flush, reset, state checks and native continuation excluded",
        "correctness":"all blocks/entities, ordered chat and exact exported future at window end and after 32 native continuation ticks; intermediate boundaries covered by existing differential tests, not this timed loop",
        "samples":[]});
    println!("{name} O{tier}: building independent native reference for {ticks} active ticks");
    let (expected, expected_continuation, checked_frames) = native_reference(name, ticks)?;
    report["native_reference"] = json!({"final":expected,"continuation":expected_continuation,"pinned_screen_frames_checked":checked_frames});
    let mut times = Vec::new();
    for sample in 1..=iterations {
        let load_started = Instant::now();
        let mut world = load(name)?;
        let load_seconds = load_started.elapsed().as_secs_f64();
        let bounds = world.get_corners();
        let mut compiler = Compiler::default();
        let compile_started = Instant::now();
        compiler
            .compile(
                &world,
                bounds,
                CompilerOptions {
                    optimization_level: Some(tier),
                    budget_multiplier: 8,
                    ..Default::default()
                },
                world.scheduler().iter_entries().collect(),
                Default::default(),
            )
            .context("Graph admission failed")?;
        let compile_seconds = compile_started.elapsed().as_secs_f64();
        let stats = compiler.stats().unwrap();
        let compile_statistics = json!({
            "total_seconds":stats.total_duration.as_secs_f64(),
            "graph_seconds":stats.graph.duration.as_secs_f64(),
            "passes":stats.graph.passes.iter().map(|pass| json!({
                "name":pass.name,"enabled":pass.enabled,"seconds":pass.duration.as_secs_f64()
            })).collect::<Vec<_>>()
        });
        ensure!(compiler.backend_kind() == Some(BackendKind::Graph), "expected Graph execution");
        let coverage = compiler.graph_execution_coverage().context("missing Graph coverage")?;
        let preparation_tables: Vec<_> = compiler
            .stats()
            .unwrap()
            .graph
            .preparation_tables
            .iter()
            .map(|table| json!({"stage":table.stage,"table":table.table,"entries":table.entries,"estimated_bytes":table.estimated_bytes}))
            .collect();
        if name == "chungus" {
            ensure!(
                coverage.fixed_classic_nodes == coverage.component_nodes && coverage.fixed_classic_wires == coverage.wire_nodes,
                "CHUNGUS did not enter whole fixed execution"
            );
        }
        let activation_started = Instant::now();
        compiler.on_use_block(start(name));
        let activation_seconds = activation_started.elapsed().as_secs_f64();
        let coverage_after_activation = compiler.graph_execution_coverage().unwrap();
        let before = compiler.graph_execution_counts().context("missing Graph counters")?;
        let started = Instant::now();
        for _ in 0..ticks {
            compiler.tick_with_world(&mut world)?;
        }
        let seconds = started.elapsed().as_secs_f64();
        let after = compiler.graph_execution_counts().unwrap();
        let coverage_at_window_end = compiler.graph_execution_coverage().unwrap();
        ensure!(after.ticks - before.ticks == u64::from(ticks), "incomplete Graph window");
        ensure!(
            after.scheduled_ticks > before.scheduled_ticks && after.callbacks > before.callbacks,
            "inactive workload"
        );
        let flush_started = Instant::now();
        compiler.flush(&mut world);
        let final_flush_seconds = flush_started.elapsed().as_secs_f64();
        let reset_started = Instant::now();
        compiler.reset(&mut world, bounds);
        let reset_seconds = reset_started.elapsed().as_secs_f64();
        ensure!(
            snapshot(&world, ticks) == expected,
            "{name} O{tier} sample {sample}: final state/future mismatch"
        );
        for _ in 0..32 {
            world.tick_interpreted();
        }
        ensure!(
            snapshot(&world, ticks + 32) == expected_continuation,
            "{name} O{tier} sample {sample}: native continuation mismatch"
        );
        times.push(seconds);
        report["samples"]
            .as_array_mut()
            .unwrap()
            .push(json!({"sample":sample,"core_seconds":seconds,
            "game_ticks_per_second":f64::from(ticks)/seconds,"microseconds_per_tick":seconds*1e6/f64::from(ticks),
            "load_seconds":load_seconds,"compile_seconds":compile_seconds,"activation_seconds":activation_seconds,
            "final_flush_seconds":final_flush_seconds,"reset_seconds":reset_seconds,"coverage":coverage,
            "coverage_after_activation":coverage_after_activation,"coverage_at_window_end":coverage_at_window_end,
            "preparation_tables":preparation_tables,"compile_statistics":compile_statistics,
            "work":{"ticks":after.ticks-before.ticks,"scheduled_ticks":after.scheduled_ticks-before.scheduled_ticks,
                "callbacks":after.callbacks-before.callbacks,"wire_walks":after.wire_walks-before.wire_walks,
                "piston_samples":after.piston_samples-before.piston_samples,"piston_actions":after.piston_actions-before.piston_actions,
                "classic_net_invocations":after.classic_net_invocations-before.classic_net_invocations,
                "classic_receiver_ops":after.classic_receiver_ops-before.classic_receiver_ops},
            "bounds":[[bounds.0.x,bounds.0.y,bounds.0.z],[bounds.1.x,bounds.1.y,bounds.1.z]],
            "native_state_and_continuation_matched":true}));
        println!(
            "{name} O{tier} sample {sample}: {seconds:.6}s / {ticks} ticks = {:.1} TPS; compile {compile_seconds:.3}s; native state/future matched",
            f64::from(ticks) / seconds
        );
    }
    times.sort_by(f64::total_cmp);
    let median = if iterations % 2 == 0 {
        (times[iterations / 2 - 1] + times[iterations / 2]) / 2.0
    } else {
        times[iterations / 2]
    };
    report["summary"] = json!({"median_core_seconds":median,"range_core_seconds":[times[0],times[iterations-1]],
        "median_game_ticks_per_second":f64::from(ticks)/median,"median_microseconds_per_tick":median*1e6/f64::from(ticks)});
    let output = root().join(argument("--output").unwrap_or("target/classic-engine-baseline.json"));
    std::fs::create_dir_all(output.parent().context("output requires parent")?)?;
    std::fs::write(&output, serde_json::to_vec_pretty(&report)?)?;
    println!("report: {}", output.display());
    Ok(())
}
