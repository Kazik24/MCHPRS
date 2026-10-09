use std::path::Path;
use std::time::Instant;

use criterion::*;
use mchprs_blocks::BlockPos;
use mchprs_core::plot::{PlotWorld, PLOT_WIDTH};
use mchprs_core::redpiler::{Compiler, CompilerOptions};
use mchprs_core::world::storage::Chunk;
use mchprs_save_data::plot_data::PlotData;

const START_BUTTON: BlockPos = BlockPos::new(187, 99, 115);

fn load_world(path: impl AsRef<Path>) -> PlotWorld {
    let data = PlotData::load_from_file(path, false).unwrap();

    let chunks: Vec<Chunk> = data
        .chunk_data
        .into_iter()
        .enumerate()
        .map(|(i, c)| Chunk::load(i as i32 / PLOT_WIDTH, i as i32 % PLOT_WIDTH, c))
        .collect();

    PlotWorld::from_chunks(0, 0, chunks, data.pending_ticks.into_iter().collect())
}

fn init_compiler() -> Compiler {
    let mut world = load_world("./benches/chungus_mandelbrot_plot");
    let mut compiler: Compiler = Default::default();
    if std::env::var_os("MCHPRS_BENCH_INSTANT").is_some() {
        let fixture =
            std::fs::File::open("../../test_data/instant-pistons-io/INSTANT_OBSERVER.schem")
                .unwrap();
        let clipboard = mchprs_core::plot::worldedit::load_schematic(fixture).unwrap();
        mchprs_core::plot::worldedit::paste_clipboard(
            &mut world,
            &clipboard,
            BlockPos::new(
                4 + clipboard.offset_x,
                220 + clipboard.offset_y,
                4 + clipboard.offset_z,
            ),
            false,
        );
    }

    let mut options = CompilerOptions::parse("-O").unwrap();
    if std::env::var_os("MCHPRS_BENCH_INSTANT").is_some() {
        // Port discovery also traverses the ordinary CPU's large dust network.
        options.budget_multiplier = 8;
    }
    let bounds = world.get_corners();
    let monitor = Default::default();
    compiler
        .compile(&world, bounds, options, Vec::new(), monitor)
        .unwrap();
    compiler.on_use_block(START_BUTTON);
    compiler
}

fn chungus_mandelbrot(c: &mut Criterion) {
    let mut compiler = init_compiler();

    c.bench_function("chungus-mandelbrot-tick", |b| {
        b.iter(|| compiler.tick());
    });
}

fn mandelbrot_full(_c: &mut Criterion) {
    // HACKKKKKKK, oh how I wish Criterion::filter_matches was public
    let run = std::env::args().any(|arg| "chungus-mandelbrot-full".contains(&arg));
    if !run {
        return;
    }

    println!("Running full chungus mandelbrot, this can take a while!");
    let mut compiler = init_compiler();
    let start = Instant::now();
    for _ in 0..12411975 {
        compiler.tick();
    }
    println!("Mandelbrot benchmark completed in {:?}", start.elapsed());
}

criterion_group!(chungus, chungus_mandelbrot, mandelbrot_full);
criterion_main!(chungus);
