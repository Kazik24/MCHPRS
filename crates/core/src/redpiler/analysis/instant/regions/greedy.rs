//! Input-event candidate discovery over existing boundary witnesses.
use super::*;

/// A witnessed producer-to-consumer dependency; this is not an execution proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct EventEdge {
    pub(crate) source: usize,
    pub(crate) target: usize,
    pub(crate) kind: BoundaryPortKind,
}

/// Select possible computation owners from exact incoming events; reads remain ports.
pub(crate) fn greedy_event_owners(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    eligible: &[bool],
    retained_bytes: usize,
    monitor: &TaskMonitor,
) -> Result<(Vec<bool>, Vec<RouteOccurrence>), String> {
    let (selected, occurrences, _) = greedy_event_owners_impl(graph, catalog, eligible, retained_bytes, monitor, false)?;
    Ok((selected, occurrences))
}

/// Preserve the particular owner pairs reached by the event worklist.
#[cfg(test)]
pub(crate) fn greedy_event_pairs(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    eligible: &[bool],
    retained_bytes: usize,
    monitor: &TaskMonitor,
) -> Result<(Vec<bool>, Vec<EventEdge>), String> {
    let (selected, _, edges) = greedy_event_owners_impl(graph, catalog, eligible, retained_bytes, monitor, true)?;
    Ok((selected, edges))
}

