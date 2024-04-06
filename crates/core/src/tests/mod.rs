use std::path::Path;

use mchprs_blocks::blocks::{Block, ButtonFace};
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_save_data::plot_data::PlotData;
use mchprs_world::TickPriority;
use sha2::{Digest, Sha256};

use crate::plot::{PlotWorld, PLOT_WIDTH};
use crate::redpiler::{Compiler, CompilerOptions};
use crate::redstone;
use crate::world::storage::Chunk;
use crate::world::World;

pub fn load_test_plot(path: impl AsRef<Path>) -> PlotWorld {
    let data = PlotData::load_from_file(path, false).unwrap();

    let chunks: Vec<Chunk> = data
        .chunk_data
        .into_iter()
        .enumerate()
        .map(|(i, c)| Chunk::load(i as i32 / PLOT_WIDTH, i as i32 % PLOT_WIDTH, c))
        .collect();
    PlotWorld {
        x: 0,
        z: 0,
        chunks,
        to_be_ticked: data.pending_ticks.into_iter().collect(),
        packet_senders: Vec::new(),
    }
}

pub fn click_floor_button(world: &mut PlotWorld, pos: BlockPos) {
    let block = world.get_block(pos);
    let Block::StoneButton { mut button } = block else {
        return;
    };
    assert!(matches!(button.face, ButtonFace::Floor));
    button.powered = true;
    world.set_block(pos, Block::StoneButton { button });
    world.schedule_tick(pos, 10, TickPriority::Normal);
    redstone::update_surrounding_blocks(world, pos);
    redstone::update_surrounding_blocks(world, pos.offset(BlockFace::Bottom));
}

const CHUNGUS_START_BUTTON: BlockPos = BlockPos::new(187, 99, 115);

#[test]
fn can_load_chungus_plot() {
    load_test_plot("./benches/chungus_mandelbrot_plot");
}

fn calculate_world_hash(world: &PlotWorld) -> Box<[u8]> {
    let mut hasher = Sha256::new();
    for chunk in &world.chunks {
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..256 {
                    let ch = chunk.get_block(x, y, z);
                    hasher.update(ch.to_le_bytes());
                }
            }
        }
    }
    let hash = hasher.finalize();
    hash.to_vec().into_boxed_slice()
}

#[test]
fn run_mandelbrot_chungus() {
    let mut plot = load_test_plot("./benches/chungus_mandelbrot_plot");
    click_floor_button(&mut plot, CHUNGUS_START_BUTTON);

    for _ in 0..1000 {
        plot.tick_interpreted();
    }

    let hash = calculate_world_hash(&plot);
    //hash after 1000 ticks for interpreted engine (master commit 33cfc6dd84)
    assert_eq!(hash.as_ref(),b"\xaf\xe1\xb6\xf2\xe9\xfa\xe4\x5b\xa9\x68\xc1\x0a\x6e\x4b\xf7\xb0\x29\x78\xc5\xb3\x9c\xc3\xec\xb4\xe0\x73\x0a\xf3\x8e\x94\x20\x05");
}

#[test]
fn run_mandelbrot_chungus_compiled() {
    let mut plot = load_test_plot("./benches/chungus_mandelbrot_plot");

    let mut compiler: Compiler = Default::default();
    let options = CompilerOptions::parse("-O");
    let bounds = plot.get_corners();
    compiler.compile(&plot, bounds, options, Vec::new(), Default::default());
    compiler.on_use_block(CHUNGUS_START_BUTTON);

    for _ in 0..1000 {
        compiler.tick();
    }
    compiler.flush(&mut plot);

    let hash = calculate_world_hash(&plot);
    //hash after 1000 ticks for compiled engine (master commit 33cfc6dd84)
    assert_eq!(hash.as_ref(),b"\x08\xbc\x30\xaf\x0f\xa9\x8f\xa1\x3b\x9e\x21\x93\xfe\xf6\xba\xf9\xc2\x3a\x1d\xcf\x54\xa5\x92\xdc\xeb\x9e\xb7\x16\x27\x0a\xae\xda");
}

#[test]
fn run_mandelbrot_chungus_interpreted_to_compiled() {
    let mut plot = load_test_plot("./benches/chungus_mandelbrot_plot");
    click_floor_button(&mut plot, CHUNGUS_START_BUTTON);

    for _ in 0..1000 {
        plot.tick_interpreted();
    }

    let mut compiler: Compiler = Default::default();
    let options = CompilerOptions::parse("-O");
    let bounds = plot.get_corners();
    let ticks = plot.to_be_ticked.iter_entries().collect();
    compiler.compile(&plot, bounds, options, ticks, Default::default());

    for _ in 0..1000 {
        compiler.tick();
    }
    compiler.flush(&mut plot);

    let hash = calculate_world_hash(&plot);
    //hash after 1000 interpreted and then 1000 compiled ticks (master commit 33cfc6dd84)
    assert_eq!(hash.as_ref(),b"\x02\xaf\x81\x4f\x25\x38\xde\x2e\xbb\x1b\x43\xc5\x19\x9b\xb7\xee\x0f\x85\x07\xcd\xbc\x03\x22\xaf\xdd\xf4\x19\xe3\xd0\x1d\xf7\x39");
}

#[test]
fn run_mandelbrot_chungus_compiled_to_interpreted() {
    let mut plot = load_test_plot("./benches/chungus_mandelbrot_plot");

    let mut compiler: Compiler = Default::default();
    let options = CompilerOptions::parse("-O");
    let bounds = plot.get_corners();
    compiler.compile(&plot, bounds, options, Vec::new(), Default::default());
    compiler.on_use_block(CHUNGUS_START_BUTTON);

    for _ in 0..1000 {
        compiler.tick();
    }
    compiler.flush(&mut plot);

    compiler.reset(&mut plot, bounds);

    for _ in 0..28 {
        // todo when transferring from compiled to interpreted, chungus stops running after around 30 ticks
        // println!("{}", plot.to_be_ticked.len());
        plot.tick_interpreted();
    }

    let hash = calculate_world_hash(&plot);
    //hash after 1000 compiled and then 1000 interpreted ticks (master commit 33cfc6dd84)
    assert_eq!(hash.as_ref(),b"\x66\xb7\x25\x7c\x39\xd1\xc6\x14\xe1\xd0\x97\xbb\x50\xf7\xe9\xd9\x2e\xd8\xc5\xd4\x23\x2f\x68\x1f\x56\x5d\x7e\xe6\xa0\x74\x38\xda");
}
