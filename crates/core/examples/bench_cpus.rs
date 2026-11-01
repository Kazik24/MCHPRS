#[path = "../test_support/cpus.rs"]
#[allow(dead_code)]
mod cpus;

use mchprs_blocks::BlockPos;
use mchprs_core::{plot::PlotWorld, world::World};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
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

fn run_sample(cpu: cpus::Cpu, ticks: u32, sample: usize, kernel_only: bool, llvm: bool) -> Value {
    let full_publication = cfg!(feature = "redpiler-full-publication") || std::env::var_os("MCHPRS_BENCH_PUBLISH_ALL").is_some();
    let wait_for_trace = std::env::var_os("MCHPRS_BENCH_WAIT_FOR_TRACE").is_some();
    let diagnostic = wait_for_trace || std::env::var_os("MCHPRS_BENCH_ALLOW_DIVERGENCE").is_some();
    let import = Instant::now();
    let mut native = cpus::load_cpu(cpu);
    let native_import = import.elapsed();
    let native_preparation = prepare_native(&mut native, cpu);
    let run = if cpu.name == "zcore12b_sierpinski" { BlockPos::new(207, 10, 154) } else { cpu.start };
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
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cpus::compile_cpu_with_options(initial, cpu,
        mchprs_core::redpiler::CompilerOptions { llvm, publication:full_publication.then_some(mchprs_core::redpiler::PublicationMode::All), ..Default::default() })));
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
    if cpu.name == "zcore12b_sierpinski" {
        compiler.try_on_use_block(cpu.origin + cpu.start).unwrap();
        for _ in 0..native_preparation {
            compiler.tick().unwrap();
            compiler.try_flush(&mut world).unwrap();
        }
    }
    let mut first_difference = None;
    let initial = outputs(&world, cpu);
    if full_publication && initial != expected[0] {
        first_difference = Some(json!({"tick":0,"native":expected[0],"compiled":initial}));
    }
    if wait_for_trace {
        eprintln!("trace-ready pid={}", std::process::id());
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).unwrap();
    }
    compiler.try_on_use_block(cpu.origin + run).unwrap();
    let mut compiled_ticks = Duration::ZERO;
    let mut publication = Duration::ZERO;
    let mut actual = vec![initial];
    if kernel_only {
        let begin = Instant::now();
        for _ in 0..ticks {
            compiler.tick().unwrap();
        }
        compiled_ticks = begin.elapsed();
        if wait_for_trace {
            eprintln!("trace-complete pid={}", std::process::id());
            let mut line = String::new();
            std::io::stdin().read_line(&mut line).unwrap();
        }
    } else {
        for tick in 1..=ticks {
            let begin = Instant::now();
            compiler.tick().unwrap();
            compiled_ticks += begin.elapsed();
            let begin = Instant::now();
            compiler.try_flush(&mut world).unwrap();
            publication += begin.elapsed();
            let observed = outputs(&world, cpu);
            if full_publication && observed != expected[tick as usize] && first_difference.is_none() {
                first_difference = Some(json!({"tick":tick,"native":expected[tick as usize],"compiled":observed}));
            }
            actual.push(observed);
        }
    }
    assert!(compiler.is_active(), "compiled run must not fall back to native");
    let (reset_publication_ms, final_reset_difference) = if full_publication && !kernel_only {
        (Value::Null, None)
    } else {
        // Reset projects the complete state after runs without full per-tick publication.
        let begin = Instant::now();
        assert!(
            !compiler.current_flags().unwrap().update,
            "reset requires the full fixture bounds when updates are enabled"
        );
        compiler.try_reset(&mut world, (cpu.origin, cpu.origin)).unwrap();
        let duration = begin.elapsed().as_secs_f64() * 1000.0;
        let final_output = outputs(&world, cpu);
        if kernel_only {
            actual.push(final_output.clone());
        }
        let difference = (final_output != *expected.last().unwrap()).then(|| {
            json!({
                "tick":ticks,"native":expected.last().unwrap(),"compiled_after_full_reset":final_output
            })
        });
        (json!(duration), difference)
    };
    let compiled_total = compiled_ticks + publication;
    let status = if diagnostic {
        "diagnostic_only"
    } else if first_difference.is_some() || final_reset_difference.is_some() {
        "output_divergence"
    } else if full_publication && !kernel_only {
        "passed"
    } else {
        "final_reset_match"
    };
    json!({"status":status,"diagnostic_only":diagnostic,"sample":sample,
        "fixture":cpu.schematic,"sha256":cpu.sha256,"ticks":ticks,"native_output_changes":native_changes,
        "native_preparation_ticks":native_preparation,"native_import_ms":native_import.as_secs_f64()*1000.0,
        "compiled_import_ms":compiled_import.as_secs_f64()*1000.0,"compile_ms":compile.as_secs_f64()*1000.0,
        "admission_wall_ms":admission_wall.as_secs_f64()*1000.0,
        "native_ms":native_ticks.as_secs_f64()*1000.0,"compiled_tick_ms":compiled_ticks.as_secs_f64()*1000.0,
        "publication_ms":if kernel_only { Value::Null } else { json!(publication.as_secs_f64()*1000.0) },
        "native_tps":ticks as f64/native_ticks.as_secs_f64(),"compiled_tick_tps":ticks as f64/compiled_ticks.as_secs_f64(),
        "execution_mode":if kernel_only { "kernel-only" } else { "tick-and-publication" },
        "publication_visibility":if kernel_only { "final-reset-only" } else if full_publication { "complete" } else { "redstone-only" },
        "output_validation":if full_publication && !kernel_only { "per-tick-published" } else { "final-reset-only" },
        "published_output_ticks":if kernel_only { json!([0,ticks]) } else { json!((0..=ticks).collect::<Vec<_>>()) },
        "reset_publication_ms":reset_publication_ms,"final_reset_difference":final_reset_difference,
        "compiled_publication_tps":if kernel_only { Value::Null } else { json!(ticks as f64/compiled_total.as_secs_f64()) },
        "first_difference":first_difference,"native_outputs":expected,"published_outputs":actual})
}

