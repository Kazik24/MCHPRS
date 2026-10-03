//! Additional Java 1.21.5 source-derived expectations for the current fork.
//! Diagnostic tests intentionally fail until the simulation is repaired.
use crate::plot::{PlotWorld, PLOT_WIDTH};
use crate::world::storage::Chunk;
use crate::world::World;
use mchprs_blocks::block_entities::{BlockEntity, MovingPistonEntity};
use mchprs_blocks::blocks::{Block, RedstoneObserver, RedstonePiston, RedstonePistonHead};
use mchprs_blocks::{BlockFace, BlockFacing, BlockPos};

fn empty_world() -> PlotWorld {
    let chunks = (0..PLOT_WIDTH)
        .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
        .collect();
    PlotWorld::from_chunks(0, 0, chunks, Default::default())
}
fn base() -> BlockPos { BlockPos::new(40, 30, 40) }
fn named(name: &str) -> Block { Block::from_name(name).unwrap() }
fn piston(facing: BlockFacing, sticky: bool, extended: bool) -> RedstonePiston {
    RedstonePiston { facing, sticky, extended }
}
fn start(world: &mut PlotWorld, pos: BlockPos, facing: BlockFacing, payload: Block) {
    world.set_block(pos, Block::Piston { piston: piston(facing, true, false) });
    world.set_block(pos.offset(facing.into()), payload);
    let power_face = if facing == BlockFacing::Down { BlockFace::North } else { BlockFace::Bottom };
    world.set_block(pos.offset(power_face), Block::RedstoneBlock {});
    super::update(world.get_block(pos), world, pos, None);
    world.picotick_advance(1);
}
fn settle(world: &mut PlotWorld) { for _ in 0..8 { world.tick_interpreted(); } }
fn extended(world: &PlotWorld, pos: BlockPos) -> bool {
    matches!(world.get_block(pos), Block::Piston { piston: RedstonePiston { extended: true, .. } })
}
macro_rules! blocked_extension {
    ($test:ident, $name:literal) => {
        #[test]
        fn $test() {
            let mut world = empty_world(); let pos = base(); let block = named($name);
            start(&mut world, pos, BlockFacing::East, block); settle(&mut world);
            println!("BLOCKED {} base={:?} head={:?} destination={:?}", $name,
                world.get_block(pos), world.get_block(pos.offset(BlockFace::East)),
                world.get_block(pos.offset(BlockFace::East).offset(BlockFace::East)));
            assert!(!extended(&world, pos), "Java rejects this movement");
            assert_eq!(world.get_block(pos.offset(BlockFace::East)), block);
        }
    };
}
blocked_extension!(bedrock_blocks_extension, "bedrock");
blocked_extension!(obsidian_blocks_extension, "obsidian");
blocked_extension!(crying_obsidian_blocks_extension, "crying_obsidian");
blocked_extension!(respawn_anchor_blocks_extension, "respawn_anchor");
blocked_extension!(untyped_chest_blocks_extension, "chest");
blocked_extension!(sign_blocks_extension_without_losing_text, "oak_sign");

#[test]
fn sticky_retraction_cannot_pull_obsidian() {
    let mut world = empty_world(); let pos = base(); let head = pos.offset(BlockFace::East);
    let target = head.offset(BlockFace::East);
    world.set_block(pos, Block::Piston { piston: piston(BlockFacing::East, true, true) });
    world.set_block(head, Block::PistonHead { head: RedstonePistonHead {
        facing: BlockFacing::East, sticky: true, short: false } });
    world.set_block(target, Block::Obsidian {});
    super::update(world.get_block(pos), &mut world, pos, None); settle(&mut world);
    println!("PULL_OBSIDIAN head={:?} target={:?}", world.get_block(head), world.get_block(target));
    assert_eq!(world.get_block(target), Block::Obsidian {});
    assert_eq!(world.get_block(head), Block::Air);
}

