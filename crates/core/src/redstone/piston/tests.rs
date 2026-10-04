//! Timing, lifecycle and completion regressions; movement rules are deferred.
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
fn base() -> BlockPos {
    BlockPos::new(40, 30, 40)
}
fn named(name: &str) -> Block {
    Block::from_name(name).unwrap()
}
fn piston(facing: BlockFacing, sticky: bool, extended: bool) -> RedstonePiston {
    RedstonePiston {
        facing,
        sticky,
        extended,
    }
}
fn start(world: &mut PlotWorld, pos: BlockPos, facing: BlockFacing, payload: Block) {
    world.set_block(
        pos,
        Block::Piston {
            piston: piston(facing, true, false),
        },
    );
    world.set_block(pos.offset(facing.into()), payload);
    let power_face = if facing == BlockFacing::Down {
        BlockFace::North
    } else {
        BlockFace::Bottom
    };
    world.set_block(pos.offset(power_face), Block::RedstoneBlock {});
    crate::redstone::update(world.get_block(pos), world, pos, None);
    world.picotick_advance(1);
}
fn settle(world: &mut PlotWorld) {
    for _ in 0..8 {
        world.tick_interpreted();
    }
}
fn extended(world: &PlotWorld, pos: BlockPos) -> bool {
    matches!(
        world.get_block(pos),
        Block::Piston {
            piston: RedstonePiston { extended: true, .. }
        }
    )
}
#[test]
fn movable_trapdoor_is_preserved() {
    let mut world = empty_world();
    let pos = base();
    let block = named("iron_trapdoor");
    start(&mut world, pos, BlockFacing::East, block);
    settle(&mut world);
    let target = pos.offset(BlockFace::East).offset(BlockFace::East);
    println!("TRAPDOOR destination={:?}", world.get_block(target));
    assert!(matches!(
        world.get_block(target),
        Block::IronTrapdoor { .. }
    ));
}

#[test]
fn movement_cannot_lose_payload_at_plot_boundary() {
    let mut world = empty_world();
    let pos = BlockPos::new(254, 30, 40);
    start(&mut world, pos, BlockFacing::East, Block::Stone {});
    settle(&mut world);
    println!(
        "PLOT_EDGE base={:?} last_cell={:?}",
        world.get_block(pos),
        world.get_block(pos.offset(BlockFace::East))
    );
    assert_eq!(
        world.get_block(pos.offset(BlockFace::East)),
        Block::Stone {},
        "An isolated plot must reject a move to inaccessible storage"
    );
    assert!(!extended(&world, pos));
}

#[test]
fn movement_cannot_lose_payload_at_height_boundary() {
    let mut world = empty_world();
    let pos = BlockPos::new(40, 254, 40);
    start(&mut world, pos, BlockFacing::Up, Block::Stone {});
    settle(&mut world);
    println!(
        "HEIGHT_EDGE base={:?} last_cell={:?}",
        world.get_block(pos),
        world.get_block(pos.offset(BlockFace::Top))
    );
    assert_eq!(world.get_block(pos.offset(BlockFace::Top)), Block::Stone {});
    assert!(!extended(&world, pos));
}

#[test]
fn completion_cannot_recreate_replaced_base() {
    let mut world = empty_world();
    let pos = base();
    start(&mut world, pos, BlockFacing::East, Block::Stone {});
    world.set_block(pos, Block::GoldBlock {});
    settle(&mut world);
    println!("REPLACED_BASE current={:?}", world.get_block(pos));
    assert_eq!(
        world.get_block(pos),
        Block::GoldBlock {},
        "Java moving-entity completion never blindly rewrites a neighboring base"
    );
}

#[test]
fn removing_base_removes_stationary_head() {
    let mut world = empty_world();
    let pos = base();
    start(&mut world, pos, BlockFacing::East, Block::Stone {});
    settle(&mut world);
    crate::interaction::destroy(world.get_block(pos), &mut world, pos);
    println!(
        "ORPHAN_HEAD current={:?}",
        world.get_block(pos.offset(BlockFace::East))
    );
    assert_eq!(world.get_block(pos.offset(BlockFace::East)), Block::Air);
}

