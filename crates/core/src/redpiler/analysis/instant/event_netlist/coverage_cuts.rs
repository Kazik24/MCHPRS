//! Conservative callback coverage cuts for the experimental frame compiler.
use super::corridor_events::{CorridorEvent, corridor_events};
use super::native_cuts::{ResourceCuts, cut_unsupported_resources};
use crate::redpiler::TaskMonitor;
use crate::redpiler::components::NodeType;
use crate::redpiler::program_graph::*;
use rustc_hash::FxHashSet;

/// This only certifies represented incoming callback occurrences. It is not an
/// execution certificate: outgoing effects, event application and handoff still
/// require their own checks before any ownership can transfer.
pub(crate) struct CoveredCandidates {
    pub(crate) resources: ResourceCuts,
    pub(crate) corridor_events: Vec<CorridorEvent>,
    pub(crate) cut_owners: Vec<(usize, RouteOccurrence)>,
    pub(crate) rounds: usize,
}

/// A missing occurrence cuts its complete catalog owner, then discovery and
/// resource closure run again; unrelated native reads never transfer ownership.
pub(crate) fn cut_uncovered_callbacks(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    eligible: &[bool],
    monitor: &TaskMonitor,
) -> Result<CoveredCandidates, String> {
    let mut eligible = eligible.to_vec();
    let mut cut_owners = Vec::new();
    let max_rounds = 64usize.saturating_mul(monitor.budget_multiplier());
    for round in 0..max_rounds {
        if monitor.cancelled() {
            return Err("cancelled while cutting uncovered callbacks".into());
        }
        let resources = cut_unsupported_resources(graph, catalog, &eligible, monitor)?;
        for (id, selected) in resources.candidates.selected.iter().copied().enumerate() {
            eligible[id] &= selected;
        }
        let events = corridor_events(graph, catalog, &resources.candidates, u32::MAX, monitor)?;
        let mut covered = FxHashSet::default();
        for event in &events {
            let owner = resources.candidates.partition.owners.nodes[event.entry_wire.index()].ok_or("corridor entry has no owner")?;
            covered.insert((owner, event.entry));
            covered.insert((event.cell as usize, event.notification));
        }
        // Direct source-qualified piston samples are representable by the same
        // cell contract, even when there is no preceding selected corridor.
        for (id, cell) in catalog.cells.iter().enumerate() {
            if !resources.candidates.selected[id] {
                continue;
            }
            for use_ in &cell.uses {
                if use_.retraction.transfer.is_none() {
                    continue;
                }
                for &occurrence in &use_.activations {
                    let delivery = graph.routes[occurrence.route.index()].deliveries[occurrence.ordinal as usize];
                    if !matches!(delivery.kind, DeliveryKind::Update { .. }) {
                        continue;
                    }
                    let receiver = delivery
                        .receiver
                        .or_else(|| match (graph.sites[delivery.target.index()].source, delivery.kind) {
                            (SiteSource::Component(node), DeliveryKind::Update { .. }) => Some(CallbackReceiver::Component(node)),
                            _ => None,
                        });
                    let actor = match receiver {
                        Some(CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor)) => Some(actor),
                        Some(CallbackReceiver::Component(node)) => match graph.nodes[node.index()].kind {
                            CircuitNodeKind::Piston(actor) => Some(actor),
                            _ => None,
                        },
                        _ => None,
                    };
                    if actor == Some(use_.actor) {
                        covered.insert((id, occurrence));
                    }
                }
            }
        }
        let mut missing = Vec::new();
        let mut marked = FxHashSet::default();
        for candidate in &resources.candidates.ports {
            if monitor.cancelled() {
                return Err("cancelled while checking callback coverage".into());
            }
            let port = catalog.ports[candidate.row as usize];
            let (BoundaryDirection::Input, BoundaryPortKind::Callback { occurrence }) = (port.direction, port.kind) else {
                continue;
            };
            let owner = match port.owner {
                BoundaryOwner::Cell(id) => id as usize,
                BoundaryOwner::Corridor(id) => catalog.cells.len() + id as usize,
            };
            let route = &graph.routes[occurrence.route.index()];
            if let Some(source_owner) = resources.candidates.partition.owners.nodes[route.source.index()]
                && resources.candidates.selected[source_owner]
                && resources.candidates.partition.owner_region[source_owner] == candidate.region
            {
                // This is internal to the selected region; its propagation is
                // a separate executable-worklist requirement.
                continue;
            }
            let delivery = route.deliveries[occurrence.ordinal as usize];
            if matches!(graph.guards[delivery.guard.index()], Guard::Not(id) if matches!(graph.guards[id.index()], Guard::Always)) {
                continue;
            }
            let receiver = delivery
                .receiver
                .or_else(|| match (graph.sites[delivery.target.index()].source, delivery.kind) {
                    (SiteSource::Component(node), DeliveryKind::Update { .. }) => Some(CallbackReceiver::Component(node)),
                    _ => None,
                });
            match receiver {
                None | Some(CallbackReceiver::Contact(_)) => continue,
                Some(CallbackReceiver::Component(node)) => {
                    let kind = &graph.nodes[node.index()].kind;
                    if matches!(kind, CircuitNodeKind::Contact(_)) {
                        continue;
                    }
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
                Some(CallbackReceiver::PistonBase(_) | CallbackReceiver::PistonHead(_)) => {}
            }
            if !covered.contains(&(owner, occurrence)) && marked.insert(owner) {
                missing.push((owner, occurrence));
            }
        }
        if missing.is_empty() {
            return Ok(CoveredCandidates {
                resources,
                corridor_events: events,
                cut_owners,
                rounds: round + 1,
            });
        }
        missing.sort_unstable_by_key(|(owner, _)| *owner);
        for (owner, occurrence) in missing {
            if !eligible[owner] {
                return Err("callback cut did not reduce eligibility".into());
            }
            eligible[owner] = false;
            cut_owners.push((owner, occurrence));
        }
    }
    Err("callback-coverage fixed point exceeded its round budget".into())
}
