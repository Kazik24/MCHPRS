//! Compile the three native-backend fixtures and print the sampled memory census.
//!
//! Run with `MCHPRS_REDPILER_PHASE_TIMINGS=1 cargo run -p mchprs_core --example census_sampled`.
use std::{fs::File, path::Path, sync::Arc};

use mchprs_blocks::BlockPos;
use mchprs_core::{
    plot::{
        PLOT_WIDTH, PlotWorld,
        worldedit::{load_schematic, paste_clipboard},
    },
    redpiler::{Compiler, CompilerOptions, TaskMonitor},
    world::{World, storage::Chunk},
};

#[path = "../test_support/chungus.rs"]
mod chungus;
#[path = "../test_support/cpus.rs"]
mod cpus;

fn compile(name: &str, world: &PlotWorld, bounds: (BlockPos, BlockPos)) {
    let mut compiler = Compiler::default();
    compiler
        .compile(
            world,
            bounds,
            CompilerOptions::default(),
            world.scheduler().iter_entries().collect(),
            Arc::new(TaskMonitor::default()),
        )
        .unwrap_or_else(|error| panic!("{name} compile rejected: {error}"));
    eprintln!("CENSUS_FIXTURE_DONE {name}");
}

fn schematic_bounds(path: &Path, origin: BlockPos) -> (BlockPos, BlockPos) {
    let clip = load_schematic(File::open(path).unwrap()).unwrap();
    (
        origin,
        origin + BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1),
    )
}

fn main() {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data");
    let zcore = cpus::CPUS[3];
    let world = cpus::load_cpu(zcore);
    let path = data.join(zcore.schematic);
    compile(zcore.name, &world, schematic_bounds(&path, zcore.origin));

    let path = data.join("piston-research/FPU_27.schem");
    let origin = BlockPos::new(8, 8, 8);
    let mut clip = load_schematic(File::open(&path).unwrap()).unwrap();
    clip.offset_x = 0;
    clip.offset_y = 0;
    clip.offset_z = 0;
    let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
    let mut world = PlotWorld::from_chunks(0, 0, chunks, Default::default());
    paste_clipboard(&mut world, &clip, origin, false);
    let bounds = (
        origin,
        origin + BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1),
    );
    compile("FPU_27", &world, bounds);

    let world = chungus::init_world();
    compile("Chungus", &world, world.get_corners());
}
