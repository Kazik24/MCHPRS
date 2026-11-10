//! Piston records, captured payloads and groups with overlapping contact cells.
//! An actor is a piston base; a token is a block its group can move.
use super::{ScratchEntry, ScratchError, ScratchWorld};
use crate::interaction::connections::{SIDES, has_connections};
use crate::{redpilator::TaskMonitor, world::World};
use mchprs_blocks::{BlockFacing, BlockPos, blocks::Block};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PistonActor {
    /// stable actor index assigned during piston discovery.
    pub(super) id: usize,
    /// world position of the piston base component.
    pub(super) base: BlockPos,
    /// direction in which the piston extends.
    pub(super) facing: BlockFacing,
    /// whether this is a sticky piston.
    pub(super) sticky: bool,
    /// adjacent home cell immediately in front of the base.
    pub(super) home: BlockPos,
    /// far cell at the end of the piston head's travel.
    pub(super) far: BlockPos,
    /// captured initial extended state.
    pub(super) extended: bool,
    /// whether a piston head is initially present.
    pub(super) head_present: bool,
    /// connected piston-group index for overlapping motion/contact groups.
    pub(super) group: usize,
}

#[derive(Clone, Debug)]
pub(super) struct PistonToken {
    /// value carried by this movable token.
    pub(super) block: Block,
    /// initial world position, or `None` for an initially empty piston token.
    pub(super) position: Option<BlockPos>,
    /// legal cells this token may occupy as its piston group moves.
    pub(super) cells: Vec<BlockPos>,
}

pub(super) fn discover_piston_actors(
    world: &impl World,
    bounds: (BlockPos, BlockPos),
    entry: ScratchEntry,
) -> Result<Vec<PistonActor>, ScratchError> {
    let scratch = ScratchWorld::from_world_monitored(world, bounds, entry, None)?;
    discover_piston_actors_monitored(&scratch, bounds, None)
}

/// Capture stationary piston actors from an existing scratch snapshot.
///
/// Inputs: The scratch world, inclusive capture bounds and optional cancellation monitor.
///
/// Scan piston bases, read their contact cells, reject moving/mismatched heads or block entities, check
/// scratch boundaries and group overlapping contacts.
pub(super) fn discover_piston_actors_monitored(
    scratch: &ScratchWorld,
    bounds: (BlockPos, BlockPos),
    monitor: Option<&TaskMonitor>,
) -> Result<Vec<PistonActor>, ScratchError> {
    let world = scratch.world();
    let error = |pos, reason| ScratchError { position: Some(pos), reason };
    let mut actors = Vec::new();
    for y in bounds.0.y..=bounds.1.y {
        if monitor.is_some_and(TaskMonitor::cancelled) {
            return Err(ScratchError {
                position: None,
                reason: "compilation cancelled",
            });
        }
        for z in bounds.0.z..=bounds.1.z {
            for x in bounds.0.x..=bounds.1.x {
                let base = BlockPos::new(x, y, z);
                let block = world.get_block(base);
                let Block::Piston { piston } = block else {
                    if matches!(block, Block::MovingPiston { .. }) {
                        return Err(error(base, "moving piston state is unsupported"));
                    }
                    continue;
                };
                if world.get_block_entity(base).is_some() {
                    return Err(error(base, "unsupported block entity in piston capture"));
                }
                let facing = piston.facing;
                let home = base.offset(facing.into());
                let far = home.offset(facing.into());
                let home_block = world.get_block(home);
                let _ = world.get_block(far);
                if world.get_block_entity(home).is_some() {
                    return Err(error(home, "unsupported block entity at piston contact"));
                }
                if world.get_block_entity(far).is_some() {
                    return Err(error(far, "unsupported block entity at piston contact"));
                }
                let head_present = match home_block {
                    Block::PistonHead { head } if head.facing == facing && head.sticky == piston.sticky && !head.short => true,
                    Block::PistonHead { .. } => return Err(error(home, "piston head does not match base state")),
                    Block::MovingPiston { .. } => return Err(error(home, "moving piston state is unsupported")),
                    _ => false,
                };
                if !piston.extended && head_present {
                    return Err(error(base, "piston base and head state disagree"));
                }
                actors.push(PistonActor {
                    id: actors.len(),
                    base,
                    facing,
                    sticky: piston.sticky,
                    home,
                    far,
                    extended: piston.extended,
                    head_present,
                    group: 0,
                });
            }
        }
    }
    scratch.finish_probe()?;
    assign_piston_groups(&mut actors);
    Ok(actors)
}