#[test]
fn breaking_stationary_head_removes_base() {
    let mut world = empty_world();
    let pos = base();
    let head = pos.offset(BlockFace::East);
    start(&mut world, pos, BlockFacing::East, Block::Stone {});
    settle(&mut world);
    crate::interaction::destroy(world.get_block(head), &mut world, head);
    println!(
        "BROKEN_HEAD base={:?} head={:?}",
        world.get_block(pos),
        world.get_block(head)
    );
    assert_eq!(world.get_block(pos), Block::Air);
}

#[test]
fn mismatched_head_does_not_mark_base_extended() {
    let mut world = empty_world();
    let pos = base();
    let invalid_head = Block::PistonHead {
        head: RedstonePistonHead {
            facing: BlockFacing::North,
            sticky: false,
            short: false,
        },
    };
    start(&mut world, pos, BlockFacing::East, invalid_head);
    settle(&mut world);
    println!("MISMATCHED_HEAD base={:?}", world.get_block(pos));
    assert!(!extended(&world, pos));
}

#[test]
fn moving_payload_has_its_own_destination_entity() {
    let mut world = empty_world();
    let pos = base();
    start(&mut world, pos, BlockFacing::East, Block::Stone {});
    let target = pos.offset(BlockFace::East).offset(BlockFace::East);
    println!(
        "MOVING_DESTINATION block={:?} entity={:?}",
        world.get_block(target),
        world.get_block_entity(target)
    );
    assert!(matches!(
        world.get_block(target),
        Block::MovingPiston { .. }
    ));
    assert!(
        matches!(world.get_block_entity(target), Some(BlockEntity::MovingPiston(e))
        if !e.source && e.block_state == Block::Stone {}.get_id())
    );
}

#[test]
fn normal_retraction_uses_moving_source_at_base() {
    let mut world = empty_world();
    let pos = base();
    start(&mut world, pos, BlockFacing::East, Block::Stone {});
    settle(&mut world);
    world.set_block(pos.offset(BlockFace::Bottom), Block::Air);
    crate::redstone::update(world.get_block(pos), &mut world, pos, None);
    world.picotick_advance(1);
    println!(
        "RETRACT_SOURCE base={:?} entity={:?}",
        world.get_block(pos),
        world.get_block_entity(pos)
    );
    assert!(matches!(world.get_block(pos), Block::MovingPiston { .. }));
    assert!(
        matches!(world.get_block_entity(pos), Some(BlockEntity::MovingPiston(e)) if !e.extending && e.source)
    );
}

#[test]
fn movement_progress_advances_before_completion() {
    let mut world = empty_world();
    let pos = base();
    start(&mut world, pos, BlockFacing::East, Block::Stone {});
    world.tick_interpreted();
    let head = pos.offset(BlockFace::East);
    println!("PROGRESS entity={:?}", world.get_block_entity(head));
    assert!(
        matches!(world.get_block_entity(head), Some(BlockEntity::MovingPiston(e)) if e.get_progress() > 0.0)
    );
}

#[test]
fn half_progress_is_exactly_representable() {
    let mut entity = MovingPistonEntity::default();
    entity.set_progress(0.5);
    println!(
        "HALF_PROGRESS encoded={} decoded={}",
        entity.progress,
        entity.get_progress()
    );
    assert_eq!(entity.get_progress(), 0.5);
}

#[test]
fn moved_powered_observer_does_not_stay_powered_forever() {
    let mut world = empty_world();
    let pos = base();
    // A powered observer normally has an outstanding pulse-off tick at its old position.
    world.set_block(pos.offset(BlockFace::East), world_observer());
    world.schedule_half_tick(
        pos.offset(BlockFace::East),
        2,
        mchprs_world::TickPriority::Normal,
    );
    start(
        &mut world,
        pos,
        BlockFacing::East,
        Block::Observer {
            observer: RedstoneObserver {
                facing: BlockFacing::Down,
                powered: true,
            },
        },
    );
    settle(&mut world);
    let target = pos.offset(BlockFace::East).offset(BlockFace::East);
    println!("MOVED_OBSERVER destination={:?}", world.get_block(target));
    assert!(matches!(
        world.get_block(target),
        Block::Observer {
            observer: RedstoneObserver { powered: false, .. }
        }
    ));
}

