#[path = "../test_support/cpus.rs"]
#[allow(dead_code)]
mod cpus;

use mchprs_blocks::BlockPos;
use mchprs_core::redpiler::CompilerOptions;
use mchprs_core::world::World;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::Instant;

fn prepare() -> u32 {
    let cpu = cpus::CPUS[3];
    let mut world = cpus::load_cpu(cpu);
    cpus::click_cpu(&mut world, cpu, cpu.start);
    let mut idle = 0;
    for tick in 1..=4096 {
        world.tick_interpreted();
        let quiet =
            world.scheduler().iter_entries().next().is_none() && world.piston_state().events.is_empty() && world.piston_state().motions.is_empty();
        idle = if quiet { idle + 1 } else { 0 };
        if idle == 20 {
            return tick;
        }
    }
    panic!("ZCore loading did not settle");
}

fn run(llvm: bool, ticks: u32, preparation: u32, index: usize) -> (Value, Value) {
    let cpu = cpus::CPUS[index];
    let world = cpus::load_cpu(cpu);
    let (mut world, mut compiler, wall) = cpus::compile_cpu_with_options(world, cpu, CompilerOptions { llvm, ..Default::default() });
    let setup = compiler.stats().unwrap().duration;
    compiler.try_on_use_block(cpu.origin + cpu.start).unwrap();
    for _ in 0..preparation {
        compiler.tick().unwrap();
    }
    if index == 3 {
        compiler.try_on_use_block(cpu.origin + BlockPos::new(207, 10, 154)).unwrap();
    }
    let start = Instant::now();
    for _ in 0..ticks {
        compiler.tick().unwrap();
    }
    let elapsed = start.elapsed();
    assert!(compiler.is_active());
    let reset = Instant::now();
    compiler.try_reset(&mut world, (cpu.origin, cpu.origin)).unwrap();
    let reset_ms = reset.elapsed().as_secs_f64() * 1000.0;
    let output = json!(cpus::screen(&world, cpu));
    let hash = format!("{:x}", Sha256::digest(serde_json::to_vec(&output).unwrap()));
    let record = json!({"backend":if llvm {"llvm"} else {"rust"},"ticks":ticks,"preparation_ticks":preparation,
        "setup_ms":setup.as_secs_f64()*1000.0,"admission_ms":wall.as_secs_f64()*1000.0,"kernel_ms":elapsed.as_secs_f64()*1000.0,
        "tps":ticks as f64/elapsed.as_secs_f64(),"reset_ms":reset_ms,"screen_sha256":hash});
    eprintln!("LLVM paired trial: {record}");
    (record, output)
}

fn main() {
    let ticks = std::env::args().nth(1).map_or(10_000, |s| s.parse::<u32>().unwrap());
    let pairs = std::env::args().nth(2).map_or(7, |s| s.parse::<usize>().unwrap());
    assert!(ticks > 0 && pairs > 0);
    let index = if std::env::args().any(|s| s == "--anpu") { 1 } else { 3 };
    let rust_only = std::env::args().any(|s| s == "--rust-only");
    let preparation = if index == 3 { prepare() } else { 0 };
    let name = if index == 3 { "zcore" } else { "anpu" };
    let suffix = if rust_only { "rust-profile" } else { "llvm-paired" };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../target/redpiler-bench-results/{name}-{suffix}.json"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut results = Vec::new();
    let mut expected = None;
    for pair in 0..pairs {
        let backends: &[bool] = if rust_only {
            &[false]
        } else if pair % 2 == 0 {
            &[false, true]
        } else {
            &[true, false]
        };
        for &llvm in backends {
            let (mut result, output) = run(llvm, ticks, preparation, index);
            if let Some(expected) = &expected {
                assert_eq!(&output, expected, "{name} output mismatch in pair {pair}, llvm={llvm}");
            } else {
                expected = Some(output);
            }
            result["pair"] = json!(pair + 1);
            results.push(result);
            let report = json!({"fixture":cpus::CPUS[index].schematic,"fixture_sha256":cpus::CPUS[index].sha256,"mode":"kernel-only; snapshots outside timing",
                "ticks":ticks,"requested_pairs":pairs,"alternating_order":!rust_only,"results":results});
            std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        }
    }
    println!("wrote {}", path.display());
}
