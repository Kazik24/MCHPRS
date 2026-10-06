//! Executable admission for a synchronous, acyclic response network.
//! A conditional conductor can participate internally; an ordinary consumer
//! currently needs a far redstone supply with a private observer reset.
use super::boundary::Boundaries;
use super::logic::{self, WaveLogic};
use crate::redpiler::analysis::{AnalysisReport, AdmissionIssue};
use crate::redpiler::compile_graph::CompileGraph;
use crate::redpiler::{CompilerInput, CompilerOptions, TaskMonitor};
use crate::world::{for_each_block_optimized, World};
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::Block;
use mchprs_blocks::{BlockFace, BlockFacing, BlockPos};
use mchprs_world::TickEntry;
use rustc_hash::FxHashSet;
use std::sync::Arc;

pub(crate) struct PreparedInstant {
    pub logic: WaveLogic,
    pub groups: Vec<Vec<usize>>,
    pub aliases: Vec<(usize, BlockPos, bool)>,
    pub owned: FxHashSet<BlockPos>,
    pub template: Vec<(BlockPos, Block, Option<BlockEntity>)>,
    pub bounds: (BlockPos, BlockPos),
    pub logical_tick: u64,
}

pub(crate) fn prepare(
    world: &impl World,
    report: &AnalysisReport,
    ticks: &[TickEntry],
    options: &CompilerOptions,
    monitor: Arc<TaskMonitor>,
) -> Result<(CompileGraph, PreparedInstant), String> {
    if options.export { return Err("instant runtime export is not implemented".into()); }
    for issue in &report.issues {
        if !matches!(issue, AdmissionIssue::PistonRuntimeUnavailable { .. } | AdmissionIssue::ObserverRuntimeUnavailable { .. }) {
            return Err(issue.to_string());
        }
    }
    let mut reset_owners = FxHashSet::default();
    let mut observer_pistons = FxHashSet::default();
    let mut owned = FxHashSet::default();
    for (id, p) in report.pistons.iter().enumerate() {
        if !p.piston.sticky || !p.piston.extended || !p.powered || p.piston.facing == BlockFacing::Up {
            return Err(format!("piston at {:?} needs a ready, powered, extended sticky mechanism", p.pos));
        }
        if !matches!(world.get_block(p.head), Block::PistonHead { head } if head.sticky && head.facing == p.piston.facing)
            || world.get_block_entity(p.payload).is_some() {
            return Err(format!("piston at {:?} has an invalid head or payload entity", p.pos));
        }
        owned.extend([p.pos, p.head, p.payload]);
        let observer_pos = p.pos.offset(BlockFace::Top);
        if let Block::Observer { observer } = world.get_block(observer_pos) {
            let cap = observer_pos.offset(BlockFace::Top);
            if observer.facing != BlockFacing::Down || observer.powered || !world.get_block(cap).is_solid()
                || report.payload_groups.iter().any(|g| g.positions.contains(&cap)) {
                return Err(format!("piston at {:?} has an unsupported observer reset", p.pos));
            }
            if report.recognition[id].failures.iter().any(|f| !matches!(f, crate::redpiler::analysis::families::RecognitionFailure::UnsupportedPayload { .. })) {
                return Err(format!("piston at {:?} has an unverified observer return path: {:?}",p.pos, report.recognition[id].failures));
            }
            reset_owners.insert(observer_pos);
            observer_pistons.insert(id);
            owned.insert(observer_pos);
        }
    }
    if let Some(pos) = report.observers.iter().find(|p| !reset_owners.contains(p)) {
        return Err(format!("observer at {pos:?} has no supported region owner"));
    }
    if ticks.iter().any(|t| owned.contains(&t.pos)) {
        return Err("instant entry contains pending reset or movement work".into());
    }
    let logic = logic::extract(world, report, &monitor)?;
    if logic.evaluate(|pos| crate::redstone::source_strength(world.get_block(pos), world, pos)).iter().any(|&f| f) {
        return Err("instant network is not in its ready electrical state".into());
    }
    let mut aliases = Vec::new();
    for (id, group) in report.payload_groups.iter().enumerate() {
        // A shared moving payload needs an ownership and reset protocol rather
        // than arbitrarily choosing which near alias supplies ordinary logic.
        if group.members.len() != 1 { return Err(format!("shared payload group {id} needs an ownership runtime")); }
        let p = &report.pistons[group.members[0]];
        for &alias in &group.positions {
            aliases.push((id, alias, alias == p.payload && world.get_block(alias) == Block::RedstoneBlock));
        }
    }
    for (consumer, positions) in &logic.consumers {
        for &pos in positions {
            let owner = report.pistons.iter().position(|p| p.payload == pos);
            if world.get_block(pos) != Block::RedstoneBlock || owner.is_none_or(|id| !observer_pistons.contains(&id)) {
                return Err(format!("ordinary consumer at {consumer:?} sees moving conductor, near payload or unverified reset context at {pos:?}"));
            }
        }
    }
    owned.extend(logic.wires.iter().copied());
    let boundaries = Boundaries::executable(report, &logic.wires, &logic.sources);
    let input = CompilerInput { world, bounds: report.bounds, ticks, boundaries: Some(&boundaries) };
    let graph = crate::redpiler::passes::make_default_pass_manager().run_passes(options, &input, monitor.clone()).map_err(|e| e.to_string())?;
    let mut template = Vec::new();
    for_each_block_optimized(world, report.bounds.0, report.bounds.1, |pos| {
        let block = world.get_block(pos);
        if block != Block::Air { template.push((pos, block, world.get_block_entity(pos).cloned())); }
    });
    if monitor.cancelled() { return Err("instant compilation cancelled".into()); }
    Ok((graph, PreparedInstant {
        logic,
        groups: report.payload_groups.iter().map(|g|g.members.clone()).collect(),
        aliases, owned, template, bounds: report.bounds, logical_tick: world.piston_state().logical_tick,
    }))
}