fn greedy_event_owners_impl(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    eligible: &[bool],
    retained_bytes: usize,
    monitor: &TaskMonitor,
    capture_pairs: bool,
) -> Result<(Vec<bool>, Vec<RouteOccurrence>, Vec<EventEdge>), String> {
    let count = catalog
        .cells
        .len()
        .checked_add(catalog.corridors.len())
        .ok_or("instant owner count overflow")?;
    if eligible.len() != count {
        return Err("instant event candidate table has stale owner dimensions".into());
    }
    let mut budget = WorkBudget::new(monitor);
    budget.retain_bytes(retained_bytes)?;
    budget.retain_bytes(eligible.len())?;
    budget.retain_bytes(
        count
            .checked_mul(
                std::mem::size_of::<Vec<(BoundaryPortKind, &[usize])>>() + std::mem::size_of::<Vec<usize>>() + 4 + std::mem::size_of::<usize>(),
            )
            .ok_or("instant event index size overflow")?,
    )?;
    let event_index = EventIndex::new(catalog, eligible, &mut budget)?;
    let fans = fan::index(catalog.cells.len(), count, &catalog.ports, &mut budget, |owner, kind| {
        eligible[owner] && matches!(kind, BoundaryPortKind::Callback { .. } | BoundaryPortKind::Power { .. })
    })?;
    let mut outgoing = vec![Vec::<(BoundaryPortKind, &[usize])>::new(); count];
    let mut queue = std::collections::VecDeque::new();
    let mut enqueued = FxHashSet::default();
    for (&kind, fan) in &fans {
        budget.visit()?;
        if let BoundaryPortKind::Callback { occurrence } = kind {
            if fan.outputs.is_empty() {
                for &owner in &fan.inputs {
                    if event_index.accepts(catalog, owner, occurrence) {
                        enqueue_event(&mut queue, &mut enqueued, owner, occurrence, &mut budget)?;
                    }
                }
            }
        }
        for &owner in &fan.outputs {
            budget.visit()?;
            budget.retain_bytes(2 * std::mem::size_of::<(BoundaryPortKind, &[usize])>())?;
            outgoing[owner].push((kind, &fan.inputs));
        }
    }
    let mut reached = vec![false; count];
    let mut useful = vec![false; count];
    let mut predecessors = vec![Vec::<usize>::new(); count];
    let mut power_edges = Vec::<(usize, &[usize])>::new();
    let mut power_edge_links = capture_pairs.then(Vec::<u32>::new);
    let mut power_seen = FxHashSet::default();
    let mut predecessor_seen = FxHashSet::default();
    let mut executable_edges = FxHashSet::default();
    let mut owner_edges = capture_pairs.then(FxHashSet::<EventEdge>::default);
    let mut expanded_uses = FxHashSet::default();
    let mut expanded_internal_routes = FxHashSet::default();
    while let Some((owner, incoming)) = queue.pop_front() {
        budget.visit()?;
        if owner >= catalog.cells.len() && reached[owner] {
            continue;
        }
        reached[owner] = true;
        if owner < catalog.cells.len() {
            let mut fresh = false;
            for &index in event_index.uses.get(&(owner, incoming)).into_iter().flatten() {
                budget.visit()?;
                if expanded_uses.insert((owner, index)) {
                    budget.retain_bytes(4 * std::mem::size_of::<(usize, usize)>())?;
                    fresh = true;
                    let routes = &catalog.cells[owner].uses[index].retraction.routes;
                    for route in [Some(routes.base_changed), Some(routes.near_changed), routes.far_changed]
                        .into_iter()
                        .flatten()
                    {
                        if !expanded_internal_routes.insert((owner, route)) {
                            continue;
                        }
                        budget.retain_bytes(4 * std::mem::size_of::<(usize, RouteId)>())?;
                        for &occurrence in event_index.internal.get(&(owner, route)).into_iter().flatten() {
                            budget.visit()?;
                            enqueue_event(&mut queue, &mut enqueued, owner, occurrence, &mut budget)?;
                        }
                    }
                }
            }
            if !fresh {
                continue;
            }
        }
        for (kind, targets) in &outgoing[owner] {
            budget.visit()?;
            let occurrence = match *kind {
                BoundaryPortKind::Power { link } => {
                    if !power_seen.contains(&(owner, link))
                        && event_index.changes_power_source(graph, catalog, owner, incoming, link)
                        && power_seen.insert((owner, link))
                    {
                        budget.retain_bytes(4 * std::mem::size_of::<(usize, u32)>())?;
                        budget.retain_bytes(2 * std::mem::size_of::<(usize, &[usize])>())?;
                        power_edges.push((owner, targets));
                        if let Some(links) = &mut power_edge_links {
                            budget.retain_bytes(2 * std::mem::size_of::<u32>())?;
                            links.push(link);
                        }
                    }
                    continue;
                }
                BoundaryPortKind::Callback { occurrence } => occurrence,
                _ => continue,
            };
            let phase = event_index.phase(graph, catalog, owner, incoming, occurrence);
            match phase {
                EventPhase::Unrelated => continue,
                EventPhase::Deferred => {
                    useful[owner] = true;
                    continue;
                }
                EventPhase::Immediate => {}
            }
            let mut connected = false;
            for &target in *targets {
                budget.visit()?;
                if event_index.accepts(catalog, target, occurrence) {
                    if executable_edges.insert(occurrence) {
                        budget.retain_bytes(4 * std::mem::size_of::<RouteOccurrence>())?;
                    }
                    add_predecessor(&mut predecessors, &mut predecessor_seen, target, owner, &mut budget)?;
                    record_owner_edge(
                        &mut owner_edges,
                        EventEdge {
                            source: owner,
                            target,
                            kind: *kind,
                        },
                        &mut budget,
                    )?;
                    enqueue_event(&mut queue, &mut enqueued, target, occurrence, &mut budget)?;
                    connected = true;
                } else {
                    useful[owner] = true;
                }
            }
            if !connected && targets.is_empty() {
                useful[owner] = true;
            }
        }
    }
    for (index, (owner, targets)) in power_edges.into_iter().enumerate() {
        budget.visit()?;
        if targets.is_empty() {
            useful[owner] = true;
        }
        for &target in targets {
            budget.visit()?;
            if reached[target] {
                add_predecessor(&mut predecessors, &mut predecessor_seen, target, owner, &mut budget)?;
                if let Some(links) = &power_edge_links {
                    record_owner_edge(
                        &mut owner_edges,
                        EventEdge {
                            source: owner,
                            target,
                            kind: BoundaryPortKind::Power { link: links[index] },
                        },
                        &mut budget,
                    )?;
                }
            } else {
                useful[owner] = true;
            }
        }
    }
    let mut pending = useful
        .iter()
        .enumerate()
        .filter_map(|(owner, &yes)| yes.then_some(owner))
        .collect::<Vec<_>>();
    while let Some(owner) = pending.pop() {
        budget.visit()?;
        for &previous in &predecessors[owner] {
            if reached[previous] && !useful[previous] {
                useful[previous] = true;
                pending.push(previous);
            }
        }
    }
    budget.retain_bytes(
        executable_edges
            .len()
            .checked_mul(std::mem::size_of::<RouteOccurrence>())
            .ok_or("instant edge list size overflow")?,
    )?;
    let mut edges = executable_edges.into_iter().collect::<Vec<_>>();
    edges.sort_unstable_by_key(|occurrence| (occurrence.route, occurrence.ordinal));
    let selected: Vec<_> = reached.into_iter().zip(useful).map(|(reached, useful)| reached && useful).collect();
    let mut pairs = Vec::new();
    for edge in owner_edges.unwrap_or_default() {
        budget.visit()?;
        if selected[edge.source] && selected[edge.target] {
            budget.retain_bytes(2 * std::mem::size_of::<EventEdge>())?;
            pairs.push(edge);
        }
    }
    pairs.sort_unstable_by_key(|edge| {
        let key = match edge.kind {
            BoundaryPortKind::Power { link } => (0, link as usize, 0),
            BoundaryPortKind::Callback { occurrence } => (1, occurrence.route.index(), occurrence.ordinal),
            _ => unreachable!("only event and power dependencies are retained"),
        };
        (edge.source, edge.target, key)
    });
    budget.check_cancelled()?;
    Ok((selected, edges, pairs))
}

