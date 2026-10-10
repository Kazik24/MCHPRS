use crate::world::{BlockAction, World};
use mchprs_blocks::block_entities::{BlockEntity, MovingPistonEntity};
use mchprs_blocks::blocks::{Block, RedstonePiston};
use mchprs_blocks::{BlockFace, BlockFacing, BlockPos};
use mchprs_world::{AdvancePhase, PistonAction, PistonEvent};
use smallvec::SmallVec;

mod instant;

#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) mod trace;

const NEIGHBORS: [BlockFace; 6] = [
    BlockFace::West,
    BlockFace::East,
    BlockFace::Bottom,
    BlockFace::Top,
    BlockFace::North,
    BlockFace::South,
];

// Java BlockBehaviour.UPDATE_SHAPE_ORDER differs from neighbor power callbacks.
// Observers schedule their pulse ticks in this order; swapping vertical and
// horizontal faces can make a falling wire edge recheck a quasi-powered piston
// after its powering observer has switched off, causing an extra piston cycle.
const SHAPE_NEIGHBORS: [BlockFace; 6] = [
    BlockFace::West,
    BlockFace::East,
    BlockFace::North,
    BlockFace::South,
    BlockFace::Bottom,
    BlockFace::Top,
];

fn powered(world: &impl World, pos: BlockPos, face: BlockFace) -> bool {
    let p = pos.offset(face);
    super::has_redstone_power(world.get_block(p), world, p, face)
}

pub fn should_piston_extend(world: &impl World, facing: BlockFacing, pos: BlockPos) -> bool {
    if let Some(routes) = world
        .instant_piston_cache()
        .and_then(|cache| cache.power_routes(pos, facing.into()))
    {
        return routes.iter().any(|route| {
            let block = Block::from_id(world.get_wire_block_raw(route.source));
            if block.is_solid() {
                route
                    .strong
                    .iter()
                    .zip(BlockFace::values())
                    .any(|(&source, side)| {
                        super::get_strong_power(
                            Block::from_id(world.get_wire_block_raw(source)),
                            world,
                            source.pos,
                            side,
                            true,
                        ) > 0
                    })
            } else {
                super::get_weak_power(block, world, route.source.pos, route.side, true) > 0
            }
        });
    }
    let front: BlockFace = facing.into();
    if NEIGHBORS
        .into_iter()
        .any(|f| f != front && powered(world, pos, f))
    {
        return true;
    }
    // Direct signal from below, and quasi-connectivity around the block above.
    let above = pos.offset(BlockFace::Top);
    powered(world, above, BlockFace::Bottom)
        || NEIGHBORS
            .into_iter()
            .any(|f| f != BlockFace::Bottom && powered(world, above, f))
}

pub fn update_piston_state(world: &mut impl World, piston: RedstonePiston, pos: BlockPos) {
    let extending = should_piston_extend(world, piston.facing, pos);
    #[cfg(test)]
    trace::sample(world, pos, piston.extended, extending);
    if extending == piston.extended {
        return;
    }
    let facing = piston.facing.into();
    let action = if extending {
        if !walk_payload_line(world, pos.offset(facing), facing, |_| {}) {
            return;
        }
        PistonAction::Extend
    } else {
        let ahead = pos.offset(facing).offset(facing);
        let early = match world.get_block_entity(ahead) {
            Some(BlockEntity::MovingPiston(e)) if e.extending && e.facing == facing => {
                let index = world.piston_motion_index(ahead, None);
                let state = world.piston_state();
                index.map(|i| &state.motions[i]).is_some_and(|motion| {
                    motion.previous_progress < 0.5
                        || motion.last_tick == state.logical_tick
                        || matches!(
                            state.phase,
                            AdvancePhase::ScheduledTicks | AdvancePhase::PistonEvents
                        )
                })
            }
            _ => false,
        };
        if early {
            PistonAction::RetractWithoutPull
        } else {
            PistonAction::Retract
        }
    };
    let event = PistonEvent {
        pos,
        sticky: piston.sticky,
        facing,
        action,
    };
    world.enqueue_piston_event(event);
}

// Legacy scheduled base ticks become requests, never movement completion or cooldowns.
pub fn piston_tick(world: &mut impl World, piston: RedstonePiston, pos: BlockPos) {
    update_piston_state(world, piston, pos);
}

