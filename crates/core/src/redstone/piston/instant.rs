//! Physical retract/reset qualification; every event, motion and callback stays native.
use super::{NEIGHBORS, SHAPE_NEIGHBORS};
use crate::redstone::{has_redstone_power, power};
use crate::world::{PowerRoute, WireNeighbor, World};
use mchprs_blocks::block_entities::{BlockEntity, MovingPistonEntity};
use mchprs_blocks::blocks::{Block, RedstonePiston};
use mchprs_blocks::{BlockFace, BlockFacing, BlockPos};
use mchprs_world::PistonAction;

fn reset_geometry(world: &impl World, base: BlockPos) -> bool {
    let observer = base.offset(BlockFace::Top);
    let cap = observer.offset(BlockFace::Top);
    matches!(world.get_block(observer), Block::Observer { observer } if observer.facing == BlockFacing::Down)
        && world.get_block(cap).is_solid()
        && world.get_block_entity(observer).is_none()
        && world.get_block_entity(cap).is_none()
}

pub(crate) fn candidate_retract(
    world: &impl World,
    piston: RedstonePiston,
    base: BlockPos,
    action: PistonAction,
) -> bool {
    if action != PistonAction::Retract
        || !piston.sticky
        || !piston.extended
        || piston.facing == BlockFacing::Up
        || !reset_geometry(world, base)
    {
        return false;
    }
    let face = piston.facing.into();
    let head = base.offset(face);
    let far = head.offset(face);
    if !matches!(world.get_block(base), Block::Piston { piston: current } if current == piston)
        || !matching_head(world.get_block(head), face)
        || world.get_block(far) != Block::RedstoneBlock
        || [base, head, far].into_iter().any(|pos| {
            world.get_block_entity(pos).is_some() || world.piston_motion_index(pos, None).is_some()
        })
    {
        return false;
    }
    // One payload must not be owned by two stationary sticky actors.
    !NEIGHBORS.into_iter().any(|direction| {
        let owner = far.offset(direction).offset(direction);
        owner != base
            && matches!(world.get_block(owner), Block::Piston { piston }
                if piston.sticky && BlockFace::from(piston.facing) == direction.opposite())
    })
}

fn power_positions(base: BlockPos, face: BlockFace) -> impl Iterator<Item = (BlockPos, BlockFace)> {
    let above = base.offset(BlockFace::Top);
    NEIGHBORS
        .into_iter()
        .filter(move |&side| side != face)
        .map(move |side| (base.offset(side), side))
        .chain(std::iter::once((base, BlockFace::Bottom)))
        .chain(
            NEIGHBORS
                .into_iter()
                .filter(|&side| side != BlockFace::Bottom)
                .map(move |side| (above.offset(side), side)),
        )
}

/// The native power positions, including solid-block strong sources, in read order.
pub(crate) fn routes(world: &impl World, base: BlockPos, face: BlockFace) -> [PowerRoute; 11] {
    let neighbor = |pos| WireNeighbor {
        pos,
        cell: world.wire_location(pos),
    };
    let strong_sides = BlockFace::values();
    let mut positions = power_positions(base, face);
    std::array::from_fn(|_| {
        let (pos, side) = positions.next().unwrap();
        PowerRoute {
            source: neighbor(pos),
            side,
            strong: std::array::from_fn(|index| neighbor(pos.offset(strong_sides[index]))),
        }
    })
}

pub(crate) fn reset_driven(world: &impl World, base: BlockPos, face: BlockFace) -> bool {
    if !reset_geometry(world, base) || face == BlockFace::Top {
        return false;
    }
    let observer = base.offset(BlockFace::Top);
    let cap = observer.offset(BlockFace::Top);
    if !matches!(world.get_block(observer), Block::Observer { observer } if observer.powered) {
        return false;
    }
    // An unpowered alternate cap writer can become active during this reset.
    if SHAPE_NEIGHBORS.into_iter().any(|side| {
        let source = cap.offset(side);
        source != observer
            && power::emits_strong_power(world.get_block(source), world, source, side, true)
    }) {
        return false;
    }
    power_positions(base, face).all(|(source, side)| {
        source == cap || !has_redstone_power(world.get_block(source), world, source, side)
    })
}

