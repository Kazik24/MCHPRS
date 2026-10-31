use mchprs_blocks::{
    BlockFace, BlockPos,
    blocks::{Block, ButtonFace},
};
use mchprs_core::{
    plot::{PLOT_WIDTH, PlotWorld},
    redstone,
    world::{World, storage::Chunk},
};
use mchprs_save_data::plot_data::PlotData;
use mchprs_world::TickPriority;

pub fn init_world() -> PlotWorld {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("benches/chungus_mandelbrot_plot");
    let data = PlotData::load_from_file(path, false).unwrap();
    let width = data.chunk_data.len().isqrt();
    assert!(width > 0 && width * width == data.chunk_data.len() && width <= PLOT_WIDTH as usize);
    let mut chunks: Vec<_> = (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect();
    // This saved benchmark uses a 16-chunk plot; today's plot width is 32.
    for (index, chunk) in data.chunk_data.into_iter().enumerate() {
        let (x, z) = (index / width, index % width);
        chunks[x * PLOT_WIDTH as usize + z] = Chunk::load(x as i32, z as i32, chunk);
    }
    let mut world = PlotWorld::from_chunks(0, 0, chunks, data.pending_ticks.into_iter().collect());
    let pos = BlockPos::new(187, 99, 115);
    let Block::StoneButton { mut button } = world.get_block(pos) else {
        panic!("Chungus start button missing");
    };
    assert_eq!(button.face, ButtonFace::Floor);
    button.powered = true;
    world.set_block(pos, Block::StoneButton { button });
    world.schedule_tick(pos, 10, TickPriority::Normal);
    redstone::update_surrounding_blocks(&mut world, pos);
    redstone::update_surrounding_blocks(&mut world, pos.offset(BlockFace::Bottom));
    world
}