pub(crate) fn execute_event(world: &mut impl World, event: PistonEvent) {
    #[cfg(test)]
    super::instant_piston_tests::record_event(world, "event_execute", event);
    let Block::Piston { piston } = world.get_block(event.pos) else {
        return;
    };
    if piston.sticky != event.sticky {
        return;
    }
    let power = should_piston_extend(world, piston.facing, event.pos);
    match event.action {
        PistonAction::Extend if power && !piston.extended => {
            if !extend(world, piston, event.pos) {
                return;
            }
        }
        PistonAction::Retract | PistonAction::RetractWithoutPull if !power && piston.extended => {
            retract(world, piston, event.pos, event.action, event.facing);
        }
        _ => return,
    }
    world.block_action(
        event.pos,
        BlockAction::Piston {
            action: event.action,
            piston: RedstonePiston {
                facing: event.facing.into(),
                ..piston
            },
        },
    );
    #[cfg(test)]
    super::instant_piston_tests::record_event(world, "event_applied", event);
    #[cfg(test)]
    trace::applied(world, event);
}

fn moving(
    world: &mut impl World,
    pos: BlockPos,
    piston: RedstonePiston,
    carried: Block,
    extending: bool,
    source: bool,
    carried_entity: Option<BlockEntity>,
) {
    world.delete_block_entity(pos);
    world.set_block(
        pos,
        Block::MovingPiston {
            moving: piston.into(),
        },
    );
    world.set_block_entity(
        pos,
        BlockEntity::MovingPiston(MovingPistonEntity {
            facing: piston.facing.into(),
            progress: 0,
            block_state: carried.get_id(),
            extending,
            source,
        }),
    );
    world.set_piston_carried_entity(pos, carried_entity.map(Box::new));
}

fn extend(world: &mut impl World, piston: RedstonePiston, pos: BlockPos) -> bool {
    let facing = piston.facing.into();
    let awaiting_reset = world
        .instant_piston_cache()
        .is_some_and(|cache| cache.awaiting_reset(pos, facing));
    let mut reset = awaiting_reset && instant::reset_driven(world, pos, facing);
    if awaiting_reset && !reset {
        if let Some(cache) = world.instant_piston_cache_mut() {
            cache.invalidate(pos);
        }
    }
    let Some(line) = payload_line(world, pos.offset(facing), facing) else {
        if let Some(cache) = world.instant_piston_cache_mut() {
            cache.invalidate(pos);
        }
        return false;
    };
    if reset {
        let head = pos.offset(facing);
        let far = head.offset(facing);
        reset = line.as_slice() == [head]
            && world.get_block(head) == Block::RedstoneBlock
            && world.get_block_entity(head).is_none()
            && world.get_block(far) == Block::Air
            && world.get_block_entity(far).is_none();
        if !reset {
            if let Some(cache) = world.instant_piston_cache_mut() {
                cache.invalidate(pos);
            }
        }
    }
    // Snapshot before writing overlapping source and destination cells.
    let payloads: SmallVec<[_; 2]> = line
        .iter()
        .map(|&p| (p, world.get_block(p), world.get_block_entity(p).cloned()))
        .collect();
    for (p, block, entity) in payloads.into_iter().rev() {
        moving(world, p.offset(facing), piston, block, true, false, entity);
    }
    // Every source is overwritten by the preceding payload's destination;
    // the first source is replaced by the moving head below.
    moving(
        world,
        pos.offset(facing),
        piston,
        Block::PistonHead {
            head: piston.into(),
        },
        true,
        true,
        None,
    );
    // Destination writes notify watched faces as well as vacated sources.
    for &p in line.iter().rev() {
        shape_changed(world, p.offset(facing));
    }
    for &p in line.iter().rev() {
        notify(world, p);
    }
    notify(world, pos.offset(facing));
    world.set_block(
        pos,
        Block::Piston {
            piston: piston.extend(true),
        },
    );
    notify(world, pos);
    if reset {
        let head = pos.offset(facing);
        let ahead = head.offset(facing);
        let identities = [head, ahead].map(|cell| {
            world
                .piston_motion_index(cell, None)
                .map(|index| world.piston_state().motions[index].identity)
        });
        if let [Some(head), Some(payload)] = identities {
            let routes = world
                .instant_piston_cache()
                .filter(|cache| !cache.has_routes(pos, facing))
                .map(|_| instant::routes(world, pos, facing));
            if let Some(cache) = world.instant_piston_cache_mut() {
                cache.begin_reset(pos, facing, [head, payload], routes);
            }
        }
    }
    true
}

