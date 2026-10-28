//! Bounded source-qualified dust-to-actor event witnesses.
use super::discovery::EventCandidates;
use crate::redpiler::TaskMonitor;
use crate::redpiler::program_graph::*;
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub(crate) struct CorridorEvent {
    pub(crate) entry: RouteOccurrence,
    pub(crate) entry_wire: NodeId,
    pub(crate) source_wire: NodeId,
    pub(crate) notification: RouteOccurrence,
    pub(crate) actor: PistonId,
    pub(crate) cell: u32,
    /// Original directed power links, in traversal order.
    pub(crate) links: Vec<u32>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct WireStrengthChange {
    pub(crate) node: NodeId,
    pub(crate) before: u8,
    pub(crate) after: u8,
}

impl CorridorEvent {
    /// Resolve ingress using the notification source wire's own strength change.
    /// A settled sample can miss an intermediate fall/rise; this is only valid
    /// for the explicitly idealized frame, not exact native event replay.
    /// A captured downstream view requires the caller's captured-site snapshot;
    /// missing target data declines to the native continuation.
    pub(crate) fn activates(
        &self,
        graph: &CompileGraph,
        entry: RouteOccurrence,
        change: WireStrengthChange,
        captured: Option<&[Option<ObservedSite>]>,
        mut guard: impl FnMut(GuardId, Option<&[Option<ObservedSite>]>) -> bool,
    ) -> Result<bool, &'static str> {
        if change.node != self.source_wire {
            return Err("corridor change does not identify the notification source wire");
        }
        if entry != self.entry || change.before == change.after {
            return Ok(false);
        }
        for &link in &self.links {
            if !guard(graph.power_links[link as usize].guard, None) {
                return Ok(false);
            }
        }
        let delivery = graph.routes[self.notification.route.index()].deliveries[self.notification.ordinal as usize];
        let view = if delivery.view == OperandView::Captured {
            let captured = captured.ok_or("captured corridor notification requires native continuation")?;
            if captured.get(delivery.target.index()).and_then(Option::as_ref).is_none() {
                return Err("captured corridor target is unavailable");
            }
            Some(captured)
        } else {
            None
        };
        Ok(guard(delivery.guard, view))
    }

    /// Evaluate the source-qualified cell request and return its conditional
    /// token transfer. The caller owns deduplication, writes and restoration.
    pub(crate) fn transition(
        &self,
        graph: &CompileGraph,
        catalog: &BoundaryCatalog,
        entry: RouteOccurrence,
        change: WireStrengthChange,
        captured: Option<&[Option<ObservedSite>]>,
        guard: impl FnMut(GuardId, Option<&[Option<ObservedSite>]>) -> bool,
        mut read: impl FnMut(CellRead) -> bool,
    ) -> Result<Option<CellResourceTransfer>, String> {
        if !self.activates(graph, entry, change, captured, guard).map_err(str::to_owned)? {
            return Ok(None);
        }
        let cell = catalog.cells.get(self.cell as usize).ok_or("corridor cell is missing")?;
        let use_ = cell
            .uses
            .iter()
            .find(|use_| use_.actor == self.actor)
            .ok_or("corridor actor is missing")?;
        let mut values = Vec::new();
        cell.evaluate(&mut values, |source| match source {
            CellRead::Activation(actor) => actor == self.actor,
            other => read(other),
        })
        .map_err(str::to_owned)?;
        if !values[cell.stationary_entry.index()] || !values[use_.requested.index()] || !values[use_.valid.index()] {
            return Ok(None);
        }
        let transfer = use_.retraction.transfer.ok_or("corridor actor has no represented token transfer")?;
        if !values[transfer.when_at_source.index()] {
            return Ok(None);
        }
        Ok(Some(transfer))
    }
}

