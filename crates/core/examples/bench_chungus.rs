//! cargo run -p mchprs_core --profile bench --example bench_chungus --features redpiler-full-publication -- 128
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

fn main() {
    assert!(
        cfg!(feature = "redpiler-full-publication"),
        "enable redpiler-full-publication to measure complete publication"
    );
    let ticks: u32 = std::env::args().nth(1).map_or(128, |arg| arg.parse().expect("positive tick count"));
    assert!(ticks > 0);
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
    compiler
        .compile(
            &compiled,
            bounds,
            CompilerOptions::default(),
            compiled.scheduler().iter_entries().collect(),
            Arc::new(TaskMonitor::default()),
        )
        .expect("Chungus compile rejected");
    println!("compile: {:?}; checked positions: {}", start.elapsed(), positions.len());
    let mut native_time = Duration::ZERO;
    let mut compiled_time = Duration::ZERO;
    for tick in 1..=ticks {
        let start = Instant::now();
        native.tick_interpreted();
        native_time += start.elapsed();
        let start = Instant::now();
        compiler.tick().expect("compiled tick failed");
        compiler.try_flush(&mut compiled).expect("complete publication failed");
        compiled_time += start.elapsed();
        // shortcut: compare block states only, add entity/event traces before claiming full behavioral parity.
        for &pos in &positions {
            assert_eq!(
                native.get_block(pos),
                compiled.get_block(pos),
                "Chungus block trace diverged at tick {tick}, {pos:?}"
            );
        }
        native.take_block_changes();
        compiled.take_block_changes();
    }
    println!("{ticks} ticks matched; native: {native_time:?}; compiled tick + full publication: {compiled_time:?}");
}