fn matching_base(block: Block, face: BlockFace, extended: bool) -> bool {
    matches!(block, Block::Piston { piston }
        if piston.sticky && BlockFace::from(piston.facing) == face && piston.extended == extended)
}

fn matching_head(block: Block, face: BlockFace) -> bool {
    matches!(block, Block::PistonHead { head }
        if head.sticky && !head.short && BlockFace::from(head.facing) == face)
}

fn companion(
    world: &impl World,
    pos: BlockPos,
    face: BlockFace,
    extending: bool,
    source: bool,
    carried: impl Fn(Block) -> bool,
) -> bool {
    if let Some(BlockEntity::MovingPiston(entity)) = world.get_block_entity(pos) {
        matches!(world.get_block(pos), Block::MovingPiston { moving }
            if moving.sticky && BlockFace::from(moving.facing) == face)
            && entity.facing == face
            && entity.extending == extending
            && entity.source == source
            && carried(Block::from_id(entity.block_state))
            && world.piston_motion_index(pos, None).is_some_and(|index| {
                let motion = &world.piston_state().motions[index];
                let (base, slot) = match (extending, source) {
                    (false, true) => (pos, 0),
                    (false, false) => (pos.offset(face.opposite()), 1),
                    (true, true) => (pos.offset(face.opposite()), 0),
                    (true, false) => (pos.offset(face.opposite()).offset(face.opposite()), 1),
                };
                motion.carried_entity.is_none()
                    && world.instant_piston_cache().is_some_and(|cache| {
                        cache.tracks(base, face, slot, motion.identity, extending)
                    })
            })
    } else {
        world.get_block_entity(pos).is_none() && carried(world.get_block(pos))
    }
}

