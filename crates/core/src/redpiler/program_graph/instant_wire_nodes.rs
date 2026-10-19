//! Closed local signal transfers; all callbacks only queue work, so captures stay stable.
use super::*;
use rustc_hash::FxHashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub(crate) struct InstantWireNode {
    /// Canonical receiver slots; execution retains each receiver's first qualifying sample.
    pub(crate) receivers: u32,
}

pub(crate) fn infer_instant_wire_nodes(graph: &CompileGraph, monitor: &crate::redpiler::TaskMonitor) -> Result<Vec<Option<InstantWireNode>>, String> {
    let mechanisms = crate::redpiler::passes::utils::instant_mechanisms::discover_mechanisms(graph, monitor)?;
    let actors: FxHashSet<_> = mechanisms.iter().map(|cell| cell.piston).collect();
    let resets: FxHashSet<_> = mechanisms.iter().map(|cell| cell.observer).collect();
    let mut outgoing: FxHashMap<SiteId, Vec<PowerTarget>> = FxHashMap::default();
    for link in &graph.power_links {
        if super::wire_callback_selection::fixed_live_guard(graph, link.guard) != Some(false) {
            outgoing.entry(link.from).or_default().push(link.to);
        }
    }
    let mut aliases: FxHashMap<NodeId, Vec<SiteId>> = FxHashMap::default();
    for (id, site) in graph.sites.iter().enumerate() {
        if let SiteSource::Component(node) = site.source {
            aliases.entry(node).or_default().push(SiteId::new(id as u32));
        }
    }
    let owned = owned_wires(graph);
    let mut result = vec![None; graph.wires.len()];
    for (index, wire) in graph.wires.iter().enumerate() {
        if monitor.cancelled() {
            return Err("cancelled while compiling instant wire nodes".into());
        }
        if owned.contains(&wire.node)
            || wire.execution.instant_reset.is_some()
            || !wire.execution.query_facts.unwrap_or_else(|| WireQueryFacts::infer(graph, wire)).isolated
        {
            continue;
        }
        if !closed_power(graph, wire.node, &aliases, &outgoing, &actors) {
            continue;
        }
        let Some(route) = wire.turbo_route else { continue };
        let mut receivers = 0;
        let mut closed = true;
        for (slot, &site) in wire.canonical_neighbors.iter().enumerate() {
            let mut accept = |receiver| match receiver {
                CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor) if actors.contains(&actor) => receivers |= 1 << slot,
                CallbackReceiver::Component(node) => match graph.nodes[node.index()].kind {
                    CircuitNodeKind::Piston(actor) if actors.contains(&actor) => receivers |= 1 << slot,
                    CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. } if resets.contains(&node) => receivers |= 1 << slot,
                    CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::PressurePlate) => {}
                    _ => closed = false,
                },
                CallbackReceiver::Contact(_) => {}
                _ => closed = false,
            };
            match graph.sites[site.index()].source {
                SiteSource::Constant(_) => {}
                SiteSource::Component(node) => accept(CallbackReceiver::Component(node)),
                SiteSource::Contact(contact) => {
                    for receiver in graph.contacts[contact.index()].variants.iter().filter_map(|variant| variant.receiver) {
                        accept(receiver);
                    }
                }
            }
            for row in graph.routes[route.index()].deliveries.iter().filter(|row| row.target == site) {
                if let Some(receiver) = row.receiver {
                    accept(receiver);
                }
            }
        }
        if closed && receivers != 0 {
            result[index] = Some(InstantWireNode { receivers });
        }
    }
    Ok(result)
}

fn closed_power(
    graph: &CompileGraph,
    node: NodeId,
    aliases: &FxHashMap<NodeId, Vec<SiteId>>,
    outgoing: &FxHashMap<SiteId, Vec<PowerTarget>>,
    actors: &FxHashSet<PistonId>,
) -> bool {
    closed_net_power(graph, node, None, aliases, outgoing, actors)
}

fn closed_net_power(
    graph: &CompileGraph,
    node: NodeId,
    members: Option<&FxHashSet<NodeId>>,
    aliases: &FxHashMap<NodeId, Vec<SiteId>>,
    outgoing: &FxHashMap<SiteId, Vec<PowerTarget>>,
    actors: &FxHashSet<PistonId>,
) -> bool {
    let mut stack = aliases.get(&node).cloned().unwrap_or_default();
    let mut seen = FxHashSet::default();
    while let Some(site) = stack.pop() {
        if !seen.insert(site) {
            continue;
        }
        for target in outgoing.get(&site).into_iter().flatten() {
            match *target {
                PowerTarget::Conductor { site, .. } => stack.push(site),
                PowerTarget::Input { node: target, .. } if target == node => {}
                PowerTarget::Input { node: target, .. } if members.is_some_and(|members| members.contains(&target)) => {}
                PowerTarget::Input { node: target, .. } if matches!(graph.nodes[target.index()].kind, CircuitNodeKind::Piston(actor) if actors.contains(&actor)) =>
                    {}
                _ => return false,
            }
        }
    }
    true
}

