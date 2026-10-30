//! Private sampled experiment. Inputs may change only after the previous transition settles.
#![allow(dead_code)]
use super::event_netlist::scratch::{ScratchEntry, ScratchError, ScratchWorld};
use super::{TickScheduler, backend::ScheduledBlockTick};
use crate::world::{RedstoneUpdateObservation, World};
use mchprs_blocks::{
    BlockFacing, BlockPos,
    blocks::{Block, LeverFace},
};
use mchprs_world::{TickEntry, TickPriority};

#[derive(Clone, PartialEq, Eq)]
struct Frame {
    blocks: Vec<Block>,
    pending: Vec<TickEntry>,
    deliveries: Vec<RedstoneUpdateObservation>,
}
#[derive(Clone)]
enum Operation {
    RefreshWire(usize),
    UpdateLamp(usize),
}
struct Program {
    positions: Vec<BlockPos>,
    initial: Vec<Block>,
    source: Option<usize>,
    transitions: [Vec<Operation>; 2],
    wire: Option<usize>,
    lamp: Option<usize>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct PistonActor {
    id: usize,
    base: BlockPos,
    facing: BlockFacing,
    sticky: bool,
    home: BlockPos,
    far: BlockPos,
    extended: bool,
    head_present: bool,
    group: usize,
}
struct Runtime<'a> {
    program: &'a Program,
    blocks: Vec<Block>,
    pending: TickScheduler<ScheduledBlockTick>,
    operations: usize,
}
impl Program {
    fn start(&self) -> Runtime<'_> {
        Runtime {
            program: self,
            blocks: self.initial.clone(),
            pending: TickScheduler::default(),
            operations: 0,
        }
    }
}
impl Runtime<'_> {
    fn set_input(&mut self, powered: bool) {
        assert!(
            self.pending.iter_entries().next().is_none(),
            "transition must settle before changing input"
        );
        let Some(source) = self.program.source else { return };
        let Block::Lever { lever } = self.blocks[source] else { unreachable!() };
        if lever.powered != powered {
            let Block::Lever { mut lever } = self.blocks[source] else {
                unreachable!()
            };
            lever.powered = powered;
            self.blocks[source] = Block::Lever { lever };
            for operation in &self.program.transitions[usize::from(powered)] {
                match *operation {
                    Operation::RefreshWire(slot) => {
                        let Block::RedstoneWire { mut wire } = self.blocks[slot] else {
                            unreachable!()
                        };
                        wire.power = if powered { 15 } else { 0 };
                        self.blocks[slot] = Block::RedstoneWire { wire };
                    }
                    Operation::UpdateLamp(slot) => {
                        let Block::RedstoneWire { wire } = self.blocks[self.program.wire.unwrap()] else {
                            unreachable!()
                        };
                        let Block::RedstoneLamp { lit } = self.blocks[slot] else {
                            unreachable!()
                        };
                        if wire.power > 0 && !lit {
                            self.blocks[slot] = Block::RedstoneLamp { lit: true };
                        } else if wire.power == 0 && lit {
                            let tick = ScheduledBlockTick {
                                pos: self.program.positions[slot],
                                block_type: Some(self.blocks[slot].registry_id()),
                            };
                            if !self.pending.contains(&tick) {
                                self.pending.schedule_tick(tick, 2, TickPriority::Normal);
                            }
                        }
                    }
                }
                self.operations += 1;
            }
        }
    }
    fn drain_ticks(&mut self) {
        while let Some(tick) = self.pending.this_tick().pop_first() {
            let lamp = self.program.lamp.unwrap();
            if tick.pos == self.program.positions[lamp] {
                let Block::RedstoneWire { wire } = self.blocks[self.program.wire.unwrap()] else {
                    unreachable!()
                };
                if wire.power == 0 {
                    self.blocks[lamp] = Block::RedstoneLamp { lit: false };
                }
                self.operations += 1;
            }
        }
    }
    fn tick(&mut self) {
        self.drain_ticks();
        self.pending.end_last_tick_move_next();
        self.drain_ticks();
    }
}
fn compile(world: &impl World, bounds: (BlockPos, BlockPos), entry: ScratchEntry) -> Result<Program, ScratchError> {
    let mut scratch = ScratchWorld::from_world(world, bounds, entry)?;
    let mut positions = vec![];
    let mut source = None;
    let mut wires = 0;
    let mut lamps = 0;
    let error = |pos, reason| ScratchError { position: Some(pos), reason };
    for y in bounds.0.y..=bounds.1.y {
        for z in bounds.0.z..=bounds.1.z {
            for x in bounds.0.x..=bounds.1.x {
                let pos = BlockPos::new(x, y, z);
                match world.get_block(pos) {
                    Block::Air | Block::Stone {} => {}
                    Block::Lever { lever } if lever.face == LeverFace::Floor && source.is_none() => {
                        source = Some(positions.len());
                        positions.push(pos);
                    }
                    Block::RedstoneWire { .. } => {
                        wires += 1;
                        positions.push(pos);
                    }
                    Block::RedstoneLamp { .. } => {
                        lamps += 1;
                        positions.push(pos);
                    }
                    _ => return Err(error(pos, "unsupported component")),
                }
                if world.get_block_entity(pos).is_some() {
                    return Err(error(pos, "unsupported block entity"));
                }
            }
        }
    }
    if !positions.is_empty() && (source.is_none() || wires != 1 || lamps != 1) {
        return Err(error(bounds.0, "requires one lever, dust and lamp"));
    }
    if let Some(src) = source {
        let lever = positions[src];
        let wire = *positions
            .iter()
            .find(|&&p| matches!(world.get_block(p), Block::RedstoneWire { .. }))
            .unwrap();
        let lamp = *positions
            .iter()
            .find(|&&p| matches!(world.get_block(p), Block::RedstoneLamp { .. }))
            .unwrap();
        let delta = wire - lever;
        if delta.y != 0 || delta.x.abs() + delta.z.abs() != 1 || lamp != wire + delta {
            return Err(error(wire, "requires straight horizontal lever-dust-lamp"));
        }
        for pos in [lever, wire, lamp] {
            if world.get_block(pos - BlockPos::new(0, 1, 0)) != (Block::Stone {}) {
                return Err(error(pos, "requires stone support"));
            }
        }
    }
    for y in bounds.0.y..=bounds.1.y {
        for z in bounds.0.z..=bounds.1.z {
            for x in bounds.0.x..=bounds.1.x {
                let pos = BlockPos::new(x, y, z);
                if world.get_block(pos) != Block::Air && !positions.contains(&pos) && !positions.iter().any(|&p| pos == p - BlockPos::new(0, 1, 0)) {
                    return Err(error(pos, "unrepresented fixed connection"));
                }
            }
        }
    }
    let initial = positions.iter().map(|&p| world.get_block(p)).collect();
    let mut program = Program {
        positions,
        initial,
        source,
        transitions: [vec![], vec![]],
        wire: None,
        lamp: None,
    };
    if let Some(source) = source {
        let pos = program.positions[source];
        program.wire = program
            .positions
            .iter()
            .position(|&p| matches!(world.get_block(p), Block::RedstoneWire { .. }));
        program.lamp = program
            .positions
            .iter()
            .position(|&p| matches!(world.get_block(p), Block::RedstoneLamp { .. }));
        // shortcut: settled binary source only, add state-keyed probes before allowing overlapping input edges.
        let initial_power = match world.get_block(pos) {
            Block::Lever { lever } => lever.powered,
            _ => unreachable!(),
        };
        let mut first_edge = None;
        let mut samples: [Option<Vec<Frame>>; 2] = [None, None];
        for powered in [!initial_power, initial_power, !initial_power] {
            let w = scratch.world_mut();
            if let Block::Lever { mut lever } = w.get_block(pos) {
                if lever.powered == powered {
                    continue;
                }
                lever.powered = powered;
                w.set_block(pos, Block::Lever { lever });
            }
            let mut frames = vec![];
            w.observe_redstone_updates(true);
            crate::redstone::update_surrounding_blocks(w, pos);
            for step in 0..=4 {
                if step > 0 {
                    w.tick_interpreted();
                }
                frames.push(Frame {
                    blocks: program.positions.iter().map(|&p| w.get_block(p)).collect(),
                    pending: w.scheduler().iter_entries().collect(),
                    deliveries: w.take_redstone_update_observations(),
                });
                w.observe_redstone_updates(true);
            }
            if !frames.last().unwrap().pending.is_empty() {
                return Err(error(pos, "probe did not settle"));
            }
            if powered == initial_power && frames.last().unwrap().blocks != program.initial {
                return Err(error(pos, "loaded circuit is not a settled sampled state"));
            }
            if let Some(previous) = &first_edge {
                if powered != initial_power && *previous != frames {
                    return Err(error(pos, "source transition depends on unrepresented state"));
                }
            } else {
                first_edge = Some(frames.clone());
            }
            let wire = program.wire.unwrap();
            let lamp = program.lamp.unwrap();
            let operations: Vec<_> = frames[0]
                .deliveries
                .iter()
                .filter_map(|delivery| {
                    if delivery.pos == program.positions[wire] {
                        Some(Operation::RefreshWire(wire))
                    } else if delivery.pos == program.positions[lamp] {
                        Some(Operation::UpdateLamp(lamp))
                    } else {
                        None
                    }
                })
                .collect();
            if !operations.iter().any(|op| matches!(op, Operation::RefreshWire(_)))
                || !operations.iter().any(|op| matches!(op, Operation::UpdateLamp(_)))
            {
                return Err(error(pos, "probe did not observe wire and lamp updates"));
            }
            program.transitions[usize::from(powered)] = operations;
            samples[usize::from(powered)] = Some(frames);
            scratch.finish_probe()?;
        }
        let mut runtime = program.start();
        for powered in [!initial_power, initial_power] {
            runtime.set_input(powered);
            for (step, frame) in samples[usize::from(powered)].as_ref().unwrap().iter().enumerate() {
                if step != 0 {
                    runtime.tick();
                }
                if runtime.blocks != frame.blocks || runtime.pending.iter_entries().collect::<Vec<_>>() != frame.pending {
                    return Err(error(pos, "sampled behavior is not represented by runtime"));
                }
            }
        }
    }
    Ok(program)
}

