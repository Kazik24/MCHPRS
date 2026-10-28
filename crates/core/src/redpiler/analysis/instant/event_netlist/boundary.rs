//! Close one candidate's physical owners and derive its whole-region cuts.
use super::discovery::EventCandidates;
use crate::redpiler::TaskMonitor;
use crate::redpiler::analysis::instant::boundaries::{self, BoundaryScan, OwnerMap};
use crate::redpiler::analysis::instant::ownership::{self, OwnershipGroup};
use crate::redpiler::components::NodeType;
use crate::redpiler::program_graph::*;
use rustc_hash::{FxHashMap, FxHashSet};

/// This is a prospective cut, not an execution certificate or ownership transfer.
pub(crate) struct CandidateBoundary {
    pub(crate) owners: OwnerMap,
    pub(crate) groups: Vec<OwnershipGroup>,
    /// Complete original crossing rows and source routes, including non-power effects.
    pub(crate) scan: BoundaryScan,
    /// Native consumers to query in full, including their independently owned inputs.
    pub(crate) aggregate_targets: Vec<PowerTarget>,
    /// Resources directly read through GuardState or ContactAlternative crossings.
    /// Other crossing kinds still need their own representation before execution.
    pub(crate) direct_resource_reads: Vec<(ResourceId, ContactId)>,
}

/// Original outside callback occurrences grouped by their owned source node.
pub(crate) struct CallbackSource {
    pub(crate) node: NodeId,
    pub(crate) occurrences: Vec<RouteOccurrence>,
}

pub(crate) fn callback_sources(graph: &CompileGraph, boundary: &CandidateBoundary, monitor: &TaskMonitor) -> Result<Vec<CallbackSource>, String> {
    let mut by_node = FxHashMap::<NodeId, Vec<RouteOccurrence>>::default();
    for port in &boundary.scan.ports {
        if monitor.cancelled() {
            return Err("cancelled while indexing callback sources".into());
        }
        if let (BoundaryDirection::Output, BoundaryPortKind::Callback { occurrence }) = (port.direction, port.kind) {
            let route = graph.routes.get(occurrence.route.index()).ok_or("missing callback source route")?;
            let delivery = route.deliveries.get(occurrence.ordinal as usize).ok_or("missing callback occurrence")?;
            let receiver = delivery
                .receiver
                .or_else(|| match (graph.sites[delivery.target.index()].source, delivery.kind) {
                    (SiteSource::Component(node), DeliveryKind::Update { .. }) => Some(CallbackReceiver::Component(node)),
                    _ => None,
                });
            let Some(receiver) = receiver else { continue };
            if let CallbackReceiver::Component(node) = receiver {
                let kind = &graph.nodes[node.index()].kind;
                if matches!(delivery.kind, DeliveryKind::Shape { .. }) && !matches!(kind, CircuitNodeKind::Wire(_)) {
                    continue;
                }
                if matches!(
                    kind,
                    CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::PressurePlate | NodeType::Constant)
                ) {
                    continue;
                }
            }
            let node = route.source;
            if boundary.owners.nodes.get(node.index()).copied().flatten().is_none() {
                return Err(format!("callback route {:?} has no candidate source owner", occurrence.route));
            }
            by_node.entry(node).or_default().push(occurrence);
        }
    }
    let mut sources = by_node
        .into_iter()
        .map(|(node, mut occurrences)| {
            occurrences.sort_unstable_by_key(|occurrence| (occurrence.route, occurrence.ordinal));
            occurrences.dedup();
            CallbackSource { node, occurrences }
        })
        .collect::<Vec<_>>();
    sources.sort_unstable_by_key(|source| source.node.index());
    Ok(sources)
}

