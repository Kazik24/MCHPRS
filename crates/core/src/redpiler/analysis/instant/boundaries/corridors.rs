//! Connected non-classic wire components for boundary catalogs and transport.
use super::{Budget, MAX_BOUNDARY_BYTES, OwnerMap};
use crate::redpiler::TaskMonitor;
use crate::redpiler::program_graph::*;
use rustc_hash::{FxHashMap, FxHashSet};

pub(super) fn discover_corridors(
    graph: &CompileGraph,
    owners: &OwnerMap,
    monitor: &TaskMonitor,
    extra_seeds: &[NodeId],
    all_wires: bool,
) -> Result<Vec<WireCorridor>, String> {
    discover_corridors_with_budget(graph, owners, extra_seeds, all_wires, Budget::bounded(monitor))
}

pub(super) fn discover_corridors_with_budget(
    graph: &CompileGraph,
    owners: &OwnerMap,
    extra_seeds: &[NodeId],
    all_wires: bool,
    mut budget: Budget<'_>,
) -> Result<Vec<WireCorridor>, String> {
    let classic: FxHashSet<_> = graph
        .plans
        .classic_nets
        .iter()
        .flat_map(|net| net.wires.iter().map(|wire| wire.node))
        .collect();
    let wire_nodes: Vec<_> = graph.wires.iter().map(|wire| wire.node).filter(|node| !classic.contains(node)).collect();
    let wire_vertices: FxHashMap<_, _> = wire_nodes.iter().enumerate().map(|(index, &node)| (node, index)).collect();
    let conductive_sites: Vec<_> = graph
        .sites
        .iter()
        .enumerate()
        .filter_map(|(index, site)| is_static_conductor(graph, site).then_some(SiteId::new(index as u32)))
        .collect();
    let site_vertices: FxHashMap<_, _> = conductive_sites
        .iter()
        .enumerate()
        .map(|(index, &site)| (site, wire_nodes.len() + index))
        .collect();
    let total = wire_nodes
        .len()
        .checked_add(conductive_sites.len())
        .ok_or("wire-corridor vertex count overflow")?;
    let parent_bytes = total
        .checked_mul(std::mem::size_of::<usize>())
        .ok_or("wire-corridor index size overflow")?;
    if parent_bytes > MAX_BOUNDARY_BYTES {
        return Err("wire-corridor index exceeds the bounded preparation allocation".into());
    }
    budget.bytes = parent_bytes;
    let mut parents: Vec<_> = (0..total).collect();
    let vertex_for_site = |site: SiteId| -> Option<usize> {
        match graph.sites[site.index()].source {
            SiteSource::Component(node) => wire_vertices.get(&node).copied(),
            SiteSource::Constant(_) => site_vertices.get(&site).copied(),
            SiteSource::Contact(_) => None,
        }
    };

    for relation in &graph.wire_relations {
        budget.visit()?;
        let Some(&wire) = wire_vertices.get(&relation.from) else { continue };
        if let Some(site) = vertex_for_site(relation.to) {
            join_components(&mut parents, wire, site);
        }
    }
    for link in &graph.power_links {
        budget.visit()?;
        let PowerTarget::Conductor { site, .. } = link.to else { continue };
        if let (Some(from), Some(to)) = (vertex_for_site(link.from), vertex_for_site(site)) {
            join_components(&mut parents, from, to);
        }
    }
    for route in &graph.routes {
        budget.visit()?;
        let Some(&source) = wire_vertices.get(&route.source) else { continue };
        for delivery in route.deliveries.iter() {
            budget.visit()?;
            for target in wire_receivers(graph, delivery) {
                if let Some(&target) = wire_vertices.get(&target) {
                    join_components(&mut parents, source, target);
                }
            }
        }
    }

    let mut seeded = FxHashSet::default();
    for link in &graph.power_links {
        budget.visit()?;
        let source_owner = owners.sites[link.from.index()];
        let target_owner = match link.to {
            PowerTarget::Input { node, .. } => owners.nodes[node.index()],
            PowerTarget::Conductor { site, .. } => owners.sites[site.index()],
        };
        if source_owner.is_some() || target_owner.is_some() {
            for site in [
                Some(link.from),
                match link.to {
                    PowerTarget::Conductor { site, .. } => Some(site),
                    _ => None,
                },
            ]
            .into_iter()
            .flatten()
            {
                if let SiteSource::Component(node) = graph.sites[site.index()].source {
                    mark_wire_seed(&wire_vertices, &mut parents, &mut seeded, node);
                }
                if let Some(vertex) = vertex_for_site(site) {
                    seeded.insert(component_root(&mut parents, vertex));
                }
            }
        }
    }
    for route in &graph.routes {
        budget.visit()?;
        let source_owned = owners.nodes[route.source.index()].is_some();
        for delivery in route.deliveries.iter() {
            budget.visit()?;
            let target_owned = super::scan::delivery_owner(graph, owners, delivery).is_some();
            if source_owned || target_owned {
                if let Some(&wire) = wire_vertices.get(&route.source) {
                    seeded.insert(component_root(&mut parents, wire));
                }
                for target in wire_receivers(graph, delivery) {
                    if let Some(&wire) = wire_vertices.get(&target) {
                        seeded.insert(component_root(&mut parents, wire));
                    }
                }
            }
        }
    }
    for relation in &graph.wire_relations {
        budget.visit()?;
        let target_owned = owners.sites[relation.to.index()].is_some();
        if target_owned {
            mark_wire_seed(&wire_vertices, &mut parents, &mut seeded, relation.from);
        }
    }
    for wire in &graph.wires {
        budget.visit()?;
        if classic.contains(&wire.node) {
            continue;
        }
        for &neighbor in &wire.canonical_neighbors {
            budget.visit()?;
            if owners.sites[neighbor.index()].is_some() {
                mark_wire_seed(&wire_vertices, &mut parents, &mut seeded, wire.node);
            }
        }
    }

    for &wire in extra_seeds {
        budget.visit()?;
        mark_wire_seed(&wire_vertices, &mut parents, &mut seeded, wire);
    }
    if all_wires {
        for &wire in &wire_nodes {
            budget.visit()?;
            mark_wire_seed(&wire_vertices, &mut parents, &mut seeded, wire);
        }
    }

    let mut groups: FxHashMap<usize, (Vec<NodeId>, Vec<SiteId>)> = FxHashMap::default();
    for (index, &wire) in wire_nodes.iter().enumerate() {
        budget.visit()?;
        let root = component_root(&mut parents, index);
        if seeded.contains(&root) {
            if all_wires {
                budget.retain(&wire)?;
                if !groups.contains_key(&root) {
                    budget.retain_bytes(std::mem::size_of::<(usize, (Vec<NodeId>, Vec<SiteId>))>())?;
                }
            }
            groups.entry(root).or_default().0.push(wire);
        }
    }
    for &site in &conductive_sites {
        budget.visit()?;
        let root = component_root(&mut parents, site_vertices[&site]);
        if seeded.contains(&root) {
            if all_wires {
                budget.retain(&site)?;
            }
            groups.entry(root).or_default().1.push(site);
        }
    }
    let mut corridors: Vec<_> = groups
        .into_values()
        .filter(|(wires, _)| !wires.is_empty())
        .map(|(mut wires, mut sites)| {
            wires.sort_unstable();
            sites.sort_unstable();
            WireCorridor {
                wires: wires.into_boxed_slice(),
                conductive_sites: sites.into_boxed_slice(),
            }
        })
        .collect();
    corridors.sort_by_key(|corridor| corridor.wires[0]);
    Ok(corridors)
}

