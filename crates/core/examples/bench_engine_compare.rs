//! Fixed game-tick benchmark; publication is disabled unless an interval is requested.
#[path = "../test_support/cpus.rs"]
#[allow(dead_code)]
mod cpus;

use mchprs_blocks::BlockPos;
use mchprs_core::{
    plot::worldedit::load_schematic,
    redpilator::{CompilerOptions, PublicationMode},
    world::World,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{path::PathBuf, time::Instant};

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let cpu_name = args.first().expect("zcore or pm1");
    let output = PathBuf::from(args.get(1).expect("output JSON"));
    let pm1 = cpu_name == "pm1";
    assert!(pm1 || cpu_name == "zcore");
    let mut ticks = if pm1 { 11_752u32 } else { 5_000 };
    let mut publication = PublicationMode::Redstone;
    let mut flush_every = 0u32;
    let mut relaxed_mode = false;
    let mut samples = 3u32;
    for arg in &args[2..] {
        if arg == "--relaxed" {
            relaxed_mode = true;
        } else if let Some(mode) = arg.strip_prefix("--publication=") {
            publication = PublicationMode::parse(mode).expect("publication: io, redstone or all");
        } else if let Some(interval) = arg.strip_prefix("--flush-every=") {
            flush_every = interval.parse().expect("publication interval in game ticks");
        } else if let Some(count) = arg.strip_prefix("--samples=") {
            samples = count.parse().expect("sample count");
        } else {
            ticks = arg.parse().expect("tick count or --publication/--flush-every option");
        }
    }
    let program_ticks = if pm1 {
        11_752
    } else if ticks > 5_000 {
        50_000
    } else {
        5_000
    };
    assert!(ticks > 0 && ticks <= program_ticks && samples > 0);
    let cpu = if pm1 {
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
        cpus::CPUS[3]
    };
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test_data").join(cpu.schematic);
    let clip = load_schematic(std::fs::File::open(file).unwrap()).unwrap();
    let bounds = (
        cpu.origin,
        cpu.origin + BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1),
    );
    let (_, seed, _) = cpus::compile_cpu_with_options(
        cpus::load_cpu(cpu),
        cpu,
        CompilerOptions {
            llvm: true,
            relaxed_mode,
            publication: Some(publication),
            ..Default::default()
        },
    );
    let compile_ms = seed.stats().unwrap().duration.as_secs_f64() * 1000.0;
    let preparation = if pm1 { 299 } else { 413 };
    let kernel_analysis = std::env::var_os("MCHPRS_ENGINE_KERNEL_ANALYSIS").is_some();
    let mut expected_snapshot = None;
    let mut results = Vec::new();
    for sample in 1..=samples {
        let mut world = cpus::load_cpu(cpu);
        let mut compiler = seed
            .benchmark_restart(&world, bounds, world.scheduler().iter_entries().collect())
            .unwrap();
        compiler.try_on_use_block(cpu.origin + cpu.start).unwrap();
        for _ in 0..preparation {
            compiler.tick().unwrap();
        }
        compiler.try_flush(&mut world).unwrap();
        if !pm1 {
            compiler.try_on_use_block(cpu.origin + BlockPos::new(207, 10, 154)).unwrap();
        }
        let mut publication_calls = 0u32;
        let mut publication_elapsed = std::time::Duration::ZERO;
        let start = Instant::now();
        for tick in 1..=ticks {
            compiler.tick().unwrap();
            if flush_every > 0 && (tick % flush_every == 0 || tick == ticks) {
                let published = Instant::now();
                compiler.try_flush(&mut world).unwrap();
                publication_elapsed += published.elapsed();
                publication_calls += 1;
            }
        }
        let elapsed = start.elapsed();
        let native_state_hash = kernel_analysis.then(|| {
            let snapshot = compiler.benchmark_snapshot().unwrap();
            if let Some(expected) = &expected_snapshot {
                assert_eq!(&snapshot, expected, "fresh executor state differs");
            } else {
                expected_snapshot = Some(snapshot.clone());
            }
            let bytes: Vec<_> = snapshot.iter().flat_map(|word| word.to_le_bytes()).collect();
            if sample == 1 {
                std::fs::write(output.with_extension("native-state.bin"), &bytes).unwrap();
            }
            format!("{:x}", Sha256::digest(&bytes))
        });
        for _ in ticks..program_ticks {
            compiler.tick().unwrap();
        }
        if pm1 {
            for _ in 0..4 {
                compiler.tick().unwrap();
            }
        }
        compiler.try_reset(&mut world, bounds).unwrap();
        let screen = cpus::screen(&world, cpu);
        let screen_hash = format!("{:x}", Sha256::digest(serde_json::to_vec(&screen).unwrap()));
        assert_eq!(
            screen_hash,
            if pm1 {
                "322d3d336985811875bc4e99b1eec21b69a83792efda6c0b20ac270eef407e3e"
            } else {
                if program_ticks == 50_000 {
                    "6597ffbdb45a34e2570052f1835b9a9807e4ac9853cf41966a3ca2af2551cdfc"
                } else {
                    "563a79fbd276a5b3601a404c3b7f278fe302f171960d5a6ed294c34219c7175b"
                }
            }
        );
        let messages = world.command_output().count();
        if pm1 {
            let reference = cpus::reference(cpus::CPUS[0]);
            let expected: Vec<_> = reference
                .chat_trace
                .iter()
                .filter(|(tick, _)| *tick <= 12051)
                .map(|(_, text)| text.as_str())
                .collect();
            assert_eq!(world.command_output().collect::<Vec<_>>(), expected, "ordered PM1 outputs");
            assert_eq!(messages, 1534);
            assert!(world.scheduler().iter_entries().next().is_none());
            assert!(world.piston_state().events.is_empty() && world.piston_state().motions.is_empty());
        }
        let row = json!({"backend":"llvm","relaxed_mode":relaxed_mode,"cpu":cpu_name,"sample":sample,"ticks":ticks,"preparation_ticks":preparation,"flush_every":flush_every,"publication":publication.name(),"publication_calls":publication_calls,"publication_ms":publication_elapsed.as_secs_f64()*1000.0,"elapsed_ms":elapsed.as_secs_f64()*1000.0,"kernel_ms":(elapsed-publication_elapsed).as_secs_f64()*1000.0,"kernel_tps":ticks as f64/(elapsed-publication_elapsed).as_secs_f64(),"tps":ticks as f64/elapsed.as_secs_f64(),"compile_ms":compile_ms,"screen_sha256":screen_hash,"messages":messages,"native_state_sha256":native_state_hash});
        println!("FAIR_RESULT {row}");
        results.push(row);
        std::fs::write(&output, serde_json::to_vec_pretty(&results).unwrap()).unwrap();
    }
}
