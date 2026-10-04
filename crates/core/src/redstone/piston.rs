use crate::world::{BlockAction, World};
use mchprs_blocks::block_entities::{BlockEntity, MovingPistonEntity};
use mchprs_blocks::blocks::{Block, RedstoneMovingPiston, RedstonePiston, RedstonePistonHead};
use mchprs_blocks::{BlockFace, BlockFacing, BlockPos};
use mchprs_world::{AdvancePhase, PistonAction, PistonEvent};

#[cfg(test)]
mod tests;

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
    super::get_redstone_power(world.get_block(p), world, p, face) > 0
}

pub fn should_piston_extend(world: &impl World, facing: BlockFacing, pos: BlockPos) -> bool {
    let front: BlockFace = facing.into();
    if NEIGHBORS
        .into_iter()
        .any(|f| f != front && powered(world, pos, f))
    {
        return true;
    }
    // Direct signal from below, and quasi-connectivity around the block above.
    powered(world, pos.offset(BlockFace::Top), BlockFace::Bottom)
        || NEIGHBORS
            .into_iter()
            .any(|f| f != BlockFace::Bottom && powered(world, pos.offset(BlockFace::Top), f))
}

pub fn update_piston_state(world: &mut impl World, piston: RedstonePiston, pos: BlockPos) {
    let extending = should_piston_extend(world, piston.facing, pos);
    if extending == piston.extended {
        return;
    }
    let facing = piston.facing.into();
    let action = if extending {
        if payload_line(world, pos.offset(facing), facing).is_none() {
            return;
        }
        PistonAction::Extend
    } else {
        let ahead = pos.offset(facing).offset(facing);
        let early = match world.get_block_entity(ahead) {
            Some(BlockEntity::MovingPiston(e)) if e.extending && e.facing == facing => {
                let s = world.piston_state();
                s.motions.iter().find(|m| m.pos == ahead).is_some_and(|m| {
                    m.previous_progress < 0.5
                        || m.last_tick == s.logical_tick
                        || matches!(
                            s.phase,
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
    if !world.piston_state().events.contains(&event) {
        world.piston_state_mut().events.push_back(event);
    }
}

// Legacy scheduled base ticks become requests, never movement completion or cooldowns.
pub fn piston_tick(world: &mut impl World, piston: RedstonePiston, pos: BlockPos) {
    update_piston_state(world, piston, pos);
}

pub(crate) fn execute_event(world: &mut impl World, event: PistonEvent) {
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
            moving: RedstoneMovingPiston {
                facing: piston.facing,
                sticky: piston.sticky,
            },
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
    if let Some(motion) = world
        .piston_state_mut()
        .motions
        .iter_mut()
        .find(|m| m.pos == pos)
    {
        motion.carried_entity = carried_entity.map(Box::new);
    }
}

fn extend(world: &mut impl World, piston: RedstonePiston, pos: BlockPos) -> bool {
    let facing = piston.facing.into();
    let Some(line) = payload_line(world, pos.offset(facing), facing) else {
        return false;
    };
    // Snapshot before writing overlapping source and destination cells.
    let payloads: Vec<_> = line
        .iter()
        .map(|&p| (p, world.get_block(p), world.get_block_entity(p).cloned()))
        .collect();
    for (p, block, entity) in payloads.iter().rev() {
        moving(
            world,
            p.offset(facing),
            piston,
            *block,
            true,
            false,
            entity.clone(),
        );
    }
    // Every source is overwritten by the preceding payload's destination;
    // the first source is replaced by the moving head below.
    moving(
        world,
        pos.offset(facing),
        piston,
        Block::PistonHead {
            head: RedstonePistonHead {
                facing: piston.facing,
                sticky: piston.sticky,
                short: false,
            },
        },
        true,
        true,
        None,
    );
    // Destination writes notify watched faces as well as vacated sources.
    for &(p, _, _) in payloads.iter().rev() {
        shape_changed(world, p.offset(facing));
    }
    for &(p, _, _) in payloads.iter().rev() {
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
        // For this timing repair, the front payload is assumed to stick.
        // Slime/honey side attachments and push reactions are deliberately deferred.
        let block = world.get_block(ahead);
        if block != Block::Air
            && !matches!(block, Block::MovingPiston { .. })
            && !immovable_container(block)
        {
            let entity = world.get_block_entity(ahead).cloned();
            moving(world, head, piston, block, false, false, entity);
            world.delete_block_entity(ahead);
            world.set_block(ahead, Block::Air);
            notify(world, ahead);
        }
    }

    notify(world, head);
}

pub(crate) fn tick_motion(world: &mut impl World, pos: BlockPos, identity: u64) {
    let Some(i) = world
        .piston_state()
        .motions
        .iter()
        .position(|m| m.pos == pos && m.identity == identity)
    else {
        return;
    };
    if !matches!(world.get_block(pos), Block::MovingPiston { .. })
        || !matches!(
            world.get_block_entity(pos),
            Some(BlockEntity::MovingPiston(_))
        )
    {
        world.piston_state_mut().motions.remove(i);
        return;
    }
    let complete;
    let progress;
    {
        let s = world.piston_state_mut();
        let motion = &mut s.motions[i];
        motion.last_tick = s.logical_tick;
        motion.previous_progress = motion.progress;
        complete = motion.progress >= 1.0;
        if !complete {
            motion.progress = (motion.progress + 0.5).min(1.0);
        }
        progress = motion.previous_progress;
    }
    if complete {
        finish(world, pos, false);
    } else if let Some(BlockEntity::MovingPiston(e)) = world.get_block_entity_mut(pos) {
        e.set_progress(progress);
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
        block.set_properties(std::collections::HashMap::from([("waterlogged", "false")]));
    }
    let carried_entity = world
        .piston_state()
        .motions
        .iter()
        .find(|m| m.pos == pos)
        .and_then(|m| m.carried_entity.clone());
    world.delete_block_entity(pos);
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
}

fn shape_changed(world: &mut impl World, pos: BlockPos) {
    for face in SHAPE_NEIGHBORS {
        let neighbor = pos.offset(face);
        crate::interaction::change(world.get_block(neighbor), world, neighbor, face.opposite());
        // Observers watch shape/state changes, not ordinary neighbor power callbacks.
        let block = world.get_block(neighbor);
        if matches!(block, Block::Observer { .. }) {
            super::update(block, world, neighbor, Some(face.opposite()));
        }
    }
}

pub(crate) fn notify(world: &mut impl World, pos: BlockPos) {
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

fn payload_line(world: &impl World, start: BlockPos, facing: BlockFace) -> Option<Vec<BlockPos>> {
    let mut line = Vec::new();
    let mut pos = start;
    loop {
        if !(0..crate::plot::PLOT_BLOCK_HEIGHT).contains(&pos.y)
            || world
                .get_chunk(pos.x.div_euclid(16), pos.z.div_euclid(16))
                .is_none()
        {
            return None;
        }
        let block = world.get_block(pos);
        if immovable_container(block) {
            return None;
        }
        match block {
            Block::Air => return Some(line),
            // A moving entity belongs to another operation; do not nest its state.
            Block::MovingPiston { .. } | Block::PistonHead { .. } => return None,
            _ => line.push(pos),
        }
        pos = pos.offset(facing);
    }
}