#[test]
fn old_observer_tick_cannot_complete_new_moving_head() {
    let mut world = empty_world();
    let pos = base();
    let head = pos.offset(BlockFace::East);
    world.set_block(
        head,
        Block::Observer {
            observer: RedstoneObserver {
                facing: BlockFacing::Down,
                powered: true,
            },
        },
    );
    world.schedule_half_tick(head, 1, mchprs_world::TickPriority::Normal);
    start(&mut world, pos, BlockFacing::East, world_observer());
    world.tick_interpreted();
    println!(
        "OLD_TICK head_after_one_game_tick={:?}",
        world.get_block(head)
    );
    assert!(
        matches!(world.get_block(head), Block::MovingPiston { .. }),
        "Java scheduled block ticks retain their block type and reject a replacement type"
    );
}

#[test]
fn moved_waterlogged_stairs_are_not_still_waterlogged() {
    let mut world = empty_world();
    let pos = base();
    let mut block = named("oak_stairs");
    block.set_properties([("waterlogged", "true")].into_iter().collect());
    assert_eq!(
        block.properties().get("waterlogged").map(String::as_str),
        Some("true")
    );
    start(&mut world, pos, BlockFacing::East, block);
    settle(&mut world);
    let target = pos.offset(BlockFace::East).offset(BlockFace::East);
    let moved = world.get_block(target);
    println!(
        "WATERLOGGED destination={:?} properties={:?}",
        moved,
        moved.properties()
    );
    assert_eq!(moved.get_name(), "oak_stairs");
    assert_eq!(
        moved.properties().get("waterlogged").map(String::as_str),
        Some("false"),
        "Java moving-entity completion clears waterlogging"
    );
}

#[test]
fn moving_support_breaks_torch_above_old_payload() {
    let mut world = empty_world();
    let pos = base();
    let torch = pos.offset(BlockFace::East).offset(BlockFace::Top);
    world.set_block(torch, Block::RedstoneTorch { lit: true });
    start(&mut world, pos, BlockFacing::East, Block::Stone {});
    settle(&mut world);
    println!("SUPPORT_REMOVED torch={:?}", world.get_block(torch));
    assert_eq!(
        world.get_block(torch),
        Block::Air,
        "Java shape updates remove a torch when its support becomes a horizontal piston head"
    );
}
fn world_observer() -> Block {
    Block::Observer {
        observer: RedstoneObserver {
            facing: BlockFacing::Down,
            powered: true,
        },
    }
}

#[test]
fn control_single_stone_extension_all_six_directions() {
    for facing in [
        BlockFacing::Down,
        BlockFacing::Up,
        BlockFacing::North,
        BlockFacing::South,
        BlockFacing::West,
        BlockFacing::East,
    ] {
        let mut world = empty_world();
        let pos = base();
        start(&mut world, pos, facing, Block::Stone {});
        settle(&mut world);
        assert!(extended(&world, pos));
        assert_eq!(
            world.get_block(pos.offset(facing.into()).offset(facing.into())),
            Block::Stone {}
        );
    }
}

#[test]
fn control_front_power_is_excluded() {
    let mut world = empty_world();
    let pos = base();
    world.set_block(
        pos,
        Block::Piston {
            piston: piston(BlockFacing::East, true, false),
        },
    );
    world.set_block(pos.offset(BlockFace::East), Block::RedstoneBlock {});
    assert!(!super::should_piston_extend(&world, BlockFacing::East, pos));
}