fn discover_piston_actors(world: &impl World, bounds: (BlockPos, BlockPos), entry: ScratchEntry) -> Result<Vec<PistonActor>, ScratchError> {
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
                let far_block = world.get_block(far);
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
                    Block::Air => false,
                    _ => return Err(error(home, "piston contact cell is occupied")),
                };
                if far_block != Block::Air {
                    return Err(error(far, "piston contact cell is occupied"));
                }
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

fn assign_piston_groups(actors: &mut [PistonActor]) {
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
    use mchprs_blocks::block_entities::BlockEntity;
    use crate::plot::{PLOT_WIDTH, PlotWorld};
    use crate::world::storage::Chunk;
    use mchprs_blocks::{
        BlockDirection, BlockFacing,
        blocks::{Lever, RedstonePiston, RedstonePistonHead, RedstoneWire},
    };
    fn host() -> PlotWorld {
        PlotWorld::from_chunks(
            0,
            0,
            (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
            Default::default(),
        )
    }
    fn entry(w: &PlotWorld) -> ScratchEntry {
        ScratchEntry {
            scheduled: w.scheduler().iter_entries().collect(),
            piston: w.piston_state().clone(),
        }
    }
    #[test]
    fn empty() {
        let w = host();
        let p = compile(&w, (BlockPos::new(10, 10, 10), BlockPos::new(20, 20, 20)), entry(&w)).unwrap();
        p.start().tick();
    }
    #[test]
    fn rejects_unrepresented_blocks() {
        let mut w = host();
        let pos = BlockPos::new(15, 15, 15);
        w.set_block(pos, Block::RedstoneBlock);
        let err = match compile(&w, (BlockPos::new(10, 10, 10), BlockPos::new(20, 20, 20)), entry(&w)) {
            Err(err) => err,
            Ok(_) => panic!("unsupported block admitted"),
        };
        assert_eq!(err.position, Some(pos));
    }
    #[test]
    fn native_edges_and_host_preservation() {
        let source = BlockPos::new(15, 15, 15);
        let bounds = (BlockPos::new(10, 10, 10), BlockPos::new(22, 20, 20));
        let snapshot = |world: &PlotWorld| {
            let blocks: Vec<_> = (bounds.0.y..=bounds.1.y)
                .flat_map(|y| (bounds.0.z..=bounds.1.z).flat_map(move |z| (bounds.0.x..=bounds.1.x).map(move |x| BlockPos::new(x, y, z))))
                .map(|pos| (world.get_block_raw(pos), serde_json::to_value(world.get_block_entity(pos)).unwrap()))
                .collect();
            (
                blocks,
                world.scheduler().iter_entries().collect::<Vec<_>>(),
                serde_json::to_value(world.piston_state()).unwrap(),
            )
        };

        for initially_powered in [false, true] {
            let mut w = host();
            w.set_block(
                source,
                Block::Lever {
                    lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
                },
            );
            w.set_block(
                BlockPos::new(16, 15, 15),
                Block::RedstoneWire {
                    wire: RedstoneWire::default(),
                },
            );
            w.set_block(BlockPos::new(17, 15, 15), Block::RedstoneLamp { lit: false });
            for x in 15..=17 {
                w.set_block(BlockPos::new(x, 14, 15), Block::Stone {});
            }

            if initially_powered {
                if let Block::Lever { mut lever } = w.get_block(source) {
                    lever.powered = true;
                    w.set_block(source, Block::Lever { lever });
                }
                crate::redstone::update_surrounding_blocks(&mut w, source);
                for _ in 0..4 {
                    w.tick_interpreted();
                }
                assert_eq!(w.get_block(source), Block::Lever { lever: Lever::new(LeverFace::Floor, BlockDirection::North, true) });
                assert_eq!(w.get_block(BlockPos::new(16, 15, 15)), Block::RedstoneWire { wire: RedstoneWire { power: 15, ..Default::default() } });
                assert_eq!(w.get_block(BlockPos::new(17, 15, 15)), Block::RedstoneLamp { lit: true });
                assert!(w.scheduler().iter_entries().next().is_none());
            }

            let before = snapshot(&w);
            let p = compile(&w, bounds, entry(&w)).unwrap();
            assert_eq!(before, snapshot(&w));
            assert_eq!(p.initial, p.positions.iter().map(|&pos| w.get_block(pos)).collect::<Vec<_>>());
            let mut r = p.start();
            let compare = |runtime: &Runtime<'_>, world: &PlotWorld| {
                assert_eq!(runtime.blocks, p.positions.iter().map(|&pos| world.get_block(pos)).collect::<Vec<_>>());
                assert_eq!(
                    runtime.pending.iter_entries().collect::<Vec<_>>(),
                    world.scheduler().iter_entries().collect::<Vec<_>>()
                );
            };
            compare(&r, &w);

            let mut powered = initially_powered;
            for _ in 0..12 {
                powered = !powered;
                if let Block::Lever { mut lever } = w.get_block(source) {
                    lever.powered = powered;
                    w.set_block(source, Block::Lever { lever });
                }
                crate::redstone::update_surrounding_blocks(&mut w, source);
                r.set_input(powered);
                compare(&r, &w);
                if !powered {
                    assert_eq!(w.get_block(BlockPos::new(17, 15, 15)), Block::RedstoneLamp { lit: true });
                    assert_eq!(r.blocks[p.lamp.unwrap()], Block::RedstoneLamp { lit: true });
                }
                for tick in 1..=4 {
                    w.tick_interpreted();
                    r.tick();
                    compare(&r, &w);
                    if !powered {
                        assert_eq!(w.get_block(BlockPos::new(17, 15, 15)), Block::RedstoneLamp { lit: tick < 4 });
                        assert_eq!(r.blocks[p.lamp.unwrap()], Block::RedstoneLamp { lit: tick < 4 });
                    }
                }
                assert!(w.scheduler().iter_entries().next().is_none());
                assert!(r.pending.iter_entries().next().is_none());
            }
            assert!(r.operations > 0);
        }
    }

    #[test]
    fn rejects_probe_reads_outside_capture_without_mutating_host() {
        let source = BlockPos::new(15, 15, 15);
        let wire = BlockPos::new(16, 15, 15);
        let lamp = BlockPos::new(17, 15, 15);
        let full_snapshot_bounds = (BlockPos::new(10, 10, 10), BlockPos::new(22, 16, 20));
        let snapshot = |world: &PlotWorld| {
            let blocks = (full_snapshot_bounds.0.y..=full_snapshot_bounds.1.y)
                .flat_map(|y| (full_snapshot_bounds.0.z..=full_snapshot_bounds.1.z).flat_map(move |z| (full_snapshot_bounds.0.x..=full_snapshot_bounds.1.x).map(move |x| BlockPos::new(x, y, z))))
                .map(|pos| (world.get_block_raw(pos), serde_json::to_value(world.get_block_entity(pos)).unwrap()))
                .collect::<Vec<_>>();
            (
                blocks,
                world.scheduler().iter_entries().collect::<Vec<_>>(),
                serde_json::to_value(world.piston_state()).unwrap(),
            )
        };
        let chain = || {
            let mut world = host();
            world.set_block(
                source,
                Block::Lever {
                    lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
                },
            );
            world.set_block(wire, Block::RedstoneWire { wire: RedstoneWire::default() });
            world.set_block(lamp, Block::RedstoneLamp { lit: false });
            for x in 15..=17 {
                world.set_block(BlockPos::new(x, 14, 15), Block::Stone {});
            }
            world
        };

        for (bounds, add_outside_source) in [
            ((BlockPos::new(15, 14, 15), BlockPos::new(17, 15, 15)), false),
            ((BlockPos::new(10, 10, 10), BlockPos::new(22, 15, 20)), true),
        ] {
            let mut world = chain();
            if add_outside_source {
                world.set_block(BlockPos::new(16, 16, 15), Block::RedstoneBlock {});
            }
            let before = snapshot(&world);
            let error = match compile(&world, bounds, entry(&world)) {
                Err(error) => error,
                Ok(_) => panic!("probe reading outside capture was admitted"),
            };
            assert_eq!(error.reason, "scratch probe read outside captured boundary");
            let position = error.position.expect("boundary error includes escaped read position");
            assert!(
                position.x < bounds.0.x || position.x > bounds.1.x
                    || position.y < bounds.0.y || position.y > bounds.1.y
                    || position.z < bounds.0.z || position.z > bounds.1.z,
                "reported boundary position {position:?} is inside {bounds:?}"
            );
            assert_eq!(before, snapshot(&world));
        }
    }

    #[test]
    fn discovers_static_empty_piston_actors_and_preserves_source() {
        let bounds = (BlockPos::new(10, 10, 10), BlockPos::new(22, 20, 22));
        let mut world = host();
        let bases = [BlockPos::new(12, 15, 12), BlockPos::new(18, 15, 12), BlockPos::new(12, 15, 17)];
        world.set_block(
            bases[0],
            Block::Piston {
                piston: RedstonePiston { facing: BlockFacing::East, sticky: false, extended: false },
            },
        );
        world.set_block(
            bases[1],
            Block::Piston {
                piston: RedstonePiston { facing: BlockFacing::South, sticky: false, extended: true },
            },
        );
        world.set_block(
            bases[1].offset(mchprs_blocks::BlockFace::South),
            Block::PistonHead {
                head: RedstonePistonHead { facing: BlockFacing::South, sticky: false, short: false },
            },
        );
        world.set_block(
            bases[2],
            Block::Piston {
                piston: RedstonePiston { facing: BlockFacing::North, sticky: true, extended: false },
            },
        );
        let before = (
            (bounds.0.y..=bounds.1.y)
                .flat_map(|y| (bounds.0.z..=bounds.1.z).flat_map(move |z| (bounds.0.x..=bounds.1.x).map(move |x| BlockPos::new(x, y, z))))
                .map(|pos| (world.get_block_raw(pos), serde_json::to_value(world.get_block_entity(pos)).unwrap()))
                .collect::<Vec<_>>(),
            world.scheduler().iter_entries().collect::<Vec<_>>(),
            serde_json::to_value(world.piston_state()).unwrap(),
        );
        let actors = discover_piston_actors(&world, bounds, entry(&world)).unwrap();
        assert_eq!(
            actors,
            vec![
                PistonActor { id: 0, base: bases[0], facing: BlockFacing::East, sticky: false, home: bases[0].offset(mchprs_blocks::BlockFace::East), far: bases[0].offset(mchprs_blocks::BlockFace::East).offset(mchprs_blocks::BlockFace::East), extended: false, head_present: false, group: 0 },
                PistonActor { id: 1, base: bases[1], facing: BlockFacing::South, sticky: false, home: bases[1].offset(mchprs_blocks::BlockFace::South), far: bases[1].offset(mchprs_blocks::BlockFace::South).offset(mchprs_blocks::BlockFace::South), extended: true, head_present: true, group: 1 },
                PistonActor { id: 2, base: bases[2], facing: BlockFacing::North, sticky: true, home: bases[2].offset(mchprs_blocks::BlockFace::North), far: bases[2].offset(mchprs_blocks::BlockFace::North).offset(mchprs_blocks::BlockFace::North), extended: false, head_present: false, group: 2 },
            ]
        );
        assert_eq!(
            before,
            (
                (bounds.0.y..=bounds.1.y)
                    .flat_map(|y| (bounds.0.z..=bounds.1.z).flat_map(move |z| (bounds.0.x..=bounds.1.x).map(move |x| BlockPos::new(x, y, z))))
                    .map(|pos| (world.get_block_raw(pos), serde_json::to_value(world.get_block_entity(pos)).unwrap()))
                    .collect::<Vec<_>>(),
                world.scheduler().iter_entries().collect::<Vec<_>>(),
                serde_json::to_value(world.piston_state()).unwrap(),
            )
        );
        assert!(compile(&world, bounds, entry(&world)).is_err(), "lamp-only runtime admission must reject pistons");
    }

    #[test]
    fn rejects_invalid_or_unreadable_piston_contacts() {
        let base = BlockPos::new(15, 15, 15);
        let facing = BlockFacing::East;
        let home = base.offset(mchprs_blocks::BlockFace::East);
        let far = home.offset(mchprs_blocks::BlockFace::East);
        let bounds = (BlockPos::new(10, 10, 10), BlockPos::new(22, 20, 20));
        let mut mismatched = host();
        mismatched.set_block(base, Block::Piston { piston: RedstonePiston { facing, sticky: false, extended: true } });
        mismatched.set_block(home, Block::PistonHead { head: RedstonePistonHead { facing: BlockFacing::West, sticky: false, short: false } });
        let error = discover_piston_actors(&mismatched, bounds, entry(&mismatched)).unwrap_err();
        assert_eq!(error.position, Some(home));
        assert_eq!(error.reason, "piston head does not match base state");

        let mut occupied = host();
        occupied.set_block(base, Block::Piston { piston: RedstonePiston { facing, sticky: false, extended: false } });
        occupied.set_block(far, Block::Stone {});
        let error = discover_piston_actors(&occupied, bounds, entry(&occupied)).unwrap_err();
        assert_eq!(error.position, Some(far));
        assert_eq!(error.reason, "piston contact cell is occupied");

        let mut moving = host();
        moving.set_block(base, Block::MovingPiston { moving: Default::default() });
        let error = discover_piston_actors(&moving, bounds, entry(&moving)).unwrap_err();
        assert_eq!(error.position, Some(base));
        assert_eq!(error.reason, "moving piston state is unsupported");

        let mut entity = host();
        entity.set_block_entity(BlockPos::new(16, 15, 16), BlockEntity::Comparator { output_strength: 0 });
        let error = discover_piston_actors(&entity, bounds, entry(&entity)).unwrap_err();
        assert_eq!(error.position, Some(BlockPos::new(16, 15, 16)));
        assert_eq!(error.reason, "unsupported block entity in piston capture");

        let mut escaped = host();
        let edge = BlockPos::new(22, 15, 15);
        escaped.set_block(edge, Block::Piston { piston: RedstonePiston { facing: BlockFacing::East, sticky: false, extended: false } });
        let capture = (BlockPos::new(10, 10, 10), BlockPos::new(22, 20, 20));
        let error = discover_piston_actors(&escaped, capture, entry(&escaped)).unwrap_err();
        assert_eq!(error.reason, "scratch probe read outside captured boundary");
        assert!(error.position.is_some_and(|pos| pos.x > capture.1.x));
    }

    #[test]
    fn groups_pistons_by_transitive_contact_overlap() {
        let make = |id, home, far| PistonActor {
            id,
            base: BlockPos::zero(),
            facing: BlockFacing::East,
            sticky: false,
            home,
            far,
            extended: false,
            head_present: false,
            group: usize::MAX,
        };
        let mut actors = vec![
            make(0, BlockPos::new(0, 0, 0), BlockPos::new(1, 0, 0)),
            make(1, BlockPos::new(10, 0, 0), BlockPos::new(11, 0, 0)),
            make(2, BlockPos::new(1, 0, 0), BlockPos::new(2, 0, 0)),
            make(3, BlockPos::new(2, 0, 0), BlockPos::new(3, 0, 0)),
        ];
        assign_piston_groups(&mut actors);
        assert_eq!(actors.iter().map(|actor| actor.group).collect::<Vec<_>>(), [0, 1, 0, 0]);
        assert_eq!(actors.iter().map(|actor| actor.id).collect::<Vec<_>>(), [0, 1, 2, 3]);
    }
}

#[cfg(test)]
mod rejection_tests {
    use super::*;
    use crate::plot::{PLOT_WIDTH, PlotWorld};
    use crate::world::storage::Chunk;
    #[test]
    fn rejects_pending_and_boundary() {
        let mut w = PlotWorld::from_chunks(
            0,
            0,
            (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
            Default::default(),
        );
        let pos = BlockPos::new(10, 10, 10);
        w.schedule_tick(pos, 2, mchprs_world::TickPriority::Normal);
        let entry = ScratchEntry {
            scheduled: w.scheduler().iter_entries().collect(),
            piston: w.piston_state().clone(),
        };
        assert!(compile(&w, (pos, pos), entry).is_err());
        let entry = ScratchEntry {
            scheduled: vec![],
            piston: w.piston_state().clone(),
        };
        assert!(compile(&w, (BlockPos::new(-1, 0, 0), pos), entry).is_err());
    }
}
