#[path = "../test_support/cpus.rs"]
#[allow(dead_code)]
mod cpus;

use mchprs_blocks::BlockPos;
use mchprs_core::{plot::PlotWorld, world::World};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn quiet(world: &PlotWorld) -> bool {
    world.scheduler().iter_entries().next().is_none() && world.piston_state().events.is_empty() && world.piston_state().motions.is_empty()
}

fn prepare_native(world: &mut PlotWorld, cpu: cpus::Cpu) -> u32 {
    if cpu.name != "zcore12b_sierpinski" {
        return 0;
    }
    cpus::click_cpu(world, cpu, cpu.start);
    let mut idle = 0;
    for tick in 1..=4096 {
        world.tick_interpreted();
        idle = if quiet(world) { idle + 1 } else { 0 };
        if idle == 20 {
            return tick;
        }
    }
    panic!("Load did not settle");
}

fn outputs(world: &PlotWorld, cpu: cpus::Cpu) -> Value {
    if cpu.screen.is_some() {
        return json!(cpus::screen(world, cpu));
    }
    let mut words = Vec::new();
    cpus::collect_ram(world, cpu, 0, &mut words);
    json!(words.pop().unwrap().1)
}

fn run_sample(cpu: cpus::Cpu, ticks: u32, sample: usize) -> Value {
    let import = Instant::now();
    let mut native = cpus::load_cpu(cpu);
    let native_import = import.elapsed();
    let native_preparation = prepare_native(&mut native, cpu);
    let run = if cpu.screen.is_some() { BlockPos::new(207, 10, 154) } else { cpu.start };
    let mut expected = vec![outputs(&native, cpu)];
    let mut native_ticks = Duration::ZERO;
    cpus::click_cpu(&mut native, cpu, run);
    for _ in 1..=ticks {
        let begin = Instant::now();
        native.tick_interpreted();
        native_ticks += begin.elapsed();
        expected.push(outputs(&native, cpu));
    }
    let native_changes = expected.windows(2).filter(|pair| pair[0] != pair[1]).count();
    assert!(native_changes > 0, "benchmark prefix has no observed CPU output activity");
    let import = Instant::now();
    let initial = cpus::load_cpu(cpu);
    let compiled_import = import.elapsed();
    let compilation = Instant::now();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cpus::compile_cpu(initial, cpu)));
    let (mut world, mut compiler, admission_wall) = match outcome {
        Ok(value) => value,
        Err(error) => {
            let message = error
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| error.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "compile panic".into());
            return json!({"status":"compile_declined_or_failed","sample":sample,"fixture":cpu.schematic,"sha256":cpu.sha256,
                "ticks":ticks,"native_ms":native_ticks.as_secs_f64()*1000.0,"native_tps":ticks as f64/native_ticks.as_secs_f64(),
                "native_output_changes":native_changes,"compile_attempt_ms":compilation.elapsed().as_secs_f64()*1000.0,"error":message});
        }
    };
    let compile = compiler.stats().unwrap().duration;
    if cpu.screen.is_some() {
        compiler.try_on_use_block(cpu.origin + cpu.start).unwrap();
        for _ in 0..native_preparation {
            compiler.tick().unwrap();
            compiler.try_flush(&mut world).unwrap();
        }
    }
    let mut first_difference = None;
    let initial = outputs(&world, cpu);
    if initial != expected[0] {
        first_difference = Some(json!({"tick":0,"native":expected[0],"compiled":initial}));
    }
    if std::env::var_os("MCHPRS_BENCH_WAIT_FOR_TRACE").is_some() {
        eprintln!("trace-ready pid={}", std::process::id());
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).unwrap();
    }
    compiler.try_on_use_block(cpu.origin + run).unwrap();
    let mut compiled_ticks = Duration::ZERO;
    let mut publication = Duration::ZERO;
    let mut actual = vec![initial];
    for tick in 1..=ticks {
        let begin = Instant::now();
        compiler.tick().unwrap();
        compiled_ticks += begin.elapsed();
        let begin = Instant::now();
        compiler.try_flush(&mut world).unwrap();
        publication += begin.elapsed();
        let observed = outputs(&world, cpu);
        if observed != expected[tick as usize] && first_difference.is_none() {
            first_difference = Some(json!({"tick":tick,"native":expected[tick as usize],"compiled":observed}));
        }
        actual.push(observed);
    }
    assert!(compiler.is_active(), "compiled run must not fall back to native");
    let compiled_total = compiled_ticks + publication;
    json!({"status":if first_difference.is_some(){"output_divergence"}else{"passed"},"sample":sample,
        "fixture":cpu.schematic,"sha256":cpu.sha256,"ticks":ticks,"native_output_changes":native_changes,
        "native_preparation_ticks":native_preparation,"native_import_ms":native_import.as_secs_f64()*1000.0,
        "compiled_import_ms":compiled_import.as_secs_f64()*1000.0,"compile_ms":compile.as_secs_f64()*1000.0,
        "admission_wall_ms":admission_wall.as_secs_f64()*1000.0,
        "native_ms":native_ticks.as_secs_f64()*1000.0,"compiled_tick_ms":compiled_ticks.as_secs_f64()*1000.0,
        "publication_ms":publication.as_secs_f64()*1000.0,
        "native_tps":ticks as f64/native_ticks.as_secs_f64(),"compiled_tick_tps":ticks as f64/compiled_ticks.as_secs_f64(),
        "publication_visibility":if cfg!(feature = "redpiler-full-publication") { "complete" } else { "redstone-only" },
        "compiled_publication_tps":ticks as f64/compiled_total.as_secs_f64(),
        "first_difference":first_difference,"native_outputs":expected,"compiled_outputs":actual})
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let name = args.get(1).map(String::as_str).unwrap_or("bubblesort");
    let index = match name {
        "bubblesort" => 2,
        "zcore" => 3,
        _ => panic!("expected bubblesort or zcore"),
    };
    let ticks: u32 = args.get(2).map(|v| v.parse().unwrap()).unwrap_or(512);
    let runs: usize = args.get(3).map(|v| v.parse().unwrap()).unwrap_or(3);
    assert!(ticks > 0 && runs > 0);
    let continue_on_divergence = std::env::var_os("MCHPRS_BENCH_ALLOW_DIVERGENCE").is_some();
    let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/mair-six-build-bench");
    std::fs::create_dir_all(&destination).unwrap();
    let mut results = Vec::new();
    for sample in 1..=runs {
        let result = run_sample(cpus::CPUS[index], ticks, sample);
        eprintln!(
            "{name} sample {sample}: status={} native TPS={} compiled {} publication TPS={} compile ms={}",
            result["status"], result["native_tps"], result["publication_visibility"], result["compiled_publication_tps"], result["compile_ms"]
        );
        let passed = result["status"] == "passed";
        results.push(result);
        std::fs::write(
            destination.join(format!("{name}-production.json")),
            serde_json::to_vec_pretty(&results).unwrap(),
        )
        .unwrap();
        if !passed && !continue_on_divergence {
            break;
        }
    }
}