/// Capture movable blocks and their legal cells, rejecting groups the runtime cannot model.
///
/// Inputs: The captured world and discovered piston actors with contact-group IDs.
///
/// Collect each group's legal cells, distinguish fixed scenery from payloads, reject unsupported entities
/// or movement domains, and enforce the cell/token limits.
pub(super) fn discover_piston_tokens(world: &impl World, actors: &[PistonActor]) -> Result<Vec<PistonToken>, ScratchError> {
    let mut tokens = Vec::new();
    for group in 0..actors.iter().map(|p| p.group + 1).max().unwrap_or(0) {
        let members: Vec<_> = actors.iter().filter(|p| p.group == group).collect();
        let mut cells = Vec::new();
        for piston in &members {
            for cell in [piston.home, piston.far] {
                if !cells.contains(&cell) {
                    cells.push(cell);
                }
            }
        }
        let at = members[0].base;
        for pos in members.iter().map(|piston| piston.base).chain(cells.iter().copied()) {
            for target in std::iter::once(pos).chain(SIDES.map(|(face, _)| pos.offset(face))) {
                if has_connections(world.get_block(target)) {
                    return Err(ScratchError {
                        position: Some(target),
                        reason: "piston movement can change a pane, iron bar or fence connection",
                    });
                }
            }
        }
        if cells.len() > 6 {
            return Err(ScratchError {
                position: Some(at),
                reason: "piston group exceeds six contact cells",
            });
        }
        let start = tokens.len();
        let mut scenery = Vec::new();
        for &cell in &cells {
            let block = world.get_block(cell);
            if matches!(block, Block::Air | Block::PistonHead { .. }) {
                continue;
            }
            let home = members.iter().any(|p| p.home == cell);
            if !home && !members.iter().any(|p| p.sticky && p.far == cell) {
                scenery.push(cell);
                continue;
            }
            if world.get_block_entity(cell).is_some() {
                return Err(ScratchError {
                    position: Some(cell),
                    reason: "unsupported carried block entity",
                });
            }
            if !matches!(block, Block::RedstoneBlock)
                && block.slab_type().is_none()
                && (block.pressure_plate_powered().is_some() || !is_passive_geometry(block))
            {
                return Err(ScratchError {
                    position: Some(cell),
                    reason: "unsupported movable block",
                });
            }
            if home && members.iter().filter(|p| p.home == cell).all(|p| !p.sticky) && !members.iter().any(|p| p.far == cell) {
                return Err(ScratchError {
                    position: Some(cell),
                    reason: "normal piston would push token out of its domain",
                });
            }
            tokens.push(PistonToken {
                block,
                position: Some(cell),
                cells: cells.clone(),
            });
        }
        for token in &mut tokens[start..] {
            token.cells.retain(|cell| !scenery.contains(cell));
        }
        if tokens.len() - start > 2 {
            return Err(ScratchError {
                position: Some(at),
                reason: "piston group exceeds two tokens",
            });
        }
        if tokens.len() - start == 2
            && (members.len() != 2
                || !members.iter().all(|p| p.sticky)
                || members[0].far != members[1].far
                || members[0].facing != members[1].facing.opposite())
        {
            return Err(ScratchError {
                position: Some(at),
                reason: "unsupported two-token piston group",
            });
        }
    }
    Ok(tokens)
}

pub(super) fn is_passive_geometry(block: Block) -> bool {
    let plate_power = block.pressure_plate_powered();
    !crate::redstone::has_neighbor_update(block)
        && !matches!(block, Block::Composter { .. } | Block::Lever { .. })
        && plate_power != Some(true)
        && (block.is_solid()
            || (block.is_cube() && block.is_transparent())
            || matches!(block, Block::TripwireHook { .. })
            || plate_power == Some(false))
}

/// Group pistons whose contact cells overlap, including indirect overlaps.
///
/// Inputs: A mutable actor list containing each piston's home and far contact cells.
///
/// Use a union-find parent array to merge actors sharing a contact cell, compress parent paths, then
/// assign compact group IDs in actor order.
pub(super) fn assign_piston_groups(actors: &mut [PistonActor]) {
    let mut parents: Vec<_> = (0..actors.len()).collect();
    let mut owner_by_cell = std::collections::HashMap::new();
    for (index, actor) in actors.iter().enumerate() {
        for cell in [actor.home, actor.far] {
            if let Some(&owner) = owner_by_cell.get(&cell) {
                let left = piston_group_root(&mut parents, index);
                let right = piston_group_root(&mut parents, owner);
                parents[left] = right;
            } else {
                owner_by_cell.insert(cell, index);
            }
        }
    }
    let mut root_groups = std::collections::HashMap::new();
    for (index, actor) in actors.iter_mut().enumerate() {
        let root = piston_group_root(&mut parents, index);
        let next = root_groups.len();
        actor.group = *root_groups.entry(root).or_insert(next);
    }
}

fn piston_group_root(parents: &mut [usize], mut index: usize) -> usize {
    while parents[index] != index {
        parents[index] = parents[parents[index]];
        index = parents[index];
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plot::PlotWorld;
    use crate::world::storage::Chunk;
    use mchprs_blocks::BlockFace;

    #[test]
    fn piston_compilation_rejects_moving_geometry_next_to_a_fence() {
        let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
        let base = BlockPos::new(8, 20, 8);
        let home = base.offset(BlockFace::East);
        let far = home.offset(BlockFace::East);
        let actor = PistonActor {
            id: 0,
            base,
            facing: BlockFacing::East,
            sticky: true,
            home,
            far,
            extended: false,
            head_present: false,
            group: 0,
        };
        world.set_block(home, Block::RedstoneBlock);
        assert!(discover_piston_tokens(&world, &[actor]).is_ok());

        let fence = far.offset(BlockFace::North);
        world.set_block(fence, Block::from_name("oak_fence").unwrap());
        let error = discover_piston_tokens(&world, &[actor]).unwrap_err();
        assert_eq!(error.position, Some(fence));
        assert!(error.reason.contains("fence connection"));
    }
}
