use std::time::Instant;

use criterion::*;
use mchprs_core::redpiler::{Compiler, CompilerOptions, TaskMonitor};
use std::sync::Arc;

#[path = "../test_support/chungus.rs"]
mod support;
use support::init_world;

fn chungus_native_tick(c: &mut Criterion) {
    let mut world = init_world();
    c.bench_function("chungus-mandelbrot-native-tick", |b| b.iter(|| black_box(world.tick_interpreted())));
}

fn chungus_redpiler_tick(c: &mut Criterion) {
    let world = init_world();
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &world,
            world.get_corners(),
            CompilerOptions::default(),
            world.scheduler().iter_entries().collect(),
            Arc::new(TaskMonitor::default()),
        )
        .unwrap();
    c.bench_function("chungus-mandelbrot-redpiler-tick", |b| b.iter(|| black_box(compiler.tick().unwrap())));
}

fn mandelbrot_full(_c: &mut Criterion) {
    if !std::env::args().any(|arg| "chungus-mandelbrot-full".contains(&arg)) {
        return;
    }
    let mut world = init_world();
    let start = Instant::now();
    for _ in 0..12_411_975 {
        world.tick_interpreted();
    }
    println!("Native Mandelbrot benchmark completed in {:?}", start.elapsed());
}

criterion_group!(chungus, chungus_native_tick, chungus_redpiler_tick, mandelbrot_full);
criterion_main!(chungus);
