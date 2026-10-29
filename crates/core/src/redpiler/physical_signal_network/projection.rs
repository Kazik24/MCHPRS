//! Validated visible state projection for the bounded signal runtime.
use super::{PhysicalSignalNetwork, guard_matches};
use crate::redpiler::components::NodeType;
use crate::redpiler::program_graph::{CircuitNodeKind, ComponentState};
use crate::world::World;
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::{BlockPos, blocks::Block};
use std::collections::HashSet;

/// Original owned cells, captured before an event runtime publishes anything.
pub(crate) struct SelectionSnapshot(Vec<(BlockPos, Block, Option<BlockEntity>)>);

impl SelectionSnapshot {
    pub(crate) fn capture<W: World>(network: &PhysicalSignalNetwork, world: &W, bounds: (BlockPos, BlockPos)) -> Result<Self, &'static str> {
        let cells = network.projection_blocks(bounds)?;
        if cells.iter().any(|(position, _)| world.get_block_entity(*position).is_some()) {
            return Err("event admission: block entities in owned cells are unsupported");
        }
        Ok(Self(
            cells
                .into_iter()
                .map(|(position, _)| (position, world.get_block(position), world.get_block_entity(position).cloned()))
                .collect(),
        ))
    }

    /// Restore only captured cells; scheduler and piston queues remain owned by the world.
    pub(crate) fn restore<W: World>(&self, world: &mut W) -> Result<(), &'static str> {
        for (position, block, entity) in &self.0 {
            if world.get_block(*position) != *block {
                world.set_block(*position, *block);
                if world.get_block(*position) != *block {
                    return Err("world refused restoration block write");
                }
            }
            match entity {
                Some(entity) => world.set_block_entity(*position, entity.clone()),
                None if world.get_block_entity(*position).is_some() => world.delete_block_entity(*position),
                None => {}
            }
            if world.get_block_entity(*position).is_some() != entity.is_some() {
                return Err("world refused restoration block entity write");
            }
        }
        Ok(())
    }
}

impl PhysicalSignalNetwork {
    /// Build the complete write list before touching the world. Contacts own piston and
    /// movable-resource cells; ordinary components own their positions.
    pub(crate) fn project<W: World>(&self, world: &mut W, bounds: (BlockPos, BlockPos)) -> Result<(), &'static str> {
        let mut applied = Vec::new();
        for (position, block) in self.projection_blocks(bounds)? {
            let before = world.get_block(position);
            if before != block {
                applied.push((position, before));
                world.set_block(position, block);
                if world.get_block(position) != block {
                    for (position, previous) in applied.into_iter().rev() {
                        world.set_block(position, previous);
                        if world.get_block(position) != previous {
                            return Err("world refused projection rollback");
                        }
                    }
                    return Err("world refused projection block write");
                }
            }
        }
        Ok(())
    }

    fn projection_blocks(&self, bounds: (BlockPos, BlockPos)) -> Result<Vec<(BlockPos, Block)>, &'static str> {
        let contains =
            |p: BlockPos| p.x >= bounds.0.x && p.x <= bounds.1.x && p.y >= bounds.0.y && p.y <= bounds.1.y && p.z >= bounds.0.z && p.z <= bounds.1.z;
        let mut owned = HashSet::new();
        let mut writes = Vec::new();
        for (index, node) in self.program.nodes().iter().enumerate() {
            let Some(position) = node.position else { continue };
            let block = match (&node.kind, &self.state.components[index]) {
                (CircuitNodeKind::Wire(_), ComponentState::Wire(wire)) => Block::RedstoneWire { wire: *wire },
                (CircuitNodeKind::Electrical(NodeType::Lever), ComponentState::Electrical(state)) => {
                    let Some(Block::Lever { mut lever }) = node.block else {
                        return Err("lever projection block is unavailable");
                    };
                    lever.powered = state.powered;
                    Block::Lever { lever }
                }
                (CircuitNodeKind::Electrical(NodeType::Lamp), ComponentState::Electrical(state)) => Block::RedstoneLamp { lit: state.powered },
                (CircuitNodeKind::Electrical(NodeType::Repeater { .. }), ComponentState::Electrical(state)) => {
                    let Some(Block::RedstoneRepeater { mut repeater }) = node.block else {
                        return Err("repeater projection block is unavailable");
                    };
                    repeater.powered = state.powered;
                    repeater.locked = state.repeater_locked;
                    Block::RedstoneRepeater { repeater }
                }
                (CircuitNodeKind::Piston(_) | CircuitNodeKind::Contact(_) | CircuitNodeKind::Electrical(NodeType::Constant), _) => continue,
                _ => return Err("unsupported component projection"),
            };
            if !contains(position) || !owned.insert(position) {
                return Err("projection component is outside or overlaps the selected region");
            }
            writes.push((position, block));
        }
        for contact in self.program.contacts() {
            if owned.contains(&contact.position) {
                continue;
            }
            if !contains(contact.position) {
                return Err("projection contact is outside the selected region");
            }
            let mut selected = None;
            for variant in &contact.variants {
                if guard_matches(variant.when, &self.program, &self.state, 0)? {
                    selected = Some(self.program.profile_block(variant.profile).ok_or("projection profile is unavailable")?);
                    break;
                }
            }
            let block = match selected.ok_or("projection contact has no active profile")? {
                Block::MovingPiston { .. } => Block::Air,
                block => block,
            };
            if !owned.insert(contact.position) {
                return Err("projection contact overlaps another contact");
            }
            writes.push((contact.position, block));
        }
        Ok(writes)
    }
}
