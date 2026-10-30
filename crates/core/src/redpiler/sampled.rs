//! Private sampled experiment. Inputs may change only after the previous transition settles.
#![allow(dead_code)]
use super::event_netlist::scratch::{ScratchEntry, ScratchError, ScratchWorld};
use super::{TickScheduler, backend::ScheduledBlockTick};
use crate::world::{RedstoneUpdateObservation, World};
use mchprs_blocks::{
    BlockPos,
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
    fn tick(&mut self) {
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
        self.pending.end_last_tick_move_next();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plot::{PLOT_WIDTH, PlotWorld};
    use crate::world::storage::Chunk;
    use mchprs_blocks::{
        BlockDirection,
        blocks::{Lever, RedstoneWire},
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
        let mut w = host();
        let source = BlockPos::new(15, 15, 15);
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
        let before = snapshot(&w);
        let p = compile(&w, bounds, entry(&w)).unwrap();
        assert_eq!(before, snapshot(&w));
        assert_eq!(p.initial, p.positions.iter().map(|&pos| w.get_block(pos)).collect::<Vec<_>>());
        let mut r = p.start();
        for powered in [true, false, true] {
            if let Block::Lever { mut lever } = w.get_block(source) {
                lever.powered = powered;
                w.set_block(source, Block::Lever { lever });
            }
            crate::redstone::update_surrounding_blocks(&mut w, source);
            r.set_input(powered);
            for step in 0..=4 {
                if step > 0 {
                    w.tick_interpreted();
                    r.tick();
                }
                assert_eq!(r.blocks, p.positions.iter().map(|&pos| w.get_block(pos)).collect::<Vec<_>>());
                assert_eq!(
                    r.pending.iter_entries().collect::<Vec<_>>(),
                    w.scheduler().iter_entries().collect::<Vec<_>>()
                );
            }
        }
        assert!(r.operations > 0);
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
