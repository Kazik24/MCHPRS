//! Extract electrical links, component ports, watched wires and fixed geometry.
use super::{CompilePhases, Context, Inventory, check_cancelled};
use crate::redpilator::sampled::{
    ElectricalLink, Program, ScratchError, ScratchWorld,
    group_probe::ProbeIndexes,
    program::{ArrangementSource, ComparatorInput, WatchedWire},
};
use crate::world::World;
use mchprs_blocks::{BlockPos, block_entities::BlockEntity, blocks::Block};
use std::collections::HashMap;

/// Build the initial program and its electrical dependency records.
///
/// Inputs: The captured context, isolated scratch world, parsed inventory, position index and phase reporter.
///
/// Discover source-to-reader power paths, translate positions to component IDs, record watchers and diode
/// overrides, and capture fixed geometry and piston scenery.
pub(in super::super) fn run<'a>(
    context: &Context<'_, impl World>,
    scratch: ScratchWorld,
    inventory: Inventory,
    position_index: &'a HashMap<BlockPos, usize>,
    phases: &mut CompilePhases<'_>,
) -> Result<(Program, ProbeIndexes<'a>), ScratchError> {
    let world = context.world;
    let bounds = context.bounds;
    let monitor = context.monitor;
    let Inventory {
        positions,
        sources,
        mut repeaters,
        copper_bulbs,
        wire_positions,
        lamp_positions,
        hopper_positions,
        note_positions,
        pistons,
        tokens,
    } = inventory;
    let error = |pos, reason| ScratchError { position: Some(pos), reason };

    phases.next("electrical dependency discovery");
    let discovery = (|| {
        let mut dependencies = if let Some(monitor) = monitor {
            crate::redpilator::scratch::discover_wire_sources_monitored(scratch.world(), &wire_positions, monitor)?
        } else {
            crate::redpilator::scratch::discover_wire_sources(scratch.world(), &wire_positions)?
        };
        let command_positions = positions.iter().copied().filter(|&pos| world.get_block(pos).is_command_block());
        let powered_receivers = lamp_positions
            .into_iter()
            .chain(hopper_positions)
            .chain(note_positions)
            .chain(copper_bulbs)
            .chain(command_positions);
        for pos in powered_receivers {
            check_cancelled(monitor)?;
            dependencies.extend(crate::redpilator::scratch::discover_lamp_sources(scratch.world(), pos)?);
        }
        for &pos in &positions {
            check_cancelled(monitor)?;
            let block = world.get_block(pos);
            if matches!(block, Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. }) {
                dependencies.extend(crate::redpilator::scratch::discover_torch_sources(scratch.world(), pos, block)?);
            }
        }
        for piston in &pistons {
            check_cancelled(monitor)?;
            dependencies.extend(crate::redpilator::scratch::discover_piston_sources(
                scratch.world(),
                piston.base,
                piston.facing,
            )?);
        }
        for record in &mut repeaters {
            check_cancelled(monitor)?;
            let pos = positions[record.component];
            let Block::RedstoneRepeater { repeater } = world.get_block(pos) else {
                unreachable!()
            };
            let face = repeater.facing.block_face();
            let rear = pos.offset(face);
            let block = scratch.world().get_block(rear);
            if matches!(block, Block::RedstoneWire { .. }) {
                for mut source in crate::redpilator::scratch::discover_wire_sources(scratch.world(), &[rear])? {
                    source.wire = pos;
                    dependencies.push(source);
                }
            } else if block.is_solid() {
                for side in mchprs_blocks::BlockFace::values() {
                    let at = rear.offset(side);
                    let source = scratch.world().get_block(at);
                    if crate::redstone::power::emits_strong_power(source, scratch.world(), at, side, true) {
                        if matches!(source, Block::RedstoneWire { .. }) {
                            for mut source in crate::redpilator::scratch::discover_wire_sources(scratch.world(), &[at])? {
                                source.wire = pos;
                                dependencies.push(source);
                            }
                            continue;
                        }
                        dependencies.push(crate::redpilator::scratch::WireSource {
                            wire: pos,
                            source: at,
                            attenuation: 0,
                        });
                    }
                }
            } else if crate::redstone::power::emits_weak_power(block, scratch.world(), rear, face, true) {
                dependencies.push(crate::redpilator::scratch::WireSource {
                    wire: pos,
                    source: rear,
                    attenuation: 0,
                });
            }
            for side in [repeater.facing.rotate(), repeater.facing.rotate_ccw()] {
                let at = pos.offset(side.block_face());
                let block = world.get_block(at);
                if crate::redstone::is_diode(block)
                    && crate::redstone::power::emits_strong_power(block, scratch.world(), at, side.block_face(), false)
                {
                    let source = position_index
                        .get(&at)
                        .copied()
                        .ok_or_else(|| error(at, "unrepresented repeater side source"))?;
                    if !matches!(block, Block::RedstoneRepeater { .. } | Block::RedstoneComparator { .. }) {
                        return Err(error(at, "unsupported repeater side source"));
                    }
                    record.side_sources.as_mut().unwrap().push(source);
                }
            }
        }
        Ok::<_, ScratchError>(dependencies)
    })();
    scratch.finish_probe()?;

    let discovery = discovery?;
    phases.next("link and watcher construction");
    let initial: Vec<_> = positions.iter().map(|&p| world.get_block(p)).collect();
    let probe_indexes = ProbeIndexes::new(position_index, &positions, &initial, &tokens, monitor)?;

    let mut links = vec![];
    for dependency in discovery {
        let source = position_index
            .get(&dependency.source)
            .copied()
            .ok_or_else(|| error(dependency.source, "unrepresented discovered wire source"))?;
        let target = position_index
            .get(&dependency.wire)
            .copied()
            .ok_or_else(|| error(dependency.wire, "unrepresented discovered wire"))?;
        links.push(ElectricalLink {
            source,
            target,
            attenuation: dependency.attenuation,
        });
    }

    let mut watched_wires: Vec<WatchedWire> = Vec::new();
    let piston_positions: Vec<_> = pistons.iter().map(|piston| piston.base).collect();
    for watch in crate::redpilator::scratch::discover_wire_watchers(world, &wire_positions, &piston_positions) {
        let wire = position_index[&watch.wire];
        let piston = position_index[&watch.piston]
            | if watch.head {
                crate::redpilator::sampled::program::PISTON_HEAD_NOTIFICATION
            } else {
                0
            };
        if let Some(record) = watched_wires.iter_mut().find(|record| record.wire == wire) {
            record.watchers.push(piston);
        } else {
            watched_wires.push(WatchedWire {
                wire,
                watchers: vec![piston],
            });
        }
    }

    for &pos in &wire_positions {
        let observers: Vec<_> = crate::world::wire_cache::positions(pos)
            .into_iter()
            .filter_map(|at| {
                position_index
                    .get(&at)
                    .copied()
                    .filter(|_| matches!(world.get_block(at), Block::Observer { .. }))
            })
            .collect();
        if observers.is_empty() {
            continue;
        }
        let wire = position_index[&pos];
        if let Some(record) = watched_wires.iter_mut().find(|record| record.wire == wire) {
            record.watchers.extend(observers);
        } else {
            watched_wires.push(WatchedWire { wire, watchers: observers });
        }
    }
    scratch.finish_probe()?;

    let output_programs = vec![[vec![], vec![]]; positions.len()];
    let mut program =
        Program::new(positions, initial, sources, output_programs, links, watched_wires, repeaters).map_err(|err| error(bounds.0, err.reason))?;
    program.pistons = pistons;
    program.tokens = tokens;

    for (&pos, block) in program.positions.iter().zip(&program.initial) {
        if block.is_command_block() {
            if let Some(BlockEntity::CommandBlock(entity)) = world.get_block_entity(pos) {
                program.command_blocks.insert(pos, entity.clone());
            }
            let face = block.facing().unwrap_or(mchprs_blocks::BlockFace::North);
            let next = pos.offset(face);
            let previous = pos.offset(face.opposite());
            if world.get_block(next).get_name() == "chain_command_block" && !position_index.contains_key(&next) {
                return Err(error(next, "command block chain leaves capture bounds"));
            }
            if block.conditional() == Some(true) && world.get_block(previous).is_command_block() && !position_index.contains_key(&previous) {
                return Err(error(previous, "command block condition leaves capture bounds"));
            }
        }
    }

    if program.initial.iter().any(|block| block.is_command_block()) {
        program.command_output_state = world.command_block_output_state();
    }

    for component in 0..program.initial.len() {
        let Block::RedstoneComparator { comparator } = program.initial[component] else {
            continue;
        };
        let pos = program.positions[component];
        let rear = pos.offset(comparator.facing.block_face());
        let override_at = |at| -> Result<Option<ComparatorInput>, ScratchError> {
            let block = world.get_block(at);
            if !crate::redstone::comparator::has_override(block) {
                return Ok(None);
            }
            if block.is_command_block() {
                let source = position_index
                    .get(&at)
                    .copied()
                    .ok_or_else(|| error(at, "unrepresented command block comparator input"))?;
                Ok(Some(ComparatorInput::CommandBlock(source)))
            } else if block.is_copper_bulb() {
                let source = position_index
                    .get(&at)
                    .copied()
                    .ok_or_else(|| error(at, "unrepresented copper bulb comparator input"))?;
                Ok(Some(ComparatorInput::CopperBulb(source)))
            } else {
                Ok(Some(ComparatorInput::Static(crate::redstone::comparator::get_override(block, world, at))))
            }
        };
        let front = world.get_block(pos.offset(comparator.facing.opposite().block_face()));
        let front_facing = match front {
            Block::RedstoneRepeater { repeater } => Some(repeater.facing),
            Block::RedstoneComparator { comparator } => Some(comparator.facing),
            _ => None,
        };
        program.comparators.push(crate::redpilator::sampled::program::ComparatorRecord {
            component,
            stored: match world.get_block_entity(pos) {
                Some(BlockEntity::Comparator { output_strength }) => *output_strength,
                _ => 0,
            },
            rear_override: override_at(rear)?,
            far_override: if world.get_block(rear).is_solid() {
                override_at(rear.offset(comparator.facing.block_face()))?
            } else {
                None
            },
            output_is_diode: front_facing.is_some_and(|facing| facing != comparator.facing.opposite()),
        });
        for ((source, side), (attenuation, _)) in
            crate::redpilator::sampled::group_probe::inputs_of(world, &program, component, monitor, &probe_indexes)?
        {
            let ArrangementSource::Component(source) = source else {
                continue;
            };
            let link = ElectricalLink {
                source,
                target: component,
                attenuation,
            };
            if side {
                program.side_links.push(link);
            } else if program.comparators.last().unwrap().rear_override.is_none() {
                program.push_link(link);
            }
        }
    }

    for y in bounds.0.y..=bounds.1.y {
        for z in bounds.0.z..=bounds.1.z {
            for x in bounds.0.x..=bounds.1.x {
                let pos = BlockPos::new(x, y, z);
                let block = world.get_block(pos);
                if !matches!(block, Block::Air | Block::PistonHead { .. }) && !probe_indexes.components.contains_key(&pos) {
                    program.geometry.push((pos, block));
                }
            }
        }
    }

    for piston in &program.pistons {
        for cell in [piston.home, piston.far] {
            if !matches!(world.get_block(cell), Block::Air | Block::PistonHead { .. })
                && !program.tokens.iter().any(|token| token.position == Some(cell))
                && !program.piston_scenery.contains(&cell)
            {
                program.piston_scenery.push(cell);
            }
        }
    }
    Ok((program, probe_indexes))
}
