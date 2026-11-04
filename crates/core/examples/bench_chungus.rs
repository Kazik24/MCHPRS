//! cargo run -p mchprs_core --profile bench --example bench_chungus --features redpiler-full-publication -- 512 3
use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, Instant},
};

use mchprs_blocks::{BlockFace, blocks::Block};
use mchprs_core::{
    redpiler::{Compiler, CompilerOptions, TaskMonitor},
    world::{World, for_each_block_optimized},
};

#[path = "../test_support/chungus.rs"]
mod support;

fn run_sample(ticks: u32, sample: usize, llvm: bool) -> serde_json::Value {
    let mut native = support::init_world();
    let mut compiled = support::init_world();
    let bounds = native.get_corners();
    let mut positions = HashSet::new();
    for_each_block_optimized(&native, bounds.0, bounds.1, |pos| {
        let block = native.get_block(pos);
        if block != Block::Air {
            positions.insert(pos);
        }
        if matches!(block, Block::Piston { .. }) {
            for face in [
                BlockFace::Top,
                BlockFace::Bottom,
                BlockFace::North,
                BlockFace::South,
                BlockFace::East,
                BlockFace::West,
            ] {
                positions.insert(pos.offset(face));
                positions.insert(pos.offset(face).offset(face));
            }
        }
    });
    assert!(!positions.is_empty());
    let mut positions: Vec<_> = positions.into_iter().collect();
    positions.sort_unstable_by_key(|pos| (pos.x, pos.y, pos.z));
    let mut compiler = Compiler::default();
    let start = Instant::now();
    let result = compiler.compile(
        &compiled,
        bounds,
        CompilerOptions {
            llvm,
            publication: Some(mchprs_core::redpiler::PublicationMode::All),
            ..Default::default()
        },
        compiled.scheduler().iter_entries().collect(),
        Arc::new(TaskMonitor::default()),
    );
    let compile_seconds = start.elapsed().as_secs_f64();
    if let Err(error) = result {
        return serde_json::json!({"status":"compile_rejected","backend":if llvm {"llvm"} else {"rust"},"reason":error.to_string(),"compile_seconds":compile_seconds});
    }
    let mut native_time = Duration::ZERO;
    let mut compiled_time = Duration::ZERO;
    let mut publication_time = Duration::ZERO;
    let mut changed_blocks = 0usize;
    for tick in 1..=ticks {
        let start = Instant::now();
        native.tick_interpreted();
        native_time += start.elapsed();
        let start = Instant::now();
        compiler.tick().expect("compiled tick failed");
        compiled_time += start.elapsed();
        let start = Instant::now();
        compiler.try_flush(&mut compiled).expect("complete publication failed");
        publication_time += start.elapsed();
        // shortcut: compare block states only, add entity/event traces before claiming full behavioral parity.
        for &pos in &positions {
            assert_eq!(
                native.get_block(pos),
                compiled.get_block(pos),
                "Chungus block trace diverged at tick {tick}, {pos:?}"
            );
        }
        changed_blocks += native.take_block_changes().len();
        compiled.take_block_changes();
    }
    serde_json::json!({
        "sample":sample,"status":"passed","backend":if llvm {"llvm"} else {"rust"},"ticks":ticks,"checked_positions":positions.len(),"changed_blocks":changed_blocks,
        "compile_seconds":compile_seconds,"native_tps":ticks as f64/native_time.as_secs_f64(),
        "compiled_tick_tps":ticks as f64/compiled_time.as_secs_f64(),
        "compiled_publication_tps":ticks as f64/(compiled_time+publication_time).as_secs_f64(),
        "native_seconds":native_time.as_secs_f64(),"compiled_tick_seconds":compiled_time.as_secs_f64(),
        "publication_seconds":publication_time.as_secs_f64(),"publication_visibility":"complete",
        "validation":"per-tick block states; entities and events not compared"
    })
}

fn main() {
    let llvm = std::env::args().any(|arg| arg == "--llvm");
    let paired = std::env::args().any(|arg| arg == "--paired");
    let ticks: u32 = std::env::args().nth(1).map_or(512, |arg| arg.parse().expect("positive tick count"));
    let runs: usize = std::env::args().nth(2).map_or(3, |arg| arg.parse().expect("positive sample count"));
    assert!(ticks > 0 && runs > 0);
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(if paired {
        "../../target/redpiler-bench-results/chungus-paired.json"
    } else if llvm {
        "../../target/redpiler-bench-results/chungus-llvm.json"
    } else {
        "../../target/redpiler-bench-results/chungus.json"
    });
    std::fs::create_dir_all(output.parent().unwrap()).unwrap();
    let mut reports = Vec::new();
    for sample in 1..=runs {
        let backends: &[bool] = if !paired {
            std::slice::from_ref(&llvm)
        } else if sample % 2 == 1 {
            &[false, true]
        } else {
            &[true, false]
        };
        for &backend in backends {
            let report = run_sample(ticks, sample, backend);
            println!("Chungus sample {sample}: {report}");
            reports.push(report);
            std::fs::write(&output, serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
        }
    }
}