#[derive(Debug)]
pub(crate) struct ClosedInstantNet {
    pub(crate) corridor: u32,
    pub(crate) wires: usize,
    pub(crate) actors: Vec<PistonId>,
}

#[derive(Debug, Default)]
pub(crate) struct ClosedInstantNetCensus {
    pub(crate) closed: Vec<ClosedInstantNet>,
    pub(crate) declines: std::collections::BTreeMap<&'static str, (usize, usize)>,
}

pub(crate) fn infer_abstract_instant_nets(graph: &CompileGraph, monitor: &crate::redpiler::TaskMonitor) -> Result<Vec<bool>, String> {
    if monitor.cancelled() {
        return Err("cancelled before abstract net discovery".into());
    }
    let mut masks = vec![false; graph.wires.len()];
    let Some(catalog) = &graph.plans.instant.boundaries else {
        return Ok(masks);
    };
    let census = census_closed_instant_nets(graph, catalog, monitor)?;
    for net in census.closed.iter().filter(|net| net.wires > 1) {
        debug_assert!(!net.actors.is_empty());
        for &node in &catalog.corridors[net.corridor as usize].wires {
            let CircuitNodeKind::Wire(index) = graph.nodes[node.index()].kind else {
                unreachable!()
            };
            masks[index] = true;
        }
    }
    Ok(masks)
}

/// Structural closure is a candidate boundary, not proof that final-state settling preserves requests.
pub(crate) fn census_closed_instant_nets(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    monitor: &crate::redpiler::TaskMonitor,
) -> Result<ClosedInstantNetCensus, String> {
    let actors: FxHashSet<_> = catalog.cells.iter().flat_map(|cell| cell.uses.iter().map(|use_| use_.actor)).collect();
    let resets: FxHashSet<_> = catalog
        .cells
        .iter()
        .flat_map(|cell| cell.uses.iter().filter_map(|use_| use_.reset.observer()))
        .collect();
    let allowed_contacts: FxHashSet<_> = catalog
        .cells
        .iter()
        .flat_map(|cell| cell.resources.iter())
        .flat_map(|pin| graph.resources[pin.resource.index()].contacts.iter().copied())
        .chain(actors.iter().flat_map(|actor| {
            let piston = graph.pistons[actor.index()];
            [piston.base, piston.near, piston.far]
        }))
        .collect();
    let incoming = crate::redpiler::passes::utils::instant_mechanisms::index_power(graph);
    let mut outgoing: FxHashMap<SiteId, Vec<PowerTarget>> = FxHashMap::default();
    for link in &graph.power_links {
        if super::wire_callback_selection::fixed_live_guard(graph, link.guard) != Some(false) {
            outgoing.entry(link.from).or_default().push(link.to);
        }
    }
    let mut aliases: FxHashMap<NodeId, Vec<SiteId>> = FxHashMap::default();
    for (id, site) in graph.sites.iter().enumerate() {
        if let SiteSource::Component(node) = site.source {
            aliases.entry(node).or_default().push(SiteId::new(id as u32));
        }
    }
    let owned = owned_wires(graph);
    let mut census = ClosedInstantNetCensus::default();
    for (id, corridor) in catalog.corridors.iter().enumerate() {
        if monitor.cancelled() {
            return Err("cancelled while checking closed instant nets".into());
        }
        let members: FxHashSet<_> = corridor.wires.iter().copied().collect();
        if corridor.wires.iter().any(|node| {
            owned.contains(node)
                || matches!(graph.nodes[node.index()].kind,CircuitNodeKind::Wire(index) if graph.wires[index].execution.instant_reset.is_some())
        }) {
            let entry = census.declines.entry("overlapping_execution_owner").or_default();
            entry.0 += 1;
            entry.1 += members.len();
            continue;
        }
        let mut readers = FxHashSet::default();
        let mut issue = None;
        for &node in &corridor.wires {
            if !closed_instant_sources(graph, node, &members, &actors, &resets, &allowed_contacts, &incoming) {
                issue.get_or_insert("outside_signal_source");
            }
            if !closed_net_power(graph, node, Some(&members), &aliases, &outgoing, &actors) {
                issue.get_or_insert("outside_electrical_consumer");
            }
            let CircuitNodeKind::Wire(index) = graph.nodes[node.index()].kind else {
                return Err("corridor contains a non-wire".into());
            };
            let wire = &graph.wires[index];
            let mut accept = |receiver| match receiver {
                CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor) if actors.contains(&actor) => {
                    readers.insert(actor);
                }
                CallbackReceiver::Component(node) if members.contains(&node) => {}
                CallbackReceiver::Component(node) => match graph.nodes[node.index()].kind {
                    CircuitNodeKind::Piston(actor) if actors.contains(&actor) => {
                        readers.insert(actor);
                    }
                    CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. } if resets.contains(&node) => {}
                    CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::PressurePlate) => {}
                    CircuitNodeKind::Wire(_) => {
                        issue.get_or_insert("outside_wire_callback");
                    }
                    _ => {
                        issue.get_or_insert("classic_or_unrecognized_receiver");
                    }
                },
                CallbackReceiver::Contact(_) => {}
                _ => {
                    issue.get_or_insert("unrecognized_piston_receiver");
                }
            };
            let admission = wire.execution.callback_selection.or_else(|| WireCallbackSelection::infer(graph, index));
            for (slot, &site) in wire.canonical_neighbors.iter().enumerate() {
                if admission.is_some_and(|plan| plan.known & (1 << slot) != 0 && plan.admitted & (1 << slot) == 0) {
                    continue;
                }
                match graph.sites[site.index()].source {
                    SiteSource::Constant(_) => {}
                    SiteSource::Component(node) => accept(CallbackReceiver::Component(node)),
                    SiteSource::Contact(contact) => {
                        for receiver in graph.contacts[contact.index()].variants.iter().filter_map(|variant| variant.receiver) {
                            accept(receiver);
                        }
                    }
                }
            }
        }
        if readers.is_empty() {
            issue.get_or_insert("no_instant_receiver");
        }
        if let Some(issue) = issue {
            let entry = census.declines.entry(issue).or_default();
            entry.0 += 1;
            entry.1 += members.len();
        } else {
            let mut actors: Vec<_> = readers.into_iter().collect();
            actors.sort_unstable();
            census.closed.push(ClosedInstantNet {
                corridor: id as u32,
                wires: members.len(),
                actors,
            });
        }
    }
    Ok(census)
}

