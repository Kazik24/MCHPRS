//! Structural roles must not turn stationary actors into unsampled memory.
use super::*;

#[test]
fn conductor_reset_requires_a_fixed_repeater_supply_without_lock_inputs() {
    use mchprs_blocks::blocks::{
        Lever, LeverFace, RedstoneRepeater, RedstoneWire, RedstoneWireSide,
    };
    use mchprs_blocks::BlockDirection;
    let mut world = empty();
    let piston = RedstonePiston {
        facing: BlockFacing::South,
        sticky: true,
        extended: true,
    };
    let head = BASE.offset(BlockFace::South);
    let dust = head.offset(BlockFace::Bottom);
    let emitter = head.offset(BlockFace::East);
    let supply = emitter.offset(BlockFace::East);
    world.set_block(BASE, Block::Piston { piston });
    world.set_block(
        head,
        Block::PistonHead {
            head: piston.into(),
        },
    );
    world.set_block(head.offset(BlockFace::South), Block::Stone {});
    world.set_block(BASE.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        dust,
        Block::RedstoneWire {
            wire: RedstoneWire::new(
                RedstoneWireSide::Side,
                RedstoneWireSide::Side,
                RedstoneWireSide::Side,
                RedstoneWireSide::Side,
                0,
            ),
        },
    );
    world.set_block(dust.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(emitter.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        emitter,
        Block::RedstoneRepeater {
            repeater: RedstoneRepeater {
                facing: BlockDirection::East,
                powered: true,
                ..Default::default()
            },
        },
    );
    world.set_block(supply, Block::RedstoneBlock);
    let input = BASE.offset(BlockFace::North);
    world.set_block(input.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        input,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, true),
        },
    );
    let report = analyze_world(&world);
    assert!(
        report.recognition[0].is_matched(),
        "{:?}",
        report.recognition[0]
    );
    world.set_block(supply, Block::Air);
    assert!(!analyze_world(&world).recognition[0].is_matched());
    world.set_block(supply, Block::RedstoneBlock);
    let lock = emitter.offset(BlockFace::South);
    world.set_block(
        lock,
        Block::RedstoneRepeater {
            repeater: RedstoneRepeater {
                facing: BlockDirection::South,
                powered: false,
                ..Default::default()
            },
        },
    );
    assert!(!analyze_world(&world).recognition[0].is_matched());
}

// A repeater feeds one receiving dust through a separately movable conductor.
