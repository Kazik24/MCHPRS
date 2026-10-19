use super::*;
use mchprs_blocks::BlockFacing;
use mchprs_blocks::blocks::{Lever, LeverFace, RedstoneObserver, RedstoneRepeater};
use std::cell::Cell;

#[test]
fn lazy_sides_match_the_existing_neighborhood_oracle() {
    let blocks = [
        Block::Air,
        Block::Stone {},
        Block::RedstoneBlock,
        Block::Target,
        Block::RedstoneWire {
            wire: RedstoneWire::default(),
        },
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
        Block::Observer {
            observer: RedstoneObserver {
                facing: BlockFacing::North,
                powered: false,
            },
        },
        Block::Observer {
            observer: RedstoneObserver {
                facing: BlockFacing::East,
                powered: true,
            },
        },
        Block::RedstoneRepeater {
            repeater: RedstoneRepeater {
                facing: BlockDirection::North,
                delay: 1,
                powered: false,
                locked: false,
            },
        },
        Block::RedstoneRepeater {
            repeater: RedstoneRepeater {
                facing: BlockDirection::East,
                delay: 1,
                powered: true,
                locked: true,
            },
        },
    ];
    let mut random = 0x93a5_67bdu32;
    for case in 0..4096 {
        let neighbors: [Block; 24] = std::array::from_fn(|_| {
            random = random.wrapping_mul(1664525).wrapping_add(1013904223);
            blocks[(random >> 16) as usize % blocks.len()]
        });
        let wire = RedstoneWire {
            power: (case % 16) as u8,
            ..Default::default()
        };
        let reads = Cell::new(0);
        let actual = get_raw_sides_from_neighbor_read(wire, |slot| {
            reads.set(reads.get() + 1);
            neighbors[slot]
        });
        assert_eq!(actual, get_raw_sides_from_neighbors(wire, &neighbors), "neighborhood {case}");
        assert!(reads.get() <= 13);
    }
}

#[test]
fn lazy_sides_omit_irrelevant_and_blocked_geometry_reads() {
    for block in [Block::Stone {}, Block::RedstoneWire { wire: Default::default() }] {
        let reads = Cell::new(0);
        let actual = get_raw_sides_from_neighbor_read(RedstoneWire::default(), |slot| {
            assert!([0, 1, 3, 4, 5].contains(&slot));
            reads.set(reads.get() + 1);
            block
        });
        assert_eq!(actual, get_raw_sides_from_neighbors(RedstoneWire::default(), &[block; 24]));
        assert_eq!(reads.get(), 5);
    }
}