pub(crate) fn completed(
    world: &mut impl World,
    pos: BlockPos,
    entity: MovingPistonEntity,
    identity: u64,
    interrupted: bool,
) {
    if !world
        .instant_piston_cache()
        .is_some_and(|cache| !cache.is_empty())
    {
        return;
    }
    let face = entity.facing;
    let (base, slot) = match (
        entity.extending,
        entity.source,
        Block::from_id(entity.block_state),
    ) {
        (false, true, Block::Piston { .. }) => (pos, 0),
        (false, false, Block::RedstoneBlock) => (pos.offset(face.opposite()), 1),
        (true, true, Block::PistonHead { .. }) => (pos.offset(face.opposite()), 0),
        (true, false, Block::RedstoneBlock) => {
            (pos.offset(face.opposite()).offset(face.opposite()), 1)
        }
        _ => return,
    };
    if !world
        .instant_piston_cache()
        .is_some_and(|cache| cache.tracks(base, face, slot, identity, entity.extending))
    {
        return;
    }
    let head = base.offset(face);
    let far = head.offset(face);
    let restored = world.get_block_entity(pos).is_none()
        && match (entity.extending, slot) {
            (false, 0) => matching_base(world.get_block(pos), face, false),
            (true, 0) => matching_head(world.get_block(pos), face),
            (_, 1) => world.get_block(pos) == Block::RedstoneBlock,
            _ => false,
        };
    let footprint_ok = !interrupted
        && restored
        && face != BlockFace::Top
        && reset_geometry(world, base)
        && if entity.extending {
            matching_base(world.get_block(base), face, true)
                && world.get_block_entity(base).is_none()
                && companion(world, head, face, true, true, |block| {
                    matching_head(block, face)
                })
                && companion(world, far, face, true, false, |block| {
                    block == Block::RedstoneBlock
                })
        } else {
            companion(world, base, face, false, true, |block| {
                matching_base(block, face, false)
            }) && companion(world, head, face, false, false, |block| {
                block == Block::RedstoneBlock
            }) && world.get_block(far) == Block::Air
                && world.get_block_entity(far).is_none()
        };
    if let Some(cache) = world.instant_piston_cache_mut() {
        cache.completed(base, face, slot, identity, entity.extending, footprint_ok);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plot::{PLOT_WIDTH, PlotWorld};
    use crate::world::storage::Chunk;
    use mchprs_blocks::blocks::RedstoneObserver;

    fn cell() -> (PlotWorld, BlockPos, RedstonePiston) {
        let chunks = (0..PLOT_WIDTH)
            .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
            .collect();
        let mut world = PlotWorld::from_chunks(0, 0, chunks, Default::default());
        let base = BlockPos::new(4, 8, 4);
        let piston = RedstonePiston {
            facing: BlockFacing::East,
            sticky: true,
            extended: true,
        };
        world.set_block(base, Block::Piston { piston });
        world.set_block(
            base.offset(BlockFace::East),
            Block::PistonHead {
                head: piston.into(),
            },
        );
        world.set_block(
            base.offset(BlockFace::East).offset(BlockFace::East),
            Block::RedstoneBlock,
        );
        world.set_block(
            base.offset(BlockFace::Top),
            Block::Observer {
                observer: RedstoneObserver {
                    facing: BlockFacing::Down,
                    powered: false,
                },
            },
        );
        world.set_block(
            base.offset(BlockFace::Top).offset(BlockFace::Top),
            Block::Stone {},
        );
        (world, base, piston)
    }

    #[test]
    fn qualification_rejects_drop_retraction_and_shared_payload() {
        let (mut world, base, piston) = cell();
        assert!(candidate_retract(
            &world,
            piston,
            base,
            PistonAction::Retract
        ));
        assert!(!candidate_retract(
            &world,
            piston,
            base,
            PistonAction::RetractWithoutPull
        ));
        let other = base
            .offset(BlockFace::East)
            .offset(BlockFace::East)
            .offset(BlockFace::North)
            .offset(BlockFace::North);
        world.set_block(
            other,
            Block::Piston {
                piston: RedstonePiston {
                    facing: BlockFacing::South,
                    ..piston
                },
            },
        );
        assert!(!candidate_retract(
            &world,
            piston,
            base,
            PistonAction::Retract
        ));
    }

    #[test]
    fn reset_requires_observer_pulse_without_an_alternate_cap_writer() {
        let (mut world, base, _) = cell();
        assert!(!reset_driven(&world, base, BlockFace::East));
        let observer_pos = base.offset(BlockFace::Top);
        world.set_block(
            observer_pos,
            Block::Observer {
                observer: RedstoneObserver {
                    facing: BlockFacing::Down,
                    powered: true,
                },
            },
        );
        assert!(reset_driven(&world, base, BlockFace::East));
        // An inactive observer is still a possible competing reset writer.
        let cap = observer_pos.offset(BlockFace::Top);
        world.set_block(
            cap.offset(BlockFace::West),
            Block::Observer {
                observer: RedstoneObserver {
                    facing: BlockFacing::West,
                    powered: false,
                },
            },
        );
        assert!(!reset_driven(&world, base, BlockFace::East));
    }

    #[test]
    fn routes_preserve_native_order_for_each_facing() {
        let (world, base, _) = cell();
        for face in NEIGHBORS {
            let actual = routes(&world, base, face).map(|route| (route.source.pos, route.side));
            let expected: Vec<_> = NEIGHBORS
                .into_iter()
                .filter(|&side| side != face)
                .map(|side| (base.offset(side), side))
                .chain(std::iter::once((base, BlockFace::Bottom)))
                .chain(
                    NEIGHBORS
                        .into_iter()
                        .filter(|&side| side != BlockFace::Bottom)
                        .map(|side| (base.offset(BlockFace::Top).offset(side), side)),
                )
                .collect();
            assert_eq!(actual.as_slice(), expected);
            assert_eq!(actual[5], (base, BlockFace::Bottom));
        }
    }

    #[test]
    fn solid_power_addresses_preserve_native_order_for_each_facing() {
        let (world, base, _) = cell();
        for face in BlockFace::values() {
            for route in routes(&world, base, face) {
                for (source, side) in route.strong.into_iter().zip(BlockFace::values()) {
                    assert_eq!(source.pos, route.source.pos.offset(side));
                    assert_eq!(source.cell, world.wire_location(source.pos));
                }
            }
        }
    }

    fn native_state(world: &PlotWorld) -> serde_json::Value {
        let mut blocks = Vec::new();
        let bounds = world.get_corners();
        crate::world::for_each_block_optimized(world, bounds.0, bounds.1, |pos| {
            let state = world.get_block_raw(pos);
            if state != 0 {
                blocks.push((pos, state));
            }
        });
        blocks.sort_by_key(|(pos, _)| (pos.x, pos.y, pos.z));
        let mut entities: Vec<_> = world
            .get_chunks()
            .iter()
            .flat_map(|chunk| {
                chunk
                    .block_entities
                    .iter()
                    .map(move |(pos, entity)| (chunk.x, chunk.z, *pos, entity))
            })
            .collect();
        entities.sort_by_key(|(x, z, pos, _)| (*x, *z, pos.x, pos.y, pos.z));
        serde_json::json!({"blocks":blocks,"entities":entities,
            "pistons":world.piston_state(),
            "scheduler":world.scheduler().iter_entries().collect::<Vec<_>>()})
    }

    fn source(world: &mut PlotWorld, base: BlockPos, powered: bool) {
        let pos = base.offset(BlockFace::West);
        world.set_block(
            pos,
            if powered {
                Block::RedstoneBlock
            } else {
                Block::Air
            },
        );
        super::super::notify(world, pos);
    }

    fn paired_tick(
        cached: &mut PlotWorld,
        native: &mut PlotWorld,
    ) -> Vec<super::super::trace::Entry> {
        native.instant_piston_cache_mut().unwrap().clear();
        let actual = super::super::trace::capture(|| cached.tick_interpreted());
        let expected = super::super::trace::capture(|| native.tick_interpreted());
        assert_eq!(
            actual, expected,
            "ordered native samples and accepted events"
        );
        assert_eq!(
            native_state(cached),
            native_state(native),
            "complete physical state"
        );
        actual
    }

    fn paired_source(
        cached: &mut PlotWorld,
        native: &mut PlotWorld,
        base: BlockPos,
        powered: bool,
    ) {
        native.instant_piston_cache_mut().unwrap().clear();
        let actual = super::super::trace::capture(|| source(cached, base, powered));
        let expected = super::super::trace::capture(|| source(native, base, powered));
        assert_eq!(actual, expected);
        assert_eq!(native_state(cached), native_state(native));
    }

    fn proven_pair() -> (PlotWorld, PlotWorld, BlockPos) {
        let (mut cached, base, _) = cell();
        let (mut native, _, _) = cell();
        paired_source(&mut cached, &mut native, base, true);
        paired_source(&mut cached, &mut native, base, false);
        let mut reset_seen = false;
        for _ in 0..12 {
            let entries = paired_tick(&mut cached, &mut native);
            if !reset_seen
                && entries.iter().any(|entry| {
                    matches!(entry.operation, super::super::trace::Operation::Applied(event)
                        if event.pos == base && event.action == PistonAction::Extend)
                })
            {
                reset_seen = true;
                paired_source(&mut cached, &mut native, base, true);
            }
        }
        assert!(reset_seen);
        assert_eq!(cached.instant_piston_cache().unwrap().stats().1, 1);
        (cached, native, base)
    }

    #[test]
    fn proven_routes_read_changed_solid_power_sources_live() {
        let (mut cached, mut native, base) = proven_pair();
        let solid = base.offset(BlockFace::West);
        let strong = solid.offset(BlockFace::Bottom);
        cached.set_block(solid, Block::Stone {});
        native.set_block(solid, Block::Stone {});
        native.instant_piston_cache_mut().unwrap().clear();
        for lit in [false, true, false, true] {
            cached.set_block(strong, Block::RedstoneTorch { lit });
            native.set_block(strong, Block::RedstoneTorch { lit });
            assert_eq!(cached.instant_piston_cache().unwrap().stats().1, 1);
            let actual = super::super::should_piston_extend(&cached, BlockFacing::East, base);
            let expected = super::super::should_piston_extend(&native, BlockFacing::East, base);
            assert_eq!(actual, expected, "live torch state {lit}");
            assert_eq!(actual, lit, "solid neighbor must follow its strong source");
            assert_eq!(native_state(&cached), native_state(&native));
        }
        assert!(cached.instant_piston_cache().unwrap().stats().2 > 0);
    }

    #[test]
    fn replacing_proven_cap_with_furnace_revokes_and_preserves_native_behavior() {
        let (mut cached, mut native, base) = proven_pair();
        let cap = base.offset(BlockFace::Top).offset(BlockFace::Top);
        let furnace = Block::from_name("furnace").unwrap();
        cached.set_block(cap, furnace);
        native.set_block(cap, furnace);
        assert_eq!(cached.instant_piston_cache().unwrap().stats().1, 0);
        assert!(!reset_geometry(&cached, base));
        paired_source(&mut cached, &mut native, base, false);
        for _ in 0..12 {
            paired_tick(&mut cached, &mut native);
            assert_eq!(cached.instant_piston_cache().unwrap().stats().1, 0);
        }
    }

    #[test]
    fn editing_tracked_moving_entity_revokes_and_preserves_native_behavior() {
        let (mut cached, mut native, base) = proven_pair();
        paired_source(&mut cached, &mut native, base, false);
        paired_tick(&mut cached, &mut native);
        let head = base.offset(BlockFace::East);
        for world in [&mut cached, &mut native] {
            let Some(BlockEntity::MovingPiston(entity)) = world.get_block_entity_mut(head) else {
                panic!("native retraction must have a pulled-block motion");
            };
            assert_eq!(Block::from_id(entity.block_state), Block::RedstoneBlock);
            entity.block_state = Block::Stone {}.get_id();
        }
        assert_eq!(cached.instant_piston_cache().unwrap().stats().1, 0);
        for _ in 0..12 {
            paired_tick(&mut cached, &mut native);
            assert_eq!(cached.instant_piston_cache().unwrap().stats().1, 0);
        }
    }

    #[test]
    fn deleting_tracked_moving_entity_revokes_and_preserves_native_behavior() {
        let (mut cached, mut native, base) = proven_pair();
        paired_source(&mut cached, &mut native, base, false);
        paired_tick(&mut cached, &mut native);
        let head = base.offset(BlockFace::East);
        assert!(matches!(
            cached.get_block_entity(head),
            Some(BlockEntity::MovingPiston(_))
        ));
        assert_eq!(cached.instant_piston_cache().unwrap().stats().1, 1);
        cached.delete_block_entity(head);
        native.delete_block_entity(head);
        assert_eq!(cached.instant_piston_cache().unwrap().stats().1, 0);
        assert!(matches!(cached.get_block(head), Block::MovingPiston { .. }));
        assert_eq!(native_state(&cached), native_state(&native));
        for _ in 0..12 {
            paired_tick(&mut cached, &mut native);
            assert_eq!(cached.instant_piston_cache().unwrap().stats().1, 0);
        }
    }

    #[test]
    fn replacing_tracked_moving_block_with_air_revokes_and_preserves_native_behavior() {
        let (mut cached, mut native, base) = proven_pair();
        paired_source(&mut cached, &mut native, base, false);
        paired_tick(&mut cached, &mut native);
        let head = base.offset(BlockFace::East);
        assert!(matches!(cached.get_block(head), Block::MovingPiston { .. }));
        assert_eq!(cached.instant_piston_cache().unwrap().stats().1, 1);
        cached.set_block(head, Block::Air);
        native.set_block(head, Block::Air);
        assert_eq!(cached.instant_piston_cache().unwrap().stats().1, 0);
        assert_eq!(native_state(&cached), native_state(&native));
        for _ in 0..12 {
            paired_tick(&mut cached, &mut native);
            assert_eq!(cached.instant_piston_cache().unwrap().stats().1, 0);
        }
    }

    #[test]
    fn observer_reset_pushing_multiple_blocks_is_never_proven() {
        let (mut cached, base, _) = cell();
        let (mut native, _, _) = cell();
        paired_source(&mut cached, &mut native, base, true);
        paired_source(&mut cached, &mut native, base, false);
        for _ in 0..12 {
            paired_tick(&mut cached, &mut native);
            if cached
                .instant_piston_cache()
                .unwrap()
                .awaiting_reset(base, BlockFace::East)
            {
                break;
            }
        }
        assert!(
            cached
                .instant_piston_cache()
                .unwrap()
                .awaiting_reset(base, BlockFace::East)
        );
        let far = base.offset(BlockFace::East).offset(BlockFace::East);
        cached.set_block(far, Block::Stone {});
        native.set_block(far, Block::Stone {});
        let mut reset_seen = false;
        for _ in 0..12 {
            let entries = paired_tick(&mut cached, &mut native);
            if !reset_seen
                && entries.iter().any(|entry| {
                    matches!(entry.operation, super::super::trace::Operation::Applied(event)
                        if event.pos == base && event.action == PistonAction::Extend)
                })
            {
                reset_seen = true;
                paired_source(&mut cached, &mut native, base, true);
            }
            assert_eq!(cached.instant_piston_cache().unwrap().stats().1, 0);
        }
        assert!(
            reset_seen,
            "the native observer must still execute its reset"
        );
        assert_eq!(cached.get_block(far), Block::RedstoneBlock);
        assert_eq!(
            cached.get_block(far.offset(BlockFace::East)),
            Block::Stone {}
        );
    }

    #[test]
    fn native_observer_cycle_learns_only_after_complete_reset_and_reuses_live_routes() {
        let (mut cached, base, _) = cell();
        let (mut native, _, _) = cell();
        source(&mut cached, base, true);
        source(&mut native, base, true);
        assert_eq!(cached.instant_piston_cache().unwrap().stats(), (0, 0, 0));
        for cycle in 0..2 {
            paired_source(&mut cached, &mut native, base, false);
            let mut reset_seen = false;
            for _ in 0..12 {
                let entries = paired_tick(&mut cached, &mut native);
                if !reset_seen
                    && entries.iter().any(|entry| {
                        matches!(entry.operation, super::super::trace::Operation::Applied(event)
                        if event.pos == base && event.action == PistonAction::Extend)
                    })
                {
                    reset_seen = true;
                    // External rearm holds the already observer-driven reset until motion finishes.
                    paired_source(&mut cached, &mut native, base, true);
                }
            }
            assert!(reset_seen, "native observer must initiate the reset");
            let stats = cached.instant_piston_cache().unwrap().stats();
            assert_eq!(
                stats.1, 1,
                "cycle {cycle} must finish its paired reset motions"
            );
            if cycle == 1 {
                assert!(stats.2 > 0, "subsequent samples must use proven routes");
            }
        }
        cached.set_block(base.offset(BlockFace::Top), Block::Air);
        assert_eq!(cached.instant_piston_cache().unwrap().stats().1, 0);
    }

    #[test]
    fn held_zero_observer_cycle_learns_and_preserves_every_native_reset() {
        let (mut cached, base, _) = cell();
        let (mut native, _, _) = cell();
        source(&mut cached, base, true);
        source(&mut native, base, true);
        paired_source(&mut cached, &mut native, base, false);
        let mut learned = false;
        for _ in 0..24 {
            paired_tick(&mut cached, &mut native);
            learned |= cached.instant_piston_cache().unwrap().stats().1 != 0;
        }
        assert!(
            learned,
            "native observer reset must complete and establish proof"
        );
        assert!(
            cached.instant_piston_cache().unwrap().stats().2 > 0,
            "later native cycles must reuse proven routes"
        );
    }

    #[test]
    fn manually_rearmed_piston_without_observer_is_never_learned() {
        let (mut world, base, _) = cell();
        world.set_block(base.offset(BlockFace::Top), Block::Air);
        source(&mut world, base, true);
        for powered in [false, true, false, true] {
            source(&mut world, base, powered);
            for _ in 0..8 {
                world.tick_interpreted();
            }
            assert_eq!(world.instant_piston_cache().unwrap().stats(), (0, 0, 0));
        }
    }
}
