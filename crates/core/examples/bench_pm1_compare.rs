//! Matched runs of the user-saved PM1 fixture; Start only, no preparation ticks.
#[path = "../test_support/cpus.rs"]
#[allow(dead_code)]
mod cpus;
#[path = "../test_support/pm1.rs"]
#[allow(dead_code)]
mod pm1;

use mchprs_core::redpilator::{CompilerOptions, PublicationMode};
use serde_json::{Value, json};
use std::{path::PathBuf, time::Instant};

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let protocol = pm1::protocol(&PathBuf::from(&args[0]));
    let output = PathBuf::from(&args[1]);
    let backend = &args[2];
    assert!(["llvm", "rust", "interpreter"].contains(&backend.as_str()));
    let ticks: u32 = args.get(3).map_or(protocol.ticks, |s| s.parse().unwrap());
    let samples: u32 = args.get(4).map_or(3, |s| s.parse().unwrap());
    assert!(ticks > 0 && samples > 0);
    let relaxed = args.iter().any(|arg| arg == "--relaxed");
    let until_idle = args.iter().any(|arg| arg == "--until-idle");
    assert!(!until_idle || backend == "interpreter");
    let flush_every: u32 = args
        .iter()
        .find_map(|s| s.strip_prefix("--flush-every="))
        .map_or(0, |s| s.parse().unwrap());
    let publication = args.iter().find_map(|s| s.strip_prefix("--publication=")).unwrap_or("io");
    assert!(PublicationMode::parse(publication).is_some());
    let expected: Option<Value> = args
        .iter()
        .find_map(|s| s.strip_prefix("--reference="))
        .map(|path| serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap());
    let cpu = pm1::cpu(&protocol);
    let seed = if backend != "interpreter" {
        let flags = format!(
            "{} {} --publish-{publication}",
            if backend == "llvm" { "--llvm" } else { "" },
            if relaxed { "--relaxed" } else { "" }
        );
        Some(cpus::compile_cpu_with_options(pm1::load(&protocol).0, cpu, CompilerOptions::parse(&flags).unwrap()).1)
    } else {
        None
    };
    let mut results = Vec::new();
    for sample in 1..=samples {
        let (mut world, bounds) = pm1::load(&protocol);
        let mut compiler = seed.as_ref().map(|seed| {
            seed.benchmark_restart(&world, bounds, world.scheduler().iter_entries().collect())
                .unwrap()
        });
        let before = pm1::outputs(&world, &protocol);
        if let Some(compiler) = &mut compiler {
            compiler.try_on_use_block(cpu.origin + cpu.start).unwrap();
        } else {
            cpus::click_cpu(&mut world, cpu, cpu.start);
        }
        let started = Instant::now();
        let mut publication_elapsed = std::time::Duration::ZERO;
        let mut completed = 0;
        let mut quiet_ticks = 0;
        for tick in 1..=ticks {
            if let Some(compiler) = &mut compiler {
                compiler.tick().unwrap();
            } else {
                world.tick_interpreted();
            }
            if flush_every != 0
                && tick % flush_every == 0
                && let Some(compiler) = &mut compiler
            {
                let publish_started = Instant::now();
                compiler.try_flush(&mut world).unwrap();
                publication_elapsed += publish_started.elapsed();
            }
            completed = tick;
            if until_idle {
                quiet_ticks = if pm1::quiet(&world) { quiet_ticks + 1 } else { 0 };
                if quiet_ticks == 16 && tick > 64 {
                    break;
                }
                if tick == 5000 {
                    let prefix = json!({"fixture_sha256":pm1::SHA256,"ticks":tick,"outputs":pm1::outputs(&world,&protocol),"commands":world.command_output().collect::<Vec<_>>()});
                    std::fs::write(output.with_extension("prefix5000.json"), serde_json::to_vec_pretty(&prefix).unwrap()).unwrap();
                }
            }
        }
        let elapsed = started.elapsed();
        if until_idle {
            assert_eq!(quiet_ticks, 16, "PM1 did not halt within the requested maximum tick count");
        }
        if let Some(compiler) = &mut compiler {
            compiler.try_reset(&mut world, bounds).unwrap();
        }
        let after = pm1::outputs(&world, &protocol);
        let commands: Vec<_> = world.command_output().map(str::to_owned).collect();
        if let Some(expected) = &expected {
            assert_eq!(expected["fixture_sha256"], pm1::SHA256);
            assert_eq!(expected["ticks"], completed);
            assert_eq!(after, expected["outputs"], "lamp or physical RAM output differs");
            assert_eq!(json!(commands), expected["commands"], "ordered PM1 command output differs");
        }
        let row = json!({"backend":backend,"relaxed":relaxed,"sample":sample,"ticks":completed,"preparation_ticks":0,
            "elapsed_ms":elapsed.as_secs_f64()*1000.0,"tps":completed as f64/elapsed.as_secs_f64(),
            "publication":publication,"flush_every":flush_every,"publication_ms":publication_elapsed.as_secs_f64()*1000.0,
            "compile_ms":seed.as_ref().map(|s|s.stats().unwrap().duration.as_secs_f64()*1000.0),
            "outputs":after,"commands":commands,"quiet":pm1::quiet(&world),"outputs_changed":before!=after,"reference_matched":expected.is_some()});
        println!(
            "PM1_RESULT backend={backend} sample={sample} ticks={completed} tps={:.1} messages={}",
            row["tps"].as_f64().unwrap(),
            row["commands"].as_array().unwrap().len()
        );
        results.push(row);
        let reference = results.last().unwrap();
        let report =
            json!({"fixture_sha256":pm1::SHA256,"ticks":completed,"outputs":reference["outputs"],"commands":reference["commands"],"samples":results});
        std::fs::write(&output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
}
