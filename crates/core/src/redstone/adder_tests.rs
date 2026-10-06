use crate::plot::worldedit::{load_schematic, paste_clipboard};
use crate::plot::{PlotWorld, PLOT_WIDTH};
use crate::world::{storage::Chunk, World};
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::Block;
use mchprs_blocks::{BlockFace, BlockPos};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Cursor;

const SCHEMATIC: &[u8] = include_bytes!("../../../../test_data/instant-pistons/ADDER_11BITS.schem");
const BIT_WIDTH: u32 = 11;
const OUTPUT_MASK: u16 = (1 << BIT_WIDTH) - 1;
fn loaded() -> (
    PlotWorld,
    Vec<BlockPos>,
    Vec<BlockPos>,
    Vec<BlockPos>,
    BlockPos,
) {
    let cb = load_schematic(Cursor::new(SCHEMATIC)).unwrap();
    assert_eq!((cb.size_x, cb.size_y, cb.size_z), (21, 7, 46));
    let labels: HashMap<_, _> = cb
        .block_entities
        .iter()
        .filter_map(|(&pos, entity)| {
            let BlockEntity::Sign(sign) = entity else {
                return None;
            };
            let row: serde_json::Value = serde_json::from_str(&sign.rows[0]).unwrap();
            let label = row.as_str().or_else(|| row["text"].as_str()).unwrap();
            Some((label.to_owned(), pos.offset(BlockFace::Bottom)))
        })
        .collect();
    let origin = BlockPos::new(40, 26, 46);
    let absolute = |p: BlockPos| BlockPos::new(p.x + origin.x, p.y + origin.y, p.z + origin.z);
    // Signs identify the first two bits of each bank. Bit zero starts at the
    // largest Z; subsequent stages follow the spacing between those signs.
    let bank = |key: &str| {
        let first = labels[&format!("{key}1")];
        let second = labels[&format!("{key}2")];
        (0..BIT_WIDTH as i32)
            .map(|i| {
                absolute(BlockPos::new(
                    first.x + (second.x - first.x) * i,
                    first.y + (second.y - first.y) * i,
                    first.z + (second.z - first.z) * i,
                ))
            })
            .collect()
    };
    let chunks = (0..PLOT_WIDTH)
        .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
        .collect();
    let mut world = PlotWorld::from_chunks(0, 0, chunks, Default::default());
    let anchor = BlockPos::new(
        origin.x + cb.offset_x,
        origin.y + cb.offset_y,
        origin.z + cb.offset_z,
    );
    paste_clipboard(&mut world, &cb, anchor, false);
    (
        world,
        bank("A"),
        bank("B"),
        bank("O"),
        absolute(BlockPos::new(2, 4, 45)),
    )
}
fn read_inputs(world: &PlotWorld, bank: &[BlockPos]) -> u16 {
    bank.iter().enumerate().fold(0, |value, (i, &p)| {
        let block = world.get_block(p);
        assert!(
            block == Block::RedstoneBlock {} || block.is_cube(),
            "invalid input {p:?}: {block:?}"
        );
        value | ((block != Block::RedstoneBlock {}) as u16) << i
    })
}
fn read_output(world: &PlotWorld, bank: &[BlockPos]) -> Option<u16> {
    let mut value = 0;
    for (i, &pos) in bank.iter().enumerate() {
        match world.get_block(pos) {
            Block::Air => value |= 1 << i,
            Block::RedstoneBlock => {}
            Block::MovingPiston { .. } => return None,
            block => panic!("invalid output {pos:?}: {block:?}"),
        }
    }
    Some(value)
}
fn trigger(world: &mut PlotWorld, tick: BlockPos) {
    assert_eq!(world.get_block(tick), Block::RedstoneBlock {});
    crate::interaction::destroy(world.get_block(tick), world, tick);
}
fn set_inputs(world: &mut PlotWorld, a: &[BlockPos], b: &[BlockPos], va: u16, vb: u16) {
    for (bank, value) in [(a, va), (b, vb)] {
        for (i, &pos) in bank.iter().enumerate() {
            world.set_block(
                pos,
                if value & (1 << i) == 0 {
                    Block::RedstoneBlock {}
                } else {
                    Block::Stone {}
                },
            );
        }
    }
    for &pos in a.iter().chain(b) {
        super::update_surrounding_blocks(world, pos);
    }
    for _ in 0..8 {
        world.tick_interpreted();
    }
}
fn compare_java_output(
    world: &mut PlotWorld,
    out: &[BlockPos],
    reference: &str,
) -> Vec<Option<u16>> {
    let reference: serde_json::Value = serde_json::from_str(reference).unwrap();
    assert_eq!(
        reference["fixtures"]["ADDER_11BITS.schem"]["sha256"].as_str(),
        Some(format!("{:x}", Sha256::digest(SCHEMATIC)).as_str())
    );
    let expected: Vec<Option<u16>> =
        serde_json::from_value(reference["fixtures"]["ADDER_11BITS.schem"]["adder_output"].clone())
            .unwrap();
    let got: Vec<_> = expected
        .iter()
        .map(|_| {
            world.tick_interpreted();
            read_output(world, out)
        })
        .collect();
    assert_eq!(got, expected);
    got
}

