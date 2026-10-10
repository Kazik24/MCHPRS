//! Deterministic end-to-end interpreter benchmarks, with frozen-state assertions.
//! Run once by default; CPU initialization and verification are excluded from timing.
#[path = "support/cpus.rs"]
mod support;
use support::*;

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let iterations = args
        .windows(2)
        .find(|pair| pair[0] == "--iterations")
        .map_or(1, |pair| {
            pair[1]
                .parse::<usize>()
                .expect("--iterations needs a positive integer")
        });
    assert!(iterations > 0);
    let selected = args
        .windows(2)
        .find(|pair| pair[0] == "--cpu")
        .map(|pair| pair[1].as_str());
    let output = args
        .windows(2)
        .find(|pair| pair[0] == "--output")
        .map(|pair| &pair[1]);
    let label = args
        .windows(2)
        .find(|pair| pair[0] == "--label")
        .map_or("current", |pair| pair[1].as_str());
    let mut reports = Vec::new();
    let screen_only = args.iter().any(|arg| arg == "--screen-only");
    let flush_every = args
        .windows(2)
        .find(|pair| pair[0] == "--flush-every")
        .map_or(0, |pair| {
            pair[1]
                .parse::<u32>()
                .expect("--flush-every needs a tick interval")
        });
    if let Some(name) = selected {
        assert!(
            CPUS.iter().any(|cpu| cpu.name == name),
            "unknown CPU {name}"
        );
    }
    for cpu in CPUS {
        if selected.is_some_and(|name| name != cpu.name) {
            continue;
        }
        let expected = reference(cpu);
        let mut times = Vec::new();
        let mut active_times = Vec::new();
        let mut active_ticks = 0;
        let mut visual_counts = Vec::new();
        let mut instant_cache_stats = Vec::new();
        for sample in 1..=iterations {
            let timing = replay_with_visuals(cpu, &expected, screen_only, flush_every);
            println!(
                "{} sample {sample}: {:.6}s / 50,000 game ticks ({:.1} TPS); active window: {} ticks / {:.6}s ({:.1} TPS); all assertions passed",
                cpu.name,
                timing.total.as_secs_f64(),
                50_000.0 / timing.total.as_secs_f64(),
                timing.active_ticks,
                timing.active.as_secs_f64(),
                f64::from(timing.active_ticks) / timing.active.as_secs_f64()
            );
            times.push(timing.total);
            active_times.push(timing.active);
            active_ticks = timing.active_ticks;
            visual_counts.push(timing.visual_counts);
            instant_cache_stats.push(timing.instant_piston_cache_stats);
            if flush_every != 0 {
                println!(
                    "visual totals: {} flushes, {} sections, {} block records",
                    timing.visual_counts.0, timing.visual_counts.1, timing.visual_counts.2
                );
            }
        }
        times.sort();
        active_times.sort();
        println!(
            "{} median: {:.6}s",
            cpu.name,
            times[times.len() / 2].as_secs_f64()
        );
        reports.push(serde_json::json!({
            "cpu":cpu.name,"schematic_sha256":cpu.sha256,"game_ticks":50_000,
            "samples_seconds":times.iter().map(|time| time.as_secs_f64()).collect::<Vec<_>>(),
            "median_seconds":times[times.len()/2].as_secs_f64(),
            "median_tps":50_000.0/times[times.len()/2].as_secs_f64(),
            "active_window_game_ticks":active_ticks,
            "active_samples_seconds":active_times.iter().map(|time| time.as_secs_f64()).collect::<Vec<_>>(),
            "active_median_seconds":active_times[active_times.len()/2].as_secs_f64(),
            "active_median_tps":f64::from(active_ticks)/active_times[active_times.len()/2].as_secs_f64(),
            "assertions":"whole-world checkpoints, ordered chat, per-tick Pong screen and BubbleSort RAM, BubbleSort halt boundary",
            "visual_mode":if screen_only { "screen" } else { "all" },
            "visual_flush_every_game_ticks":flush_every,
            "visual_counts":visual_counts,
            "instant_piston_cache_stats":instant_cache_stats,
        }));
    }
    if let Some(path) = output {
        let report = serde_json::json!({"schema":1,"label":label,"profile":"bench/release","samples":reports});
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
}
