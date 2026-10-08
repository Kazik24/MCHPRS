//! Explicit independently delivered BUD write events; data values are not clocks.
use super::outputs::PowerTerm;
use super::{
    boolean::Variable,
    clocked::{ClockedProgram, MemoryCell},
    logic::WaveLogic,
};
use crate::redpiler::analysis::{
    ports::UpdateKind,
    topology::{PowerDependencies, SourceKind, Topology},
    AnalysisLimits, AnalysisReport,
};
use crate::redpiler::TaskMonitor;
use crate::world::World;
use mchprs_blocks::blocks::Block;
use mchprs_blocks::BlockPos;
use rustc_hash::{FxHashMap, FxHashSet};

pub(crate) struct SamplingEvent {
    pub source: SamplingSource,
    pub targets: Vec<SamplingTarget>,
}

#[derive(Clone, Copy)]
pub(crate) struct SamplingTarget {
    pub actor: usize,
    /// A head callback forwards to its base only while the saved head exists.
    pub requires_extended: bool,
}

pub(crate) enum SamplingSource {
    /// A certified empty ordinary generator's settled base/head pose edge.
    Generator(usize),
    /// A static, independently driven dust net's delivered strength change.
    Power {
        writer: BlockPos,
        pos: BlockPos,
        initial: u8,
        terms: Vec<PowerTerm>,
    },
}

pub(crate) struct Classification {
    pub memory: Vec<MemoryCell>,
    pub events: Vec<SamplingEvent>,
    pub generators: Vec<usize>,
}

/// Data delivery must reach the base, rather than merely supply QC power.
/// Controls notify their support; diodes notify their output and its neighbors.
pub(crate) fn data_notifies(world: &impl World, source: BlockPos, base: BlockPos) -> bool {
    let adjacent = |pos: BlockPos| {
        let d = pos - base;
        d.x.abs() + d.y.abs() + d.z.abs() == 1
    };
    let block = world.get_block(source);
    if matches!(
        block,
        Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. }
    ) {
        let mut notified = false;
        crate::redstone::surrounding_notifications(source, |pos, _, _| notified |= pos == base);
        return notified;
    }
    if let Block::Observer { observer } = block {
        let mut notified = false;
        crate::redstone::observer_notifications(observer.facing, source, |pos, _| {
            notified |= pos == base;
        });
        return notified;
    }
    let output = match block {
        Block::RedstoneComparator { comparator } => {
            Some(source.offset(comparator.facing.opposite().block_face()))
        }
        Block::RedstoneRepeater { repeater } => {
            Some(source.offset(repeater.facing.opposite().block_face()))
        }
        _ => None,
    };
    if let Some(output) = output {
        let mut notified = false;
        let facing = match block {
            Block::RedstoneComparator { comparator } => comparator.facing.block_face(),
            Block::RedstoneRepeater { repeater } => repeater.facing.block_face(),
            _ => unreachable!(),
        };
        crate::redstone::diode_notifications(output, facing, |pos, _| notified |= pos == base);
        return notified;
    }
    if adjacent(source) {
        return true;
    }
    (matches!(block, Block::Lever { .. } | Block::StoneButton { .. })
        || block.pressure_plate_powered().is_some())
        && crate::interaction::attachment_support(block, source)
            .is_some_and(|(support, _)| adjacent(support))
}

/// Only an observer whose pulse electrically returns to the watched actor is
/// a reset candidate. Other observers retain their ordinary timed semantics.
pub(crate) fn reset_candidates(world: &impl World, report: &AnalysisReport) -> FxHashSet<BlockPos> {
    report
        .observers
        .iter()
        .copied()
        .filter(|&pos| {
            let Block::Observer { observer } = world.get_block(pos) else {
                return false;
            };
            let watched = pos.offset(observer.facing.into());
            report.pistons.iter().enumerate().any(|(actor, piston)| {
                piston.piston.sticky
                    && piston.pos == watched
                    && report.recognition[actor]
                        .inputs
                        .sources
                        .iter()
                        .any(|source| source.source == pos)
            })
        })
        .collect()
}

