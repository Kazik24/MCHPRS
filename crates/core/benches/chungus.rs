use std::path::Path;
use std::time::Instant;

use criterion::*;
use mchprs_blocks::blocks::{Block, ButtonFace};
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_core::plot::{PLOT_WIDTH, PlotWorld};
use mchprs_core::redstone;
use mchprs_core::world::{World, storage::Chunk};
use mchprs_save_data::plot_data::PlotData;
use mchprs_world::TickPriority;

const START_BUTTON: BlockPos = BlockPos::new(187, 99, 115);

fn init_world(path: impl AsRef<Path>) -> PlotWorld {
    let data = PlotData::load_from_file(path, false).unwrap();
    let chunks: Vec<Chunk> = data
        .chunk_data
        .into_iter()
        .enumerate()
        .map(|(i, chunk)| Chunk::load(i as i32 / PLOT_WIDTH, i as i32 % PLOT_WIDTH, chunk))
        .collect();
    let mut world = PlotWorld::from_chunks(0, 0, chunks, data.pending_ticks.into_iter().collect());
    let Block::StoneButton { mut button } = world.get_block(START_BUTTON) else {
        panic!("Chungus start button missing");
    };
    assert_eq!(button.face, ButtonFace::Floor);
    button.powered = true;
    world.set_block(START_BUTTON, Block::StoneButton { button });
    world.schedule_tick(START_BUTTON, 10, TickPriority::Normal);
    redstone::update_surrounding_blocks(&mut world, START_BUTTON);
    redstone::update_surrounding_blocks(&mut world, START_BUTTON.offset(BlockFace::Bottom));
    world
}

fn chungus_native_tick(c: &mut Criterion) {
    let mut world = init_world("./benches/chungus_mandelbrot_plot");
    c.bench_function("chungus-mandelbrot-native-tick", |b| b.iter(|| black_box(world.tick_interpreted())));
}

fn mandelbrot_full(_c: &mut Criterion) {
    if !std::env::args().any(|arg| "chungus-mandelbrot-full".contains(&arg)) {
        return;
    }
    let mut world = init_world("./benches/chungus_mandelbrot_plot");
    let start = Instant::now();
    for _ in 0..12_411_975 {
        world.tick_interpreted();
    }
    println!("Native Mandelbrot benchmark completed in {:?}", start.elapsed());
}

criterion_group!(chungus, chungus_native_tick, mandelbrot_full);
criterion_main!(chungus);
