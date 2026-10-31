use mchprs_blocks::BlockPos;
use mchprs_core::{
    plot::{
        PLOT_WIDTH, PlotWorld,
        worldedit::{load_schematic, paste_clipboard},
    },
    redpiler::{Compiler, CompilerOptions, TaskMonitor},
    world::{World, storage::Chunk},
};
use std::sync::Arc;

#[path = "../test_support/chungus.rs"]
mod chungus;

fn schematic(name: &str) -> (PlotWorld, (BlockPos, BlockPos)) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data").join(name);
    let mut clip = load_schematic(std::fs::File::open(path).unwrap()).unwrap();
    clip.offset_x = 0;
    clip.offset_y = 0;
    clip.offset_z = 0;
    let origin = BlockPos::new(8, 8, 8);
    let bounds = (
        origin,
        origin + BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1),
    );
    let chunks = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
    let mut world = PlotWorld::from_chunks(0, 0, chunks, Default::default());
    paste_clipboard(&mut world, &clip, origin, false);
    (world, bounds)
}

fn check_compile(mut world: PlotWorld, bounds: (BlockPos, BlockPos)) {
    let mut compiler = Compiler::default();
    compiler
        .compile(
            &world,
            bounds,
            CompilerOptions::default(),
            world.scheduler().iter_entries().collect(),
            Arc::new(TaskMonitor::default()),
        )
        .unwrap_or_else(|error| {
            if let mchprs_core::redpiler::CompileError::Unsupported(reason) = &error {
                if let Some(pos) = reason.position {
                    panic!("{error}; block={:?}; entity={:?}", world.get_block(pos), world.get_block_entity(pos));
                }
            }
            panic!("{error}")
        });
    assert!(compiler.is_active());
    compiler.tick().unwrap();
    compiler.try_reset(&mut world, bounds).unwrap();
}

#[test]
#[ignore = "large circuit compilation gate"]
fn fpu_fixture_compiles() {
    let (world, bounds) = schematic("piston-research/FPU_27.schem");
    check_compile(world, bounds);
}

#[test]
#[ignore = "large circuit compilation gate"]
fn anpu_fixture_compiles() {
    let (world, bounds) = schematic("Q2CK@Q2CK_Anpu1_Pong_KBTV.schem");
    check_compile(world, bounds);
}

#[test]
#[ignore = "large circuit compilation gate"]
fn chungus_fixture_compiles() {
    let world = chungus::init_world();
    let bounds = world.get_corners();
    check_compile(world, bounds);
}
