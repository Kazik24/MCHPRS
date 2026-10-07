//! Compile-time proof that an observer pulse only resets pure response actors.
//! The sampled electrical extractor is an oracle here, never a runtime fallback.
use super::boolean::{BooleanArena, Expr, Variable};
use super::outputs::PowerTerm;
use crate::redpiler::analysis::ports::UpdateKind;
use crate::redpiler::analysis::topology::{SourceKind, Topology};
use crate::redpiler::analysis::{AnalysisLimits, AnalysisReport};
use crate::redpiler::TaskMonitor;
use crate::world::World;
use mchprs_blocks::block_entities::{BlockEntity, ContainerType};
use mchprs_blocks::blocks::Block;
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_world::TickEntry;
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::VecDeque;

#[derive(Clone, Copy)]
enum Dependency {
    Observer(usize),
    Wire(usize),
}

#[derive(Default)]
pub(super) struct Certification {
    pub owned: FxHashSet<BlockPos>,
    /// Electrically reset actors whose universal extension was proved.
    pub reset_actors: FxHashSet<usize>,
}

fn fixed_furnace(world: &impl World, pos: BlockPos) -> bool {
    matches!(world.get_block(pos), Block::Furnace { .. })
        && matches!(
            world.get_block_entity(pos),
            Some(BlockEntity::Container {
                ty: ContainerType::Furnace,
                ..
            })
        )
}

fn dependencies(
    arena: &BooleanArena,
    roots: impl IntoIterator<Item = Expr>,
    sources: impl IntoIterator<Item = BlockPos>,
    wires: &FxHashMap<BlockPos, usize>,
) -> Vec<Dependency> {
    let mut result = Vec::new();
    for variable in super::sequential::dependencies(arena, roots) {
        match variable {
            Variable::Observer(id) => result.push(Dependency::Observer(id)),
            Variable::Signal { pos, .. } => {
                if let Some(&id) = wires.get(&pos) {
                    result.push(Dependency::Wire(id));
                }
            }
            _ => {}
        }
    }
    result.extend(
        sources
            .into_iter()
            .filter_map(|pos| wires.get(&pos).copied().map(Dependency::Wire)),
    );
    result
}

// Unknown geometry/data chooses both branches. A known result therefore holds
// for every payload geometry and stable input assignment, including analog data.
fn fixed_value<W: World>(
    arena: &BooleanArena,
    root: Expr,
    observer: usize,
    wires: &FxHashMap<BlockPos, usize>,
    lower: &[u8],
    memo: &mut FxHashMap<Expr, Option<bool>>,
    topology: &mut Topology<'_, W>,
) -> Result<Option<bool>, String> {
    if root <= 1 {
        return Ok(Some(root == 1));
    }
    if let Some(&value) = memo.get(&root) {
        return Ok(value);
    }
    topology.step().map_err(|error| error.to_string())?;
    let decision = arena.decision(root).unwrap();
    let selected = match decision.variable {
        Variable::Observer(id) if id == observer => Some(true),
        Variable::Signal { pos, threshold, .. } => wires
            .get(&pos)
            .filter(|&&id| lower[id] > threshold)
            .map(|_| true),
        // Regulation preserves the non-dot class through every shape update;
        // a saved dot can acquire connections, so its class remains unknown.
        Variable::WireDot(pos) => match topology.world.get_block(pos) {
            Block::RedstoneWire { wire } if !crate::redstone::wire::is_dot(wire) => Some(false),
            _ => None,
        },
        _ => None,
    };
    let value = if let Some(high) = selected {
        fixed_value(
            arena,
            if high { decision.high } else { decision.low },
            observer,
            wires,
            lower,
            memo,
            topology,
        )?
    } else {
        let low = fixed_value(arena, decision.low, observer, wires, lower, memo, topology)?;
        let high = fixed_value(arena, decision.high, observer, wires, lower, memo, topology)?;
        if low == high {
            low
        } else {
            None
        }
    };
    memo.insert(root, value);
    Ok(value)
}

