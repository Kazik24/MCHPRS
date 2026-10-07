//! Compile electrical data and delivered notifications as separate channels.
use super::boolean::{BooleanArena, Expr, Variable};
use super::logic;
use super::outputs::PowerTerm;
use super::program::PreparedInstant;
use crate::redpiler::analysis::AnalysisReport;
use crate::redpiler::{CompilerOptions, TaskMonitor};
use crate::world::World;
use mchprs_blocks::blocks::{Block, RedstoneWire};
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_world::TickEntry;
use rustc_hash::{FxHashMap, FxHashSet};

pub(crate) struct Sensor {
    pub pos: BlockPos,
    pub terms: Vec<PowerTerm>,
    pub initial: u8,
    pub shape: RedstoneWire,
    pub shapes: Vec<(Expr, RedstoneWire)>,
    pub neighbors: [Option<usize>; 24],
    pub shape_updates: Vec<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum WireUpdate { Wire(usize), Base(usize), Head(usize) }

pub(crate) struct Observer {
    pub pos: BlockPos,
    pub powered: bool,
    pub samples: Vec<WireUpdate>,
}

pub(crate) struct PreparedSequential {
    pub wire_updates: Vec<WireUpdate>,
    pub sensors: Vec<Sensor>,
    pub observers: Vec<Observer>,
    pub watched: FxHashMap<BlockPos, Vec<usize>>,
    pub notifications: FxHashMap<BlockPos, Vec<WireUpdate>>,
    pub notification_sensors: FxHashMap<BlockPos, Vec<(usize, BlockFace)>>,
    pub source_samples: FxHashMap<BlockPos, Vec<WireUpdate>>,
    pub observer_sensors: Vec<Vec<usize>>,
    pub source_sensors: FxHashMap<BlockPos, Vec<usize>>,
    pub payloads: Vec<Block>,
}

pub(crate) fn prepare(
    world: &impl World,
    report: &AnalysisReport,
    ticks: &[TickEntry],
    options: &CompilerOptions,
    monitor: &TaskMonitor,
) -> Result<PreparedInstant, String> {
    let extracted = logic::sequential::extract(world, report, monitor)?;
    let bases: FxHashMap<_, _> = report.pistons.iter().enumerate().map(|(i, p)| (p.pos, i)).collect();
    let heads: FxHashMap<_, _> = report.pistons.iter().enumerate().map(|(id, p)| (p.head, id)).collect();
    let sample_at = |pos: BlockPos| bases.get(&pos).copied().map(WireUpdate::Base).or_else(|| heads.get(&pos).copied().map(WireUpdate::Head));
    let neighbors = [BlockFace::West, BlockFace::East, BlockFace::Bottom, BlockFace::Top, BlockFace::North, BlockFace::South];
    let adjacent = |pos: BlockPos| -> Vec<WireUpdate> {
        neighbors.into_iter().filter_map(|f| sample_at(pos.offset(f))).collect()
    };
    let mut watched: FxHashMap<BlockPos, Vec<usize>> = FxHashMap::default();
    let mut observers = Vec::new();
    for (id, &pos) in report.observers.iter().enumerate() {
        let Block::Observer { observer } = world.get_block(pos) else { unreachable!() };
        watched.entry(pos.offset(observer.facing.into())).or_default().push(id);
        let front = pos.offset(BlockFace::from(observer.facing).opposite());
        let mut samples: Vec<_> = sample_at(front).into_iter().collect();
        samples.extend(BlockFace::values().into_iter().filter(|&f| f != observer.facing.into()).filter_map(|f| sample_at(front.offset(f))));
        observers.push(Observer { pos, powered: observer.powered, samples });
    }
    for (&pos, list) in &mut watched {
        let shapes = [BlockFace::West, BlockFace::East, BlockFace::North, BlockFace::South, BlockFace::Bottom, BlockFace::Top];
        list.sort_by_key(|&id| shapes.iter().position(|&face| pos.offset(face) == observers[id].pos).unwrap());
    }
    let mut owned: FxHashSet<_> = report.observers.iter().copied().collect();
    let mut notifications = FxHashMap::default();
    for p in &report.pistons {
        if p.piston.extended && world.get_block(p.head) != Block::Air
            && !matches!(world.get_block(p.head), Block::PistonHead { head } if head.facing == p.piston.facing && head.sticky == p.piston.sticky && !head.short)
        { return Err(format!("extended piston at {:?} has an incompatible saved head at {:?}", p.pos, p.head)); }
        for pos in [p.pos, p.head, p.head.offset(p.piston.facing.into())] {
            owned.insert(pos);
            notifications.insert(pos, neighbors.into_iter().filter_map(|face| {
                let neighbor = pos.offset(face);
                bases.get(&neighbor).copied().map(WireUpdate::Base).or_else(|| heads.get(&neighbor).copied().map(WireUpdate::Head))
            }).collect());
            if world.get_block_entity(pos).is_some() {
                return Err(format!("sampled piston at {:?} has a moving-context entity at {pos:?}", p.pos));
            }
        }
        for alias in [p.head, p.head.offset(p.piston.facing.into())] {
            for face in BlockFace::values() {
                let pos = alias.offset(face);
                let block = world.get_block(pos);
                if !world.is_cursed() && crate::interaction::attachment_support(block, pos).is_some_and(|(support, _)| support == alias)
                    && (crate::redpiler::analysis::ports::is_consumer(block) || matches!(block, Block::RedstoneWire { .. } | Block::Lever { .. } | Block::StoneButton { .. }) || block.pressure_plate_powered().is_some())
                { return Err(format!("attachment at {pos:?} uses moving payload support at {alias:?}")); }
            }
        }
    }
    if ticks.iter().any(|t| owned.contains(&t.pos)) { return Err("sampled entry contains pending owned work".into()); }
    let mut sensors = Vec::new();
    for (pos, terms, shape, shapes) in extracted.sensors {
        owned.insert(pos);
        sensors.push(Sensor { pos, terms, initial: shape.power, shape, shapes, neighbors: [None; 24], shape_updates: Vec::new() });
    }
    let mut wire_updates = Vec::new();
    let mut update_ids = FxHashMap::default();
    for (id, sensor) in sensors.iter().enumerate() {
        update_ids.insert(sensor.pos, wire_updates.len());
        wire_updates.push(WireUpdate::Wire(id));
    }
    for (id, piston) in report.pistons.iter().enumerate() {
        update_ids.insert(piston.pos, wire_updates.len()); wire_updates.push(WireUpdate::Base(id));
        update_ids.insert(piston.head, wire_updates.len()); wire_updates.push(WireUpdate::Head(id));
    }
    for sensor in &mut sensors {
        sensor.neighbors = crate::world::wire_cache::positions(sensor.pos).map(|pos| update_ids.get(&pos).copied());
        for face in BlockFace::values() {
            let pos = sensor.pos.offset(face);
            if let Some(&id) = update_ids.get(&pos) { sensor.shape_updates.push(id); }
            for other in BlockFace::values() {
                if let Some(&id) = update_ids.get(&pos.offset(other)) { sensor.shape_updates.push(id); }
            }
        }
    }
    let wires: FxHashMap<_, _> = sensors.iter().enumerate().map(|(id, sensor)| (sensor.pos, id)).collect();
    for observer in &mut observers {
        let Block::Observer { observer: block } = world.get_block(observer.pos) else { unreachable!() };
        let front = observer.pos.offset(BlockFace::from(block.facing).opposite());
        observer.samples = std::iter::once(front).chain(BlockFace::values().into_iter()
            .filter(|&face| face != block.facing.into()).map(|face| front.offset(face)))
            .filter_map(|pos| wires.get(&pos).copied().map(WireUpdate::Wire).or_else(|| sample_at(pos))).collect();
    }
    for (&pos, samples) in &mut notifications {
        *samples = neighbors.into_iter().filter_map(|face| {
            let neighbor = pos.offset(face);
            wires.get(&neighbor).copied().map(WireUpdate::Wire).or_else(|| sample_at(neighbor))
        }).collect();
    }
    let mut notification_sensors = FxHashMap::default();
    for &pos in notifications.keys() {
        let shapes = [BlockFace::West, BlockFace::East, BlockFace::North, BlockFace::South, BlockFace::Bottom, BlockFace::Top];
        notification_sensors.insert(pos, shapes.into_iter().filter_map(|face| wires.get(&pos.offset(face)).copied().map(|id| (id, face))).collect());
    }
    let mut observer_sensors = vec![Vec::new(); observers.len()];
    let mut source_sensors: FxHashMap<BlockPos, Vec<usize>> = FxHashMap::default();
    for (id, sensor) in sensors.iter().enumerate() {
        for variable in dependencies(&extracted.logic.arena, sensor.terms.iter().map(|t| t.guard)) {
            match variable {
                Variable::Geometry { .. } => {},
                Variable::Observer(observer) => observer_sensors[observer].push(id),
                Variable::Signal { pos, .. } => source_sensors.entry(pos).or_default().push(id),
                Variable::WireDot(pos) => source_sensors.entry(pos).or_default().push(id),
                _ => unreachable!(),
            }
        }
        for pos in sensor.terms.iter().filter_map(|t| t.source) { source_sensors.entry(pos).or_default().push(id); }
    }
    let mut source_samples = FxHashMap::default();
    for &pos in &extracted.logic.sources {
        let block = world.get_block(pos);
        let mut samples = match block {
            Block::RedstoneRepeater { repeater } => {
                let front = pos.offset(repeater.facing.opposite().block_face());
                let mut result: Vec<_> = sample_at(front).into_iter().collect(); result.extend(adjacent(front)); result
            }
            Block::RedstoneComparator { comparator } => {
                let front = pos.offset(comparator.facing.opposite().block_face());
                let mut result: Vec<_> = sample_at(front).into_iter().collect(); result.extend(adjacent(front)); result
            }
            Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. } => {
                let mut result = adjacent(pos);
                for face in BlockFace::values() { for vertical in [BlockFace::Top, BlockFace::Bottom] {
                    if let Some(sample) = sample_at(pos.offset(face).offset(vertical)) { result.push(sample); }
                }} result
            }
            _ => adjacent(pos),
        };
        if let Some((support, _)) = crate::interaction::attachment_support(block, pos) { samples.extend(adjacent(support)); }
        let mut seen = FxHashSet::default(); samples.retain(|sample| seen.insert(*sample)); source_samples.insert(pos, samples);
    }
    for list in observer_sensors.iter_mut().chain(source_sensors.values_mut()) { list.sort_unstable(); list.dedup(); }
    let mut aliases = Vec::new();
    for (group, payload) in report.payload_groups.iter().enumerate() {
        aliases.extend(payload.positions.iter().map(|&pos| (group, pos, world.get_block(pos) == Block::RedstoneBlock)));
    }
    let template = owned.iter().map(|&pos| (pos, world.get_block(pos), world.get_block_entity(pos).cloned())).collect();
    Ok(PreparedInstant {
        sequential: Some(PreparedSequential { wire_updates, sensors, observers, watched, notifications, notification_sensors, source_samples,
            observer_sensors, source_sensors, payloads: extracted.payloads }),
        assume_instant: options.assume_instant, pistons: report.pistons.clone(), output_offset: 0,
        clocked: None, controls: Vec::new(), logic: extracted.logic,
        groups: report.payload_groups.iter().map(|g| g.members.clone()).collect(), aliases,
        owned, template, bounds: report.bounds, logical_tick: world.piston_state().logical_tick,
    })
}

pub(crate) fn dependencies(arena: &BooleanArena, roots: impl IntoIterator<Item=Expr>) -> Vec<Variable> {
    let mut pending: Vec<_> = roots.into_iter().collect();
    let mut visited = FxHashSet::default();
    let mut variables = FxHashSet::default();
    while let Some(root) = pending.pop() { if visited.insert(root) { if let Some(d) = arena.decision(root) {
        variables.insert(d.variable); pending.extend([d.low, d.high]);
    }}}
    variables.into_iter().collect()
}