fn retract(
    world: &mut impl World,
    piston: RedstonePiston,
    pos: BlockPos,
    action: PistonAction,
    captured_facing: BlockFace,
) {
    let facing: BlockFace = piston.facing.into();
    let head = pos.offset(facing);
    let candidate = world.instant_piston_cache().is_some()
        && instant::candidate_retract(world, piston, pos, action);
    let mut pulled_redstone = false;
    if matches!(world.get_block(head), Block::MovingPiston { .. }) {
        finish(world, head, true);
    }
    let carried_base = RedstonePiston {
        facing: captured_facing.into(),
        extended: false,
        ..piston
    };
    moving(
        world,
        pos,
        piston,
        Block::Piston {
            piston: carried_base,
        },
        false,
        true,
        None,
    );
    notify(world, pos);
    let ahead = head.offset(facing);
    let moving_extension = matches!(world.get_block(ahead), Block::MovingPiston { .. })
        && matches!(world.get_block_entity(ahead), Some(BlockEntity::MovingPiston(e))
            if e.extending && e.facing == facing);
    world.delete_block_entity(head);
    world.set_block(head, Block::Air);
    if piston.sticky && moving_extension {
        // Java finalizes an in-flight payload here even for a normal retract event.
        finish(world, ahead, true);
    } else if piston.sticky && action == PistonAction::Retract {
        let block = world.get_block(ahead);
        if block != Block::Air
            && !matches!(block, Block::MovingPiston { .. })
            && !immovable_container(block)
        {
            let entity = world.get_block_entity(ahead).cloned();
            pulled_redstone = block == Block::RedstoneBlock {} && entity.is_none();
            moving(world, head, piston, block, false, false, entity);
            world.delete_block_entity(ahead);
            world.set_block(ahead, Block::Air);
            notify(world, ahead);
        }
    }

    notify(world, head);
    if candidate && pulled_redstone {
        let identities = [pos, head].map(|cell| {
            world
                .piston_motion_index(cell, None)
                .map(|index| world.piston_state().motions[index].identity)
        });
        if let [Some(base), Some(payload)] = identities {
            if let Some(cache) = world.instant_piston_cache_mut() {
                cache.begin_retract(pos, facing, [base, payload]);
            }
        }
    } else if let Some(cache) = world.instant_piston_cache_mut() {
        cache.invalidate(pos);
    }
}

pub(crate) fn tick_motion(world: &mut impl World, pos: BlockPos, identity: u64) {
    let Some(i) = world.piston_motion_index(pos, Some(identity)) else {
        return;
    };
    if !matches!(world.get_block(pos), Block::MovingPiston { .. })
        || !matches!(
            world.get_block_entity(pos),
            Some(BlockEntity::MovingPiston(_))
        )
    {
        if let Some(cache) = world.instant_piston_cache_mut() {
            cache.reject_motion(pos, identity);
        }
        world.remove_piston_motion(i);
        return;
    }
    let (complete, progress) = world.advance_piston_motion(i);
    if complete {
        finish(world, pos, false);
    } else {
        world.set_piston_progress(pos, progress);
    }
}

fn finish(world: &mut impl World, pos: BlockPos, interrupted: bool) {
    if !matches!(world.get_block(pos), Block::MovingPiston { .. }) {
        return;
    }
    let Some(BlockEntity::MovingPiston(entity)) = world.get_block_entity(pos).cloned() else {
        return;
    };
    let mut block = if interrupted && entity.source {
        Block::Air
    } else {
        Block::from_id(entity.block_state)
    };
    if !interrupted {
        block = block.without_waterlogging();
    }
    let motion = world.piston_motion_index(pos, None);
    let carried_entity =
        motion.and_then(|i| world.piston_state().motions[i].carried_entity.clone());
    let identity = motion.map(|index| world.piston_state().motions[index].identity);
    world.delete_piston_entity(pos);
    world.set_block(pos, block);
    if let Some(entity) = carried_entity {
        world.set_block_entity(pos, *entity);
    }
    if let Block::PistonHead { head } = block {
        let base = world.get_block(pos.offset(BlockFace::from(head.facing).opposite()));
        if !matches!(base, Block::Piston { piston } if piston.extended && piston.facing == head.facing && piston.sticky == head.sticky)
        {
            block = Block::Air;
            world.set_block(pos, block);
        }
    }
    if let Block::Observer { mut observer } = block {
        if observer.powered && !world.pending_tick_at(pos) {
            observer.powered = false;
            block = Block::Observer { observer };
            world.set_block(pos, block);
            super::on_observer_state_change(observer.facing, world, pos);
        }
    }
    if !crate::interaction::is_valid_position(block, world, pos) {
        world.delete_block_entity(pos);
        world.set_block(pos, Block::Air);
    }
    // Restored components get their placement/recheck callback once. Neighbor
    // notifications alone must not recheck the source during every event write.
    let restored = world.get_block(pos);
    if !matches!(restored, Block::Observer { .. } | Block::PistonHead { .. }) {
        super::update(restored, world, pos, None);
    }
    notify(world, pos);
    if world
        .instant_piston_cache()
        .is_some_and(|cache| !cache.is_empty())
    {
        if let Some(identity) = identity {
            instant::completed(world, pos, entity, identity, interrupted);
        }
    }
}

