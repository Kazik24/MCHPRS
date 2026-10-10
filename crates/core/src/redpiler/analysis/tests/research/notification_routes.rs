//! Native evidence for notification routes outside the fixed-writer sampler.
use super::*;
use crate::redstone::instant_piston_tests::capture_at;

fn extended_sticky(world: &mut PlotWorld, pos: BlockPos, payload: Block) {
    let piston = RedstonePiston {
        facing: BlockFacing::Down,
        sticky: true,
        extended: true,
    };
    world.set_block(pos, Block::Piston { piston });
    world.set_block(
        pos.offset(BlockFace::Bottom),
        Block::PistonHead {
            head: piston.into(),
        },
    );
    world.set_block(pos + BlockPos::new(0, -2, 0), payload);
}

#[test]
fn bud_head_request_rechecks_restored_power_before_commit() {
    let mut world = empty();
    let cell = BASE;
    let head = cell.offset(BlockFace::Bottom);
    let payload = Block::from_name("gray_wool").unwrap();
    extended_sticky(&mut world, cell, payload);
    let data = cell + BlockPos::new(0, 2, 0);
    assert!(!crate::redstone::piston::should_piston_extend(
        &world,
        BlockFacing::Down,
        cell,
    ));
    let callbacks = capture_at(&[cell, head], || {
        crate::redstone::update(
            world.get_block(head),
            &mut world,
            head,
            Some(BlockFace::West),
        );
        // A request records the desired action; execution must recheck power.
        world.set_block(data, Block::RedstoneBlock);
        world.tick_interpreted();
    });
    let requested = callbacks
        .iter()
        .position(|entry| {
            entry["kind"] == "event_enqueue"
                && entry["data"]["pos"] == json!(cell)
                && entry["data"]["action"] == "Retract"
        })
        .unwrap();
    let validated = callbacks
        .iter()
        .position(|entry| entry["kind"] == "event_execute" && entry["data"]["pos"] == json!(cell))
        .unwrap();
    assert!(requested < validated);
    assert!(!callbacks
        .iter()
        .any(|entry| { entry["kind"] == "event_applied" && entry["data"]["pos"] == json!(cell) }));
    assert!(matches!(world.get_block(cell), Block::Piston { piston } if piston.extended));
    assert!(matches!(world.get_block(head), Block::PistonHead { .. }));
    assert_eq!(world.get_block(cell + BlockPos::new(0, -2, 0)), payload);
    assert!(world.piston_state().events.is_empty());
    assert!(world.piston_state().motions.is_empty());
}
