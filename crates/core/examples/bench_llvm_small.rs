use mchprs_blocks::{BlockFace, BlockPos, blocks::{Block, LeverFace}};
use mchprs_core::{
    plot::{PLOT_WIDTH, PlotWorld, worldedit::{load_schematic, paste_clipboard}},
    redpiler::{Compiler, CompilerOptions, TaskMonitor},
    redstone,
    world::{World, storage::Chunk},
};
use std::{path::PathBuf, sync::Arc, time::Instant};

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let interpreter = args.iter().any(|arg| arg == "--interpreter");
    let counter = args.iter().any(|arg| arg == "--counter");
    let llvm = args.iter().any(|arg| arg == "--llvm");
    let iterations = args.iter().find_map(|arg| arg.parse().ok()).unwrap_or(1_000_000);
    let schematic = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(if counter {
        "../../test_data/instant-pistons-io/COUNTER_BASIC.schem"
    } else {
        "../../test_data/graph-fixtures/simple_and_gate.schem"
    });
    let clipboard = load_schematic(std::fs::File::open(schematic).unwrap()).unwrap();
    let anchor = BlockPos::new(64, 40, 64);
    let start = anchor - BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z);
    let mut world = PlotWorld::from_chunks(
        0,
        0,
        (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
        Default::default(),
    );
    paste_clipboard(&mut world, &clipboard, anchor, false);
    let max = start + BlockPos::new(clipboard.size_x as i32 - 1, clipboard.size_y as i32 - 1, clipboard.size_z as i32 - 1);
    let bounds = (start, max);
    // Walk the pasted cuboid in X/Y/Z order and keep only lever blocks for repeated input toggles.
    let levers: Vec<_> = (start.x..=max.x)
        .flat_map(|x| (start.y..=max.y).flat_map(move |y| (start.z..=max.z).map(move |z| BlockPos::new(x, y, z))))
        .filter(|&pos| matches!(world.get_block(pos), Block::Lever { .. }))
        .collect();
    assert_eq!(levers.len(), if counter { 1 } else { 2 }, "unexpected input lever count");
    let mut compiler = Compiler::default();
    let compile_start = Instant::now();
    if !interpreter {
        compiler.compile(
            &world,
            bounds,
            CompilerOptions { llvm, ..Default::default() },
            world.scheduler().iter_entries().collect(),
            Arc::new(TaskMonitor::default()),
        ).unwrap();
    }
    let compile_seconds = compile_start.elapsed().as_secs_f64();
    for _ in 0..10_000 {
        for &lever in &levers {
            if interpreter { toggle(&mut world, lever); } else { compiler.try_on_use_block(lever).unwrap(); }
        }
        if interpreter { world.tick_interpreted(); } else { compiler.tick().unwrap(); }
    }
    let start = Instant::now();
    for _ in 0..iterations {
        for &lever in &levers {
            if interpreter { toggle(&mut world, lever); } else { compiler.try_on_use_block(lever).unwrap(); }
        }
        if interpreter { world.tick_interpreted(); } else { compiler.tick().unwrap(); }
    }
    let elapsed = start.elapsed().as_secs_f64();
    if !interpreter { assert!(compiler.is_active()); }
    println!("{{\"fixture\":\"{}\",\"backend\":\"{}\",\"inputs_per_tick\":{},\"ticks\":{},\"compile_seconds\":{compile_seconds},\"elapsed_seconds\":{elapsed},\"tps\":{}}}", if counter { "COUNTER_BASIC.schem" } else { "simple_and_gate.schem" }, if interpreter { "interpreter" } else if llvm { "llvm" } else { "rust" }, levers.len(), iterations, iterations as f64 / elapsed);
}

fn toggle(world: &mut PlotWorld, pos: BlockPos) {
    let Block::Lever { mut lever } = world.get_block(pos) else { panic!("missing lever at {pos:?}") };
    lever.powered = !lever.powered;
    world.set_block(pos, Block::Lever { lever });
    redstone::update_surrounding_blocks(world, pos);
    let support = match lever.face {
        LeverFace::Ceiling => BlockFace::Top,
        LeverFace::Floor => BlockFace::Bottom,
        LeverFace::Wall => lever.facing.opposite().block_face(),
    };
    redstone::update_surrounding_blocks(world, pos.offset(support));
}
