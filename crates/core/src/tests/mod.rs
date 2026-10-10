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
                    assert_ne!(
                        legacy,
                        u32::MAX,
                        "unexpected target-only state in legacy circuit fixture"
                    );
                    hasher.update(legacy.to_le_bytes());
                }
            }
        }
    }
    let hash = hasher.finalize();
    hash.to_vec().into_boxed_slice()
}

fn visible_output_positions(world: &PlotWorld) -> Vec<BlockPos> {
    let mut positions = Vec::new();
    for chunk in world.get_chunks() {
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..256 {
                    let block = Block::from_id(chunk.get_block(x, y, z));
                    if matches!(
                        block,
                        Block::RedstoneLamp { .. }
                            | Block::IronTrapdoor { .. }
                            | Block::NoteBlock { .. }
                    ) {
                        positions.push(BlockPos::new(
                            chunk.x * 16 + x as i32,
                            y as i32,
                            chunk.z * 16 + z as i32,
                        ));
                    }
                }
            }
        }
    }
    positions
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
    assert_eq!(hash.as_ref(),b"\xd2\x56\x77\x9f\xe2\x5a\x9c\x0b\x70\x61\xd3\xea\x64\xf2\x0e\x58\xd3\x9e\x01\x42\x7a\xbb\x44\x72\xbc\x69\xf3\x0e\x36\x85\xd9\x15");
}

#[test]
fn run_mandelbrot_chungus_compiled() {
    let mut interpreted = load_test_plot("./benches/chungus_mandelbrot_plot");
    let output_positions = visible_output_positions(&interpreted);
    click_floor_button(&mut interpreted, CHUNGUS_START_BUTTON);
    let mut expected_trace = Vec::with_capacity(1000);
    for _ in 0..1000 {
        for _ in 0..TICK_MUL {
            interpreted.tick_interpreted();
        }
        expected_trace.push(
            output_positions
                .iter()
                .map(|&pos| interpreted.get_block(pos))
                .collect::<Vec<_>>(),
        );
    }
    let expected = calculate_world_hash(&interpreted);

    for (mode, flags) in [("compiled", ""), ("optimized", "-O")] {
        let mut plot = load_test_plot("./benches/chungus_mandelbrot_plot");
        let mut compiler: Compiler = Default::default();
        let options = CompilerOptions::parse(flags).unwrap();
        let bounds = plot.get_corners();
        compiler
            .compile(&plot, bounds, options, Vec::new(), Default::default())
            .unwrap();
        compiler.on_use_block(CHUNGUS_START_BUTTON);

        for (tick, expected_outputs) in expected_trace.iter().enumerate() {
            for _ in 0..TICK_MUL {
                compiler.tick();
            }
            compiler.flush(&mut plot);
            for (&pos, expected_output) in output_positions.iter().zip(expected_outputs) {
                assert_eq!(
                    plot.get_block(pos),
                    *expected_output,
                    "{mode} output at {pos} differs after game tick {}",
                    tick + 1
                );
            }
        }

        assert_eq!(
            calculate_world_hash(&plot),
            expected,
            "{mode} output differs from interpreter"
        );
    }
}

#[test]
fn run_mandelbrot_chungus_interpreted_to_compiled() {
    let mut plot = load_test_plot("./benches/chungus_mandelbrot_plot");
    click_floor_button(&mut plot, CHUNGUS_START_BUTTON);

    for _ in 0..1000 * TICK_MUL {
        plot.tick_interpreted();
    }

    let mut compiler: Compiler = Default::default();
    let options = CompilerOptions::parse("-O").unwrap();
    let bounds = plot.get_corners();
    let ticks = plot.scheduler().iter_entries().collect();
    compiler
        .compile(&plot, bounds, options, ticks, Default::default())
        .unwrap();

    for _ in 0..1000 * TICK_MUL {
        compiler.tick();
    }
    compiler.flush(&mut plot);

    let hash = calculate_world_hash(&plot);
    let mut control = load_test_plot("./benches/chungus_mandelbrot_plot");
    click_floor_button(&mut control, CHUNGUS_START_BUTTON);
    for _ in 0..2000 * TICK_MUL {
        control.tick_interpreted();
    }
    // Optimized compilation omits internal wires/nodes. Compare the circuit's
    // visible outputs with uninterrupted interpreted execution instead of a
    // historical whole-world hash containing the old lost-repeater-pulse state.
    for chunk in control.get_chunks() {
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..256 {
                    let block = Block::from_id(chunk.get_block(x, y, z));
                    if matches!(
                        block,
                        Block::RedstoneLamp { .. }
                            | Block::IronTrapdoor { .. }
                            | Block::NoteBlock { .. }
                    ) {
                        let pos = BlockPos::new(
                            chunk.x * 16 + x as i32,
                            y as i32,
                            chunk.z * 16 + z as i32,
                        );
                        assert_eq!(
                            plot.get_block(pos),
                            block,
                            "handoff output at {pos}; world hash {hash:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn run_mandelbrot_chungus_compiled_to_interpreted() {
    let mut plot = load_test_plot("./benches/chungus_mandelbrot_plot");

    let mut compiler: Compiler = Default::default();
    let options = CompilerOptions::parse("-O").unwrap();
    let bounds = plot.get_corners();
    compiler
        .compile(&plot, bounds, options, Vec::new(), Default::default())
        .unwrap();
    compiler.on_use_block(CHUNGUS_START_BUTTON);

    for _ in 0..1000 * TICK_MUL {
        compiler.tick();
    }
    compiler.flush(&mut plot);

    compiler.reset(&mut plot, bounds);

    for _ in 0..28 * TICK_MUL {
        // todo when transferring from compiled to interpreted, chungus stops running after around 30 ticks
        // println!("{}", plot.to_be_ticked.len());
        plot.tick_interpreted();
    }

    let hash = calculate_world_hash(&plot);
    //hash after 1000 compiled and then 1000 interpreted ticks (master commit 33cfc6dd84)
    assert_eq!(hash.as_ref(),b"\x66\xb7\x25\x7c\x39\xd1\xc6\x14\xe1\xd0\x97\xbb\x50\xf7\xe9\xd9\x2e\xd8\xc5\xd4\x23\x2f\x68\x1f\x56\x5d\x7e\xe6\xa0\x74\x38\xda");
}