pub(super) fn certify<W: World>(
    world: &W,
    report: &AnalysisReport,
    ticks: &[TickEntry],
    monitor: &TaskMonitor,
    candidates: &[BlockPos],
    memory: &FxHashSet<usize>,
    assume_instant: bool,
) -> Result<Certification, String> {
    if candidates.is_empty() {
        return Ok(Certification::default());
    }
    // Extract once for the whole region. Its guarded local sensor graph keeps
    // paths opened by another payload geometry in the closure proof.
    let oracle = super::logic::sequential::extract(world, report, monitor)?;
    let arena = &oracle.logic.arena;
    let wires: FxHashMap<_, _> = oracle
        .sensors
        .iter()
        .enumerate()
        .map(|(id, (pos, ..))| (*pos, id))
        .collect();
    let actors: FxHashMap<_, _> = report
        .pistons
        .iter()
        .enumerate()
        .map(|(id, p)| (p.pos, id))
        .collect();
    let heads: FxHashMap<_, _> = report
        .pistons
        .iter()
        .enumerate()
        .map(|(id, p)| (p.head, id))
        .collect();
    let mobile: FxHashMap<_, _> = report
        .payload_groups
        .iter()
        .enumerate()
        .flat_map(|(id, group)| group.positions.iter().map(move |&pos| (pos, id)))
        .collect();
    let mut actor_groups = vec![usize::MAX; report.pistons.len()];
    for (group, payload) in report.payload_groups.iter().enumerate() {
        for &actor in &payload.members {
            actor_groups[actor] = group;
        }
    }
    let mut topology = Topology::new(
        world,
        report.bounds,
        monitor,
        AnalysisLimits::for_budget(monitor.budget_multiplier()).max_dependency_steps,
        mobile.clone(),
    );
    let mut users = vec![Vec::new(); wires.len()];
    let mut starts = vec![Vec::new(); report.observers.len()];
    for (id, (_, terms, _, _)) in oracle.sensors.iter().enumerate() {
        for dependency in dependencies(
            arena,
            terms.iter().map(|term| term.guard),
            terms.iter().filter_map(|term| term.source),
            &wires,
        ) {
            topology.step().map_err(|error| error.to_string())?;
            match dependency {
                Dependency::Wire(source) => users[source].push(id),
                Dependency::Observer(source) => starts[source].push(id),
            }
        }
    }
    let responses: Vec<_> = oracle
        .logic
        .responses
        .iter()
        .map(|&root| dependencies(arena, [root], [], &wires))
        .collect();
    let outputs: Vec<_> = oracle
        .logic
        .outputs
        .iter()
        .map(|output| {
            dependencies(
                arena,
                output.terms.iter().map(|term| term.guard),
                output.terms.iter().filter_map(|term| term.source),
                &wires,
            )
        })
        .collect();
    let pending: FxHashSet<_> = ticks.iter().map(|tick| tick.pos).collect();
    let mut wire_sources = FxHashMap::default();
    let mut owned = FxHashSet::default();
    let mut reset_actors = FxHashSet::default();
    let mut presentation_caps = FxHashSet::default();
    for &pos in candidates {
        let Block::Observer { observer: block } = world.get_block(pos) else {
            unreachable!()
        };
        let face = BlockFace::from(block.facing);
        let watched = pos.offset(face);
        let prefix = || {
            format!("observer at {pos:?} watches {watched:?} (minecraft:{}) and forms a notification state boundary", world.get_block(watched).get_name())
        };
        let fail = |reason: &str| format!("{}: {reason}", prefix());
        if block.powered || pending.contains(&pos) {
            return Err(fail(
                "logical reset needs a dormant observer without pending work",
            ));
        }
        let Some(&owner) = actors.get(&watched) else {
            return Err(fail(
                "the watched block is not an owned sticky response actor",
            ));
        };
        if !report.pistons[owner].piston.sticky || memory.contains(&owner) {
            return Err(fail(
                "a sampled memory/clock actor cannot be collapsed into a reset owner",
            ));
        }
        let cap = pos.offset(face.opposite());
        if world.get_block_entity(cap).is_some() && !fixed_furnace(world, cap) {
            return Err(fail(&format!(
                "fixed reset cap at {cap:?} has an unsupported block entity; only a matching stationary furnace inventory is supported"
            )));
        }
        let lamp_cap = matches!(world.get_block(cap), Block::RedstoneLamp { .. });
        if is_presentation_output(world, report, pos, cap) {
            presentation_caps.insert(cap);
        }
        if (!assume_instant && !world.get_block(cap).is_solid())
            || mobile.contains_key(&cap)
            || actors.contains_key(&cap)
            || matches!(
                world.get_block_entity(cap),
                Some(BlockEntity::MovingPiston(_))
            )
            || (!lamp_cap && pending.contains(&cap))
        {
            return Err(fail(
                "observer output needs a stationary fixed conductor without pending work",
            ));
        }
        if !assume_instant
            && !report.recognition[owner]
                .inputs
                .sources
                .iter()
                .any(|source| source.source == pos)
        {
            return Err(fail(
                "the pulse has no proved electrical return to its watched actor",
            ));
        }
        if !assume_instant {
            let return_path = topology
                .signal_inputs(cap, face.opposite())
                .map_err(|error| error.to_string())?;
            if !return_path.outside_bounds.is_empty()
                || !return_path
                    .sources
                    .iter()
                    .any(|source| source.source == pos)
            {
                return Err(fail(
                    "the output conductor lacks complete observer return context",
                ));
            }
        }
        let observer = report
            .observers
            .iter()
            .position(|&source| source == pos)
            .unwrap();
        let mut cone = FxHashSet::default();
        let mut queue: VecDeque<_> = starts[observer].iter().copied().collect();
        while let Some(wire) = queue.pop_front() {
            topology.step().map_err(|error| error.to_string())?;
            if cone.insert(wire) {
                queue.extend(users[wire].iter().copied());
            }
        }
        let influenced = |dependencies: &[Dependency]| {
            dependencies.iter().any(|dependency| match *dependency {
                Dependency::Observer(id) => id == observer,
                Dependency::Wire(id) => cone.contains(&id),
            })
        };
        // A fixed cap lamp or adjacent lamp is presentation: its lit bit
        // neither emits power nor changes conduction. Omit the reset flash,
        // keeping its native driver live; a watched flash is a state boundary.
        if let Some(output) = outputs.iter().enumerate().find_map(|(id, inputs)| {
            if !influenced(inputs) {
                return None;
            }
            let consumer = oracle.logic.outputs[id].consumer;
            if is_presentation_output(world, report, pos, consumer) {
                presentation_caps.insert(consumer);
                None
            } else {
                Some(id)
            }
        }) {
            return Err(fail(&format!(
                "reset pulse reaches ordinary data consumer {:?}",
                oracle.logic.outputs[output].consumer
            )));
        }
        for &other in &report.observers {
            let Block::Observer {
                observer: other_block,
            } = world.get_block(other)
            else {
                unreachable!()
            };
            let other_watched = other.offset(other_block.facing.into());
            if other_watched == pos {
                return Err(fail(&format!(
                    "owned reset observer is watched by observer {other:?}; the omitted reset signal is an observable state boundary"
                )));
            }
            if wires
                .get(&other_watched)
                .is_some_and(|wire| cone.contains(wire))
            {
                return Err(fail(&format!(
                    "reset dust is independently watched by observer {other:?}"
                )));
            }
        }
        let mut recipients: FxHashSet<_> = responses
            .iter()
            .enumerate()
            .filter_map(|(actor, inputs)| influenced(inputs).then_some(actor))
            .collect();
        recipients.insert(owner);
        let electrical = recipients.clone();
        // Match observer output and dust notifications, including saved heads
        // that deliver a base recheck without supplying electrical power.
        let mut delivered: Vec<_> = std::iter::once(cap)
            .chain(
                BlockFace::values()
                    .into_iter()
                    .filter(|&direction| direction != face)
                    .map(|direction| cap.offset(direction)),
            )
            .collect();
        for &wire in &cone {
            let wire_pos = oracle.sensors[wire].0;
            if pending.contains(&wire_pos) {
                return Err(fail("reset dust contains pending work"));
            }
            let support = wire_pos.offset(BlockFace::Bottom);
            if world.get_block_entity(support).is_some() && !fixed_furnace(world, support) {
                return Err(fail(&format!(
                    "reset dust support at {support:?} has an unsupported block entity; only a matching stationary furnace inventory is supported"
                )));
            }
            if mobile.contains_key(&support)
                || actors.contains_key(&support)
                || heads.contains_key(&support)
            {
                return Err(fail("reset dust depends on moving support"));
            }
            owned.extend([wire_pos, support]);
            for direction in BlockFace::values() {
                let neighbor = wire_pos.offset(direction);
                delivered.push(neighbor);
                delivered.extend(
                    BlockFace::values()
                        .into_iter()
                        .map(|other| neighbor.offset(other)),
                );
            }
        }
        for delivered_pos in delivered {
            if let Some(&actor) = actors
                .get(&delivered_pos)
                .or_else(|| heads.get(&delivered_pos))
            {
                recipients.insert(actor);
            }
        }
        // Propagate guaranteed pulse minima. Unknown data/geometry cannot add
        // power to this proof; each electrically affected actor must extend.
        // A notification-only recipient needs independently coupled data and
        // no implicit storage. Final ideal response DAG validation must still
        // prove the complete pure domain acyclic before any program activates.
        let mut lower = vec![0u8; wires.len()];
        let mut memo = FxHashMap::default();
        while !assume_instant {
            let mut changed = false;
            for &wire in &cone {
                for &PowerTerm {
                    guard,
                    source,
                    attenuation,
                } in &oracle.sensors[wire].1
                {
                    let source =
                        source.map_or(15, |source| wires.get(&source).map_or(0, |&id| lower[id]));
                    let candidate = source.saturating_sub(attenuation);
                    if candidate > lower[wire]
                        && fixed_value(
                            arena,
                            guard,
                            observer,
                            &wires,
                            &lower,
                            &mut memo,
                            &mut topology,
                        )? == Some(true)
                    {
                        lower[wire] = candidate;
                        changed = true;
                        memo.clear();
                    }
                }
            }
            if !changed {
                break;
            }
        }
        for actor in recipients {
            let piston = &report.pistons[actor];
            if !piston.piston.sticky
                || memory.contains(&actor)
                || (!assume_instant
                    && electrical.contains(&actor)
                    && fixed_value(
                        arena,
                        oracle.logic.responses[actor],
                        observer,
                        &wires,
                        &lower,
                        &mut memo,
                        &mut topology,
                    )? != Some(false))
            {
                return Err(fail(&format!(
                    "pulse does not unconditionally reset pure response piston {:?}",
                    piston.pos
                )));
            }
            let inputs = &report.recognition[actor].inputs;
            let independent_data = |kind: SourceKind| match kind {
                SourceKind::Observer => false,
                SourceKind::MobilePayload { group } => group != actor_groups[actor],
                SourceKind::Ordinary | SourceKind::Constant => true,
            };
            let adjacent_data = inputs.sources.iter().any(|source| {
                independent_data(source.kind)
                    && super::sampling::data_notifies(world, source.source, piston.pos)
            });
            let mut coupled_data = adjacent_data;
            for update in &report.ports.pistons[actor].updates {
                if update.kind != UpdateKind::WireNotification || update.independent_of_power {
                    continue;
                }
                if !wire_sources.contains_key(&update.source) {
                    let sources = topology
                        .wire_inputs(update.source)
                        .map_err(|error| error.to_string())?;
                    if !sources.outside_bounds.is_empty() {
                        return Err(fail("data notification cone leaves the selection"));
                    }
                    wire_sources.insert(update.source, sources);
                }
                coupled_data |= wire_sources[&update.source]
                    .sources
                    .iter()
                    .any(|source| independent_data(source.kind));
            }
            if !coupled_data {
                return Err(fail(&format!("reset notification independently samples piston {:?}: no independently coupled data update; explicit sampling is required", piston.pos)));
            }
        }
        if !assume_instant {
            // Keep joint electrical reset proofs for default construction
            // admission. Notification-only recipients have no such proof.
            reset_actors.extend(electrical);
        }
        owned.extend([pos, cap]);
    }
    owned.retain(|pos| {
        !presentation_caps.contains(pos)
        // Fixed inventories retain their native constant comparator owner.
        // Their analog override is independent of reset electrical power.
        && !fixed_furnace(world, *pos)
    });
    Ok(Certification {
        owned,
        reset_actors,
    })
}

pub(super) fn is_presentation_output(
    world: &impl World,
    report: &AnalysisReport,
    source: BlockPos,
    consumer: BlockPos,
) -> bool {
    let Block::Observer { observer } = world.get_block(source) else {
        return false;
    };
    let cap = source.offset(BlockFace::from(observer.facing).opposite());
    let delta = consumer - cap;
    matches!(world.get_block(consumer), Block::RedstoneLamp { .. })
        && delta.x.abs() + delta.y.abs() + delta.z.abs() <= 1
        && world.get_block(cap).is_solid()
        && !report.payload_groups.iter().any(|group| group.positions.contains(&cap) || group.positions.contains(&consumer))
        && !report.pistons.iter().any(|piston| piston.pos == cap)
        && !matches!(world.get_block_entity(cap), Some(BlockEntity::MovingPiston(_)))
        && world.get_block_entity(consumer).is_none()
        && !BlockFace::values().into_iter().any(|face| {
            let pos = consumer.offset(face);
            matches!(world.get_block(pos), Block::Observer { observer } if pos.offset(observer.facing.into()) == consumer)
        })
}
