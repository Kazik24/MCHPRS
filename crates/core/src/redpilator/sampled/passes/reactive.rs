//! Bind initially reactive components and their wire/neighbor dependencies.
use super::*;

pub(in super::super) fn run(
    world: &impl World,
    entry: &ScratchEntry,
    program: &mut Program,
    components: &std::collections::HashMap<BlockPos, usize>,
    monitor: Option<&TaskMonitor>,
) -> Result<(), ScratchError> {
    // Comparator inputs must come from the initial runtime state before marking reactions.
    let inputs = {
        let runtime = program.start();
        (0..program.initial.len())
            .map(|node| {
                if matches!(program.initial[node], Block::RedstoneComparator { .. }) {
                    (runtime.input_strength(node), runtime.side_strength(node))
                } else {
                    (0, 0)
                }
            })
            .collect::<Vec<_>>()
    };

    for (node, &block) in program.initial.iter().enumerate() {
        check_cancelled(monitor)?;
        let pos = program.positions[node];
        let pending = entry.scheduled.iter().any(|tick| tick.pos == pos);
        program.reactive[node] = match block {
            Block::RedstoneRepeater { repeater } => {
                let locked = [repeater.facing.rotate(), repeater.facing.rotate_ccw()].into_iter().any(|side| {
                    let at = pos.offset(side.block_face());
                    let block = world.get_block(at);
                    crate::redstone::is_diode(block)
                        && crate::redstone::power::emits_weak_power(block, world, at, side.block_face(), false)
                        && crate::redstone::source_strength(block, world, at) > 0
                });
                !pending && !locked && repeater.powered != (crate::redstone::diode_get_input_strength(world, pos, repeater.facing) > 0)
            }
            Block::RedstoneComparator { comparator } => {
                let record = program.comparators.iter().find(|record| record.component == node).unwrap();
                let override_level = |input: ComparatorInput| match input {
                    ComparatorInput::Static(level) => level,
                    ComparatorInput::CommandBlock(source) => program
                        .command_blocks
                        .get(&program.positions[source])
                        .map_or(0, |entity| entity.success_count.clamp(0, 15) as u8),
                    ComparatorInput::CopperBulb(source) => {
                        if world.get_block(program.positions[source]).copper_bulb_state().unwrap().0 {
                            15
                        } else {
                            0
                        }
                    }
                };
                let (mut rear, side) = inputs[node];
                if let Some(input) = record.rear_override {
                    rear = override_level(input);
                } else if rear < 15
                    && let Some(input) = record.far_override
                {
                    rear = override_level(input);
                }
                let (level, powered) = crate::redpilator::sampled::executor::compare(rear, side, comparator.mode);
                !pending && (record.stored != level || comparator.powered != powered)
            }
            Block::RedstoneTorch { lit } => !pending && lit == crate::redstone::torch_should_be_off(world, pos),
            Block::RedstoneWallTorch { lit, facing } => !pending && lit == crate::redstone::wall_torch_should_be_off(world, pos, facing),
            Block::RedstoneLamp { lit } => lit != crate::redstone::redstone_lamp_should_be_lit(world, pos) && !(lit && pending),
            Block::Hopper { enabled, .. } => enabled == crate::redstone::redstone_lamp_should_be_lit(world, pos),
            Block::IronTrapdoor { powered, .. } => powered != crate::redstone::redstone_lamp_should_be_lit(world, pos),
            Block::NoteBlock { powered, .. } => powered != crate::redstone::redstone_lamp_should_be_lit(world, pos),
            block if block.is_copper_bulb() => block.copper_bulb_state().unwrap().1 != crate::redstone::redstone_lamp_should_be_lit(world, pos),
            _ => false,
        };
    }

    // Find wires that can affect components needing an initial update.
    for node in 0..program.initial.len() {
        check_cancelled(monitor)?;
        if !program.reactive[node] {
            continue;
        }
        for at in within_two(program.positions[node]) {
            let Some(&wire) = components.get(&at).filter(|_| matches!(world.get_block(at), Block::RedstoneWire { .. })) else {
                continue;
            };
            if let Some(record) = program.reactive_wires.iter_mut().find(|record| record.wire == wire) {
                record.recipients.push(node);
            } else {
                let sources = crate::redpilator::scratch::discover_wire_sources(world, &[at])?
                    .into_iter()
                    .map(|source| {
                        components
                            .get(&source.source)
                            .copied()
                            .map(|node| (node, source.attenuation))
                            .ok_or(ScratchError {
                                position: Some(source.source),
                                reason: "unrepresented reactive wire source",
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                program.reactive_wires.push(crate::redpilator::sampled::program::ReactiveWire {
                    wire,
                    sources,
                    recipients: vec![node],
                });
            }
        }
    }

    // Index those wires by source so later source changes can find them directly.
    for (wire, record) in program.reactive_wires.iter().enumerate() {
        for &(source, _) in &record.sources {
            if !program.reactive_by_source[source].contains(&wire) {
                program.reactive_by_source[source].push(wire);
            }
        }
    }

    // Record components notified by each source's native update footprint.
    for node in 0..program.initial.len() {
        let pos = program.positions[node];
        let adjacent = |p: BlockPos| update_order().map(|face| p.offset(face));
        let footprint = match program.initial[node] {
            Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. } => within_two(pos),
            Block::RedstoneRepeater { repeater } => {
                let front = pos.offset(repeater.facing.opposite().block_face());
                adjacent(front).into_iter().chain([front]).collect()
            }
            Block::RedstoneComparator { comparator } => {
                let front = pos.offset(comparator.facing.opposite().block_face());
                adjacent(front).into_iter().chain([front]).collect()
            }
            Block::Lever { .. } | Block::StoneButton { .. } => update_footprint(pos, program.initial[node]),
            block if block.pressure_plate_powered().is_some() => update_footprint(pos, block),
            _ => Vec::new(),
        };
        for at in footprint {
            if let Some(&target) = components.get(&at) {
                if target != node && program.reactive[target] && !program.reactive_neighbours[node].contains(&target) {
                    program.reactive_neighbours[node].push(target);
                }
            }
        }
    }

    Ok(())
}

/// Collect unique cells reached by one or two neighbor updates, in update order.
fn within_two(pos: BlockPos) -> Vec<BlockPos> {
    let adjacent = |p: BlockPos| update_order().map(|face| p.offset(face));
    let mut cells = Vec::new();

    for cell in adjacent(pos).into_iter().flat_map(adjacent).chain(adjacent(pos)) {
        if !cells.contains(&cell) {
            cells.push(cell);
        }
    }

    cells
}