fn owned_wires(graph: &CompileGraph) -> FxHashSet<NodeId> {
    graph
        .plans
        .dust_paths
        .iter()
        .flat_map(|path| path.wires.iter().copied())
        .chain(graph.plans.classic_nets.iter().flat_map(|net| net.wires.iter().map(|wire| wire.node)))
        .chain(
            graph
                .plans
                .instant
                .proxies
                .iter()
                .flat_map(|proxy| proxy.routes.iter())
                .filter_map(|route| {
                    let source = graph.routes[route.route.index()].source;
                    matches!(graph.nodes[source.index()].kind, CircuitNodeKind::Wire(_)).then_some(source)
                }),
        )
        .collect()
}

fn closed_instant_sources(
    graph: &CompileGraph,
    node: NodeId,
    members: &FxHashSet<NodeId>,
    actors: &FxHashSet<PistonId>,
    resets: &FxHashSet<NodeId>,
    contacts: &FxHashSet<ContactId>,
    incoming: &crate::redpiler::passes::utils::instant_mechanisms::PowerIndex,
) -> bool {
    let mut stack = vec![PowerTarget::Input { node, port: InputPort::Main }];
    let mut seen = FxHashSet::default();
    while let Some(target) = stack.pop() {
        if !seen.insert(target) {
            continue;
        }
        for link in incoming.get(&target).into_iter().flatten() {
            if link.attenuation >= 15 || super::wire_callback_selection::fixed_live_guard(graph, link.guard) == Some(false) {
                continue;
            }
            if let PowerPort::Conducted { include_dust } = link.from_port {
                stack.push(PowerTarget::Conductor {
                    site: link.from,
                    include_dust,
                });
                continue;
            }
            match graph.sites[link.from.index()].source {
                SiteSource::Constant(_) => {}
                SiteSource::Contact(contact)
                    if contacts.contains(&contact)
                        || graph.contacts[contact.index()]
                            .variants
                            .iter()
                            .all(|variant| graph.profiles[variant.profile.index()].electrical == ProfileElectrical::None) => {}
                SiteSource::Component(node) => match graph.nodes[node.index()].kind {
                    CircuitNodeKind::Wire(_) if members.contains(&node) => {}
                    CircuitNodeKind::Piston(actor) if actors.contains(&actor) => {}
                    CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. } if resets.contains(&node) => {}
                    CircuitNodeKind::Electrical(NodeType::Constant | NodeType::Lever | NodeType::Button | NodeType::PressurePlate) => {}
                    _ => return false,
                },
                _ => return false,
            }
        }
    }
    true
}