#[test]
fn sticky_retraction_cannot_pull_glazed_terracotta() {
    let mut world = empty_world(); let pos = base(); let head = pos.offset(BlockFace::East);
    let target = head.offset(BlockFace::East); let block = named("white_glazed_terracotta");
    world.set_block(pos, Block::Piston { piston: piston(BlockFacing::East, true, true) });
    world.set_block(head, Block::PistonHead { head: RedstonePistonHead {
        facing: BlockFacing::East, sticky: true, short: false } });
    world.set_block(target, block);
    super::update(world.get_block(pos), &mut world, pos, None); settle(&mut world);
    println!("PULL_GLAZED head={:?} target={:?}", world.get_block(head), world.get_block(target));
    assert_eq!(world.get_block(target), block, "Java PUSH_ONLY blocks cannot be pulled");
    assert_eq!(world.get_block(head), Block::Air);
}

#[test]
fn movable_trapdoor_is_preserved() {
    let mut world = empty_world(); let pos = base(); let block = named("iron_trapdoor");
    start(&mut world, pos, BlockFacing::East, block); settle(&mut world);
    let target = pos.offset(BlockFace::East).offset(BlockFace::East);
    println!("TRAPDOOR destination={:?}", world.get_block(target));
    assert!(matches!(world.get_block(target), Block::IronTrapdoor { .. }));
}

macro_rules! sticky_attachment {
    ($test:ident, $name:literal) => {
        #[test]
        fn $test() {
            let mut world = empty_world(); let pos = base();
            let side = pos.offset(BlockFace::East).offset(BlockFace::South);
            let side_target = side.offset(BlockFace::East);
            world.set_block(side, Block::GoldBlock {});
            start(&mut world, pos, BlockFacing::East, named($name)); settle(&mut world);
            println!("ATTACHMENT {} source={:?} target={:?}", $name,
                world.get_block(side), world.get_block(side_target));
            assert_eq!(world.get_block(side), Block::Air);
            assert_eq!(world.get_block(side_target), Block::GoldBlock {});
        }
    };
}
sticky_attachment!(slime_moves_side_attachment, "slime_block");
sticky_attachment!(honey_moves_side_attachment, "honey_block");

#[test]
fn thirteenth_block_prevents_extension() {
    let mut world = empty_world(); let pos = base();
    for n in 1..=13 { world.set_block(BlockPos::new(pos.x + n, pos.y, pos.z), Block::Stone {}); }
    start(&mut world, pos, BlockFacing::East, Block::Stone {}); settle(&mut world);
    println!("LIMIT base={:?}", world.get_block(pos));
    assert!(!extended(&world, pos), "Java's resolver has a total limit of twelve moved blocks");
}

#[test]
fn immovable_destination_preserves_chain() {
    let mut world = empty_world(); let pos = base();
    let target = pos.offset(BlockFace::East).offset(BlockFace::East);
    world.set_block(target, Block::Bedrock {});
    start(&mut world, pos, BlockFacing::East, Block::Stone {}); settle(&mut world);
    println!("BLOCKED_CHAIN base={:?} destination={:?}", world.get_block(pos), world.get_block(target));
    assert_eq!(world.get_block(target), Block::Bedrock {});
    assert!(!extended(&world, pos));
}

#[test]
fn movement_cannot_lose_payload_at_plot_boundary() {
    let mut world = empty_world(); let pos = BlockPos::new(254, 30, 40);
    start(&mut world, pos, BlockFacing::East, Block::Stone {}); settle(&mut world);
    println!("PLOT_EDGE base={:?} last_cell={:?}", world.get_block(pos), world.get_block(pos.offset(BlockFace::East)));
    assert_eq!(world.get_block(pos.offset(BlockFace::East)), Block::Stone {},
        "An isolated plot must reject a move to inaccessible storage");
    assert!(!extended(&world, pos));
}

#[test]
fn movement_cannot_lose_payload_at_height_boundary() {
    let mut world = empty_world(); let pos = BlockPos::new(40, 254, 40);
    start(&mut world, pos, BlockFacing::Up, Block::Stone {}); settle(&mut world);
    println!("HEIGHT_EDGE base={:?} last_cell={:?}", world.get_block(pos), world.get_block(pos.offset(BlockFace::Top)));
    assert_eq!(world.get_block(pos.offset(BlockFace::Top)), Block::Stone {});
    assert!(!extended(&world, pos));
}

#[test]
fn completion_cannot_recreate_replaced_base() {
    let mut world = empty_world(); let pos = base();
    start(&mut world, pos, BlockFacing::East, Block::Stone {});
    world.set_block(pos, Block::GoldBlock {}); settle(&mut world);
    println!("REPLACED_BASE current={:?}", world.get_block(pos));
    assert_eq!(world.get_block(pos), Block::GoldBlock {},
        "Java moving-entity completion never blindly rewrites a neighboring base");
}

