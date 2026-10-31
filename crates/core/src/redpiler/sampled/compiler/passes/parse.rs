//! Capture and validate the world inventory before electrical discovery.
use super::*;

pub(in super::super) fn run(context: &Context<'_, impl World>, phases: &mut CompilePhases) -> Result<(ScratchWorld, Inventory), ScratchError> {
    let world = context.world;
    let bounds = context.bounds;
    let entry = context.entry;
    let monitor = context.monitor;
    check_cancelled(monitor)?;
    let scratch = ScratchWorld::from_world_monitored(world, bounds, entry.clone(), monitor)?;
    phases.next("piston actor and token discovery");
    let pistons = discover_piston_actors_monitored(world, bounds, entry.clone(), monitor)?;
    let tokens = discover_piston_tokens(scratch.world(), &pistons)?;
    if phases.enabled {
        let piston_groups = pistons.iter().map(|piston| piston.group).max().map_or(0, |group| group + 1);
        let token_cells: usize = tokens.iter().map(|token| token.cells.len()).sum();
        eprintln!(
            "Redpiler inventory: piston_actors={} piston_groups={} tokens={} token_cells={token_cells}",
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
    for y in bounds.0.y..=bounds.1.y {
        if monitor.is_some_and(TaskMonitor::cancelled) {
            return Err(ScratchError {
                position: None,
                reason: "compilation cancelled",
            });
        }
        for z in bounds.0.z..=bounds.1.z {
            for x in bounds.0.x..=bounds.1.x {
                let pos = BlockPos::new(x, y, z);
                match world.get_block(pos) {
                    Block::Air | Block::Stone {} | Block::Wool { .. } | Block::Glowstone | Block::Target => {}
                    block if block.slab_type().is_some() => {}
                    block if block.is_sign() => {}
                    Block::Lever { lever } if matches!(lever.face, LeverFace::Floor | LeverFace::Wall) => {
                        sources.push(positions.len());
                        positions.push(pos);
                    }
                    Block::StoneButton { .. } => {
                        sources.push(positions.len());
                        positions.push(pos);
                    }
                    block if block.is_command_block() => positions.push(pos),
                    Block::RedstoneWire { .. } => {
                        positions.push(pos);
                    }
                    Block::RedstoneLamp { .. } => {
                        positions.push(pos);
                    }
                    Block::Hopper { .. } => positions.push(pos),
                    Block::NoteBlock { .. } => positions.push(pos),
                    Block::RedstoneBlock => positions.push(pos),
                    Block::RedstoneComparator { .. } => positions.push(pos),
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
                    block if is_passive_geometry(block) => {}
                    _ => return Err(error(pos, "unsupported component")),
                }
                if world.get_block(pos).is_command_block() && world.get_block_entity(pos).is_none() {
                    return Err(error(pos, "command block execution is unsupported"));
                }
                if let Some(entity) = world.get_block_entity(pos) {
                    let block = world.get_block(pos);
                    let supported = match (block, entity) {
                        (Block::RedstoneComparator { .. }, BlockEntity::Comparator { output_strength: 0..=15 }) => true,
                        (block, BlockEntity::Container { comparator_override, ty, .. }) => {
                            *comparator_override <= 15 && ContainerType::from_block(block) == Some(*ty)
                        }
                        (block, BlockEntity::CommandBlock(command)) if block.is_command_block() => {
                            if command.automatic || command.powered || entry.scheduled.iter().any(|tick| tick.pos == pos) {
                                return Err(error(pos, "command block execution is unsupported"));
                            }
                            true
                        }
                        (block, BlockEntity::Sign(_)) => block.is_sign(),
                        _ => false,
                    };
                    if !supported {
                        return Err(error(pos, "unsupported block entity"));
                    }
                }
            }
        }
    }
    {
        check_cancelled(monitor)?;
        for pos in positions.iter().copied().filter(|&pos| {
            matches!(
                world.get_block(pos),
                Block::Lever { .. }
                    | Block::StoneButton { .. }
                    | Block::RedstoneWire { .. }
                    | Block::RedstoneTorch { .. }
                    | Block::RedstoneWallTorch { .. }
            )
        }) {
            if !crate::interaction::is_valid_position(world.get_block(pos), scratch.world(), pos) {
                return Err(error(pos, "invalid attachment support"));
            }
        }
    }
    let wire_positions: Vec<_> = positions
        .iter()
        .copied()
        .filter(|&pos| matches!(scratch.world().get_block(pos), Block::RedstoneWire { .. }))
        .collect();
    let lamp_positions: Vec<_> = positions
        .iter()
        .copied()
        .filter(|&pos| matches!(scratch.world().get_block(pos), Block::RedstoneLamp { .. }))
        .collect();
    let hopper_positions: Vec<_> = positions
        .iter()
        .copied()
        .filter(|&pos| matches!(scratch.world().get_block(pos), Block::Hopper { .. }))
        .collect();
    let note_positions: Vec<_> = positions
        .iter()
        .copied()
        .filter(|&pos| matches!(scratch.world().get_block(pos), Block::NoteBlock { .. }))
        .collect();
    if phases.enabled {
        eprintln!(
            "Redpiler inventory: sampled_components={} wires={} lamps={} repeaters={} copper_bulbs={}",
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
