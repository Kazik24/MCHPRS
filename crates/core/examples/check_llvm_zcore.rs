#[path = "../test_support/cpus.rs"]
#[allow(dead_code)]
mod cpus;

use mchprs_blocks::BlockPos;
use mchprs_core::{
    plot::worldedit::load_schematic,
    redpiler::{CompileError, Compiler, CompilerOptions, TaskMonitor},
    world::World,
};
use std::{fs::File, sync::Arc, time::Instant};

fn main() {
    let cpu = if std::env::args().any(|arg| arg == "--pm1") {
        cpus::Cpu {
            schematic: "PM1_SORT_FIXED.schem",
            sha256: "ad68a0160990c72612c0105802fade337758de127dcdbd4cc1a195059ad2d893",
            ..cpus::CPUS[0]
        }
    } else {
        cpus::CPUS[3]
    };
    let world = cpus::load_cpu(cpu);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test_data")
        .join(cpu.schematic);
    let clip = load_schematic(File::open(path).unwrap()).unwrap();
    let bounds = (
        cpu.origin,
        cpu.origin + BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1),
    );
    let ticks = world.scheduler().iter_entries().collect();
    let mut compiler = Compiler::default();
    let started = Instant::now();
    let result = compiler.compile(
        &world,
        bounds,
        CompilerOptions {
            llvm: !std::env::args().any(|arg| arg == "--rust"),
            ..Default::default()
        },
        ticks,
        Arc::new(TaskMonitor::default()),
    );
    if let Err(CompileError::Unsupported(error)) = &result
        && let Some(pos) = error.position
    {
        eprintln!("rejected position={pos:?} block={:?} entity={:?}", world.get_block(pos), world.get_block_entity(pos));
    }
    println!(
        "fixture={} schematic={} setup_ms={:.3} active={} result={:?}",
        cpu.name,
        cpu.schematic,
        started.elapsed().as_secs_f64() * 1000.0,
        compiler.is_active(),
        result.map_err(|error| error.to_string())
    );
}