fn provenance() -> Value {
    let executable = std::env::current_exe().unwrap();
    let executable_sha256 = format!("{:x}", Sha256::digest(std::fs::read(executable).unwrap()));
    json!({
        "executable_sha256":executable_sha256,
        "features":{"redpiler_full_publication":cfg!(feature = "redpiler-full-publication"),"redpiler_trace":cfg!(feature = "redpiler-trace")},
        "source_provenance":"external build manifest required: record git HEAD, SHA-256 of the dirty patch including untracked source files, rustc -Vv, and the exact cargo build command"
    })
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let name = args.get(1).map(String::as_str).unwrap_or("bubblesort");
    let index = match name {
        "bubblesort" => 2,
        "zcore" => 3,
        "anpu" => 1,
        _ => panic!("expected bubblesort, zcore or anpu"),
    };
    let ticks: u32 = args.get(2).map(|v| v.parse().unwrap()).unwrap_or(512);
    let runs: usize = args.get(3).map(|v| v.parse().unwrap()).unwrap_or(3);
    let kernel_only = match args.get(4).map(String::as_str) {
        None => false,
        Some("kernel") => true,
        Some("integrated") => false,
        _ => panic!("expected optional mode: kernel"),
    };
    let llvm = args.iter().any(|arg| arg == "--llvm");
    assert!(ticks > 0 && runs > 0);
    let continue_on_divergence = std::env::var_os("MCHPRS_BENCH_ALLOW_DIVERGENCE").is_some();
    let provenance = provenance();
    let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/redpiler-bench-results");
    std::fs::create_dir_all(&destination).unwrap();
    let mut results = Vec::new();
    for sample in 1..=runs {
        let mut result = run_sample(cpus::CPUS[index], ticks, sample, kernel_only, llvm);
        result.as_object_mut().unwrap().insert("backend".into(), json!(if llvm { "llvm" } else { "rust" }));
        eprintln!(
            "{name} sample {sample}: status={} native TPS={} kernel TPS={} compiled {} publication TPS={} compile ms={}",
            result["status"],
            result["native_tps"],
            result["compiled_tick_tps"],
            result["publication_visibility"],
            result["compiled_publication_tps"],
            result["compile_ms"]
        );
        let passed = result["status"] == "passed" || result["status"] == "final_reset_match";
        result.as_object_mut().unwrap().insert("provenance".into(), provenance.clone());
        results.push(result);
        std::fs::write(
            destination.join(format!("{name}-{}{}.json", if kernel_only { "kernel" } else { "production" }, if llvm { "-llvm" } else { "" })),
            serde_json::to_vec_pretty(&results).unwrap(),
        )
        .unwrap();
        if !passed && !continue_on_divergence {
            break;
        }
    }
}