#[test]
fn removing_base_removes_stationary_head() {
    let mut world = empty_world(); let pos = base();
    start(&mut world, pos, BlockFacing::East, Block::Stone {}); settle(&mut world);
    crate::interaction::destroy(world.get_block(pos), &mut world, pos);
    println!("ORPHAN_HEAD current={:?}", world.get_block(pos.offset(BlockFace::East)));
    assert_eq!(world.get_block(pos.offset(BlockFace::East)), Block::Air);
}

#[test]
fn breaking_stationary_head_removes_base() {
    let mut world = empty_world(); let pos = base(); let head = pos.offset(BlockFace::East);
    start(&mut world, pos, BlockFacing::East, Block::Stone {}); settle(&mut world);
    crate::interaction::destroy(world.get_block(head), &mut world, head);
    println!("BROKEN_HEAD base={:?} head={:?}", world.get_block(pos), world.get_block(head));
    assert_eq!(world.get_block(pos), Block::Air);
}

#[test]
fn mismatched_head_does_not_mark_base_extended() {
    let mut world = empty_world(); let pos = base();
    let invalid_head = Block::PistonHead { head: RedstonePistonHead {
        facing: BlockFacing::North, sticky: false, short: false } };
    start(&mut world, pos, BlockFacing::East, invalid_head); settle(&mut world);
    println!("MISMATCHED_HEAD base={:?}", world.get_block(pos));
    assert!(!extended(&world, pos));
}

#[test]
fn moving_payload_has_its_own_destination_entity() {
    let mut world = empty_world(); let pos = base();
    start(&mut world, pos, BlockFacing::East, Block::Stone {});
    let target = pos.offset(BlockFace::East).offset(BlockFace::East);
    println!("MOVING_DESTINATION block={:?} entity={:?}", world.get_block(target), world.get_block_entity(target));
    assert!(matches!(world.get_block(target), Block::MovingPiston { .. }));
    assert!(matches!(world.get_block_entity(target), Some(BlockEntity::MovingPiston(e))
        if !e.source && e.block_state == Block::Stone {}.get_id()));
}

#[test]
fn normal_retraction_uses_moving_source_at_base() {
    let mut world = empty_world(); let pos = base();
    start(&mut world, pos, BlockFacing::East, Block::Stone {}); settle(&mut world);
    world.set_block(pos.offset(BlockFace::Bottom), Block::Air);
    super::update(world.get_block(pos), &mut world, pos, None); world.picotick_advance(1);
    println!("RETRACT_SOURCE base={:?} entity={:?}", world.get_block(pos), world.get_block_entity(pos));
    assert!(matches!(world.get_block(pos), Block::MovingPiston { .. }));
    assert!(matches!(world.get_block_entity(pos), Some(BlockEntity::MovingPiston(e)) if !e.extending && e.source));
}

#[test]
fn movement_progress_advances_before_completion() {
    let mut world = empty_world(); let pos = base();
    start(&mut world, pos, BlockFacing::East, Block::Stone {}); world.tick_interpreted();
    let head = pos.offset(BlockFace::East);
    println!("PROGRESS entity={:?}", world.get_block_entity(head));
    assert!(matches!(world.get_block_entity(head), Some(BlockEntity::MovingPiston(e)) if e.get_progress() > 0.0));
}

#[test]
fn half_progress_is_exactly_representable() {
    let mut entity = MovingPistonEntity::default(); entity.set_progress(0.5);
    println!("HALF_PROGRESS encoded={} decoded={}", entity.progress, entity.get_progress());
    assert_eq!(entity.get_progress(), 0.5);
}

#[test]
fn moved_powered_observer_does_not_stay_powered_forever() {
    let mut world = empty_world(); let pos = base();
    // A powered observer normally has an outstanding pulse-off tick at its old position.
    world.schedule_half_tick(pos.offset(BlockFace::East), 2, mchprs_world::TickPriority::Normal);
    start(&mut world, pos, BlockFacing::East, Block::Observer {
        observer: RedstoneObserver { facing: BlockFacing::Down, powered: true } });
    settle(&mut world);
    let target = pos.offset(BlockFace::East).offset(BlockFace::East);
    println!("MOVED_OBSERVER destination={:?}", world.get_block(target));
    assert!(matches!(world.get_block(target), Block::Observer {
        observer: RedstoneObserver { powered: false, .. } }));
}