fn mark_wire_seed(wire_vertices: &FxHashMap<NodeId, usize>, parents: &mut [usize], seeded: &mut FxHashSet<usize>, wire: NodeId) {
    if let Some(&vertex) = wire_vertices.get(&wire) {
        seeded.insert(component_root(parents, vertex));
    }
}

fn wire_receivers(graph: &CompileGraph, delivery: &RouteDelivery) -> Vec<NodeId> {
    let mut receivers = Vec::new();
    match delivery.receiver {
        Some(CallbackReceiver::Component(node)) if matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Wire(_)) => receivers.push(node),
        Some(CallbackReceiver::Contact(contact)) => {
            for variant in &graph.contacts[contact.index()].variants {
                if let Some(CallbackReceiver::Component(node)) = variant.receiver
                    && matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Wire(_))
                {
                    receivers.push(node);
                }
            }
        }
        _ => {}
    }
    receivers.sort_unstable();
    receivers.dedup();
    receivers
}

fn is_static_conductor(graph: &CompileGraph, site: &SiteSpec) -> bool {
    let SiteSource::Constant(profile) = site.source else { return false };
    let profile = graph.profiles[profile.index()];
    profile.solid && profile.cube && !profile.transparent
}

fn component_root(parents: &mut [usize], mut index: usize) -> usize {
    while parents[index] != index {
        parents[index] = parents[parents[index]];
        index = parents[index];
    }
    index
}

fn join_components(parents: &mut [usize], first: usize, second: usize) {
    let first = component_root(parents, first);
    let second = component_root(parents, second);
    parents[first.max(second)] = first.min(second);
}
