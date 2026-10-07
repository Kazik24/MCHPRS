//! Executable admission for a synchronous, acyclic response network.
//! A conditional conductor can participate internally; an ordinary consumer
//! receives a compiled electrical port derived from conditional geometry.
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
    pub sequential: Option<super::sequential::PreparedSequential>,
    pub assume_instant: bool,
    pub pistons: Vec<crate::redpiler::analysis::PistonDescriptor>,
    pub output_offset: usize,
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
) -> Result<(CompileGraph, Vec<PreparedInstant>), String> {
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
    let mut programs = Vec::new();
    for region in super::regions::split(world, report, &monitor)? {
        programs.push(prepare_region(
            world,
            &region,
            ticks,
            options,
            monitor.clone(),
        )?);
    }
    let wires = programs
        .iter()
        .flat_map(|p| p.logic.wires.iter().copied())
        .collect();
    let sources: Vec<_> = programs
        .iter()
        .flat_map(|p| p.logic.sources.iter().copied())
        .collect();
    let mut outputs = Vec::new();
    for program in &mut programs {
        program.output_offset = outputs.len();
        outputs.extend(program.logic.outputs.iter().cloned());
    }
    let mut boundaries = Boundaries::executable(report, &wires, &sources, &outputs);
    for program in &programs {
        if let Some(sequential) = &program.sequential {
            boundaries.retain_sequential_sources(program.logic.sources.iter().copied()
                .chain(program.logic.outputs.iter().map(|output| output.consumer)));
            boundaries.own_sampled_wires(sequential.sensors.iter().map(|sensor| sensor.pos));
        }
    }
    let input = CompilerInput {
        world,
        bounds: report.bounds,
        ticks,
        boundaries: Some(&boundaries),
    };
    let graph = crate::redpiler::passes::run_passes(options, &input, &monitor)
        .map_err(|e| e.to_string())?;
    for program in &mut programs {
        // Keep player controls upstream of ordinary timed input stages for handoff.
        let mut visited = FxHashSet::default();
        let mut pending: Vec<_> = graph
            .node_indices()
            .filter(|&id| {
                graph[id]
                    .block
                    .is_some_and(|(pos, _)| program.logic.sources.contains(&pos))
            })
            .collect();
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
                    program.controls.push(pos);
                }
            }
            pending.extend(graph.neighbors_directed(id, petgraph::Direction::Incoming));
        }
        program.controls.sort_by_key(|p| (p.y, p.z, p.x));
    }
    Ok((graph, programs))
}

