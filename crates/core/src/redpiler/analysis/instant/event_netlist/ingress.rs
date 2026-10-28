//! Source-qualified actor activations crossing one prospective region boundary.
use super::boundary::CandidateBoundary;
use crate::redpiler::TaskMonitor;
use crate::redpiler::program_graph::*;
use rustc_hash::FxHashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct IngressActivation {
    pub(crate) occurrence: RouteOccurrence,
    pub(crate) actor: PistonId,
    pub(crate) cell: u32,
}

/// Retains only actual actor callback occurrences, not power reads or discovery order.
/// Dynamic contact receivers remain unresolved and must stay on the native path.
pub(crate) fn actor_ingress(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    boundary: &CandidateBoundary,
    monitor: &TaskMonitor,
) -> Result<(Vec<IngressActivation>, usize), String> {
    let mut incoming = FxHashSet::default();
    for port in &boundary.scan.ports {
        if monitor.cancelled() {
            return Err("cancelled while indexing actor ingress".into());
        }
        if let (BoundaryDirection::Input, BoundaryPortKind::Callback { occurrence }) = (port.direction, port.kind) {
            incoming.insert(occurrence);
        }
    }
    let mut accepted = FxHashSet::default();
    let mut unresolved = 0;
    for (cell, contract) in catalog.cells.iter().enumerate() {
        for use_ in &contract.uses {
            if boundary.owners.actors.get(use_.actor.index()).copied().flatten().is_none() {
                continue;
            }
            for &occurrence in &use_.activations {
                if !incoming.contains(&occurrence) {
                    continue;
                }
                let route = graph.routes.get(occurrence.route.index()).ok_or("actor ingress route is missing")?;
                let delivery = route
                    .deliveries
                    .get(occurrence.ordinal as usize)
                    .ok_or("actor ingress occurrence is missing")?;
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
                    accepted.insert(IngressActivation {
                        occurrence,
                        actor: use_.actor,
                        cell: cell as u32,
                    });
                } else {
                    unresolved += 1;
                }
            }
        }
    }
    let mut accepted = accepted.into_iter().collect::<Vec<_>>();
    accepted.sort_unstable_by_key(|entry| (entry.occurrence.route, entry.occurrence.ordinal, entry.actor));
    Ok((accepted, unresolved))
}