pub(crate) fn candidate_boundary(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    candidates: &EventCandidates,
    region: u32,
    monitor: &TaskMonitor,
) -> Result<CandidateBoundary, String> {
    let component = candidates.partition.regions.get(region as usize).ok_or("missing candidate region")?;
    let mut owners = candidates.partition.owners.clone();
    for table in [
        &mut owners.actors,
        &mut owners.resources,
        &mut owners.contacts,
        &mut owners.nodes,
        &mut owners.sites,
    ] {
        for owner in table {
            if monitor.cancelled() {
                return Err("cancelled while remapping candidate owners".into());
            }
            *owner = match *owner {
                Some(id)
                    if candidates.selected.get(id).copied().unwrap_or(false)
                        && candidates.partition.owner_region.get(id).copied() == Some(region) =>
                {
                    Some(0)
                }
                _ => None,
            };
        }
    }
    let mut actors = Vec::new();
    let mut sources = FxHashMap::default();
    for &id in component.cells.iter() {
        if !candidates.selected.get(id as usize).copied().unwrap_or(false) {
            continue;
        }
        let cell = catalog.cells.get(id as usize).ok_or("missing candidate cell")?;
        actors.extend(cell.uses.iter().map(|use_| use_.actor));
        for pin in cell.resources.iter() {
            if sources.insert(pin.resource, pin.source).is_some_and(|prior| prior != pin.source) {
                return Err(format!("resource {:?} has conflicting candidate sources", pin.resource));
            }
        }
    }
    let closure = ownership::close_seeded_ownership(graph, &[], &actors, monitor)?;
    for group in &closure.groups {
        for &actor in &group.actors {
            if owners.actors.get(actor.index()).copied().flatten() != Some(0) {
                return Err(format!(
                    "candidate region {region} has outside actor {actor:?} in a physical ownership group"
                ));
            }
        }
        for &resource in &group.resources {
            if owners.resources.get(resource.index()).copied().flatten() != Some(0) {
                return Err(format!(
                    "candidate region {region} has outside resource {resource:?} in a physical ownership group"
                ));
            }
            let spec = graph.resources.get(resource.index()).ok_or("missing physical resource")?;
            for &contact in spec.contacts.iter() {
                if owners.contacts.get(contact.index()).copied().flatten() != Some(0) {
                    return Err(format!("candidate region {region} has outside contact {contact:?} for {resource:?}"));
                }
            }
        }
    }
    let scan = boundaries::scan_boundaries(graph, &owners, monitor)?;
    let mut seen = FxHashSet::default();
    let mut aggregate_targets = Vec::new();
    let mut resources_by_contact = FxHashMap::<ContactId, Vec<ResourceId>>::default();
    for group in &closure.groups {
        for &resource in &group.resources {
            for &contact in graph.resources[resource.index()].contacts.iter() {
                resources_by_contact.entry(contact).or_default().push(resource);
            }
        }
    }
    let mut exposed = FxHashSet::default();
    for port in &scan.ports {
        if monitor.cancelled() {
            return Err("cancelled while deriving candidate output targets".into());
        }
        if let (BoundaryDirection::Output, BoundaryPortKind::Power { link }) = (port.direction, port.kind) {
            let target = graph.power_links.get(link as usize).ok_or("candidate power link is missing")?.to;
            if seen.insert(target) {
                aggregate_targets.push(target);
            }
        }
        if port.direction == BoundaryDirection::Output {
            match port.kind {
                BoundaryPortKind::GuardState {
                    test: StateTest::ResourceAt { resource, .. } | StateTest::ResourceEnabled(resource),
                    ..
                } if owners.resources.get(resource.index()).copied().flatten() == Some(0) => {
                    exposed.insert(resource);
                }
                BoundaryPortKind::ContactAlternative { contact, .. } => {
                    if let Some(resources) = resources_by_contact.get(&contact) {
                        exposed.extend(resources.iter().copied());
                    }
                }
                _ => {}
            }
        }
    }
    let mut direct_resource_reads = exposed
        .into_iter()
        .map(|resource| {
            sources
                .get(&resource)
                .copied()
                .map(|source| (resource, source))
                .ok_or_else(|| format!("exposed resource {resource:?} has no candidate source"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    direct_resource_reads.sort_unstable_by_key(|(resource, _)| resource.index());
    Ok(CandidateBoundary {
        owners,
        groups: closure.groups,
        scan,
        aggregate_targets,
        direct_resource_reads,
    })
}
