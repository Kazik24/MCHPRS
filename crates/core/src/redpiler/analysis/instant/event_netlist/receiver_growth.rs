//! Greedily add directly reached native comparators to a research candidate.
use super::boundary::CandidateBoundary;
use super::discovery::EventCandidates;
use crate::redpiler::TaskMonitor;
use crate::redpiler::analysis::instant::boundaries;
use crate::redpiler::components::NodeType;
use crate::redpiler::program_graph::*;
use rustc_hash::FxHashSet;

/// This proposes ownership only. It neither suppresses Graph nor certifies timing.
pub(crate) fn grow_comparators(
    graph: &CompileGraph,
    candidates: &EventCandidates,
    boundary: &mut CandidateBoundary,
    monitor: &TaskMonitor,
) -> Result<Vec<NodeId>, String> {
    if monitor.cancelled() {
        return Err("cancelled while growing comparator receivers".into());
    }
    let mut owners = boundary.owners.clone();
    let mut routes = vec![Vec::<usize>::new(); graph.nodes.len()];
    for (index, route) in graph.routes.iter().enumerate() {
        routes[route.source.index()].push(index);
    }
    let mut sites = vec![Vec::<SiteId>::new(); graph.nodes.len()];
    for (index, site) in graph.sites.iter().enumerate() {
        if let SiteSource::Component(node) = site.source {
            sites[node.index()].push(SiteId::new(index as u32));
        }
    }
    let mut queue = owners
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(index, owner)| owner.is_some().then_some(NodeId::new(index as u32)))
        .collect::<Vec<_>>();
    let mut added = Vec::new();
    let mut cursor = 0usize;
    let mut work = 0usize;
    let limit = (32usize * 1024 * 1024).saturating_mul(monitor.budget_multiplier());
    while cursor < queue.len() {
        let source = queue[cursor];
        cursor += 1;
        for &route in &routes[source.index()] {
            for delivery in &graph.routes[route].deliveries {
                work = work.checked_add(1).ok_or("comparator growth work overflow")?;
                if work > limit {
                    return Err("comparator growth exceeded work budget".into());
                }
                if work & 0x3fff == 0 && monitor.cancelled() {
                    return Err("cancelled while growing comparator receivers".into());
                }
                if !matches!(delivery.kind, DeliveryKind::Update { .. }) {
                    continue;
                }
                let receiver = delivery.receiver.or_else(|| match graph.sites[delivery.target.index()].source {
                    SiteSource::Component(node) => Some(CallbackReceiver::Component(node)),
                    _ => None,
                });
                let Some(CallbackReceiver::Component(node)) = receiver else { continue };
                if owners.nodes[node.index()].is_some()
                    || candidates.partition.owners.nodes[node.index()].is_some()
                    || !matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Electrical(NodeType::Comparator { .. }))
                    || sites[node.index()]
                        .iter()
                        .any(|site| candidates.partition.owners.sites[site.index()].is_some())
                {
                    continue;
                }
                owners.nodes[node.index()] = Some(0);
                for site in &sites[node.index()] {
                    owners.sites[site.index()] = Some(0);
                }
                queue.push(node);
                added.push(node);
            }
        }
    }
    let scan = boundaries::scan_boundaries(graph, &owners, monitor)?;
    let mut seen = FxHashSet::default();
    let mut aggregate_targets = Vec::new();
    for port in &scan.ports {
        if let (BoundaryDirection::Output, BoundaryPortKind::Power { link }) = (port.direction, port.kind) {
            let target = graph.power_links[link as usize].to;
            if seen.insert(target) {
                aggregate_targets.push(target);
            }
        }
    }
    boundary.owners = owners;
    boundary.scan = scan;
    boundary.aggregate_targets = aggregate_targets;
    Ok(added)
}