fn shape_changed(world: &mut impl World, pos: BlockPos) {
    for face in SHAPE_NEIGHBORS {
        let neighbor = pos.offset(face);
        // interaction::change takes the direction from the changed block to
        // its neighbor; observer callbacks below take the opposite direction.
        crate::interaction::change(world.get_block(neighbor), world, neighbor, face);
        // Observers watch shape/state changes, not ordinary neighbor power callbacks.
        let block = world.get_block(neighbor);
        if matches!(block, Block::Observer { .. }) {
            super::update(block, world, neighbor, Some(face.opposite()));
        }
    }
}

pub(crate) fn notify(world: &mut impl World, pos: BlockPos) {
    #[cfg(test)]
    let _source = super::instant_piston_tests::notification_source(Some(pos));
    shape_changed(world, pos);
    for face in NEIGHBORS {
        let neighbor = pos.offset(face);
        let block = world.get_block(neighbor);
        if !matches!(block, Block::Observer { .. }) {
            super::update(block, world, neighbor, Some(face.opposite()));
        }
    }
}

/// Breaking an owned source/head removes its counterpart. Payload movement is
/// independent and can finish without rewriting or resurrecting that source.
pub(crate) fn remove_owned_parts(
    world: &mut impl World,
    block: Block,
    pos: BlockPos,
) -> Option<BlockPos> {
    match block {
        Block::Piston { piston } if piston.extended => {
            let head = pos.offset(piston.facing.into());
            let owned = matches!(world.get_block(head), Block::PistonHead { head }
                if head.facing == piston.facing && head.sticky == piston.sticky)
                || matches!(world.get_block(head), Block::MovingPiston { .. })
                    && matches!(world.get_block_entity(head), Some(BlockEntity::MovingPiston(e))
                        if e.source && e.extending && e.facing == BlockFace::from(piston.facing));
            if owned {
                world.delete_block_entity(head);
                world.set_block(head, Block::Air);
                Some(head)
            } else {
                None
            }
        }
        Block::PistonHead { head } => {
            remove_matching_base(world, pos, head.facing.into(), head.sticky)
        }
        Block::MovingPiston { moving } => {
            if matches!(world.get_block_entity(pos), Some(BlockEntity::MovingPiston(e)) if e.source && e.extending)
            {
                remove_matching_base(world, pos, moving.facing.into(), moving.sticky)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn remove_matching_base(
    world: &mut impl World,
    head: BlockPos,
    facing: BlockFace,
    sticky: bool,
) -> Option<BlockPos> {
    let base = head.offset(facing.opposite());
    if matches!(world.get_block(base), Block::Piston { piston }
        if piston.extended && BlockFace::from(piston.facing) == facing && piston.sticky == sticky)
    {
        world.delete_block_entity(base);
        world.set_block(base, Block::Air);
        Some(base)
    } else {
        None
    }
}

fn immovable_container(block: Block) -> bool {
    mchprs_blocks::block_entities::ContainerType::from_block(block).is_some()
}

fn payload_line(
    world: &impl World,
    start: BlockPos,
    facing: BlockFace,
) -> Option<SmallVec<[BlockPos; 8]>> {
    let mut line = SmallVec::new();
    walk_payload_line(world, start, facing, |pos| line.push(pos)).then_some(line)
}

// Validation and collection share the movement rules. The no-op visitor used
// while requesting an event avoids allocating a line that would be discarded.
fn walk_payload_line(
    world: &impl World,
    start: BlockPos,
    facing: BlockFace,
    mut visit: impl FnMut(BlockPos),
) -> bool {
    let mut pos = start;
    loop {
        if !(0..crate::plot::PLOT_BLOCK_HEIGHT).contains(&pos.y)
            || world
                .get_chunk(pos.x.div_euclid(16), pos.z.div_euclid(16))
                .is_none()
        {
            return false;
        }
        let block = world.get_block(pos);
        if immovable_container(block) {
            return false;
        }
        match block {
            Block::Air => return true,
            // A moving entity belongs to another operation; do not nest its state.
            Block::MovingPiston { .. } | Block::PistonHead { .. } => return false,
            _ => visit(pos),
        }
        pos = pos.offset(facing);
    }
}
