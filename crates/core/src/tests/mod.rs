use std::path::Path;

use mchprs_blocks::blocks::{Block, ButtonFace};
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_save_data::plot_data::PlotData;
use mchprs_world::TickPriority;
use sha2::{Digest, Sha256};

use crate::plot::{PlotWorld, PLOT_WIDTH};
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
        to_be_ticked: data.pending_ticks,
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

#[test]
fn run_mandelbrot_chungus() {
    let mut plot = load_test_plot("./benches/chungus_mandelbrot_plot");

    click_floor_button(&mut plot, CHUNGUS_START_BUTTON);

    for _ in 0..1000 {
        plot.tick_interpreted();
    }

    let mut hasher = Sha256::new();
    for chunk in &plot.chunks {
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
    //hash after 1000 ticks
    assert_eq!(hash.as_slice(),b"\xaf\xe1\xb6\xf2\xe9\xfa\xe4\x5b\xa9\x68\xc1\x0a\x6e\x4b\xf7\xb0\x29\x78\xc5\xb3\x9c\xc3\xec\xb4\xe0\x73\x0a\xf3\x8e\x94\x20\x05");
}