pub(crate) fn recognize(
    world: &impl World,
    report: &AnalysisReport,
    monitor: &TaskMonitor,
    clocked: Option<&ClockedProgram>,
    resets: &FxHashSet<BlockPos>,
) -> Result<Classification, String> {
    let mut groups = vec![0; report.pistons.len()];
    let mut mobile = FxHashMap::default();
    for (group, descriptor) in report.payload_groups.iter().enumerate() {
        for &actor in &descriptor.members {
            groups[actor] = group;
        }
        let empty = descriptor
            .members
            .iter()
            .all(|&actor| !report.pistons[actor].piston.sticky)
            && !descriptor
                .positions
                .iter()
                .any(|&pos| super::outputs::supported_payload(world.get_block(pos)));
        for &pos in &descriptor.positions {
            if !empty
                || descriptor
                    .members
                    .iter()
                    .any(|&actor| report.pistons[actor].head == pos)
            {
                mobile.insert(pos, group);
            }
        }
    }
    let mut topology = Topology::new(
        world,
        report.bounds,
        monitor,
        AnalysisLimits::for_budget(monitor.budget_multiplier()).max_dependency_steps,
        mobile,
    );
    let mut wire_inputs: FxHashMap<BlockPos, PowerDependencies> = FxHashMap::default();
    let mut coupled = vec![false; report.pistons.len()];
    for (actor, piston) in report.pistons.iter().enumerate() {
        let independent = |source: &crate::redpiler::analysis::topology::PowerDependency| {
            !resets.contains(&source.source)
                && !matches!(source.kind, SourceKind::MobilePayload { group } if group == groups[actor])
        };
        coupled[actor] = report.recognition[actor]
            .inputs
            .sources
            .iter()
            .any(|source| independent(source) && data_notifies(world, source.source, piston.pos));
        for update in &report.ports.pistons[actor].updates {
            if coupled[actor] {
                break;
            }
            if update.kind != UpdateKind::WireNotification || update.independent_of_power {
                continue;
            }
            if !wire_inputs.contains_key(&update.source) {
                wire_inputs.insert(
                    update.source,
                    topology
                        .wire_inputs(update.source)
                        .map_err(|e| e.to_string())?,
                );
            }
            coupled[actor] |= wire_inputs[&update.source].sources.iter().any(independent);
        }
    }
    let generators: Vec<_> = report
        .pistons
        .iter()
        .enumerate()
        .filter_map(|(actor, piston)| {
            (!piston.piston.sticky && !clocked.is_some_and(|clock| clock.clock == actor))
                .then_some(actor)
        })
        .collect();
    for &actor in &generators {
        let piston = &report.pistons[actor];
        for &pos in &report.observers {
            let Block::Observer { observer } = world.get_block(pos) else {
                continue;
            };
            let watched = pos.offset(observer.facing.into());
            if watched == piston.pos || watched == piston.head {
                return Err(format!(
                    "ordinary sampling generator at {:?} is watched by observer at {pos:?}; timed movement notifications require a proven shared-clock contract",
                    piston.pos
                ));
            }
        }
        let descriptor = &report.payload_groups[groups[actor]];
        let near = world.get_block(piston.head);
        let empty_near = near == Block::Air
            || matches!(near, Block::PistonHead { head } if piston.piston.extended && !head.sticky && !head.short && head.facing == piston.piston.facing);
        if descriptor.members.len() != 1
            || !empty_near
            || descriptor
                .positions
                .iter()
                .any(|&pos| super::outputs::supported_payload(world.get_block(pos)))
        {
            return Err(format!("ordinary sampling generator at {:?} must have an empty stationary head and no movable payload", piston.pos));
        }
        if !coupled[actor] {
            return Err(format!(
                "ordinary sampling generator at {:?} has no independently coupled control update",
                piston.pos
            ));
        }
    }
    let reset_actors: FxHashSet<_> = report.pistons.iter().enumerate().filter_map(|(actor, piston)| {
        resets.iter().any(|&pos| matches!(world.get_block(pos), Block::Observer { observer } if pos.offset(observer.facing.into()) == piston.pos)).then_some(actor)
    }).collect();
    let mut memory = Vec::new();
    let mut events: Vec<SamplingEvent> = Vec::new();
    for (actor, piston) in report.pistons.iter().enumerate() {
        if !piston.piston.sticky
            || coupled[actor]
            || reset_actors.contains(&actor)
            || report.recognition[actor].is_matched()
            || clocked.is_some_and(|clock| clock.memory.iter().any(|cell| cell.actor == actor))
        {
            continue;
        }
        let mut found = false;
        for &generator in &generators {
            let source = &report.pistons[generator];
            // Empty pistons change only their base and head. A head neighbor
            // is a delivered recheck; the empty far address never moves.
            let adjacent = |target: BlockPos| {
                [source.pos, source.head].into_iter().any(|pos| {
                    let d = pos - target;
                    d.x.abs() + d.y.abs() + d.z.abs() == 1
                })
            };
            let base = adjacent(piston.pos);
            let head = adjacent(piston.head);
            if base || head {
                let target = SamplingTarget {
                    actor,
                    requires_extended: !base,
                };
                found = true;
                if let Some(event) = events.iter_mut().find(|event| matches!(event.source, SamplingSource::Generator(id) if id == generator)) {
                    event.targets.push(target);
                } else { events.push(SamplingEvent { source: SamplingSource::Generator(generator), targets: vec![target] }); }
            }
        }
        let data = &report.recognition[actor].inputs;
        for update in &report.ports.pistons[actor].updates {
            if update.kind != UpdateKind::WireNotification || !update.independent_of_power {
                continue;
            }
            if !wire_inputs.contains_key(&update.source) {
                wire_inputs.insert(
                    update.source,
                    topology
                        .wire_inputs(update.source)
                        .map_err(|e| e.to_string())?,
                );
            }
            let inputs = &wire_inputs[&update.source];
            if !inputs.outside_bounds.is_empty()
                || inputs.sources.iter().any(|source| {
                    resets.contains(&source.source)
                        || matches!(source.kind, SourceKind::MobilePayload { .. })
                })
            {
                continue;
            }
            let mut writers: Vec<_> = inputs
                .sources
                .iter()
                .filter(|source| source.kind != SourceKind::Constant)
                .map(|source| source.source)
                .collect();
            writers.sort_by_key(|pos| (pos.y, pos.z, pos.x));
            writers.dedup();
            if writers.is_empty() {
                continue;
            }
            if writers.len() != 1 {
                return Err(format!("BUD at {:?} receives updates from multiple writers through {:?}; sampling needs one writer", piston.pos, update.source));
            }
            let writer = writers[0];
            if data.sources.iter().any(|source| source.source == writer) {
                return Err(format!("BUD at {:?} uses {:?} as both data and notification writer; independent sampling is required", piston.pos, writer));
            }
            found = true;
            if let Some(event) = events.iter_mut().find(|event| matches!(event.source, SamplingSource::Power { pos, .. } if pos == update.source)) {
                event.targets.push(SamplingTarget { actor, requires_extended: false });
            } else {
                let Block::RedstoneWire { wire } = world.get_block(update.source) else { unreachable!() };
                events.push(SamplingEvent { source: SamplingSource::Power { writer, pos: update.source, initial: wire.power, terms: Vec::new() }, targets: vec![SamplingTarget { actor, requires_extended: false }] });
            }
        }
        if !found {
            if let Some(update) = report.ports.pistons[actor]
                .updates
                .iter()
                .find(|update| update.kind == UpdateKind::PistonBaseChange)
            {
                let receiver = if update.requires_extended {
                    piston.head
                } else {
                    piston.pos
                };
                return Err(format!("BUD at {:?} has no independent sampling source represented by the instant runtime; piston base at {:?} can notify {:?}, but its ordered movement/reset callbacks and head eligibility need a timing certificate", piston.pos, update.source, receiver));
            }
            return Err(format!("BUD at {:?} has no independent sampling source; data changes alone cannot update memory", piston.pos));
        }
        if report.payload_groups[groups[actor]].members.len() != 1 {
            return Err(format!(
                "independently sampled BUD at {:?} shares its retained payload",
                piston.pos
            ));
        }
        memory.push(MemoryCell {
            actor,
            base: piston.pos,
            far: piston.head.offset(piston.piston.facing.into()),
            initial: !piston.piston.extended,
        });
    }
    for event in &mut events {
        event
            .targets
            .sort_by_key(|target| (target.actor, target.requires_extended));
        event.targets.dedup_by_key(|target| target.actor);
    }
    events.sort_by_key(|event| match event.source {
        SamplingSource::Generator(actor) => {
            let p = report.pistons[actor].pos;
            (p.y, p.z, p.x, 0)
        }
        SamplingSource::Power { pos, .. } => (pos.y, pos.z, pos.x, 1),
    });
    Ok(Classification {
        memory,
        events,
        generators,
    })
}

pub(crate) fn validate(
    events: &[SamplingEvent],
    logic: &WaveLogic,
    report: &AnalysisReport,
) -> Result<(), String> {
    for event in events {
        let (origin, roots) = match &event.source {
            SamplingSource::Generator(actor) => {
                (report.pistons[*actor].pos, vec![logic.responses[*actor]])
            }
            SamplingSource::Power { pos, terms, .. } => {
                (*pos, terms.iter().map(|term| term.guard).collect())
            }
        };
        let mut pending = roots;
        let mut seen = FxHashSet::default();
        while let Some(root) = pending.pop() {
            if !seen.insert(root) {
                continue;
            }
            if let Some(decision) = logic.arena.decision(root) {
                match decision.variable {
                    Variable::Memory(_) => return Err(format!("sampling control at {origin:?} depends on stored memory; this feedback is unsupported")),
                    Variable::Actuator(actor) => pending.push(logic.responses[actor]),
                    Variable::Signal { .. } => {},
                    _ => return Err(format!("sampling control at {origin:?} retains unsupported physical state")),
                }
                pending.extend([decision.low, decision.high]);
            }
        }
    }
    Ok(())
}
