//! Executable admission for a synchronous, acyclic response network.
//! A conditional conductor can participate internally; an ordinary consumer
//! currently needs a far redstone supply with a private observer reset.
use super::boundary::Boundaries;
use super::logic::{self, WaveLogic};
use crate::redpiler::analysis::{AdmissionIssue, AnalysisReport};
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
    pub clocked: Option<super::clocked::ClockedProgram>,
    pub controls: Vec<BlockPos>,
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
    if options.export {
        return Err("instant runtime export is not implemented".into());
    }
    let width = crate::plot::PLOT_BLOCK_WIDTH;
    if report.bounds.0.x.div_euclid(width) != report.bounds.1.x.div_euclid(width)
        || report.bounds.0.z.div_euclid(width) != report.bounds.1.z.div_euclid(width)
    {
        return Err("instant execution requires a selection within one plot".into());
    }
    for issue in &report.issues {
        if !matches!(
            issue,
            AdmissionIssue::PistonRuntimeUnavailable { .. }
                | AdmissionIssue::ObserverRuntimeUnavailable { .. }
        ) {
            return Err(issue.to_string());
        }
    }
    let clocked = super::clocked::recognize(world, report, &monitor)?;
    let is_clock = |id| clocked.as_ref().is_some_and(|c| c.clock == id);
    let is_memory = |id| {
        clocked
            .as_ref()
            .is_some_and(|c| c.memory.iter().any(|m| m.actor == id))
    };
    let mut reset_owners = FxHashSet::default();
    let mut observer_pistons = FxHashSet::default();
    let mut owned = FxHashSet::default();
    for (id, p) in report.pistons.iter().enumerate() {
        if (!p.piston.sticky && !is_clock(id))
            || !p.piston.extended
            || !p.powered
            || p.piston.facing == BlockFacing::Up
        {
            return Err(format!(
                "piston at {:?} needs a ready, powered, extended sticky mechanism",
                p.pos
            ));
        }
        let payload = world.get_block(p.payload);
        if !matches!(payload, Block::RedstoneBlock | Block::Wool { .. }) && !is_clock(id) {
            return Err(format!("unsupported payload minecraft:{} at {:?}, owned by piston {:?}; expected a redstone block or wool",payload.get_name(),p.payload,p.pos));
        }
        if !matches!(world.get_block(p.head), Block::PistonHead { head } if head.sticky==p.piston.sticky && head.facing == p.piston.facing && !head.short)
            || [p.pos, p.head, p.payload]
                .iter()
                .any(|&pos| world.get_block_entity(pos).is_some())
        {
            return Err(format!(
                "piston at {:?} has an invalid head or payload entity",
                p.pos
            ));
        }
        owned.extend([p.pos, p.head, p.payload]);
        for alias in [p.head, p.payload] {
            let dust = alias.offset(BlockFace::Top);
            if matches!(world.get_block(dust), Block::RedstoneWire { .. }) {
                return Err(format!("dust at {dust:?} uses moving payload support at {alias:?}; destructive wire updates need another protocol"));
            }
        }
        let ports = &report.ports.pistons[id];
        if let Some(pos) = ports.outside_bounds.first() {
            return Err(format!(
                "piston at {:?} needs update context outside the selection at {pos:?}",
                p.pos
            ));
        }
        let inputs = &report.recognition[id].inputs;
        let power_notification = ports.updates.iter().any(|u| {
            u.source != p.head
                && (inputs.wires.contains(&u.source)
                    || matches!(
                        u.kind,
                        crate::redpiler::analysis::ports::UpdateKind::AdjacentHeadChange
                    ))
        });
        let adjacent_source = inputs.sources.iter().any(|s| {
            let d = s.source - p.pos;
            d.x.abs() + d.y.abs() + d.z.abs() == 1
        });
        if !power_notification && !adjacent_source && !is_memory(id) {
            return Err(format!("piston at {:?} needs a qualifying update coupled to its power input; independent BUD sampling is not implemented",p.pos));
        }
        let observer_pos = p.pos.offset(BlockFace::Top);
        if let Block::Observer { observer } = world.get_block(observer_pos) {
            let cap = observer_pos.offset(BlockFace::Top);
            if observer.facing != BlockFacing::Down
                || observer.powered
                || !world.get_block(cap).is_solid()
                || report
                    .payload_groups
                    .iter()
                    .any(|g| g.positions.contains(&cap))
            {
                return Err(format!(
                    "piston at {:?} has an unsupported observer reset",
                    p.pos
                ));
            }
            if !report.recognition[id].resets.iter().any(|r| {
                r.family == crate::redpiler::analysis::families::ResetFamily::ObserverAbove
                    && r.source == observer_pos
            }) {
                return Err(format!(
                    "piston at {:?} has an unverified observer return path: {:?}",
                    p.pos, report.recognition[id].failures
                ));
            }
            if report.recognition[id].resets.iter().any(|r| {
                r.family != crate::redpiler::analysis::families::ResetFamily::ObserverAbove
            }) {
                return Err(format!(
                    "piston at {:?} has multiple reset families and needs a joint reset protocol",
                    p.pos
                ));
            }
            reset_owners.insert(observer_pos);
            observer_pistons.insert(id);
            owned.insert(observer_pos);
        }
    }
    if let Some(clocked) = &clocked {
        reset_owners.extend(clocked.observers.iter().copied());
        owned.extend(clocked.observers.iter().copied());
    }
    if let Some(pos) = report.observers.iter().find(|p| !reset_owners.contains(p)) {
        return Err(format!("observer at {pos:?} has no supported region owner"));
    }
    if ticks.iter().any(|t| owned.contains(&t.pos)) {
        return Err("instant entry contains pending reset or movement work".into());
    }
    let logic = if let Some(clocked) = &clocked {
        logic::extract_with_state(
            world,
            report,
            &monitor,
            clocked.memory.iter().map(|m| m.actor).collect(),
            Some(clocked.clock),
        )?
    } else {
        logic::extract(world, report, &monitor)?
    };
    if let Some(clocked) = &clocked {
        clocked.validate(world, &logic)?;
    }
    for (id, p) in report.pistons.iter().enumerate() {
        if clocked.is_none() && !observer_pistons.contains(&id) && !logic.follows_payload[id] {
            return Err(format!("piston at {:?} has neither an observer reset nor a proven payload-following response",p.pos));
        }
    }
    if let Some(exposure) = report
        .ports
        .reset_exposures
        .iter()
        .find(|e| !report.pistons.iter().any(|p| p.pos == e.consumer))
    {
        return Err(format!(
            "reset signal at {:?} is visible to ordinary consumer at {:?}",
            exposure.source, exposure.consumer
        ));
    }
    if logic
        .evaluate(|pos| crate::redstone::source_strength(world.get_block(pos), world, pos))
        .iter()
        .any(|&f| f)
    {
        return Err("instant network is not in its ready electrical state".into());
    }
    let mut aliases = Vec::new();
    for (id, group) in report.payload_groups.iter().enumerate() {
        // The response removes a shared far payload if any owner fires. Near
        // ownership is deliberately hidden and is restored by physical replay.
        let p = &report.pistons[group.members[0]];
        for &alias in &group.positions {
            aliases.push((
                id,
                alias,
                alias == p.payload && world.get_block(alias) == Block::RedstoneBlock,
            ));
        }
    }
    for (consumer, positions) in &logic.consumers {
        for &pos in positions {
            let owners: Vec<_> = report
                .pistons
                .iter()
                .enumerate()
                .filter(|(_, p)| p.payload == pos)
                .map(|(id, _)| id)
                .collect();
            if world.get_block(pos) != Block::RedstoneBlock
                || owners.is_empty()
                || owners.iter().any(|id| {
                    !observer_pistons.contains(id)
                        && !clocked
                            .as_ref()
                            .is_some_and(|c| c.observed_outputs.contains(id))
                })
            {
                return Err(format!("ordinary consumer at {consumer:?} sees moving conductor, near payload or unverified reset context at {pos:?}"));
            }
        }
    }
    owned.extend(logic.wires.iter().copied());
    owned.extend(logic.consumer_wires.iter().copied());
    owned.extend(crate::redpiler::analysis::families::reset_internals(
        &report.recognition,
    ));
    let boundaries = Boundaries::executable(report, &logic.wires, &logic.sources);
    let input = CompilerInput {
        world,
        bounds: report.bounds,
        ticks,
        boundaries: Some(&boundaries),
    };
    let graph = crate::redpiler::passes::make_default_pass_manager()
        .run_passes(options, &input, monitor.clone())
        .map_err(|e| e.to_string())?;
    // Retain interaction provenance across ordinary timed input stages. The
    // program may read a torch while the player actually changes its lever.
    let mut visited = FxHashSet::default();
    let mut pending: Vec<_> = graph
        .node_indices()
        .filter(|&id| {
            graph[id]
                .block
                .is_some_and(|(pos, _)| logic.sources.contains(&pos))
        })
        .collect();
    let mut controls = Vec::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        if let Some((pos, _)) = graph[id].block {
            if matches!(
                graph[id].ty,
                crate::redpiler::compile_graph::NodeType::Lever
                    | crate::redpiler::compile_graph::NodeType::Button
                    | crate::redpiler::compile_graph::NodeType::PressurePlate
            ) {
                controls.push(pos);
            }
        }
        pending.extend(graph.neighbors_directed(id, petgraph::Direction::Incoming));
    }
    controls.sort_by_key(|p| (p.y, p.z, p.x));
    let mut template = Vec::new();
    let (first, last) = logic
        .context
        .iter()
        .chain(owned.iter())
        .fold((report.bounds.1, report.bounds.0), |(a, b), &p| {
            (a.min(p), b.max(p))
        });
    // Settled near payloads can energize reset paths which never conduct in
    // the first response. Include their electrical neighborhood for handoff.
    let first = (first - BlockPos::new(16, 16, 16)).max(report.bounds.0);
    let last = (last + BlockPos::new(16, 16, 16)).min(report.bounds.1);
    for_each_block_optimized(world, first, last, |pos| {
        let block = world.get_block(pos);
        if block != Block::Air {
            template.push((pos, block, world.get_block_entity(pos).cloned()));
        }
    });
    if monitor.cancelled() {
        return Err("instant compilation cancelled".into());
    }
    Ok((
        graph,
        PreparedInstant {
            clocked,
            controls,
            logic,
            groups: report
                .payload_groups
                .iter()
                .map(|g| g.members.clone())
                .collect(),
            aliases,
            owned,
            template,
            bounds: report.bounds,
            logical_tick: world.piston_state().logical_tick,
        },
    ))
}
