//! Paired Graph/interpreter runtime measurements for the BubbleSort CPU.
#[path = "support/cpus.rs"]
mod support;

use mchprs_core::redpiler::{BackendKind, Compiler, CompilerOptions};
use mchprs_core::world::World;
use std::sync::Arc;

fn main() {
    let cpu = support::CPUS
        .iter()
        .copied()
        .find(|cpu| cpu.name == "cpu_bubblesort")
        .expect("CPU_BubbleSort descriptor");
    let world = support::load_cpu(cpu);
    let ticks = world.scheduler().iter_entries().collect();
    let mut compiler = Compiler::default();
    match compiler.compile(
        &world,
        world.get_corners(),
        CompilerOptions {
            budget_multiplier: 8,
            ..Default::default()
        },
        ticks,
        Arc::default(),
    ) {
        Ok(()) => println!("backend={:?}", compiler.backend_kind()),
        Err(error) => panic!("Graph compilation after the existing CPU setup failed: {error}"),
    }
    assert_eq!(compiler.backend_kind(), Some(BackendKind::Graph));
}
