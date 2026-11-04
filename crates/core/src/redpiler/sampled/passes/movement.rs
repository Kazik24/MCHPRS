//! Sample token departures/arrivals and lower their ordered notification steps.
use super::*;

pub(in super::super) fn run(
    context: &Context<'_, impl World>,
    program: &mut Program,
    probe_indexes: &ProbeIndexes<'_>,
    phases: &mut CompilePhases<'_>,
) -> Result<(), ScratchError> {
    let world = context.world;
    let bounds = context.bounds;
    let entry = context.entry;
    let monitor = context.monitor;

    phases.next("movement transition sampling");
    check_cancelled(monitor)?;
    let mut movement_probe = ScratchWorld::from_world_monitored(world, bounds, entry.clone(), monitor)?;
    let token_jobs: Vec<_> = (0..program.tokens.len()).collect();
    let movement = movement_probe.probe_jobs(&token_jobs, monitor, |probe, &token_index| {
        let token = &program.tokens[token_index];
        check_cancelled(monitor)?;
        let arrangements = vec![Vec::new(); token.cells.len()];
        let mut updates = Vec::new();
        for &cell in &token.cells {
            check_cancelled(monitor)?;
            let mut cell_updates = [Vec::new(), Vec::new()];
            for arrival in [false, true] {
                check_cancelled(monitor)?;
                probe.revert();
                if let Some(initial) = token.position {
                    probe.world_mut().set_block(initial, Block::Air);
                }
                if !arrival {
                    probe.world_mut().set_block(cell, token.block);
                }
                let w = probe.world_mut();
                w.observe_redstone_updates(true);
                w.set_block(cell, if arrival { token.block } else { Block::Air });
                crate::redstone::skipping_update_surrounding_blocks(w, cell, false);
                for event in w.take_redstone_update_observations() {
                    if let Block::Observer { observer } = event.block
                        && !event.direction.is_some_and(|direction| observer.facing == direction.into())
                    {
                        continue;
                    }
                    if let Some(&index) = probe_indexes.components.get(&event.pos)
                        && !cell_updates[usize::from(arrival)].contains(&index)
                    {
                        cell_updates[usize::from(arrival)].push(index);
                    }
                }
                if let Err(error) = probe.finish_probe() {
                    if std::env::var_os("MCHPRS_REDPILER_BOUNDARY_TRACE").is_some() {
                        eprintln!(
                            "Redpiler movement probe token={token_index} block={:?} initial={:?} cell={cell:?} arrival={arrival}: {error:?}",
                            token.block, token.position
                        );
                    }
                    return Err(error);
                }
            }
            updates.push(cell_updates);
        }
        Ok((arrangements, updates))
    })?;

    for (sources, updates) in movement {
        program.token_sources.push(sources);
        program.token_updates.push(updates);
    }

    for token in &program.tokens {
        let mut cells = Vec::new();
        for &cell in &token.cells {
            let mut watchers = Vec::new();
            for face in [
                mchprs_blocks::BlockFace::West,
                mchprs_blocks::BlockFace::East,
                mchprs_blocks::BlockFace::Bottom,
                mchprs_blocks::BlockFace::Top,
                mchprs_blocks::BlockFace::North,
                mchprs_blocks::BlockFace::South,
            ] {
                let at = cell.offset(face);
                if let Some(&component) = probe_indexes.components.get(&at) {
                    let watch = match program.initial[component] {
                        Block::Piston { .. } => true,
                        Block::Observer { observer } => at.offset(observer.facing.into()) == cell,
                        _ => false,
                    };
                    if watch {
                        watchers.push(component);
                    }
                }
            }
            cells.push(watchers);
        }
        program.token_watchers.push(cells);
    }

    let mut observers_by_target: std::collections::HashMap<BlockPos, Vec<usize>> = std::collections::HashMap::new();
    for (node, block) in program.initial.iter().enumerate() {
        if let Block::Observer { observer } = block {
            observers_by_target
                .entry(program.positions[node].offset(observer.facing.into()))
                .or_default()
                .push(node);
        }
    }

    for (source, &pos) in program.positions.iter().enumerate() {
        let footprint = update_footprint(pos, program.initial[source]);
        let mut watchers = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (order, &at) in footprint.iter().enumerate() {
            if let Some(&node) = probe_indexes.components.get(&at)
                && node != source
                && matches!(program.initial[node], Block::Piston { .. })
                && seen.insert(node)
            {
                watchers.push((order, node));
            }
        }
        // Controls also notify observers of their support, even if that block's state stays unchanged.
        let support = matches!(program.initial[source], Block::Lever { .. } | Block::StoneButton { .. })
            .then(|| crate::interaction::attachment_support(program.initial[source], pos).map(|(support, _)| support))
            .flatten();
        for observed in std::iter::once(pos).chain(support) {
            if let Some(observers) = observers_by_target.get(&observed) {
                watchers.extend(
                    observers
                        .iter()
                        .copied()
                        .filter(|&node| node != source && seen.insert(node))
                        .map(|node| (usize::MAX, node)),
                );
            }
        }
        watchers.sort_by_key(|&(order, _)| order);
        program.source_watchers[source] = watchers.into_iter().map(|(_, node)| node).collect();
    }

    for piston in &program.pistons {
        let mut watchers = Vec::new();
        for face in update_order() {
            let at = piston.home.offset(face);
            if let Some(&node) = probe_indexes.components.get(&at) {
                match program.initial[node] {
                    Block::Piston { .. } => watchers.push(node),
                    Block::Observer { observer } if at.offset(observer.facing.into()) == piston.home => watchers.push(node),
                    _ => {}
                }
            }
        }
        program.home_watchers.push(watchers.into_iter().collect());
    }

    let mut watched_by_node = vec![None; program.positions.len()];
    for (index, wire) in program.watched_wires.iter().enumerate() {
        watched_by_node[wire.wire].get_or_insert(index);
    }

    let mut move_steps = Vec::new();
    for (token, cells) in program.token_updates.iter().enumerate() {
        let mut programs = Vec::new();
        for (cell, updates) in cells.iter().enumerate() {
            let watchers = &program.token_watchers[token][cell];
            let observers: Vec<_> = watchers
                .iter()
                .copied()
                .filter(|&node| matches!(program.initial[node], Block::Observer { .. }))
                .collect();
            let mut edges = [Vec::new(), Vec::new()];
            for arrival in [false, true] {
                let mut nodes = Vec::new();
                if !arrival {
                    nodes.extend(observers.iter().copied());
                }
                nodes.extend(updates[usize::from(arrival)].iter().copied().filter(|&node| match program.initial[node] {
                    Block::RedstoneWire { .. } => watched_by_node[node].is_some(),
                    Block::Piston { .. }
                    | Block::RedstoneRepeater { .. }
                    | Block::RedstoneComparator { .. }
                    | Block::Observer { .. }
                    | Block::RedstoneLamp { .. }
                    | Block::Hopper { .. }
                    | Block::IronTrapdoor { .. }
                    | Block::NoteBlock { .. }
                    | Block::RedstoneTorch { .. }
                    | Block::RedstoneWallTorch { .. } => true,
                    block if block.is_copper_bulb() || block.is_command_block() => true,
                    _ => false,
                }));
                nodes.extend(watchers.iter().copied());
                if arrival {
                    nodes.extend(observers.iter().copied());
                }
                let mut seen = std::collections::HashSet::new();
                nodes.retain(|node| seen.insert(*node));
                edges[usize::from(arrival)] = nodes
                    .iter()
                    .map(|&node| {
                        let notifying_wires = nodes
                            .iter()
                            .copied()
                            .filter(|&wire| watched_by_node[wire].is_some_and(|index| program.watched_wires[index].watchers.contains(&node)))
                            .collect();
                        crate::redpiler::sampled::program::MoveStepSpec {
                            node,
                            watcher: watchers.contains(&node),
                            wire: watched_by_node[node].is_some(),
                            wires: notifying_wires,
                        }
                    })
                    .collect();
            }
            programs.push(edges);
        }
        move_steps.push(programs);
    }
    program.set_move_steps(move_steps);
    crate::redpiler::sampled::shapes::sample(program, &mut movement_probe, monitor)?;
    Ok(())
}