#[test]
fn control_quasi_connected_power_detected_after_base_update() {
    let mut world = empty_world();
    let pos = base();
    world.set_block(
        pos,
        Block::Piston {
            piston: piston(BlockFacing::East, true, false),
        },
    );
    world.set_block(
        pos.offset(BlockFace::Top).offset(BlockFace::East),
        Block::RedstoneBlock {},
    );
    assert!(super::should_piston_extend(&world, BlockFacing::East, pos));
    assert!(!world.pending_tick_at(pos));
    crate::redstone::update(world.get_block(pos), &mut world, pos, None);
    settle(&mut world);
    assert!(extended(&world, pos));
}

#[test]
fn extension_sets_base_state_during_event_and_does_not_schedule_a_lock() {
    let mut world = empty_world();
    start(&mut world, base(), BlockFacing::East, Block::Stone {});
    assert!(extended(&world, base()));
    assert!(matches!(
        world.get_block(base().offset(BlockFace::East)),
        Block::MovingPiston { .. }
    ));
    assert!(!world.scheduler().iter_entries().any(|e| e.pos == base()));
}

#[test]
fn power_off_request_ignores_unrelated_future_base_tick() {
    let mut world = empty_world();
    start(&mut world, base(), BlockFacing::East, Block::Stone {});
    settle(&mut world);
    world.schedule_half_tick(base(), 3, mchprs_world::TickPriority::Normal);
    world.set_block(base().offset(BlockFace::Bottom), Block::Air);
    crate::redstone::update(world.get_block(base()), &mut world, base(), None);
    assert!(world
        .piston_state()
        .events
        .iter()
        .any(|e| e.pos == base() && e.action == mchprs_world::PistonAction::Retract));
    world.picotick_advance(1);
    assert!(matches!(
        world.get_block(base()),
        Block::MovingPiston { .. }
    ));
}

#[test]
fn short_pulse_drops_but_completed_extension_pulls() {
    for movement_ticks in 0..=3 {
        let mut world = empty_world();
        let head = base().offset(BlockFace::East);
        let destination = head.offset(BlockFace::East);
        start(&mut world, base(), BlockFacing::East, Block::Stone {});
        for _ in 0..movement_ticks {
            world.tick_interpreted();
        }
        world.set_block(base().offset(BlockFace::Bottom), Block::Air);
        crate::redstone::update(world.get_block(base()), &mut world, base(), None);
        let expected = if movement_ticks < 3 {
            mchprs_world::PistonAction::RetractWithoutPull
        } else {
            mchprs_world::PistonAction::Retract
        };
        assert_eq!(world.piston_state().events.back().unwrap().action, expected);
        settle(&mut world);
        assert_eq!(
            world.get_block(destination),
            if movement_ticks < 3 {
                Block::Stone {}
            } else {
                Block::Air
            },
            "pulse {movement_ticks}"
        );
        assert_eq!(
            world.get_block(head),
            if movement_ticks < 3 {
                Block::Air
            } else {
                Block::Stone {}
            },
            "pulse {movement_ticks}"
        );
        assert!(matches!(world.get_block(base()), Block::Piston { piston } if !piston.extended));
    }
}

#[test]
fn push_chain_snapshots_preserve_overlapping_payloads() {
    let mut world = empty_world();
    let head = base().offset(BlockFace::East);
    world.set_block(head.offset(BlockFace::East), Block::GoldBlock {});
    start(&mut world, base(), BlockFacing::East, Block::Stone {});
    settle(&mut world);
    assert_eq!(
        world.get_block(head.offset(BlockFace::East)),
        Block::Stone {}
    );
    assert_eq!(
        world.get_block(head.offset(BlockFace::East).offset(BlockFace::East)),
        Block::GoldBlock {}
    );
}

#[test]
fn canceled_event_rechecks_power_and_is_deduplicated() {
    let mut world = empty_world();
    let pos = base();
    world.set_block(
        pos,
        Block::Piston {
            piston: piston(BlockFacing::East, true, false),
        },
    );
    world.set_block(pos.offset(BlockFace::Bottom), Block::RedstoneBlock {});
    for _ in 0..5 {
        crate::redstone::update(world.get_block(pos), &mut world, pos, None);
    }
    assert_eq!(world.piston_state().events.len(), 1);
    world.set_block(pos.offset(BlockFace::Bottom), Block::Air);
    world.picotick_advance(1);
    assert!(!extended(&world, pos));
    assert!(world.piston_state().motions.is_empty());
}

