#[path = "../test_support/cpus.rs"]
#[allow(dead_code)]
mod cpus;

use mchprs_blocks::BlockPos;
use mchprs_core::redpilator::CompilerOptions;
use mchprs_core::world::World;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

fn cpu(index: usize) -> cpus::Cpu {
    if index == 0 {
        cpus::Cpu {
            schematic: "PM1_SORT_FIXED.schem",
            sha256: "ad68a0160990c72612c0105802fade337758de127dcdbd4cc1a195059ad2d893",
            screen: Some(cpus::Screen {
                x: 230,
                top_y: 71,
                bottom_y: 40,
                left_z: 81,
                right_z: 112,
                sparse: false,
            }),
            ..cpus::CPUS[0]
        }
    } else {
        cpus::CPUS[index]
    }
}

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

fn run(llvm: bool, ticks: u32, preparation: u32, index: usize, io_only: bool, flush_every: u32) -> (Value, Value) {
    let cpu = cpu(index);
    let world = cpus::load_cpu(cpu);
    let (mut world, mut compiler, wall) = cpus::compile_cpu_with_options(
        world,
        cpu,
        CompilerOptions {
            llvm,
            io_only,
            ..Default::default()
        },
    );
    let setup = compiler.stats().unwrap().duration;
    compiler.try_on_use_block(cpu.origin + cpu.start).unwrap();
    for _ in 0..preparation {
        compiler.tick().unwrap();
    }
    if index == 3 {
        compiler.try_on_use_block(cpu.origin + BlockPos::new(207, 10, 154)).unwrap();
    }
    let mut elapsed = Duration::ZERO;
    let mut publication = Duration::ZERO;
    let mut publication_calls = 0;
    let mut remaining = ticks;
    while remaining > 0 {
        let batch = if flush_every == 0 { remaining } else { remaining.min(flush_every) };
        let start = Instant::now();
        for _ in 0..batch {
            compiler.tick().unwrap();
        }
        elapsed += start.elapsed();
        remaining -= batch;
        if flush_every > 0 {
            let start = Instant::now();
            compiler.try_flush(&mut world).unwrap();
            publication += start.elapsed();
            publication_calls += 1;
        }
    }
    assert!(compiler.is_active());
    if index == 0 {
        for _ in 0..4 {
            compiler.tick().unwrap();
        }
    }
    let publication_mode = compiler.current_flags().unwrap().publication_mode().name();
    let reset = Instant::now();
    compiler.try_reset(&mut world, (cpu.origin, cpu.origin)).unwrap();
    let reset_ms = reset.elapsed().as_secs_f64() * 1000.0;
    let output = json!(cpus::screen(&world, cpu));
    let hash = format!("{:x}", Sha256::digest(serde_json::to_vec(&output).unwrap()));
    let mut record = json!({"backend":if llvm {"llvm"} else {"rust"},"ticks":ticks,"preparation_ticks":preparation,
        "setup_ms":setup.as_secs_f64()*1000.0,"admission_ms":wall.as_secs_f64()*1000.0,"kernel_ms":elapsed.as_secs_f64()*1000.0,
        "tps":ticks as f64/elapsed.as_secs_f64(),"reset_ms":reset_ms,"screen_sha256":hash,
        "publication_mode":publication_mode,
        "flush_every_ticks":flush_every,"publication_calls":publication_calls,"publication_ms":publication.as_secs_f64()*1000.0,
        "tick_and_publication_tps":ticks as f64/(elapsed+publication).as_secs_f64()});
    if index == 0 {
        let expected = cpus::reference(cpus::CPUS[0]);
        let expected: Vec<_> = expected
            .chat_trace
            .iter()
            .filter(|(tick, _)| *tick <= 12_051)
            .map(|(_, text)| text.as_str())
            .collect();
        let actual: Vec<_> = world.command_output().collect();
        assert_eq!(actual, expected, "PM1 complete ordered command output");
        assert!(world.scheduler().iter_entries().next().is_none());
        assert!(world.piston_state().events.is_empty() && world.piston_state().motions.is_empty());
        record["messages"] = json!(actual.len());
        record["first_timed_tick"] = json!(preparation + 1);
        record["last_timed_tick"] = json!(preparation + ticks);
        record["screen"] = output.clone();
    }
    eprintln!("LLVM paired trial: {record}");
    (record, output)
}

fn main() {
    let ticks = std::env::args().nth(1).map_or(10_000, |s| s.parse::<u32>().unwrap());
    let pairs = std::env::args().nth(2).map_or(7, |s| s.parse::<usize>().unwrap());
    assert!(ticks > 0 && pairs > 0);
    let index = if std::env::args().any(|s| s == "--pm1") {
        0
    } else if std::env::args().any(|s| s == "--anpu") {
        1
    } else {
        3
    };
    let rust_only = std::env::args().any(|s| s == "--rust-only");
    let llvm_only = std::env::args().any(|s| s == "--llvm-only");
    assert!(!std::env::args().any(|s| matches!(s.as_str(), "--io-only" | "-i")), "use --publish-io");
    let io_only = std::env::args().any(|s| s == "--publish-io");
    // Zero keeps the original uninterrupted kernel timing; positive intervals also time publication.
    let flush_every = std::env::args()
        .find_map(|s| s.strip_prefix("--flush-every=").map(|n| n.parse::<u32>().unwrap()))
        .unwrap_or(0);
    assert!(!(rust_only && llvm_only), "choose one backend");
    if index == 0 {
        assert_eq!(ticks, 11_752, "PM1 times program ticks 300..=12051");
    }
    let preparation = if index == 3 {
        prepare()
    } else if index == 0 {
        299
    } else {
        0
    };
    let name = if index == 3 {
        "zcore"
    } else if index == 0 {
        "pm1_sort"
    } else {
        "anpu"
    };
    let suffix = if rust_only {
        "rust-profile"
    } else if llvm_only {
        "llvm-only"
    } else {
        "llvm-paired"
    };
    let suffix = format!(
        "{suffix}{}{}",
        if io_only { "-io" } else { "" },
        if flush_every == 0 {
            String::new()
        } else {
            format!("-flush-{flush_every}")
        }
    );
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../target/redpilator-bench-results/{name}-{suffix}.json"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut results = Vec::new();
    let mut expected = None;
    for pair in 0..pairs {
        let backends: &[bool] = if rust_only {
            &[false]
        } else if llvm_only {
            &[true]
        } else if pair % 2 == 0 {
            &[false, true]
        } else {
            &[true, false]
        };
        for &llvm in backends {
            let (mut result, output) = run(llvm, ticks, preparation, index, io_only, flush_every);
            if let Some(expected) = &expected {
                assert_eq!(&output, expected, "{name} output mismatch in pair {pair}, llvm={llvm}");
            } else {
                expected = Some(output);
            }
            result["pair"] = json!(pair + 1);
            results.push(result);
            let report = json!({"fixture":cpu(index).schematic,"fixture_sha256":cpu(index).sha256,
                "mode":if flush_every == 0 { "kernel-only; snapshots outside timing" } else { "tick-and-publication; snapshots outside timing" },
                "ticks":ticks,"requested_pairs":pairs,"alternating_order":!rust_only && !llvm_only,"results":results});
            std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        }
    }
    println!("wrote {}", path.display());
}