fn prepare_region(
    world: &impl World,
    report: &AnalysisReport,
    ticks: &[TickEntry],
    options: &CompilerOptions,
    monitor: Arc<TaskMonitor>,
) -> Result<PreparedInstant, String> {
    let generators: Vec<_> = report.pistons.iter().filter(|p| !p.piston.sticky).collect();
    if generators.len() > 1 || generators.iter().any(|p| p.piston.facing != BlockFacing::Down)
        || report.pistons.iter().any(|p| !p.piston.extended)
        || report.pistons.iter().any(|p| p.diagnostics.contains(&crate::redpiler::analysis::PistonDiagnostic::MissingOrMismatchedHead))
        || report.recognition.iter().any(|r| r.failures.iter().any(|failure| matches!(failure,
            crate::redpiler::analysis::families::RecognitionFailure::AdditionalResetWriter { .. })))
    {
        return super::sequential::prepare(world, report, ticks, options, &monitor);
    }
    let clocked = super::clocked::recognize(world, report, &monitor, options.assume_instant)?;
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
            || (!options.assume_instant && !p.powered)
            || p.piston.facing == BlockFacing::Up
        {
            return Err(format!(
                "piston at {:?} needs a ready, powered, extended sticky mechanism",
                p.pos
            ));
        }
        let payload = world.get_block(p.payload);
        if !super::outputs::supported_payload(payload) && !is_clock(id) {
            return Err(format!("unsupported payload minecraft:{} at {:?}, owned by piston {:?}; expected a redstone block or supported fixed conductor",payload.get_name(),p.payload,p.pos));
        }
        if !matches!(world.get_block(p.head), Block::PistonHead { head } if head.sticky==p.piston.sticky && head.facing == p.piston.facing && !head.short)
        {
            return Err(format!(
                "extended piston at {:?} needs a matching stationary head at {:?}; found minecraft:{}",
                p.pos, p.head, world.get_block(p.head).get_name()
            ));
        }
        if let Some(pos) = [p.pos, p.head, p.payload]
            .into_iter()
            .find(|&pos| world.get_block_entity(pos).is_some())
        {
            return Err(format!(
                "piston at {:?} has an unsupported moving-context entity at {pos:?}",
                p.pos
            ));
        }
        owned.extend([p.pos, p.head, p.payload]);
        for alias in [p.head, p.payload] {
            for face in BlockFace::values() {
                let pos = alias.offset(face);
                let block = world.get_block(pos);
                let electrical = crate::redpiler::analysis::ports::is_consumer(block)
                    || matches!(
                        block,
                        Block::RedstoneWire { .. }
                            | Block::Lever { .. }
                            | Block::StoneButton { .. }
                    )
                    || block.pressure_plate_powered().is_some();
                if !world.is_cursed()
                    && electrical
                    && crate::interaction::attachment_support(block, pos)
                        .is_some_and(|(support, _)| support == alias)
                {
                    return Err(format!("attachment at {pos:?} uses moving payload support at {alias:?}; destructive support updates need another protocol"));
                }
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
            if !options.assume_instant
                && (observer.facing != BlockFacing::Down
                    || observer.powered
                    || !world.get_block(cap).is_solid()
                    || report
                        .payload_groups
                        .iter()
                        .any(|g| g.positions.contains(&cap)))
            {
                return Err(format!(
                    "piston at {:?} has an unsupported observer reset",
                    p.pos
                ));
            }
            if !options.assume_instant
                && !report.recognition[id].resets.iter().any(|r| {
                    r.family == crate::redpiler::analysis::families::ResetFamily::ObserverAbove
                        && r.source == observer_pos
                })
            {
                return Err(format!(
                    "piston at {:?} has an unverified observer return path: {:?}",
                    p.pos, report.recognition[id].failures
                ));
            }
            if !options.assume_instant
                && report.recognition[id].resets.iter().any(|r| {
                    r.family != crate::redpiler::analysis::families::ResetFamily::ObserverAbove
                })
            {
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
        if !options.assume_instant
            && clocked.is_none()
            && !observer_pistons.contains(&id)
            && !logic.follows_payload[id]
        {
            return Err(format!("piston at {:?} has neither an observer reset nor a proven payload-following response",p.pos));
        }
    }
    if let Some(exposure) =
        report.ports.reset_exposures.iter().find(|e| {
            !options.assume_instant && !report.pistons.iter().any(|p| p.pos == e.consumer)
        })
    {
        return Err(format!(
            "reset signal at {:?} is visible to ordinary consumer at {:?}",
            exposure.source, exposure.consumer
        ));
    }
    if !options.assume_instant
        && logic
            .evaluate(|pos| crate::redstone::source_strength(world.get_block(pos), world, pos))
            .iter()
            .any(|&f| f)
    {
        return Err("instant network is not in its ready electrical state".into());
    }
    let mut aliases = Vec::new();
    for (id, group) in report.payload_groups.iter().enumerate() {
        // A shared far payload disappears if any owner fires. Physical mode
        // hides near ownership; logical mode selects the first firing owner.
        let p = &report.pistons[group.members[0]];
        for &alias in &group.positions {
            aliases.push((
                id,
                alias,
                alias == p.payload && world.get_block(alias) == Block::RedstoneBlock,
            ));
        }
    }
    let mut shared = vec![false; report.pistons.len()];
    for group in &report.payload_groups {
        for &actor in &group.members {
            shared[actor] = group.members.len() > 1;
        }
    }
    for output in &logic.outputs {
        let mut pending: Vec<_> = output.terms.iter().map(|term| term.guard).collect();
        let mut seen = FxHashSet::default();
        while let Some(root) = pending.pop() {
            if monitor.cancelled() {
                return Err("instant compilation cancelled".into());
            }
            if !seen.insert(root) {
                continue;
            }
            if let Some(decision) = logic.arena.decision(root) {
                if let super::boolean::Variable::Geometry {
                    actor,
                    part: super::boolean::GeometryPart::NearPayload,
                } = decision.variable
                {
                    if shared[actor] && !options.assume_instant {
                        return Err(format!("consumer at {:?} observes near ownership of a shared payload; only shared far occupancy has a compiled protocol", output.consumer));
                    }
                }
                pending.extend([decision.low, decision.high]);
            }
        }
    }
    owned.extend(logic.wires.iter().copied());
    owned.extend(logic.consumer_wires.iter().copied());
    owned.extend(crate::redpiler::analysis::families::reset_internals(
        &report.recognition,
    ));
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
    Ok(PreparedInstant {
        sequential: None,
        assume_instant: options.assume_instant,
        pistons: report.pistons.clone(),
        output_offset: 0,
        clocked,
        controls: Vec::new(),
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
    })
}