#[test]
fn replacement_payload_cancels_old_completion() {
    let mut world = empty_world();
    let dest = base().offset(BlockFace::East).offset(BlockFace::East);
    start(&mut world, base(), BlockFacing::East, Block::Stone {});
    world.tick_interpreted();
    world.set_block(dest, Block::GoldBlock {});
    settle(&mut world);
    assert_eq!(world.get_block(dest), Block::GoldBlock {});
    assert!(world.get_block_entity(dest).is_none());
}

#[test]
fn breaking_moving_head_removes_base_without_resurrection() {
    let mut world = empty_world();
    let head = base().offset(BlockFace::East);
    start(&mut world, base(), BlockFacing::East, Block::Stone {});
    crate::interaction::destroy(world.get_block(head), &mut world, head);
    settle(&mut world);
    assert_eq!(world.get_block(head), Block::Air);
    assert_eq!(world.get_block(base()), Block::Air);
}

#[test]
fn breaking_base_during_extension_removes_owned_moving_head() {
    let mut world = empty_world();
    let head = base().offset(BlockFace::East);
    start(&mut world, base(), BlockFacing::East, Block::Stone {});
    crate::interaction::destroy(world.get_block(base()), &mut world, base());
    assert_eq!(world.get_block(head), Block::Air);
    settle(&mut world);
    assert_eq!(world.get_block(head), Block::Air);
    assert_eq!(world.get_block(base()), Block::Air);
}

#[test]
fn interrupted_waterlogged_payload_retains_waterlogging() {
    let mut world = empty_world();
    let head = base().offset(BlockFace::East);
    let mut payload = named("oak_stairs");
    payload.set_properties([("waterlogged", "true")].into_iter().collect());
    start(&mut world, base(), BlockFacing::East, payload);
    world.set_block(base().offset(BlockFace::Bottom), Block::Air);
    crate::redstone::update(world.get_block(base()), &mut world, base(), None);
    settle(&mut world);
    let dropped = world.get_block(head.offset(BlockFace::East));
    assert_eq!(dropped.get_name(), "oak_stairs");
    assert_eq!(
        dropped.properties().get("waterlogged").map(String::as_str),
        Some("true")
    );
}

#[test]
fn game_nano_and_pico_steps_share_event_and_movement_order() {
    use mchprs_world::AdvancePhase;
    let mut worlds = [empty_world(), empty_world(), empty_world()];
    for world in &mut worlds {
        start(world, base(), BlockFacing::East, Block::Stone {});
    }
    for game_tick in 1..=4 {
        worlds[0].tick_interpreted();
        for mode in 1..=2 {
            for _ in 0..30 {
                if worlds[mode].piston_state().logical_tick == game_tick
                    && worlds[mode].piston_state().phase == AdvancePhase::BetweenTicks
                {
                    break;
                }
                if mode == 1 {
                    worlds[mode].nanotick_advance(1);
                } else {
                    worlds[mode].picotick_advance(1);
                }
            }
            assert_eq!(worlds[mode].piston_state().logical_tick, game_tick);
            assert_eq!(
                worlds[mode].piston_state().phase,
                AdvancePhase::BetweenTicks
            );
            assert_eq!(
                worlds[mode].piston_state().events,
                worlds[0].piston_state().events
            );
            for n in 0..=2 {
                let p = BlockPos::new(base().x + n, base().y, base().z);
                assert_eq!(worlds[mode].get_block(p), worlds[0].get_block(p));
            }
            let snapshot = |w: &PlotWorld| {
                w.piston_state()
                    .motions
                    .iter()
                    .map(|m| {
                        (
                            m.pos,
                            m.identity,
                            m.progress,
                            m.previous_progress,
                            m.last_tick,
                        )
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(snapshot(&worlds[mode]), snapshot(&worlds[0]));
        }
    }
}
