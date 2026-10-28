//! Couple executable computation; retain reset, geometry and unsupported owners as boundaries.
use super::*;

/// Partition computation once; unsupported regions retain their complete Graph boundary.
#[cfg(test)]
pub(super) fn partition(graph: &CompileGraph, catalog: &BoundaryCatalog, monitor: &TaskMonitor) -> Result<regions::RegionPartition, String> {
    partition_with_activity(graph, catalog, monitor).map(|(partition, _)| partition)
}

pub(super) fn partition_with_activity(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    monitor: &TaskMonitor,
) -> Result<(regions::RegionPartition, Vec<Option<LayerDeclineReason>>), String> {
    let mut corridor_ports: FxHashMap<(BoundaryPortKind, BoundaryDirection), FxHashSet<u32>> = FxHashMap::default();
    for (index, port) in catalog.ports.iter().enumerate() {
        if index & 0x3ff == 0 && monitor.cancelled() {
            return Err("cancelled while checking shared-cell corridor boundaries".into());
        }
        if let BoundaryOwner::Corridor(corridor) = port.owner {
            corridor_ports.entry((port.kind, port.direction)).or_default().insert(corridor);
        }
    }
    let candidates = computation_corridors(graph, catalog, monitor)?;
    let direct_shared: Vec<_> = catalog.cells.iter().map(|cell| direct_shared_observer_cell(graph, cell)).collect();
    let mut cell_has_edge = vec![false; catalog.cells.len()];
    let mut shared_cell_has_candidate = vec![false; catalog.cells.len()];
    let mut shared_cell_has_unsupported_edge = vec![false; catalog.cells.len()];
    let mut shared_source_corridors = vec![false; catalog.corridors.len()];

    // The full arrival proof authorizes shared-cell edges. This pass only admits
    // one-wire candidates with a known computation path or an adjacent movable
    // RedstoneBlock source. Private cells retain the established unconditional
    // partition ownership and use their existing strict arrival proof.
    for (index, port) in catalog.ports.iter().enumerate() {
        if index & 0x3ff == 0 && monitor.cancelled() {
            return Err("cancelled while checking shared-cell corridor boundaries".into());
        }
        let BoundaryOwner::Cell(cell_id) = port.owner else { continue };
        let cell = cell_id as usize;
        let opposite = match port.direction {
            BoundaryDirection::Input => BoundaryDirection::Output,
            BoundaryDirection::Output => BoundaryDirection::Input,
        };
        let Some(corridors) = corridor_ports.get(&(port.kind, opposite)) else {
            continue;
        };
        cell_has_edge[cell] = true;
        if direct_shared[cell] {
            for &corridor_id in corridors {
                let corridor = corridor_id as usize;
                let source_candidate = catalog.corridors[corridor].wires.len() == 1
                    && composition::corridors::may_join_single_wire_shared_arrival(graph, &catalog.cells[cell], &catalog.corridors[corridor]);
                if catalog.corridors[corridor].wires.len() == 1 && (candidates[corridor] || source_candidate) {
                    shared_cell_has_candidate[cell] = true;
                    shared_source_corridors[corridor] |= source_candidate;
                } else if matches!(port.kind, BoundaryPortKind::Power { .. }) {
                    shared_cell_has_unsupported_edge[cell] = true;
                }
            }
        }
    }

    let mut shared_single_wire_cells = vec![false; catalog.cells.len()];
    for cell in 0..catalog.cells.len() {
        shared_single_wire_cells[cell] = shared_cell_has_candidate[cell] && !shared_cell_has_unsupported_edge[cell];
    }
    let cells = catalog.cells.iter().enumerate().map(|(index, cell)| {
        private_observer_cell(graph, cell) || (direct_shared_observer_cell(graph, cell) && (!cell_has_edge[index] || shared_single_wire_cells[index]))
    });
    let corridors = catalog.corridors.iter().enumerate().map(|(index, corridor)| {
        let candidate = candidates[index] || shared_source_corridors[index];
        let wires = corridor.wires.iter().copied().collect();
        candidate && composition::corridors::closed_straight_cable(graph, &wires)
    });
    let joinable: Vec<_> = cells.chain(corridors).collect();
    drop(corridor_ports);
    drop(candidates);
    drop(direct_shared);
    drop(cell_has_edge);
    drop(shared_cell_has_candidate);
    drop(shared_cell_has_unsupported_edge);
    drop(shared_source_corridors);
    drop(shared_single_wire_cells);
    // Recognized owners can be certified independently even when they cannot merge.
    let candidates = vec![true; joinable.len()];
    let (selected, executable_edges, budget_declined) = match regions::greedy_event_owners(graph, catalog, &candidates, joinable.len(), monitor) {
        Ok((selected, edges)) => (selected, edges, false),
        Err(error) if !monitor.cancelled() && (error.contains("work budget exceeded") || error.contains("retained-row budget exceeded")) => {
            monitor.record_warning("Instant event discovery exceeded its budget; affected work remains on Graph.".into());
            (vec![false; joinable.len()], Vec::new(), true)
        }
        Err(error) => return Err(error),
    };
    drop(candidates);
    let mergeable = selected
        .iter()
        .zip(&joinable)
        .map(|(&selected, &joinable)| selected && joinable)
        .collect::<Vec<_>>();
    let retained_bytes = selected
        .len()
        .checked_add(joinable.len())
        .and_then(|bytes| bytes.checked_add(mergeable.len()))
        .ok_or("instant event retained size overflow")?
        .checked_add(
            executable_edges
                .len()
                .checked_mul(std::mem::size_of::<RouteOccurrence>())
                .ok_or("instant event retained size overflow")?,
        )
        .ok_or("instant event retained size overflow")?;
    let partition = regions::partition_with_cuts(graph, catalog, monitor, &mergeable, retained_bytes, |kind| {
        matches!(kind, BoundaryPortKind::Callback { occurrence } if executable_edges.binary_search_by_key(&(occurrence.route, occurrence.ordinal), |edge| (edge.route, edge.ordinal)).is_ok())
            && computation_dependency(graph, kind)
    })?;
    let mut declines = vec![Some(LayerDeclineReason::NoUsefulEventPath); partition.regions.len()];
    for (owner, &selected) in selected.iter().enumerate() {
        declines[partition.owner_region[owner] as usize] = if selected {
            None
        } else if budget_declined {
            Some(LayerDeclineReason::ProofBudgetExceeded)
        } else if owner >= catalog.cells.len() && !joinable[owner] {
            Some(LayerDeclineReason::SignalArrivalUnproven)
        } else {
            Some(LayerDeclineReason::NoUsefulEventPath)
        };
    }
    Ok((partition, declines))
}

