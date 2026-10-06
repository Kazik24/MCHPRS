//! Recognize a shared observer clock and its independently sampled BUD bank.
//! Storage boundaries are discovered from power and update connectivity, not
//! schematic names, signs, bit coordinates or a presumed increment function.
use super::{boolean::Variable, logic::WaveLogic};
use crate::redpiler::analysis::{ports::UpdateKind, topology::Topology, AnalysisReport};
use crate::redpiler::TaskMonitor;
use crate::world::{for_each_block_optimized, World};
use mchprs_blocks::blocks::Block;
use mchprs_blocks::{BlockFace, BlockFacing, BlockPos};
use rustc_hash::{FxHashMap, FxHashSet};

pub(crate) struct MemoryCell {
    pub actor: usize,
    pub base: BlockPos,
    pub near: BlockPos,
    pub far: BlockPos,
}

pub(crate) struct ClockedProgram {
    pub clock: usize,
    pub memory: Vec<MemoryCell>,
    pub observers: FxHashSet<BlockPos>,
}

impl ClockedProgram {
    pub fn validate(&self, world: &impl World, logic: &WaveLogic) -> Result<(), String> {
        if logic.response_sources.len() != 1
            || !matches!(
                world.get_block(logic.response_sources[0]),
                Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. }
            )
        {
            return Err("observer-clock execution needs one ordinary torch control source".into());
        }
        let mut pending = vec![logic.responses[self.clock]];
        let mut visited = FxHashSet::default();
        while let Some(root) = pending.pop() {
            if !visited.insert(root) {
                continue;
            }
            if let Some(decision) = logic.arena.decision(root) {
                if !matches!(decision.variable, Variable::Signal { .. }) {
                    return Err("clock control depends on stored data".into());
                }
                pending.extend([decision.low, decision.high]);
            }
        }
        if logic.evaluate(|_| 15)[self.clock] || !logic.evaluate(|_| 0)[self.clock] {
            return Err("clock control must release on loss of ordinary torch power".into());
        }
        Ok(())
    }
}

