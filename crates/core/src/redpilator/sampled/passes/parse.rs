//! Capture and validate the world inventory before electrical discovery.
use super::{CompilePhases, Context, Inventory, check_cancelled};
use crate::redpilator::sampled::{
    ScratchError, ScratchWorld,
    pistons::{discover_piston_actors_monitored, discover_piston_tokens},
    program::RepeaterRecord,
};
use crate::world::World;
use mchprs_blocks::{
    BlockPos,
    block_entities::{BlockEntity, ContainerType},
    blocks::Block,
};

/// Capture and validate the circuit inventory before dependency discovery.
///
/// Inputs: The immutable capture context and phase reporter.
///
/// Create one scratch snapshot, discover piston actors and payloads from it, scan supported
/// components/entities and group receiver positions while reporting progress.
pub(in super::super) fn run(context: &Context<'_, impl World>, phases: &mut CompilePhases<'_>) -> Result<(ScratchWorld, Inventory), ScratchError> {
    let world = context.world;
    let bounds = context.bounds;
    let entry = context.entry;
    let monitor = context.monitor;
    check_cancelled(monitor)?;

    let scratch = ScratchWorld::from_world_monitored(world, bounds, entry.clone(), monitor)?;

    phases.next("piston actor and token discovery");
    let pistons = discover_piston_actors_monitored(&scratch, bounds, monitor)?;
    let tokens = discover_piston_tokens(scratch.world(), &pistons)?;

    if phases.enabled {
        let piston_groups = pistons.iter().map(|piston| piston.group).max().map_or(0, |group| group + 1);
        let token_cells: usize = tokens.iter().map(|token| token.cells.len()).sum();
        eprintln!(
            "Redpilator inventory: piston_actors={} piston_groups={} tokens={} token_cells={token_cells}",
            pistons.len(),
            piston_groups,
            tokens.len()
        );
    }

    phases.next("component scan");
    let mut positions = vec![];
    let mut sources = Vec::new();
    let mut repeaters = Vec::new();
    let mut copper_bulbs = Vec::new();

    let error = |pos, reason| ScratchError { position: Some(pos), reason };
    let width = (i64::from(bounds.1.x) - i64::from(bounds.0.x) + 1).max(0) as usize;
    let depth = (i64::from(bounds.1.z) - i64::from(bounds.0.z) + 1).max(0) as usize;
    let height = (i64::from(bounds.1.y) - i64::from(bounds.0.y) + 1).max(0) as usize;
    let layer_size = width.saturating_mul(depth);
    let total_blocks = layer_size.saturating_mul(height);
    for y in bounds.0.y..=bounds.1.y {
        check_cancelled(monitor)?;
        for z in bounds.0.z..=bounds.1.z {
            for x in bounds.0.x..=bounds.1.x {
                let pos = BlockPos::new(x, y, z);
                match world.get_block(pos) {
                    Block::Air | Block::Stone {} | Block::Wool { .. } | Block::Glowstone | Block::Target => {}
                    block if block.slab_type().is_some() => {}
                    block if block.is_sign() => {}
                    Block::Lever { .. } | Block::StoneButton { .. } => {
                        sources.push(positions.len());
                        positions.push(pos);
                    }
                    block if block.pressure_plate_powered().is_some() => {
                        sources.push(positions.len());
                        positions.push(pos);
                    }
                    block if block.is_command_block() => positions.push(pos),
                    Block::RedstoneWire { .. }
                    | Block::RedstoneLamp { .. }
                    | Block::Hopper { .. }
                    | Block::IronTrapdoor { .. }
                    | Block::NoteBlock { .. }
                    | Block::RedstoneBlock
                    | Block::RedstoneComparator { .. } => positions.push(pos),
                    block if block.is_copper_bulb() => {
                        copper_bulbs.push(pos);
                        positions.push(pos);
                    }
                    Block::Piston { .. } => positions.push(pos),
                    Block::PistonHead { .. } => {}
                    Block::Observer { .. } | Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. } => positions.push(pos),
                    Block::RedstoneRepeater { repeater } => {
                        repeaters.push(RepeaterRecord {
                            component: positions.len(),
                            output_is_diode: crate::redstone::is_diode(world.get_block(pos.offset(repeater.facing.opposite().block_face()))),
                            side_sources: Some(Vec::new()),
                        });
                        positions.push(pos);
                    }
                    block if !crate::redstone::has_neighbor_update(block) => {}
                    block => return Err(error(pos, block.get_name())),
                }

                if let Some(entity) = world.get_block_entity(pos) {
                    let block = world.get_block(pos);
                    let reason = match (block, entity) {
                        (Block::RedstoneComparator { .. }, BlockEntity::Comparator { output_strength: 0..=15 }) => None,
                        (Block::RedstoneComparator { .. }, BlockEntity::Comparator { .. }) => Some("comparator output strength exceeds 15"),
                        (block, BlockEntity::Container { comparator_override, .. }) if ContainerType::from_block(block).is_some() => {
                            (*comparator_override > 15).then_some("container comparator output exceeds 15")
                        }
                        (Block::MovingPiston { .. }, _) => Some("moving piston block entity is unsupported"),
                        // The interpreter ignores stale entities attached to other block types.
                        _ => None,
                    };
                    if let Some(reason) = reason {
                        return Err(error(pos, reason));
                    }
                }
            }
        }
        if let Some(monitor) = monitor {
            monitor.set_progress(
                layer_size.saturating_mul((i64::from(y) - i64::from(bounds.0.y) + 1) as usize),
                total_blocks,
                "blocks scanned",
            );
        }
    }

    // Existing saves need not satisfy placement rules; native redstone callbacks retain them too.
    let mut wire_positions = Vec::new();
    let mut lamp_positions = Vec::new();
    let mut hopper_positions = Vec::new();
    let mut note_positions = Vec::new();
    for &pos in &positions {
        match scratch.world().get_block(pos) {
            Block::RedstoneWire { .. } => wire_positions.push(pos),
            Block::RedstoneLamp { .. } => lamp_positions.push(pos),
            Block::Hopper { .. } | Block::IronTrapdoor { .. } => hopper_positions.push(pos),
            Block::NoteBlock { .. } => note_positions.push(pos),
            _ => {}
        }
    }
    if let Some(monitor) = monitor {
        monitor.set_counts(positions.len(), wire_positions.len(), pistons.len());
    }
    if phases.enabled {
        eprintln!(
            "Redpilator inventory: sampled_components={} wires={} lamps={} repeaters={} copper_bulbs={}",
            positions.len(),
            wire_positions.len(),
            lamp_positions.len(),
            repeaters.len(),
            copper_bulbs.len()
        );
    }
    Ok((
        scratch,
        Inventory {
            positions,
            sources,
            repeaters,
            copper_bulbs,
            wire_positions,
            lamp_positions,
            hopper_positions,
            note_positions,
            pistons,
            tokens,
        },
    ))
}
