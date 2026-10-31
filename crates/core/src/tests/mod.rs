use std::path::Path;

use mchprs_blocks::blocks::{Block, ButtonFace};
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_save_data::plot_data::PlotData;
use mchprs_world::TickPriority;
use sha2::{Digest, Sha256};

use crate::plot::{PLOT_WIDTH, PlotWorld};
use crate::redstone;
use crate::world::World;
use crate::world::storage::Chunk;

const TICK_MUL: u32 = 2;

pub fn load_test_plot(path: impl AsRef<Path>) -> PlotWorld {
    let data = PlotData::load_from_file(path, false).unwrap();

    let chunks: Vec<Chunk> = data
        .chunk_data
        .into_iter()
        .enumerate()
        .map(|(i, c)| Chunk::load(i as i32 / PLOT_WIDTH, i as i32 % PLOT_WIDTH, c))
        .collect();

    PlotWorld::from_chunks(0, 0, chunks, data.pending_ticks.into_iter().collect())
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
    for chunk in world.get_chunks() {
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..256 {
                    let ch = chunk.get_block(x, y, z);
                    let legacy = mchprs_blocks::generated::TARGET_TO_LEGACY[ch as usize];
                    assert_ne!(legacy, u32::MAX, "unexpected target-only state in legacy circuit fixture");
                    hasher.update(legacy.to_le_bytes());
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

    for _ in 0..1000 * TICK_MUL {
        plot.tick_interpreted();
    }

    let hash = calculate_world_hash(&plot);
    // Full hopper/furnace state semantics and hopper lock updates change the old
    // checksum. Restoring the former Unknown classification reproduces the old
    // circuit checksum; the new one includes the corrected container states.
    assert_eq!(
        hash.as_ref(),
        b"\xd2\x56\x77\x9f\xe2\x5a\x9c\x0b\x70\x61\xd3\xea\x64\xf2\x0e\x58\xd3\x9e\x01\x42\x7a\xbb\x44\x72\xbc\x69\xf3\x0e\x36\x85\xd9\x15"
    );
}