#[test]
fn old_observer_tick_cannot_complete_new_moving_head() {
    let mut world = empty_world(); let pos = base(); let head = pos.offset(BlockFace::East);
    world.set_block(head, Block::Observer {
        observer: RedstoneObserver { facing: BlockFacing::Down, powered: true } });
    world.schedule_half_tick(head, 1, mchprs_world::TickPriority::Normal);
    start(&mut world, pos, BlockFacing::East, world_observer());
    world.tick_interpreted();
    println!("OLD_TICK head_after_one_game_tick={:?}", world.get_block(head));
    assert!(matches!(world.get_block(head), Block::MovingPiston { .. }),
        "Java scheduled block ticks retain their block type and reject a replacement type");
}

#[test]
fn moved_waterlogged_stairs_are_not_still_waterlogged() {
    let mut world = empty_world(); let pos = base(); let mut block = named("oak_stairs");
    block.set_properties([("waterlogged", "true")].into_iter().collect());
    assert_eq!(block.properties().get("waterlogged").map(String::as_str), Some("true"));
    start(&mut world, pos, BlockFacing::East, block); settle(&mut world);
    let target = pos.offset(BlockFace::East).offset(BlockFace::East);
    let moved = world.get_block(target);
    println!("WATERLOGGED destination={:?} properties={:?}", moved, moved.properties());
    assert_eq!(moved.get_name(), "oak_stairs");
    assert_eq!(moved.properties().get("waterlogged").map(String::as_str), Some("false"),
        "Java moving-entity completion clears waterlogging");
}

#[test]
fn moving_support_breaks_torch_above_old_payload() {
    let mut world = empty_world(); let pos = base();
    let torch = pos.offset(BlockFace::East).offset(BlockFace::Top);
    world.set_block(torch, Block::RedstoneTorch { lit: true });
    start(&mut world, pos, BlockFacing::East, Block::Stone {}); settle(&mut world);
    println!("SUPPORT_REMOVED torch={:?}", world.get_block(torch));
    assert_eq!(world.get_block(torch), Block::Air,
        "Java shape updates remove a torch when its support becomes a horizontal piston head");
}
fn world_observer() -> Block {
    Block::Observer { observer: RedstoneObserver { facing: BlockFacing::Down, powered: true } }
}

#[test]
fn control_single_stone_extension_all_six_directions() {
    for facing in [BlockFacing::Down, BlockFacing::Up, BlockFacing::North,
        BlockFacing::South, BlockFacing::West, BlockFacing::East] {
        let mut world = empty_world(); let pos = base();
        start(&mut world, pos, facing, Block::Stone {}); settle(&mut world);
        assert!(extended(&world, pos));
        assert_eq!(world.get_block(pos.offset(facing.into()).offset(facing.into())), Block::Stone {});
    }
}

#[test]
fn control_typed_barrel_blocks_extension() {
    let mut world = empty_world(); let pos = base(); let block = named("barrel");
    start(&mut world, pos, BlockFacing::East, block); settle(&mut world);
    assert!(!extended(&world, pos));
    assert_eq!(world.get_block(pos.offset(BlockFace::East)), block);
}

#[test]
fn control_front_power_is_excluded() {
    let mut world = empty_world(); let pos = base();
    world.set_block(pos, Block::Piston { piston: piston(BlockFacing::East, true, false) });
    world.set_block(pos.offset(BlockFace::East), Block::RedstoneBlock {});
    assert!(!super::piston::should_piston_extend(&world, BlockFacing::East, pos));
}

#[test]
fn control_quasi_connected_power_detected_after_base_update() {
    let mut world = empty_world(); let pos = base();
    world.set_block(pos, Block::Piston { piston: piston(BlockFacing::East, true, false) });
    world.set_block(pos.offset(BlockFace::Top).offset(BlockFace::East), Block::RedstoneBlock {});
    assert!(super::piston::should_piston_extend(&world, BlockFacing::East, pos));
    assert!(!world.pending_tick_at(pos));
    super::update(world.get_block(pos), &mut world, pos, None); settle(&mut world);
    assert!(extended(&world, pos));
}