fn record_owner_edge(edges: &mut Option<FxHashSet<EventEdge>>, edge: EventEdge, budget: &mut WorkBudget<'_>) -> Result<(), String> {
    if let Some(edges) = edges
        && !edges.contains(&edge)
    {
        budget.retain_bytes(4 * std::mem::size_of::<EventEdge>())?;
        edges.insert(edge);
    }
    Ok(())
}

fn enqueue_event(
    queue: &mut std::collections::VecDeque<(usize, RouteOccurrence)>,
    enqueued: &mut FxHashSet<(usize, RouteOccurrence)>,
    owner: usize,
    occurrence: RouteOccurrence,
    budget: &mut WorkBudget<'_>,
) -> Result<(), String> {
    if enqueued.insert((owner, occurrence)) {
        budget.retain_bytes(6 * std::mem::size_of::<(usize, RouteOccurrence)>())?;
        queue.push_back((owner, occurrence));
    }
    Ok(())
}

fn add_predecessor(
    predecessors: &mut [Vec<usize>],
    seen: &mut FxHashSet<(usize, usize)>,
    owner: usize,
    previous: usize,
    budget: &mut WorkBudget<'_>,
) -> Result<(), String> {
    if seen.insert((owner, previous)) {
        budget.retain_bytes(4 * std::mem::size_of::<(usize, usize)>() + 2 * std::mem::size_of::<usize>())?;
        predecessors[owner].push(previous);
    }
    Ok(())
}

