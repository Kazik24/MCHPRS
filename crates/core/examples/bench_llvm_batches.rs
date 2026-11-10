//! End-only native tick batching experiment. Compile once; restart fresh executors per trial.
#[path = "../test_support/cpus.rs"]
#[allow(dead_code)]
mod cpus;

use mchprs_blocks::BlockPos;
use mchprs_core::{plot::worldedit::load_schematic, redpilator::CompilerOptions, world::World};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, time::Instant};

fn main() {
    let output = PathBuf::from(std::env::args().nth(1).expect("output JSON path"));
    let mut results = Vec::new();
    for index in [0, 3] {
        let cpu = if index == 0 {
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
        let pristine = cpus::load_cpu(cpu);
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test_data").join(cpu.schematic);
        let clip = load_schematic(std::fs::File::open(path).unwrap()).unwrap();
        let bounds = (
            cpu.origin,
            cpu.origin + BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1),
        );
        let (_, seed, _) = cpus::compile_cpu_with_options(
            pristine,
            cpu,
            CompilerOptions {
                llvm: true,
                ..Default::default()
            },
        );
        let mut configs = vec![(0, 1)];
        let sizes = if std::env::args().any(|arg| arg == "--large") {
            vec![8, 16, 32]
        } else {
            (2..=6).collect()
        };
        for count in sizes {
            let modes = if count % 2 == 0 { [1, 2, 3] } else { [3, 2, 1] };
            configs.extend(modes.into_iter().map(|mode| (mode, count)));
        }
        configs.push((0, 1));
        let mut expected_snapshot: Option<Vec<u32>> = None;
        for (mode, batch) in configs {
            let mut world = cpus::load_cpu(cpu);
            let mut compiler = seed
                .benchmark_restart(&world, bounds, world.scheduler().iter_entries().collect())
                .unwrap();
            assert!(compiler.stats().unwrap().cached);
            compiler.benchmark_tick_batch(0, mode).unwrap();
            compiler.try_on_use_block(cpu.origin + cpu.start).unwrap();
            let prep = if index == 0 { 299 } else { 413 };
            for _ in 0..prep {
                compiler.tick().unwrap();
            }
            if index == 3 {
                compiler.try_on_use_block(cpu.origin + BlockPos::new(207, 10, 154)).unwrap();
            }
            let ticks = if index == 0 { 11752 } else { 5000 };
            let mut remaining = ticks;
            let started = Instant::now();
            while remaining > 0 {
                let count = remaining.min(batch);
                if mode == 0 {
                    for _ in 0..count {
                        compiler.tick().unwrap();
                    }
                } else {
                    compiler.benchmark_tick_batch(count, mode).unwrap();
                }
                remaining -= count;
            }
            let elapsed = started.elapsed();
            let snapshot = compiler.benchmark_snapshot().unwrap();
            if let Some(expected) = &expected_snapshot {
                if &snapshot != expected {
                    let first = snapshot.iter().zip(expected).position(|(a, b)| a != b);
                    panic!(
                        "full native state differs: {} mode={mode} batch={batch}, lengths {} vs {}, first differing word {first:?}",
                        cpu.name,
                        snapshot.len(),
                        expected.len()
                    );
                }
            } else {
                expected_snapshot = Some(snapshot.clone());
            }
            let state_hash = format!(
                "{:x}",
                Sha256::digest(snapshot.iter().flat_map(|word| word.to_le_bytes()).collect::<Vec<_>>())
            );
            if index == 0 {
                for _ in 0..4 {
                    compiler.tick().unwrap();
                }
            }
            compiler.try_reset(&mut world, (cpu.origin, cpu.origin)).unwrap();
            let screen: Value = json!(cpus::screen(&world, cpu));
            let screen_hash = format!("{:x}", Sha256::digest(serde_json::to_vec(&screen).unwrap()));
            assert_eq!(
                screen_hash,
                if index == 0 {
                    "322d3d336985811875bc4e99b1eec21b69a83792efda6c0b20ac270eef407e3e"
                } else {
                    "563a79fbd276a5b3601a404c3b7f278fe302f171960d5a6ed294c34219c7175b"
                }
            );
            let messages = world.command_output().count();
            if index == 0 {
                let reference = cpus::reference(cpus::CPUS[0]);
                let expected: Vec<_> = reference
                    .chat_trace
                    .iter()
                    .filter(|(tick, _)| *tick <= 12051)
                    .map(|(_, text)| text.as_str())
                    .collect();
                assert_eq!(world.command_output().collect::<Vec<_>>(), expected, "ordered PM1 command outputs");
                assert_eq!(messages, 1534);
                assert!(world.scheduler().iter_entries().next().is_none());
                assert!(world.piston_state().events.is_empty() && world.piston_state().motions.is_empty());
            }
            let row = json!({"fixture":cpu.name,"mode":mode,"batch":batch,"ticks":ticks,"kernel_ms":elapsed.as_secs_f64()*1000.0,"tps":ticks as f64/elapsed.as_secs_f64(),"state_sha256":state_hash,"screen_sha256":screen_hash,"messages":messages});
            println!("BATCH_RESULT {row}");
            results.push(row);
            std::fs::write(&output, serde_json::to_vec_pretty(&results).unwrap()).unwrap();
        }
    }
}
