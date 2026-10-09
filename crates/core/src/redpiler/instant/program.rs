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
use petgraph::unionfind::UnionFind;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;

pub(crate) struct PreparedInstant {
    pub pistons: Vec<crate::redpiler::analysis::PistonDescriptor>,
    pub output_offset: usize,
    pub clocked: Option<super::clocked::ClockedProgram>,
    pub independent_memory: Vec<super::clocked::MemoryCell>,
    pub sampling: Vec<super::sampling::SamplingEvent>,
    pub reset_groups: Vec<super::observer::ResetGroup>,
    pub payloads: Vec<Block>,
    pub controls: Vec<BlockPos>,
    pub logic: WaveLogic,
    pub groups: Vec<Vec<usize>>,
    pub aliases: Vec<(usize, BlockPos, bool)>,
    pub owned: FxHashSet<BlockPos>,
    /// Ordinary dust surrounding output channels and external sampling writers.
    pub propagation_wires: FxHashSet<BlockPos>,
    /// Dust positions whose electrical or callback behavior crosses the assembly boundary.
    pub observable_wires: FxHashSet<BlockPos>,
    pub sampling_wires: FxHashSet<BlockPos>,
    pub candidate_wire_count: usize,
    pub wire_retention_reasons: [usize; 7],
    pub template: Vec<(BlockPos, Block, Option<BlockEntity>)>,
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
    let admission_error = |error: String| {
        if monitor.cancelled() {
            error
        } else {
            format!("logical piston admission failed: {error}")
        }
    };
    let mut programs = Vec::new();
    for region in super::regions::split(world, report, &monitor).map_err(&admission_error)? {
        programs.push(
            prepare_region(world, &region, ticks, options, monitor.clone()).map_err(|error| {
                admission_error(format!("assembly at {:?}: {error}", region.pistons[0].pos))
            })?,
        );
    }
    let classification = classify_boundary_wires(world, report, &programs);
    if let Some(program) = programs.first_mut() {
        program.wire_retention_reasons = classification.reason_counts;
    }
    for program in &mut programs {
        program.candidate_wire_count = program.logic.wires.len();
        program
            .propagation_wires
            .retain(|pos| classification.retained.contains(pos));
        program
            .owned
            .retain(|pos| !program.propagation_wires.contains(pos));
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
    let owned = programs
        .iter()
        .flat_map(|program| program.owned.iter().copied())
        .collect();
    let propagation_wires = programs
        .iter()
        .flat_map(|program| program.propagation_wires.iter().copied())
        .collect();
    let empty_far = programs
        .iter()
        .flat_map(|program| {
            program
                .groups
                .iter()
                .enumerate()
                .filter_map(|(group, actors)| {
                    (program.payloads[group] == Block::Air)
                        .then(|| program.pistons[actors[0]].payload)
                })
        })
        .filter(|pos| world.get_block(*pos) == Block::Air)
        .filter(|pos| !report.pistons.iter().any(|piston| piston.head == *pos))
        .collect();
    let mut boundaries = Boundaries::executable(
        report,
        &wires,
        &sources,
        &outputs,
        &owned,
        &empty_far,
        &propagation_wires,
    )?;
    for event in programs.iter().flat_map(|program| &program.sampling) {
        if let super::sampling::SamplingSource::Power { writer, pos, .. } = event.source {
            boundaries.require_callbacks(writer);
            boundaries.require_callbacks(pos);
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
        {
            let mut internally_driven = FxHashSet::default();
            let mut visited = FxHashSet::default();
            let ports = program.output_offset..program.output_offset + program.logic.outputs.len();
            let mut pending: Vec<_> = graph.node_indices().filter(|&id| {
                matches!(graph[id].ty, crate::redpiler::compile_graph::NodeType::InstantOutput { port } if ports.contains(&port))
            }).collect();
            while let Some(id) = pending.pop() {
                if !visited.insert(id) {
                    continue;
                }
                if let Some((pos, _)) = graph[id].block {
                    internally_driven.insert(pos);
                }
                pending.extend(graph.neighbors_directed(id, petgraph::Direction::Outgoing));
            }
            let targets = program
                .pistons
                .iter()
                .enumerate()
                .filter_map(|(actor, piston)| {
                    (!program
                        .independent_memory
                        .iter()
                        .any(|cell| cell.actor == actor)
                        && !program.clocked.as_ref().is_some_and(|clock| {
                            clock.clock == actor
                                || clock.memory.iter().any(|cell| cell.actor == actor)
                        }))
                    .then_some(piston.pos)
                })
                .collect();
            super::sampling::validate_feedback(
                world,
                report,
                &monitor,
                &targets,
                &internally_driven,
            )
            .map_err(&admission_error)?;
        }
        // Keep player controls upstream of ordinary timed input stages for handoff.
        let sources: FxHashSet<_> = program.logic.sources.iter().copied().collect();
        let mut visited = FxHashSet::default();
        let mut pending: Vec<_> = graph
            .node_indices()
            .filter(|&id| {
                graph[id]
                    .block
                    .is_some_and(|(pos, _)| sources.contains(&pos))
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
    prepare_region_inner(world, report, ticks, options, monitor, true)
}

fn prepare_region_inner(
    world: &impl World,
    report: &AnalysisReport,
    ticks: &[TickEntry],
    options: &CompilerOptions,
    monitor: Arc<TaskMonitor>,
    try_shared_clock: bool,
) -> Result<PreparedInstant, String> {
    if monitor.cancelled() {
        return Err("instant compilation cancelled".into());
    }
    // Report unsupported material before a generic role/group diagnostic.
    for group in &report.payload_groups {
        let owner = &report.pistons[group.members[0]];
        for &pos in &group.positions {
            let block = world.get_block(pos);
            let saved_head = group
                .members
                .iter()
                .any(|&actor| report.pistons[actor].head == pos)
                && matches!(block, Block::PistonHead { .. });
            let empty_base = group
                .members
                .iter()
                .all(|&actor| !report.pistons[actor].piston.sticky)
                && report.pistons.iter().any(|p| p.pos == pos)
                && matches!(block, Block::Piston { .. });
            if block != Block::Air
                && !super::outputs::supported_payload(block)
                && !saved_head
                && !empty_base
            {
                return Err(format!("unsupported payload minecraft:{} at {pos:?}, owned by piston {:?}; expected a redstone block or supported fixed conductor", block.get_name(), owner.pos));
            }
        }
    }
    let clocked = if try_shared_clock {
        super::clocked::recognize(world, report, &monitor, options.assume_instant)?
    } else {
        None
    };
    let candidates = super::sampling::reset_candidates(world, report);
    let mut independent =
        super::sampling::recognize(world, report, &monitor, clocked.as_ref(), &candidates)?;
    let is_clock = |id| clocked.as_ref().is_some_and(|c| c.clock == id);
    let is_fixed = |id| independent.fixed.contains(&id);
    let is_generator = |id| is_clock(id) || independent.generators.contains(&id);
    let is_memory = |id| {
        clocked
            .as_ref()
            .is_some_and(|c| c.memory.iter().any(|m| m.actor == id))
            || independent.memory.iter().any(|cell| cell.actor == id)
    };
    let mut actor_groups = vec![0; report.pistons.len()];
    let mut logical_payloads = vec![Block::Air; report.payload_groups.len()];
    let mut retained_owners = vec![None; report.payload_groups.len()];
    for (group, descriptor) in report.payload_groups.iter().enumerate() {
        for &actor in &descriptor.members {
            actor_groups[actor] = group;
        }
        let first = &report.pistons[descriptor.members[0]];
        let far = first.head.offset(first.piston.facing.into());
        for &pos in &descriptor.positions {
            let block = world.get_block(pos);
            let head = descriptor
                .members
                .iter()
                .any(|&actor| report.pistons[actor].head == pos)
                && matches!(block, Block::PistonHead { .. });
            let stationary_base = descriptor.members.iter().all(|&actor| is_generator(actor))
                && report.pistons.iter().any(|p| p.pos == pos)
                && matches!(block, Block::Piston { .. });
            if block != Block::Air
                && !super::outputs::supported_payload(block)
                && !head
                && !stationary_base
            {
                return Err(format!("unsupported payload minecraft:{} at {pos:?}, owned by piston {:?}; expected a redstone block or supported fixed conductor", block.get_name(), first.pos));
            }
        }
        let material: Vec<_> = descriptor
            .positions
            .iter()
            .filter_map(|&pos| {
                let block = world.get_block(pos);
                super::outputs::supported_payload(block).then_some((pos, block))
            })
            .collect();
        if descriptor.members.len() == 1
            && is_generator(descriptor.members[0])
            && material.is_empty()
        {
            continue;
        }
        if material.len() != 1
            || descriptor.members.iter().any(|&actor| {
                let piston = &report.pistons[actor];
                piston.head.offset(piston.piston.facing.into()) != far
            })
        {
            return Err(format!("pistons sharing a payload at {:?} need exactly one supported block and the same destination", first.pos));
        }
        let owner = descriptor
            .members
            .iter()
            .copied()
            .find(|&actor| !report.pistons[actor].piston.extended);
        if material[0].0 != owner.map_or(far, |actor| report.pistons[actor].head) {
            return Err(format!(
                "shared payload at {:?} is not in its expected retracted or extended position",
                first.pos
            ));
        }
        retained_owners[group] = owner;
        logical_payloads[group] = material[0].1;
    }
    let mut reset_owners = FxHashSet::default();
    let mut observer_pistons = FxHashSet::default();
    let mut owned = FxHashSet::default();
    for (id, p) in report.pistons.iter().enumerate() {
        let retained = !p.piston.extended && (p.piston.sticky || is_generator(id));
        let far = p.head.offset(p.piston.facing.into());
        if (!p.piston.sticky && !is_generator(id))
            || (!p.piston.extended && !retained)
            || (!options.assume_instant
                && p.piston.facing == BlockFacing::Up
                && !is_memory(id)
                && !is_fixed(id)
                && !is_generator(id))
        {
            return Err(format!("piston at {:?} has unsupported geometry; expected a settled sticky piston or sampling generator", p.pos));
        }
        let payload = logical_payloads[actor_groups[id]];
        if !super::outputs::supported_payload(payload) && !is_generator(id) {
            return Err(format!("unsupported payload minecraft:{} at {:?}, owned by piston {:?}; expected a redstone block or supported fixed conductor",payload.get_name(),p.payload,p.pos));
        }
        // Shared OR groups can retract several actors while one deterministic
        // first owner holds their single payload; the other near cells are air.
        let near_payload = if retained_owners[actor_groups[id]] == Some(id) {
            payload
        } else {
            Block::Air
        };
        if retained
            && (world.get_block(p.head) != near_payload
                || (!is_generator(id) && world.get_block(far) != Block::Air))
        {
            return Err(format!("retracted logical piston at {:?} needs minecraft:{} at {:?} and an empty far cell at {far:?}", p.pos, near_payload.get_name(), p.head));
        }
        if !retained
            && !matches!(world.get_block(p.head), Block::PistonHead { head } if head.sticky==p.piston.sticky && head.facing == p.piston.facing && !head.short)
        {
            return Err(format!(
                "extended piston at {:?} needs a matching stationary head at {:?}; found minecraft:{}",
                p.pos, p.head, world.get_block(p.head).get_name()
            ));
        }
        let moving_positions: Vec<_> = if is_generator(id) {
            vec![p.pos, p.head]
        } else {
            vec![p.pos, p.head, far]
        };
        if let Some(pos) = moving_positions
            .iter()
            .copied()
            .find(|&pos| world.get_block_entity(pos).is_some())
        {
            return Err(format!(
                "piston at {:?} has an unsupported block entity in its movement area at {pos:?}",
                p.pos
            ));
        }
        owned.extend(moving_positions);
        for alias in [Some(p.head), (!is_generator(id)).then_some(far)]
            .into_iter()
            .flatten()
        {
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
                    return Err(format!("attachment at {pos:?} uses moving payload support at {alias:?}; breaking attached blocks is unsupported"));
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
            !matches!(s.kind, crate::redpiler::analysis::topology::SourceKind::MobilePayload { group } if group == actor_groups[id])
                && super::sampling::data_notifies(world, s.source, p.pos)
        });
        if !power_notification && !adjacent_source && !is_memory(id) && !is_fixed(id) && !candidates.iter().any(|&pos| matches!(world.get_block(pos), Block::Observer { observer } if pos.offset(observer.facing.into()) == p.pos)) {
            return Err(format!("piston at {:?} has no update coupled to its power input or independent sampling source",p.pos));
        }
    }
    if let Some(clocked) = &clocked {
        reset_owners.extend(clocked.observers.iter().copied());
        owned.extend(clocked.observers.iter().copied());
    }
    // A shared sampler does not prove its independent output reset pulses.
    let reset_observers: Vec<_> = candidates.iter().copied().collect();
    let mut memory = clocked.as_ref().map_or_else(FxHashSet::default, |clock| {
        clock.memory.iter().map(|cell| cell.actor).collect()
    });
    memory.extend(independent.memory.iter().map(|cell| cell.actor));
    let mut certification = super::observer::certify(
        world,
        report,
        ticks,
        &monitor,
        &reset_observers,
        &memory,
        options.assume_instant,
    )?;
    owned.extend(certification.owned);
    observer_pistons.extend(certification.reset_actors);
    reset_owners.extend(reset_observers);
    for (actor, piston) in report.pistons.iter().enumerate() {
        if reset_owners.iter().any(|&pos| matches!(world.get_block(pos), Block::Observer { observer } if pos.offset(observer.facing.into()) == piston.pos)) { observer_pistons.insert(actor); }
    }
    if ticks.iter().any(|t| owned.contains(&t.pos)) {
        return Err("instant entry contains pending reset or movement work".into());
    }
    let logic = logic::extract_ideal_with_state(
        world,
        report,
        &monitor,
        memory,
        clocked.as_ref().map(|c| c.clock),
        &owned,
        &reset_owners,
        &independent.generators,
        &mut independent.events,
    )?;
    super::sampling::validate(&independent.events, &logic, report)?;
    if let Some(clocked) = &clocked {
        if clocked.validate(world, &logic).is_err() {
            // A watched generator need not be periodic; prove independent sampling instead.
            return prepare_region_inner(world, report, ticks, options, monitor, false);
        }
    }
    for &(actor, observer) in &certification.notifying_returns {
        if logic.responses[actor] != super::boolean::FALSE {
            return Err(format!("observer at {observer:?} has a notifying reset return; its falling pulse may retract during extension and drop the retained payload"));
        }
    }
    for &pos in &report.observers {
        let Block::Observer { observer } = world.get_block(pos) else {
            unreachable!()
        };
        let watched = pos.offset(observer.facing.into());
        if !reset_owners.contains(&pos)
            && matches!(
                world.get_block(watched),
                Block::IronTrapdoor { .. } | Block::NoteBlock { .. } | Block::RedstoneLamp { .. }
            )
            && logic
                .outputs
                .iter()
                .any(|output| output.consumer == watched)
        {
            return Err(format!("ordinary observer at {pos:?} watches piston-driven output {watched:?} without state notifications; callback-dependent observations are unsupported"));
        }
    }
    for (id, p) in report.pistons.iter().enumerate() {
        if is_fixed(id) && logic.responses[id] != super::boolean::FALSE {
            return Err(format!(
                "fixed piston at {:?} has a pose-dependent power path",
                p.pos
            ));
        }
        if !options.assume_instant
            && !observer_pistons.contains(&id)
            && !logic.follows_payload[id]
            && !report.recognition[id].is_matched()
            && !is_memory(id)
            && !is_fixed(id)
            && !is_generator(id)
        {
            return Err(format!(
                "piston at {:?} has no verified observer reset or payload-following response",
                p.pos
            ));
        }
        if report.recognition[id].is_matched()
            && report.recognition[id].resets.iter().any(|reset| {
                matches!(
                    reset.family,
                    crate::redpiler::analysis::families::ResetFamily::DustBelowHead
                        | crate::redpiler::analysis::families::ResetFamily::LateralDust
                )
            })
            && !is_fixed(id)
            && !is_memory(id)
        {
            certification
                .reset_groups
                .push(super::observer::ResetGroup {
                    owner: id,
                    actors: vec![id],
                });
        }
    }
    if let Some(exposure) = report.ports.reset_exposures.iter().find(|e| {
        !report.pistons.iter().any(|p| p.pos == e.consumer)
            && !(reset_owners.contains(&e.source)
                && super::observer::is_presentation_output(world, report, e.source, e.consumer))
    }) {
        return Err(format!(
            "reset signal at {:?} is visible to ordinary consumer at {:?}",
            exposure.source, exposure.consumer
        ));
    }
    let mut aliases = Vec::new();
    for (id, group) in report.payload_groups.iter().enumerate() {
        // A shared payload uses deterministic first-owner near occupancy.
        let p = &report.pistons[group.members[0]];
        let far = p.head.offset(p.piston.facing.into());
        let redstone = logical_payloads[id] == Block::RedstoneBlock;
        for &alias in &group.positions {
            if logical_payloads[id] == Block::Air && alias != p.head {
                continue;
            }
            aliases.push((id, alias, alias == far && redstone));
        }
    }
    owned.extend(logic.wires.iter().copied());
    owned.extend(crate::redpiler::analysis::families::reset_internals(
        &report.recognition,
    ));
    let mut propagation_wires = logic.wires.clone();
    let observable_wires = logic.unprojected_consumer_wires.clone();
    let mut sampling_wires = FxHashSet::default();
    let mut topology = crate::redpiler::analysis::topology::Topology::new(
        world,
        report.bounds,
        &monitor,
        crate::redpiler::analysis::AnalysisLimits::for_budget(monitor.budget_multiplier())
            .max_dependency_steps,
        report
            .payload_groups
            .iter()
            .enumerate()
            .flat_map(|(group, payload)| payload.positions.iter().map(move |&pos| (pos, group)))
            .collect(),
    );
    for event in &independent.events {
        if let super::sampling::SamplingSource::Power { pos, .. } = event.source {
            let wires: FxHashSet<_> = topology
                .wire_inputs(pos)
                .map_err(|error| error.to_string())?
                .wires
                .into_iter()
                .collect();
            sampling_wires.extend(wires.into_iter().filter(|wire| logic.wires.contains(wire)));
        }
    }
    let internals = crate::redpiler::analysis::families::reset_internals(&report.recognition);
    propagation_wires.retain(|pos| !internals.contains(pos));
    // A provisional reset label must not hide or reset a live ordinary leaf.
    owned.retain(|pos| !logic.sources.contains(pos));
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
    let mut pistons = report.pistons.clone();
    // Canonical addresses are independent of saved pose; runtime seeds memory
    // from the descriptor before exporting a settled logical snapshot.
    for (actor, piston) in pistons.iter_mut().enumerate() {
        let far = piston.head.offset(piston.piston.facing.into());
        if !piston.piston.extended && logical_payloads[actor_groups[actor]] != Block::Air {
            template.push((far, logical_payloads[actor_groups[actor]], None));
        }
        piston.payload = far;
    }
    Ok(PreparedInstant {
        propagation_wires,
        observable_wires,
        sampling_wires,
        candidate_wire_count: 0,
        wire_retention_reasons: [0; 7],
        pistons,
        output_offset: 0,
        clocked,
        independent_memory: independent.memory,
        sampling: independent.events,
        reset_groups: certification.reset_groups,
        payloads: logical_payloads,
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
        logical_tick: world.piston_state().logical_tick,
    })
}

struct WireClassification {
    retained: FxHashSet<BlockPos>,
    reason_counts: [usize; 7],
}

fn classify_boundary_wires(
    world: &impl World,
    report: &AnalysisReport,
    programs: &[PreparedInstant],
) -> WireClassification {
    let mut candidates = FxHashSet::default();
    let mut assembly_owned = FxHashSet::default();
    let mut owners = FxHashMap::<BlockPos, (usize, bool)>::default();
    for (region, program) in programs.iter().enumerate() {
        for &pos in &program.propagation_wires {
            candidates.insert(pos);
            owners
                .entry(pos)
                .and_modify(|(_, shared)| *shared = true)
                .or_insert((region, false));
        }
        assembly_owned.extend(program.owned.iter().copied());
    }

    let positions: Vec<_> = candidates.iter().copied().collect();
    let live_inputs: FxHashSet<_> = programs
        .iter()
        .flat_map(|program| program.logic.sources.iter().copied())
        .collect();
    let indices: FxHashMap<_, _> = positions
        .iter()
        .enumerate()
        .map(|(index, &pos)| (pos, index))
        .collect();
    let mut connected = UnionFind::new(positions.len());
    let mut reasons = FxHashMap::<BlockPos, u8>::default();
    let mut mark = |pos: BlockPos, reason: u8| {
        if candidates.contains(&pos) {
            *reasons.entry(pos).or_default() |= reason;
        }
    };
    for program in programs {
        for &pos in &program.observable_wires {
            mark(pos, 1 << 0);
        }
        for &pos in &program.sampling_wires {
            mark(pos, 1 << 1);
        }
        for &(a, b) in &program.logic.wire_links {
            if let (Some(&a), Some(&b)) = (indices.get(&a), indices.get(&b)) {
                connected.union(a, b);
            }
        }
        for piston in &program.pistons {
            if let Some(actor) = report
                .pistons
                .iter()
                .position(|candidate| candidate.pos == piston.pos)
            {
                for update in &report.ports.pistons[actor].updates {
                    if !assembly_owned.contains(&update.source) {
                        mark(update.source, 1 << 2);
                    }
                }
            }
        }
    }
    for (&pos, &(_, shared)) in &owners {
        if shared {
            mark(pos, 1 << 3);
        }
    }

    for &observer_pos in &report.observers {
        if assembly_owned.contains(&observer_pos) {
            continue;
        }
        if let Block::Observer { observer } = world.get_block(observer_pos) {
            let watched = observer_pos.offset(observer.facing.into());
            if candidates.contains(&watched) {
                mark(watched, 1 << 4);
            }
        }
    }

    for &wire in &positions {
        for neighbor in crate::world::wire_cache::positions(wire) {
            if assembly_owned.contains(&neighbor) {
                continue;
            }
            if let Some(&(owner, _)) = owners.get(&neighbor) {
                if owners[&wire].0 != owner {
                    mark(wire, 1 << 3);
                    mark(neighbor, 1 << 3);
                }
            } else if !live_inputs.contains(&neighbor)
                && crate::redstone::has_neighbor_update(world.get_block(neighbor))
            {
                mark(wire, 1 << 5);
            }
        }
    }

    drop(mark);
    let mut boundary_components = FxHashSet::default();
    for &pos in reasons.keys() {
        if let Some(&index) = indices.get(&pos) {
            boundary_components.insert(connected.find(index));
        }
    }
    let retained: FxHashSet<_> = positions
        .into_iter()
        .enumerate()
        .filter_map(|(index, pos)| {
            boundary_components
                .contains(&connected.find(index))
                .then_some(pos)
        })
        .collect();
    let mut reason_counts = [0; 7];
    for &pos in &retained {
        let reason = reasons.get(&pos).copied().unwrap_or(1 << 6);
        for (index, count) in reason_counts.iter_mut().enumerate() {
            *count += usize::from(reason & (1 << index) != 0);
        }
    }
    WireClassification {
        retained,
        reason_counts,
    }
}