/// This is a local witness, not ownership transfer or a complete region plan.
/// Cycles, ambiguous parallel links, dynamic receivers and large corridors stay native.
pub(crate) fn corridor_events(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    candidates: &EventCandidates,
    region: u32,
    monitor: &TaskMonitor,
) -> Result<Vec<CorridorEvent>, String> {
    if monitor.cancelled() {
        return Err("cancelled while lowering corridor events".into());
    }
    let cells = catalog.cells.len();
    let mut result = Vec::new();
    let mut work = 0usize;
    let limit = (32usize * 1024 * 1024).saturating_mul(monitor.budget_multiplier());
    let mut adjacency = FxHashMap::<usize, FxHashMap<NodeId, Vec<(NodeId, u32)>>>::default();
    let mut neighbors = FxHashMap::<usize, FxHashMap<NodeId, FxHashSet<NodeId>>>::default();
    for (index, link) in graph.power_links.iter().enumerate() {
        work = work.checked_add(1).ok_or("corridor event work overflow")?;
        if work > limit {
            return Err("corridor event work budget exceeded".into());
        }
        if work & 0x3fff == 0 && monitor.cancelled() {
            return Err("cancelled while indexing corridor links".into());
        }
        let SiteSource::Component(from) = graph.sites[link.from.index()].source else {
            continue;
        };
        let PowerTarget::Input {
            node: to,
            port: InputPort::Main,
        } = link.to
        else {
            continue;
        };
        let Some(owner) = candidates.partition.owners.nodes.get(from.index()).copied().flatten() else {
            continue;
        };
        if owner < cells
            || candidates.partition.owners.nodes.get(to.index()).copied().flatten() != Some(owner)
            || !candidates.selected.get(owner).copied().unwrap_or(false)
            || (region != u32::MAX && candidates.partition.owner_region.get(owner).copied() != Some(region))
            || from == to
        {
            continue;
        }
        let corridor = &catalog.corridors[owner - cells];
        if !(2..=64).contains(&corridor.wires.len()) {
            continue;
        }
        adjacency.entry(owner).or_default().entry(from).or_default().push((to, index as u32));
        let graph_neighbors = neighbors.entry(owner).or_default();
        graph_neighbors.entry(from).or_default().insert(to);
        graph_neighbors.entry(to).or_default().insert(from);
    }
    let mut edges_by_source = FxHashMap::<usize, Vec<_>>::default();
    for edge in &candidates.edges {
        if edge.source >= cells {
            edges_by_source.entry(edge.source).or_default().push(edge);
        }
    }
    let mut valid_topologies = FxHashSet::default();
    for (&owner, graph_neighbors) in &neighbors {
        let corridor = &catalog.corridors[owner - cells];
        if graph_neighbors.values().map(FxHashSet::len).sum::<usize>() != 2 * (corridor.wires.len() - 1)
            || adjacency[&owner].values().any(|links| {
                let mut seen = FxHashSet::default();
                links.iter().any(|(target, _)| !seen.insert(*target))
            })
        {
            continue;
        }
        let mut seen = FxHashSet::default();
        let mut queue = VecDeque::from([corridor.wires[0]]);
        seen.insert(corridor.wires[0]);
        while let Some(node) = queue.pop_front() {
            for &next in graph_neighbors.get(&node).into_iter().flatten() {
                if seen.insert(next) {
                    queue.push_back(next);
                }
            }
        }
        if seen.len() == corridor.wires.len() {
            valid_topologies.insert(owner);
        }
    }
    for &candidate in &candidates.ports {
        work = work.checked_add(1).ok_or("corridor event work overflow")?;
        if work > limit {
            return Err("corridor event work budget exceeded".into());
        }
        if work & 0x3fff == 0 && monitor.cancelled() {
            return Err("cancelled while lowering corridor events".into());
        }
        if region != u32::MAX && candidate.region != region {
            continue;
        }
        let port = &catalog.ports[candidate.row as usize];
        let (BoundaryOwner::Corridor(id), BoundaryDirection::Input, BoundaryPortKind::Callback { occurrence: entry }) =
            (port.owner, port.direction, port.kind)
        else {
            continue;
        };
        let corridor = &catalog.corridors[id as usize];
        let owner = cells + id as usize;
        if !valid_topologies.contains(&owner) {
            continue;
        }
        let delivery = graph.routes[entry.route.index()].deliveries[entry.ordinal as usize];
        let receiver = delivery
            .receiver
            .or_else(|| match (graph.sites[delivery.target.index()].source, delivery.kind) {
                (SiteSource::Component(node), DeliveryKind::Update { .. }) => Some(CallbackReceiver::Component(node)),
                _ => None,
            });
        let Some(CallbackReceiver::Component(entry_wire)) = receiver else {
            continue;
        };
        if !corridor.wires.contains(&entry_wire) || !matches!(delivery.kind, DeliveryKind::Update { .. }) {
            continue;
        }
        let wires = corridor.wires.iter().copied().collect::<FxHashSet<_>>();
        for edge in edges_by_source.get(&owner).into_iter().flatten() {
            work = work.checked_add(1).ok_or("corridor event work overflow")?;
            if work > limit {
                return Err("corridor event work budget exceeded".into());
            }
            let BoundaryPortKind::Callback { occurrence: notification } = edge.kind else {
                continue;
            };
            let Some(contract) = catalog.cells.get(edge.target) else { continue };
            let source_wire = graph.routes[notification.route.index()].source;
            if !wires.contains(&source_wire) {
                continue;
            }
            let delivery = graph.routes[notification.route.index()].deliveries[notification.ordinal as usize];
            let actor = match delivery.receiver {
                Some(CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor)) => Some(actor),
                Some(CallbackReceiver::Component(node)) => match graph.nodes[node.index()].kind {
                    CircuitNodeKind::Piston(actor) => Some(actor),
                    _ => None,
                },
                _ => None,
            };
            let Some(actor) = actor else { continue };
            if !contract
                .uses
                .iter()
                .any(|use_| use_.actor == actor && use_.activations.contains(&notification))
            {
                continue;
            }
            let mut queue = VecDeque::from([entry_wire]);
            let mut parent = FxHashMap::<NodeId, (NodeId, u32)>::default();
            let mut visited = FxHashSet::default();
            visited.insert(entry_wire);
            while let Some(node) = queue.pop_front() {
                work = work.checked_add(1).ok_or("corridor event work overflow")?;
                if work > limit {
                    return Err("corridor event work budget exceeded".into());
                }
                if work & 0x3fff == 0 && monitor.cancelled() {
                    return Err("cancelled while searching corridor events".into());
                }
                if node == source_wire {
                    break;
                }
                for &(next, link) in adjacency[&owner].get(&node).into_iter().flatten() {
                    if visited.insert(next) {
                        parent.insert(next, (node, link));
                        queue.push_back(next);
                    }
                }
            }
            if !visited.contains(&source_wire) {
                continue;
            }
            let mut links = Vec::new();
            let mut node = source_wire;
            while node != entry_wire {
                let &(prior, link) = parent.get(&node).ok_or("missing corridor predecessor")?;
                links.push(link);
                node = prior;
            }
            links.reverse();
            result.push(CorridorEvent {
                entry,
                entry_wire,
                source_wire,
                notification,
                actor,
                cell: edge.target as u32,
                links,
            });
        }
    }
    Ok(result)
}
