//! Explicit reference capture, before changing interpreter behavior.
#[path = "../benches/support/cpus.rs"]
#[allow(dead_code)]
mod cpus;
use cpus::*;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

fn main() {
    let destination = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("usage: cpu_references <new-output-directory> [cpu-name]"),
    );
    let selected = std::env::args().nth(2);
    if let Some(name) = &selected {
        assert!(
            CPUS.iter().any(|cpu| cpu.name == name),
            "unknown CPU {name}"
        );
    }
    std::fs::create_dir_all(&destination).unwrap();
    for cpu in CPUS {
        if selected.as_ref().is_some_and(|name| name != cpu.name) {
            continue;
        }
        let path = destination.join(format!("{}.json", cpu.name));
        assert!(
            !path.exists(),
            "refusing to replace reference {}",
            path.display()
        );
        if cpu.name == "anpu_pong" {
            assert!(
                !destination.join("anpu_screen.json").exists(),
                "refusing to replace screen reference"
            );
        }
        let mut world = load_cpu(cpu);
        let mut trace = Vec::new();
        let mut ram_trace = Vec::new();
        collect_ram(&world, cpu, 0, &mut ram_trace);
        let mut frames = Vec::new();
        if cpu.name == "anpu_pong" {
            collect_screen(&world, 0, &mut frames);
        }
        let mut checkpoints = vec![checkpoint(&world, 0, &trace)];
        click_cpu(&mut world, cpu, cpu.start);
        let mut elapsed = Duration::ZERO;
        for tick in 1..=50_000 {
            let now = Instant::now();
            world.tick_interpreted();
            elapsed += now.elapsed();
            collect_chat(&world, tick, &mut trace);
            collect_ram(&world, cpu, tick, &mut ram_trace);
            if cpu.name == "anpu_pong" {
                collect_screen(&world, tick, &mut frames);
            }
            if CHECKPOINTS.contains(&tick) {
                let sample = checkpoint(&world, tick, &trace);
                println!("{}: {sample:?}; simulation {elapsed:?}", cpu.name);
                checkpoints.push(sample);
            }
        }
        if let Some(stop) = cpu.stop {
            click_cpu(&mut world, cpu, stop);
            for tick in 50_001..=50_100 {
                world.tick_interpreted();
                collect_chat(&world, tick, &mut trace);
            }
            checkpoints.push(checkpoint(&world, 50_100, &trace));
        }
        let reference = Reference {
            schema: 1,
            schematic_sha256: cpu.sha256.into(),
            checkpoints,
            chat_trace: trace,
            ram_trace,
        };
        std::fs::write(path, serde_json::to_vec_pretty(&reference).unwrap()).unwrap();
        if cpu.name == "anpu_pong" {
            println!("screen: {} distinct frames", frames.len());
            std::fs::write(
                destination.join("anpu_screen.json"),
                serde_json::to_vec_pretty(&frames).unwrap(),
            )
            .unwrap();
        }
        println!("{}: tick-only time {elapsed:?}", cpu.name);
    }
}