#[test]
fn adder_fixture_calculates_stored_inputs_after_tick_removal() {
    let (mut world, a, b, out, tick) = loaded();
    assert_eq!(
        (read_inputs(&world, &a), read_inputs(&world, &b)),
        (OUTPUT_MASK, OUTPUT_MASK)
    );
    assert_eq!(read_output(&world, &out), Some(0));
    trigger(&mut world, tick);
    world.tick_interpreted();
    assert_eq!(read_output(&world, &out), Some((2047 + 2047) & OUTPUT_MASK));
}

#[test]
fn adder_outputs_and_moving_states_match_java() {
    let (mut world, _, _, out, tick) = loaded();
    trigger(&mut world, tick);
    compare_java_output(
        &mut world,
        &out,
        include_str!("../../../../test_data/piston-repair/java-adder-traces.json"),
    );
    assert!(world.piston_state().motions.is_empty());
}

#[test]
fn adder_arithmetic_vectors() {
    for (a, b) in [
        (0, 0),
        (1, 0),
        (0, 1),
        (3, 3),
        (7, 7),
        (31, 1),
        (31, 31),
        (63, 1),
        (1023, 1),
        (1023, 1023),
        (1024, 1024),
        (2047, 0),
        (0, 2047),
        (2047, 1),
        (2047, 2047),
        (0x555, 0x2aa),
        (0x2aa, 0x555),
    ] {
        let (mut world, bank_a, bank_b, out, tick) = loaded();
        set_inputs(&mut world, &bank_a, &bank_b, a, b);
        trigger(&mut world, tick);
        world.tick_interpreted();
        assert_eq!(
            read_output(&world, &out),
            Some((a + b) & OUTPUT_MASK),
            "inputs {a} + {b}"
        );
    }
}

#[test]
fn adder_all_bit_positions_handle_carries_and_overflow() {
    for bit in 0..BIT_WIDTH {
        for a_bit in 0..=1 {
            for b_bit in 0..=1 {
                for carry in 0..=u16::from(bit != 0) {
                    let a = (a_bit << bit) | if carry != 0 { (1 << bit) - 1 } else { 0 };
                    let b = (b_bit << bit) | carry;
                    let (mut world, bank_a, bank_b, out, tick) = loaded();
                    set_inputs(&mut world, &bank_a, &bank_b, a, b);
                    trigger(&mut world, tick);
                    world.tick_interpreted();
                    assert_eq!(
                        read_output(&world, &out),
                        Some((a + b) & OUTPUT_MASK),
                        "bit {bit}, inputs {a} + {b}, carry {carry}"
                    );
                }
            }
        }
    }
}

#[test]
fn adder_changed_inputs_calculate_correct_sum_and_match_java() {
    let (mut world, a, b, out, tick) = loaded();
    set_inputs(&mut world, &a, &b, 0x555, 0x2aa);
    assert_eq!(
        (read_inputs(&world, &a), read_inputs(&world, &b)),
        (0x555, 0x2aa)
    );
    trigger(&mut world, tick);
    let got = compare_java_output(
        &mut world,
        &out,
        include_str!("../../../../test_data/piston-repair/java-adder-inputs.json"),
    );
    assert_eq!(got[0], Some(0x555 + 0x2aa));
}