pub(super) struct EventIndex {
    uses: FxHashMap<(usize, RouteOccurrence), Vec<usize>>,
    internal: FxHashMap<(usize, RouteId), Vec<RouteOccurrence>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum EventPhase {
    Unrelated,
    Immediate,
    Deferred,
}

impl EventIndex {
    pub(super) fn new(catalog: &BoundaryCatalog, eligible: &[bool], budget: &mut WorkBudget<'_>) -> Result<Self, String> {
        let mut uses = FxHashMap::<(usize, RouteOccurrence), Vec<usize>>::default();
        let mut internal = FxHashMap::<(usize, RouteId), Vec<RouteOccurrence>>::default();
        for (owner, cell) in catalog.cells.iter().enumerate() {
            if !eligible[owner] {
                continue;
            }
            for (index, use_) in cell.uses.iter().enumerate() {
                for &event in &use_.activations {
                    budget.visit()?;
                    let key = (owner, event);
                    if !uses.contains_key(&key) {
                        budget.retain_bytes(4 * std::mem::size_of::<((usize, RouteOccurrence), Vec<usize>)>())?;
                        let route_key = (owner, event.route);
                        if !internal.contains_key(&route_key) {
                            budget.retain_bytes(4 * std::mem::size_of::<((usize, RouteId), Vec<RouteOccurrence>)>())?;
                        }
                        budget.retain_bytes(2 * std::mem::size_of::<RouteOccurrence>())?;
                        internal.entry(route_key).or_default().push(event);
                    }
                    budget.retain_bytes(2 * std::mem::size_of::<usize>())?;
                    uses.entry(key).or_default().push(index);
                }
            }
        }
        Ok(Self { uses, internal })
    }

    pub(super) fn accepts(&self, catalog: &BoundaryCatalog, owner: usize, occurrence: RouteOccurrence) -> bool {
        owner >= catalog.cells.len() || self.uses.contains_key(&(owner, occurrence))
    }

    fn changes_power_source(&self, graph: &CompileGraph, catalog: &BoundaryCatalog, owner: usize, incoming: RouteOccurrence, link: u32) -> bool {
        if owner >= catalog.cells.len() {
            return true;
        }
        let Some(source) = graph
            .power_links
            .get(link as usize)
            .and_then(|link| graph.sites.get(link.from.index()))
            .map(|site| site.source)
        else {
            return false;
        };
        self.uses.get(&(owner, incoming)).into_iter().flatten().any(|&index| {
            let use_ = &catalog.cells[owner].uses[index];
            let Some(piston) = graph.pistons.get(use_.actor.index()) else {
                return false;
            };
            match source {
                SiteSource::Contact(contact) => {
                    [piston.base, piston.near, piston.far].contains(&contact)
                        || use_
                            .retraction
                            .transfer
                            .is_some_and(|transfer| [transfer.source, transfer.destination].contains(&contact))
                }
                SiteSource::Component(node) => node == piston.node || use_.reset.observer() == Some(node),
                SiteSource::Constant(_) => false,
            }
        })
    }

    pub(super) fn phase(
        &self,
        graph: &CompileGraph,
        catalog: &BoundaryCatalog,
        owner: usize,
        incoming: RouteOccurrence,
        outgoing: RouteOccurrence,
    ) -> EventPhase {
        if owner >= catalog.cells.len() {
            return EventPhase::Immediate;
        }
        let mut phase = EventPhase::Unrelated;
        for &index in self.uses.get(&(owner, incoming)).into_iter().flatten() {
            let use_ = &catalog.cells[owner].uses[index];
            let effect = &use_.retraction;
            let route = outgoing.route;
            if route == effect.routes.base_changed || effect.routes.far_changed == Some(route) || route == effect.routes.near_changed {
                return EventPhase::Immediate;
            }
            if route == effect.wake.base_restored
                || effect.wake.resource.is_some_and(|wake| wake.route == route)
                || use_
                    .reset
                    .observer()
                    .is_some_and(|observer| graph.nodes.get(observer.index()).and_then(|node| node.output_route) == Some(route))
                || matches!(&use_.reset,
                    CellReset::Payload { notification, sample, .. } | CellReset::InputRestoration { notification, sample, .. }
                    if *notification == outgoing || *sample == outgoing)
            {
                phase = EventPhase::Deferred;
            }
        }
        phase
    }
}
