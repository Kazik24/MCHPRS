//! Java-reference regression expectations. Intentionally fail on cb3d4e2.
use crate::plot::{PlotWorld, PLOT_WIDTH};
use crate::world::storage::Chunk;
use crate::world::World;
use mchprs_blocks::blocks::{Block, RedstonePiston};
use mchprs_blocks::{BlockFace, BlockFacing, BlockPos};

fn empty_world() -> PlotWorld {
    let chunks = (0..PLOT_WIDTH)
        .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
        .collect();
    PlotWorld::from_chunks(0, 0, chunks, Default::default())
}

fn start_extension(world: &mut PlotWorld, base: BlockPos, payload: Block) {
    let piston = RedstonePiston {
        facing: BlockFacing::East,
        sticky: true,
        extended: false,
    };
    world.set_block(base, Block::Piston { piston });
    world.set_block(base.offset(BlockFace::East), payload);
    world.set_block(base.offset(BlockFace::Bottom), Block::RedstoneBlock {});
    super::update(world.get_block(base), world, base, None);
    // Execute the queued extension without waiting for movement to finish.
    world.picotick_advance(1);
}

#[test]
fn extension_base_state_visible_before_completion() {
    let mut world = empty_world();
    let base = BlockPos::new(40, 30, 40);
    start_extension(&mut world, base, Block::Stone {});
    assert!(matches!(
        world.get_block(base.offset(BlockFace::East)),
        Block::MovingPiston { .. }
    ));
    println!("EXTENSION base={:?}", world.get_block(base));
    assert!(matches!(
        world.get_block(base),
        Block::Piston {
            piston: RedstonePiston { extended: true, .. }
        }
    ), "Java marks the base extended during the extension event");
}

#[test]
fn power_off_after_completion_is_not_blocked_by_cooldown() {
    let mut world = empty_world();
    let base = BlockPos::new(40, 30, 40);
    start_extension(&mut world, base, Block::Stone {});
    world.tick_interpreted();
    world.tick_interpreted();
    assert!(matches!(world.get_block(base.offset(BlockFace::East)), Block::PistonHead { .. }));
    world.set_block(base.offset(BlockFace::Bottom), Block::Air);
    super::update(world.get_block(base), &mut world, base, None);
    let pending = world.scheduler().iter_entries().collect::<Vec<_>>();
    println!("POWER_OFF pending={pending:?}");
    assert!(pending.iter().any(|entry| entry.pos == base && entry.ticks_left == 0),
        "a completed piston must queue a power-off decision immediately");
}

#[test]
fn one_game_tick_pulse_drops_sticky_payload() {
    let mut world = empty_world();
    let base = BlockPos::new(40, 30, 40);
    let head = base.offset(BlockFace::East);
    let destination = head.offset(BlockFace::East);
    start_extension(&mut world, base, Block::Stone {});
    world.tick_interpreted();
    world.set_block(base.offset(BlockFace::Bottom), Block::Air);
    super::update(world.get_block(base), &mut world, base, None);
    for _ in 0..8 {
        world.tick_interpreted();
    }
    println!("SHORT_PULSE base={:?} head={:?} destination={:?}",
        world.get_block(base), world.get_block(head), world.get_block(destination));
    assert!(matches!(world.get_block(destination), Block::Stone { .. }),
        "Java short-pulse retraction leaves the pushed payload at the destination");
    assert!(matches!(world.get_block(head), Block::Air),
        "Java drop retraction does not pull the payload back to the head position");
}

#[test]
fn pushing_two_blocks_preserves_both_payloads() {
    let mut world = empty_world();
    let base = BlockPos::new(40, 30, 40);
    let head = base.offset(BlockFace::East);
    let destination = head.offset(BlockFace::East);
    let next = destination.offset(BlockFace::East);
    world.set_block(destination, Block::GoldBlock {});
    start_extension(&mut world, base, Block::Stone {});
    world.tick_interpreted();
    world.tick_interpreted();
    println!("PUSH_CHAIN destination={:?} next={:?}", world.get_block(destination), world.get_block(next));
    assert!(matches!(world.get_block(destination), Block::Stone { .. }));
    assert!(matches!(world.get_block(next), Block::GoldBlock { .. }),
        "Java resolves the push chain instead of overwriting the second block");
}

#[test]
fn observer_strong_power_is_directional() {
    use mchprs_blocks::blocks::RedstoneObserver;
    let world = empty_world();
    let pos = BlockPos::new(40, 30, 40);
    let block = Block::Observer {
        observer: RedstoneObserver { facing: BlockFacing::Down, powered: true },
    };
    assert_eq!(super::get_strong_power(block, &world, pos, BlockFace::Bottom, false), 15);
    assert_eq!(super::get_strong_power(block, &world, pos, BlockFace::North, false), 0,
        "Java observer strong power follows its output direction");
}
