use crate::plot::worldedit::{load_schematic, paste_clipboard, WorldEditClipboard};
use crate::plot::{PlotWorld, PLOT_WIDTH};
use crate::world::{storage::Chunk, World};
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::Block;
use mchprs_blocks::{BlockFace, BlockPos};
use std::collections::HashMap;
use std::io::Cursor;

const SCHEMATIC: &[u8] = include_bytes!("../../../../test_data/ADDER_GWIEZDNY_TEST.schem");

fn marks(cb: &WorldEditClipboard) -> HashMap<String, BlockPos> {
    cb.block_entities
        .iter()
        .filter_map(|(&pos, entity)| {
            let BlockEntity::Sign(sign) = entity else {
                return None;
            };
            let row: serde_json::Value = serde_json::from_str(&sign.rows[0]).unwrap();
            let label = row.as_str().or_else(|| row["text"].as_str()).unwrap();
            Some((label.to_owned(), pos.offset(BlockFace::Bottom)))
        })
        .collect()
}
fn bit_position(first: BlockPos, second: BlockPos, bit: i32) -> BlockPos {
    BlockPos::new(
        first.x + (second.x - first.x) * bit,
        first.y + (second.y - first.y) * bit,
        first.z + (second.z - first.z) * bit,
    )
}
fn inside(cb: &WorldEditClipboard, p: BlockPos) -> bool {
    (0..cb.size_x as i32).contains(&p.x)
        && (0..cb.size_y as i32).contains(&p.y)
        && (0..cb.size_z as i32).contains(&p.z)
}
fn loaded() -> (
    PlotWorld,
    Vec<BlockPos>,
    Vec<BlockPos>,
    Vec<BlockPos>,
    BlockPos,
) {
    let cb = load_schematic(Cursor::new(SCHEMATIC)).unwrap();
    let labels = marks(&cb);
    let width = (0..16)
        .take_while(|&i| {
            ["A", "B", "O"].into_iter().all(|key| {
                inside(
                    &cb,
                    bit_position(labels[&format!("{key}1")], labels[&format!("{key}2")], i),
                )
            })
        })
        .count();
    // This fixture contains 11 marked/interpolated stages, not the requested 16.
    assert_eq!(width, 11);
    let origin = BlockPos::new(40, 26, 46);
    let absolute = |p: BlockPos| BlockPos::new(p.x + origin.x, p.y + origin.y, p.z + origin.z);
    let bank = |key: &str| {
        (0..width as i32)
            .map(|i| {
                absolute(bit_position(
                    labels[&format!("{key}1")],
                    labels[&format!("{key}2")],
                    i,
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
        absolute(labels["TICK"]),
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
fn compare_java_output(world: &mut PlotWorld, out: &[BlockPos], reference: &str) {
    let reference: serde_json::Value = serde_json::from_str(reference).unwrap();
    let expected: Vec<Option<u16>> = serde_json::from_value(
        reference["fixtures"]["ADDER_GWIEZDNY_TEST.schem"]["adder_output"].clone(),
    )
    .unwrap();
    let got: Vec<_> = expected
        .iter()
        .map(|_| {
            world.tick_interpreted();
            read_output(world, out)
        })
        .collect();
    assert_eq!(got, expected);
}

#[test]
fn signed_adder_fixture_calculates_stored_inputs_after_tick_removal() {
    let (mut world, a, b, out, tick) = loaded();
    assert_eq!((read_inputs(&world, &a), read_inputs(&world, &b)), (63, 1));
    assert_eq!(read_output(&world, &out), Some(0));
    trigger(&mut world, tick);
    world.tick_interpreted();
    assert_eq!(read_output(&world, &out), Some(63 + 1));
}

#[test]
fn signed_adder_outputs_and_moving_states_match_java() {
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
fn signed_adder_low_bit_arithmetic_vectors() {
    for (a, b) in [(0, 0), (1, 0), (0, 1), (31, 1), (63, 1)] {
        let (mut world, bank_a, bank_b, out, tick) = loaded();
        set_inputs(&mut world, &bank_a, &bank_b, a, b);
        trigger(&mut world, tick);
        world.tick_interpreted();
        assert_eq!(read_output(&world, &out), Some(a + b), "inputs {a} + {b}");
    }
}

#[test]
fn signed_adder_changed_inputs_match_reference_fixture_limit() {
    let (mut world, a, b, out, tick) = loaded();
    set_inputs(&mut world, &a, &b, 0x555, 0x2aa);
    assert_eq!(
        (read_inputs(&world, &a), read_inputs(&world, &b)),
        (0x555, 0x2aa)
    );
    trigger(&mut world, tick);
    // Java also reports 1087 instead of the mathematical 2047 for this setup.
    // Keep that circuit limitation explicit; it is not a passing arithmetic claim.
    compare_java_output(
        &mut world,
        &out,
        include_str!("../../../../test_data/piston-repair/java-adder-inputs.json"),
    );
}
