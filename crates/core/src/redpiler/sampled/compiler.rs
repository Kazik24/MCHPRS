use super::{ElectricalLink, Operation, Program, ScratchEntry, ScratchError, ScratchWorld};
use super::program::RepeaterRecord;
use crate::world::World;
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::{BlockPos, BlockFacing, blocks::{Block, LeverFace}};
#[derive(Clone, PartialEq, Eq)]
struct Frame {
    blocks: Vec<Block>,
    pending: Vec<mchprs_world::TickEntry>,
    deliveries: Vec<crate::world::RedstoneUpdateObservation>,
}#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PistonActor {
    pub(super) id: usize,
    pub(super) base: BlockPos,
    pub(super) facing: BlockFacing,
    pub(super) sticky: bool,
    pub(super) home: BlockPos,
    pub(super) far: BlockPos,
    pub(super) extended: bool,
    pub(super) head_present: bool,
    pub(super) group: usize,
}

#[derive(Clone, Debug)]
pub(super) struct PistonToken {
    pub(super) block: Block,
    pub(super) position: Option<BlockPos>,
    pub(super) cells: Vec<BlockPos>,
}

pub(super) fn discover_piston_tokens(world: &impl World, actors: &[PistonActor]) -> Result<Vec<PistonToken>, ScratchError> {
    let mut tokens = Vec::new();
    for group in 0..actors.iter().map(|p| p.group + 1).max().unwrap_or(0) {
        let members: Vec<_> = actors.iter().filter(|p| p.group == group).collect();
        let mut cells = Vec::new();
        for piston in &members {
            for cell in [piston.home, piston.far] {
                if !cells.contains(&cell) { cells.push(cell); }
            }
        }
        let at = members[0].base;
        if cells.len() > 6 {
            return Err(ScratchError { position: Some(at), reason: "piston group exceeds six contact cells" });
        }
        let start = tokens.len();
        for &cell in &cells {
            let block = world.get_block(cell);
            if matches!(block, Block::Air | Block::PistonHead { .. }) { continue; }
            let home = members.iter().any(|p| p.home == cell);
            if !home && !members.iter().any(|p| p.sticky && p.far == cell) { continue; }
            if world.get_block_entity(cell).is_some() {
                return Err(ScratchError { position: Some(cell), reason: "unsupported carried block entity" });
            }
            if !matches!(block, Block::Stone {} | Block::Wool { .. } | Block::Glowstone | Block::RedstoneBlock) && block.slab_type().is_none() {
                return Err(ScratchError { position: Some(cell), reason: "unsupported movable block" });
            }
            if home && members.iter().filter(|p| p.home == cell).all(|p| !p.sticky) && !members.iter().any(|p| p.far == cell) {
                return Err(ScratchError { position: Some(cell), reason: "normal piston would push token out of its domain" });
            }
            tokens.push(PistonToken { block, position: Some(cell), cells: cells.clone() });
        }
        if tokens.len() - start > 2 {
            return Err(ScratchError { position: Some(at), reason: "piston group exceeds two tokens" });
        }
        if tokens.len() - start == 2 && (members.len() != 2 || !members.iter().all(|p| p.sticky) || members[0].far != members[1].far || members[0].facing != members[1].facing.opposite()) {
            return Err(ScratchError { position: Some(at), reason: "unsupported two-token piston group" });
        }
    }
    Ok(tokens)
}
pub(super) fn compile(world: &impl World, bounds: (BlockPos, BlockPos), entry: ScratchEntry) -> Result<Program, ScratchError> {
    let scratch = ScratchWorld::from_world(world, bounds, entry.clone())?;
    let mut positions = vec![];
    let mut sources = Vec::new();
    let mut repeaters = Vec::new();
    let error = |pos, reason| ScratchError { position: Some(pos), reason };
    for y in bounds.0.y..=bounds.1.y {
        for z in bounds.0.z..=bounds.1.z {
            for x in bounds.0.x..=bounds.1.x {
                let pos = BlockPos::new(x, y, z);
                match world.get_block(pos) {
                    Block::Air | Block::Stone {} | Block::Wool { .. } | Block::Glowstone => {}
                    block if block.slab_type().is_some() => {}
                    block if block.is_sign() => {}
                    Block::Lever { lever } if matches!(lever.face, LeverFace::Floor | LeverFace::Wall) => {
                        sources.push(positions.len());
                        positions.push(pos);
                    }
                    Block::RedstoneWire { .. } => {
                        positions.push(pos);
                    }
                    Block::RedstoneLamp { .. } => {
                        positions.push(pos);
                    }
                    Block::RedstoneBlock => positions.push(pos),
                    Block::RedstoneRepeater { repeater } => {
                        repeaters.push(RepeaterRecord {
                            component: positions.len(),
                            output_is_diode: crate::redstone::is_diode(world.get_block(pos.offset(repeater.facing.opposite().block_face()))),
                            side_sources: Some(Vec::new()),
                        });
                        positions.push(pos);
                    }
                    _ => return Err(error(pos, "unsupported component")),
                }
                if let Some(entity) = world.get_block_entity(pos)
                    && !(world.get_block(pos).is_sign() && matches!(entity, BlockEntity::Sign(_))) {
                    return Err(error(pos, "unsupported block entity"));
                }
            }
        }
    }
    {
        for pos in positions.iter().copied().filter(|&pos| matches!(world.get_block(pos), Block::Lever { .. } | Block::RedstoneWire { .. })) {
            if !crate::interaction::is_valid_position(world.get_block(pos), scratch.world(), pos) {
                return Err(error(pos, "invalid attachment support"));
            }
        }
    }
    let wire_positions: Vec<_> = positions.iter().copied().filter(|&pos| matches!(scratch.world().get_block(pos), Block::RedstoneWire { .. })).collect();
    let lamp_positions: Vec<_> = positions.iter().copied().filter(|&pos| matches!(scratch.world().get_block(pos), Block::RedstoneLamp { .. })).collect();
    let discovery = (|| {
        let mut dependencies = crate::redpiler::scratch::discover_wire_sources(scratch.world(), &wire_positions)?;
        for lamp in lamp_positions {
            dependencies.extend(crate::redpiler::scratch::discover_lamp_sources(scratch.world(), lamp)?);
        }
        for record in &mut repeaters {
            let pos = positions[record.component];
            let Block::RedstoneRepeater { repeater } = world.get_block(pos) else { unreachable!() };
            let face = repeater.facing.block_face();
            let rear = pos.offset(face);
            let block = scratch.world().get_block(rear);
            if matches!(block, Block::RedstoneWire { .. }) {
                for mut source in crate::redpiler::scratch::discover_wire_sources(scratch.world(), &[rear])? {
                    source.wire = pos;
                    dependencies.push(source);
                }
            } else if block.is_solid() {
                for side in mchprs_blocks::BlockFace::values() {
                    let at = rear.offset(side);
                    let source = scratch.world().get_block(at);
                    if crate::redstone::power::emits_strong_power(source, scratch.world(), at, side, true) {
                        dependencies.push(crate::redpiler::scratch::WireSource { wire: pos, source: at, attenuation: 0 });
                    }
                }
            } else if crate::redstone::power::emits_weak_power(block, scratch.world(), rear, face, true) {
                dependencies.push(crate::redpiler::scratch::WireSource { wire: pos, source: rear, attenuation: 0 });
            }
            for side in [repeater.facing.rotate(), repeater.facing.rotate_ccw()] {
                let at = pos.offset(side.block_face());
                let block = world.get_block(at);
                if crate::redstone::is_diode(block) && crate::redstone::power::emits_strong_power(block, scratch.world(), at, side.block_face(), false) {
                    let source = positions.iter().position(|&position| position == at).ok_or_else(|| error(at, "unrepresented repeater side source"))?;
                    if !matches!(block, Block::RedstoneRepeater { .. }) {
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
    let initial = positions.iter().map(|&p| world.get_block(p)).collect();
    let mut links = vec![];
    {
        for dependency in discovery {
            let source = positions.iter().position(|&pos| pos == dependency.source)
                .ok_or_else(|| error(dependency.source, "unrepresented discovered wire source"))?;
            let target = positions.iter().position(|&pos| pos == dependency.wire)
                .ok_or_else(|| error(dependency.wire, "unrepresented discovered wire"))?;
            links.push(ElectricalLink { source, target, attenuation: dependency.attenuation });
        }
    }
    let output_programs = vec![[vec![], vec![]]; positions.len()];
    let mut program = Program::new(positions, initial, sources.clone(), output_programs, links, vec![], repeaters)
        .map_err(|err| error(bounds.0, err.reason))?;
    let output_sources: Vec<_> = sources.iter().copied().chain(program.repeaters.iter().map(|record| record.component)).collect();
    let mut comparisons = Vec::new();
    for (output_index, &source) in output_sources.iter().enumerate() {
        let mut scratch = ScratchWorld::from_world(world, bounds, entry.clone())?;
        let pos = program.positions[source];
        for (id, &at) in program.positions.iter().enumerate() {
            let block = scratch.world().get_block(at);
            let constant_power = program.incoming[id].iter().filter_map(|&link| {
                let link = &program.links[link];
                matches!(program.initial[link.source], Block::RedstoneBlock).then_some(15u8.saturating_sub(link.attenuation))
            }).max().unwrap_or(0);
            let quiet = match block {
                Block::Lever { mut lever } => { lever.powered = false; Block::Lever { lever } }
                Block::RedstoneRepeater { mut repeater } => { repeater.powered = false; repeater.locked = false; Block::RedstoneRepeater { repeater } }
                Block::RedstoneWire { mut wire } => { wire.power = constant_power; Block::RedstoneWire { wire } }
                Block::RedstoneLamp { .. } => Block::RedstoneLamp { lit: constant_power > 0 },
                _ => block,
            };
            scratch.world_mut().set_block(at, quiet);
        }
        let initial_power = false;
        let mut first_edge = None;
        let mut samples: [Option<Vec<Frame>>; 2] = [None, None];
        for powered in [!initial_power, initial_power, !initial_power] {
            let w = scratch.world_mut();
            let mut frames = vec![];
            if let Block::RedstoneRepeater { mut repeater } = w.get_block(pos) {
                if repeater.powered == powered {
                    repeater.powered = !powered;
                    w.set_block(pos, Block::RedstoneRepeater { repeater });
                }
            }
            w.observe_redstone_updates(true);
            match w.get_block(pos) {
                Block::Lever { lever } if lever.powered != powered => { crate::interaction::use_redstone(w.get_block(pos), w, pos, None); }
                Block::RedstoneRepeater { mut repeater } if repeater.powered != powered => {
                    repeater.powered = powered;
                    w.set_block(pos, Block::RedstoneRepeater { repeater });
                    crate::redstone::on_state_change(repeater.facing.block_facing(), w, pos);
                }
                _ => continue,
            }
            // Keep observing past the lamp delay while scheduled work remains.
            const MAX_PROBE_TICKS: usize = 128;
            for step in 0..=MAX_PROBE_TICKS {
                if step > 0 {
                    w.tick_interpreted();
                }
                frames.push(Frame {
                    blocks: program.positions.iter().map(|&p| w.get_block(p)).collect(),
                    pending: w.scheduler().iter_entries().collect(),
                    deliveries: w.take_redstone_update_observations(),
                });
                w.observe_redstone_updates(true);
                if step >= 4 && frames.last().unwrap().pending.is_empty() {
                    break;
                }
            }
            if !frames.last().unwrap().pending.is_empty() {
                return Err(error(pos, "probe did not settle"));
            }
            if let Some(previous) = &first_edge {
                if powered != initial_power && *previous != frames {
                    eprintln!("sampled source probe at {pos:?}: repeated transition differs");
                }
            } else {
                first_edge = Some(frames.clone());
            }
            let mut operations: Vec<_> = program.links.iter().enumerate()
                .filter(|(_, link)| link.source == source).map(|(id, _)| Operation::Contribution(id)).collect();
            for delivery in &frames[0].deliveries {
                if let Some(slot) = program.positions.iter().position(|&pos| pos == delivery.pos) {
                    match program.initial[slot] {
                        Block::RedstoneWire { .. } => operations.push(Operation::RefreshWire(slot)),
                        Block::RedstoneLamp { .. } => operations.push(Operation::UpdateLamp(slot)),
                        Block::RedstoneRepeater { .. } => operations.push(Operation::UpdateRepeater(slot)),
                        _ => {}
                    }
                }
            }
            // Turbo dust propagation can change downstream wires without a neighbor callback.
            for (slot, block) in program.initial.iter().enumerate() {
                if matches!(block, Block::RedstoneWire { .. })
                    && !operations.iter().any(|op| matches!(op, Operation::RefreshWire(id) if *id == slot))
                {
                    operations.push(Operation::RefreshWire(slot));
                }
            }
            for link in program.links.iter().filter(|link| link.source == source) {
                match program.initial[link.target] {
                    Block::RedstoneLamp { .. } if !operations.iter().any(|op| matches!(op, Operation::UpdateLamp(id) if *id == link.target)) => operations.push(Operation::UpdateLamp(link.target)),
                    Block::RedstoneRepeater { .. } if !operations.iter().any(|op| matches!(op, Operation::UpdateRepeater(id) if *id == link.target)) => operations.push(Operation::UpdateRepeater(link.target)),
                    _ => {}
                }
            }
            for record in &program.repeaters {
                if record.side_sources.as_ref().is_some_and(|side| side.contains(&source))
                    && !operations.iter().any(|op| matches!(op, Operation::UpdateRepeater(id) if *id == record.component)) {
                    operations.push(Operation::UpdateRepeater(record.component));
                }
            }
            program.output_programs[source][usize::from(powered)] = operations;
            samples[usize::from(powered)] = Some(frames);
            scratch.finish_probe()?;
        }
        if output_index >= sources.len() || program.initial.iter().any(|block| match block {
            Block::Lever { lever } => lever.powered,
            Block::RedstoneRepeater { repeater } => repeater.powered || repeater.locked,
            Block::RedstoneWire { wire } => wire.power != 0,
            Block::RedstoneLamp { lit } => *lit,
            _ => false,
        }) { continue; }
        comparisons.push((output_index, pos, samples));
    }
    for (output_index, pos, samples) in comparisons {
        let mut runtime = program.start();
        for powered in [true, false] {
            runtime.set_input(output_index, powered)?;
            for (step, frame) in samples[usize::from(powered)].as_ref().unwrap().iter().enumerate() {
                if step != 0 {
                    runtime.tick()?;
                }
                if runtime.blocks != frame.blocks || runtime.pending.iter_entries().collect::<Vec<_>>() != frame.pending {
                    eprintln!("sampled runtime at {pos:?}, edge={powered}, step={step}: expected blocks={:?}, pending={:?}; observed blocks={:?}, pending={:?}", frame.blocks, frame.pending, runtime.blocks, runtime.pending.iter_entries().collect::<Vec<_>>());
                }
            }
        }
    }
    Ok(program)
}

pub(super) fn discover_piston_actors(world: &impl World, bounds: (BlockPos, BlockPos), entry: ScratchEntry) -> Result<Vec<PistonActor>, ScratchError> {
    let scratch = ScratchWorld::from_world(world, bounds, entry)?;
    let world = scratch.world();
    let error = |pos, reason| ScratchError { position: Some(pos), reason };
    let mut actors = Vec::new();
    for y in bounds.0.y..=bounds.1.y {
        for z in bounds.0.z..=bounds.1.z {
            for x in bounds.0.x..=bounds.1.x {
                let base = BlockPos::new(x, y, z);
                if world.get_block_entity(base).is_some() {
                    return Err(error(base, "unsupported block entity in piston capture"));
                }
                let block = world.get_block(base);
                let Block::Piston { piston } = block else {
                    if matches!(block, Block::MovingPiston { .. }) {
                        return Err(error(base, "moving piston state is unsupported"));
                    }
                    continue;
                };
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
                    Block::PistonHead { head }
                        if head.facing == facing && head.sticky == piston.sticky && !head.short =>
                    {
                        true
                    }
                    Block::PistonHead { .. } => return Err(error(home, "piston head does not match base state")),
                    Block::MovingPiston { .. } => return Err(error(home, "moving piston state is unsupported")),
                    _ => false,
                };
                if piston.extended != head_present {
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