pub(crate) fn recognize(
    world: &impl World,
    report: &AnalysisReport,
    monitor: &TaskMonitor,
) -> Result<Option<ClockedProgram>, String> {
    let clocks: Vec<_> = report
        .pistons
        .iter()
        .enumerate()
        .filter(|(_, p)| !p.piston.sticky)
        .map(|(id, _)| id)
        .collect();
    if clocks.is_empty() {
        return Ok(None);
    }
    // Ordinary notification pistons are not automatically clocks. Report an
    // unsupported actor's actual role boundary before checking clock count.
    for &actor in &clocks {
        let p = &report.pistons[actor];
        let above = p.pos.offset(BlockFace::Top);
        if p.piston.facing != BlockFacing::Down
            || !p.piston.extended
            || !p.powered
            || world.get_block(p.payload) != Block::Air
            || !matches!(world.get_block(above),Block::Observer { observer }
                if observer.facing==BlockFacing::Down && !observer.powered)
            || !world.get_block(above.offset(BlockFace::Top)).is_solid()
        {
            return Err(format!(
                "ordinary piston at {:?} is not a ready empty observer-clock generator; independent piston update samplers are not implemented",
                p.pos
            ));
        }
    }
    if clocks.len() != 1 {
        return Err("clocked instant execution needs one owned generator".into());
    }
    let clock = clocks[0];
    let p = &report.pistons[clock];
    let above = p.pos.offset(BlockFace::Top);
    let mut observers = FxHashSet::default();
    observers.insert(above);
    let mut observed_outputs = FxHashSet::default();
    let mut sampling = None;
    for &pos in &report.observers {
        let Block::Observer { observer } = world.get_block(pos) else {
            unreachable!()
        };
        let face = BlockFace::from(observer.facing);
        if !face.is_horizontal() {
            continue;
        }
        let target = pos.offset(face);
        let Some(actor) = report.pistons.iter().position(|p| p.pos == target) else {
            return Err(format!(
                "observer at {pos:?} is not attached to an owned clock or output piston"
            ));
        };
        if observer.powered {
            return Err(format!("clock observer at {pos:?} is active at entry"));
        }
        if actor == clock {
            if sampling.replace(pos).is_some() {
                return Err("clock has multiple sampling observers".into());
            }
        } else {
            observed_outputs.insert(actor);
        }
        observers.insert(pos);
    }
    let sampling = sampling.ok_or("observer clock has no independent sampling output")?;
    let mobile = report
        .payload_groups
        .iter()
        .enumerate()
        .flat_map(|(id, g)| g.positions.iter().map(move |&p| (p, id)))
        .collect::<FxHashMap<_, _>>();
    let mut topology = Topology::new(world, report.bounds, monitor, 4_194_304, mobile);
    let mut memory = Vec::new();
    for (actor, p) in report.pistons.iter().enumerate() {
        if actor == clock
            || observed_outputs.contains(&actor)
            || matches!(
                world.get_block(p.pos.offset(BlockFace::Top)),
                Block::Observer { .. }
            )
        {
            continue;
        }
        let inputs = &report.recognition[actor].inputs;
        let coupled = report.ports.pistons[actor].updates.iter().any(|u| {
            u.source != p.head
                && (inputs.wires.contains(&u.source)
                    || matches!(u.kind, UpdateKind::AdjacentHeadChange))
        });
        if coupled {
            continue;
        }
        let mut sampled = false;
        for update in &report.ports.pistons[actor].updates {
            if update.kind != UpdateKind::WireNotification {
                continue;
            }
            let dependencies = topology
                .wire_inputs(update.source)
                .map_err(|e| e.to_string())?;
            if dependencies.sources.iter().any(|s| s.source == sampling) {
                if dependencies.sources.iter().any(|s| s.source != sampling)
                    || !dependencies.outside_bounds.is_empty()
                {
                    return Err(format!(
                        "BUD update at {:?} has another writer or incomplete context",
                        p.pos
                    ));
                }
                sampled = true;
            } else if dependencies.sources.iter().any(|s| s.source != sampling) {
                return Err(format!(
                    "BUD update at {:?} has an independent writer outside its clock",
                    p.pos
                ));
            }
        }
        if !sampled {
            return Err(format!(
                "BUD at {:?} is not sampled by the shared observer clock",
                p.pos
            ));
        }
        if report
            .payload_groups
            .iter()
            .any(|g| g.members.contains(&actor) && g.members.len() != 1)
        {
            return Err(format!(
                "clocked BUD at {:?} shares its stored payload",
                p.pos
            ));
        }
        if p.piston.facing != BlockFacing::Down
            || !p.piston.sticky
            || world.get_block(p.payload) != Block::RedstoneBlock
        {
            return Err(format!(
                "clocked BUD at {:?} needs a downward redstone-block storage mechanism",
                p.pos
            ));
        }
        memory.push(MemoryCell {
            actor,
            base: p.pos,
            near: p.head,
            far: p.payload,
        });
    }
    if memory.is_empty() || memory.len() > 64 {
        return Err("clocked instant execution needs 1..64 independent BUD cells".into());
    }
    let mut consumers = Vec::new();
    for_each_block_optimized(world, report.bounds.0, report.bounds.1, |pos| {
        let block = world.get_block(pos);
        if crate::redpiler::analysis::ports::is_consumer(block) {
            consumers.push((pos, block));
        }
    });
    for (pos, block) in consumers {
        for (root, face, input) in crate::redpiler::analysis::ports::consumer_roots(block, pos) {
            let dependencies = match input {
                crate::redpiler::analysis::ports::ConsumerInput::Main => {
                    topology.signal_inputs(root, face)
                }
                crate::redpiler::analysis::ports::ConsumerInput::ComparatorSide => {
                    topology.comparator_side_inputs(root, face)
                }
            }
            .map_err(|e| e.to_string())?;
            if let Some(source) = dependencies
                .sources
                .iter()
                .find(|s| observers.contains(&s.source))
            {
                return Err(format!(
                    "clock/reset observer at {:?} is visible to ordinary consumer at {pos:?}",
                    source.source
                ));
            }
        }
    }
    Ok(Some(ClockedProgram {
        clock,
        memory,
        observers,
    }))
}
