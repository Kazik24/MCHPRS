//! Couple executable computation; retain reset, geometry and unsupported owners as boundaries.
use super::*;

pub(super) fn partition(graph: &CompileGraph, catalog: &BoundaryCatalog, monitor: &TaskMonitor) -> Result<regions::RegionPartition, String> {
    let cells = catalog.cells.iter().map(|cell| private_observer_cell(graph, cell));
    let candidates = computation_corridors(graph, catalog, monitor)?;
    let corridors = catalog.corridors.iter().zip(candidates).map(|(corridor, candidate)| {
        let wires = corridor.wires.iter().copied().collect();
        candidate && composition::corridors::closed_straight_cable(graph, &wires)
    });
    let joinable: Vec<_> = cells.chain(corridors).collect();
    regions::partition_with_cuts(graph, catalog, monitor, &joinable, |kind| computation_dependency(graph, kind))
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