/// Only possible inter-cell transfers are absorbed; input/reset/output wires keep Graph ownership.
fn computation_corridors(graph: &CompileGraph, catalog: &BoundaryCatalog, monitor: &TaskMonitor) -> Result<Vec<bool>, String> {
    let mut owners = vec![None; graph.nodes.len()];
    for (index, corridor) in catalog.corridors.iter().enumerate() {
        for wire in &corridor.wires {
            owners[wire.index()] = Some(index);
        }
    }
    let sources: rustc_hash::FxHashSet<_> = catalog
        .cells
        .iter()
        .flat_map(|cell| cell.resources.iter().map(|resource| resource.source))
        .collect();
    let consumers: rustc_hash::FxHashSet<_> = catalog
        .cells
        .iter()
        .flat_map(|cell| cell.uses.iter().map(|use_| graph.pistons[use_.actor.index()].node))
        .collect();
    let mut incoming = vec![false; catalog.corridors.len()];
    let mut outgoing = incoming.clone();
    for (index, link) in graph.power_links.iter().enumerate() {
        if index & 0x3ff == 0 {
            check_cancelled(monitor)?;
        }
        let PowerTarget::Input { node, port: InputPort::Main } = link.to else {
            continue;
        };
        let source = graph.sites[link.from.index()].source;
        if let (Some(corridor), SiteSource::Contact(contact)) = (owners[node.index()], source) {
            incoming[corridor] |= sources.contains(&contact);
        }
        if let SiteSource::Component(source) = source {
            if let Some(corridor) = owners[source.index()] {
                outgoing[corridor] |= consumers.contains(&node);
            }
        }
    }
    Ok(incoming.into_iter().zip(outgoing).map(|(input, output)| input && output).collect())
}

/// This is only the existing executor's necessary family; the complete certificate still runs.
pub(super) fn private_observer_cell(graph: &CompileGraph, cell: &CellContract) -> bool {
    if cell.uses.len() != 1 || cell.resources.len() != 1 {
        return false;
    }
    let use_ = &cell.uses[0];
    let resource = &cell.resources[0];
    let Some(spec) = graph.resources.get(resource.resource.index()) else {
        return false;
    };
    let Some(material) = graph.profiles.get(spec.material.index()) else {
        return false;
    };
    resource.owners.as_ref() == [use_.actor]
        && use_.reset.observer_wake().is_some_and(|wake| wake.watched_actor == use_.actor)
        && mchprs_blocks::blocks::Block::from_id(material.raw_state) == mchprs_blocks::blocks::Block::RedstoneBlock
}

/// Direct shared-token composition is certified by the full cell contract. Keep it
/// separate from the single-owner corridor arrival proof below.
pub(super) fn direct_shared_observer_cell(graph: &CompileGraph, cell: &CellContract) -> bool {
    if cell.uses.len() < 2 || cell.resources.len() != 1 {
        return false;
    }
    let pin = &cell.resources[0];
    let Some(resource) = graph.resources.get(pin.resource.index()) else {
        return false;
    };
    let Some(material) = graph.profiles.get(resource.material.index()) else {
        return false;
    };
    if mchprs_blocks::blocks::Block::from_id(material.raw_state) != mchprs_blocks::blocks::Block::RedstoneBlock {
        return false;
    }

    let mut use_actors = cell.uses.iter().map(|use_| use_.actor).collect::<Vec<_>>();
    use_actors.sort_unstable();
    use_actors.dedup();
    let original_owner_count = pin.owners.len();
    let mut pin_owners = pin.owners.to_vec();
    pin_owners.sort_unstable();
    pin_owners.dedup();
    if use_actors.len() != cell.uses.len() || pin_owners.len() != original_owner_count || use_actors != pin_owners {
        return false;
    }
    cell.uses.iter().all(|use_| {
        !use_.activations.is_empty()
            && use_
                .retraction
                .transfer
                .is_some_and(|transfer| transfer.resource == pin.resource && transfer.source == pin.source)
            && use_.reset.observer_wake().is_some_and(|wake| wake.watched_actor == use_.actor)
    })
}

fn computation_dependency(graph: &CompileGraph, kind: BoundaryPortKind) -> bool {
    match kind {
        BoundaryPortKind::Power { link } => {
            let source = graph.sites[graph.power_links[link as usize].from.index()].source;
            !matches!(source, SiteSource::Component(node) if matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. }))
        }
        BoundaryPortKind::Callback { occurrence } => {
            let route = &graph.routes[occurrence.route.index()];
            match graph.nodes[route.source.index()].kind {
                CircuitNodeKind::Wire(_) => true,
                CircuitNodeKind::Piston(actor) => {
                    let routes = graph.pistons[actor.index()].routes;
                    occurrence.route == routes.base_changed || Some(occurrence.route) == routes.far_changed || occurrence.route == routes.near_changed
                }
                _ => false,
            }
        }
        // Observing a neighboring state is a perimeter obligation, not permission to own it.
        _ => false,
    }
}
