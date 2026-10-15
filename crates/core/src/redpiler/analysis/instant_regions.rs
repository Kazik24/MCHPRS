//! Conservative ownership closure and private-cell certificates, independent of pass order.
//! All owned sites and state remain observable; no reader is declared dead by this analysis.
use crate::redpiler::TaskMonitor;
use crate::redpiler::passes::utils::instant_mechanisms::{InstantMechanism, discover_mechanisms};
use crate::redpiler::program_graph::*;
mod composition;
pub(crate) mod compiler;
pub(crate) mod machine;
#[cfg(test)]
pub(crate) mod symbolic;
pub(crate) mod symbolic_actor;
pub(crate) use composition::infer_stable_regions;

fn check_cancelled(monitor: &TaskMonitor) -> Result<(), String> {
    if monitor.cancelled() {
        Err("cancelled while analyzing instant regions".into())
    } else {
        Ok(())
    }
}

fn root(parents: &mut [usize], mut index: usize) -> usize {
    while parents[index] != index {
        parents[index] = parents[parents[index]];
        index = parents[index];
    }
    index
}

fn join(parents: &mut [usize], a: usize, b: usize) {
    let a = root(parents, a);
    let b = root(parents, b);
    parents[a.max(b)] = a.min(b);
}

/// Index ownership once, then collect query and ordered event boundaries in separate scans.
pub(crate) fn infer_regions(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<Vec<InstantRegion>, String> {
    let mechanisms = discover_mechanisms(graph, monitor)?;
    if mechanisms.is_empty() {
        return Ok(Vec::new());
    }
    let (mut regions, actor_regions, contact_regions) = close_ownership(graph, &mechanisms, monitor)?;
    let (node_regions, site_regions) = bind_owned_sites(graph, &mechanisms, &actor_regions, &contact_regions, &mut regions);
    let owners = Ownership {
        actors: actor_regions,
        contacts: contact_regions,
        nodes: node_regions,
        sites: site_regions,
    };
    collect_power_boundary(graph, &owners, &mut regions, monitor)?;
    collect_callback_boundary(graph, &owners, &mut regions, monitor)?;
    collect_geometry_readers(graph, &owners, &mut regions, monitor)?;
    for region in &mut regions {
        region.resets.sort_unstable();
    }
    check_cancelled(monitor)?;
    Ok(regions)
}

type Owners = Vec<Option<usize>>;

struct Ownership {
    /// This assigns every actuator to its complete region, if recognized.
    actors: Owners,
    /// This assigns every potential occupied position to its region.
    contacts: Owners,
    /// This binds callback receivers and route sources to their owner.
    nodes: Owners,
    /// This binds live and captured geometry queries to their owner.
    sites: Owners,
}

fn close_ownership(
    graph: &CompileGraph,
    mechanisms: &[InstantMechanism],
    monitor: &TaskMonitor,
) -> Result<(Vec<InstantRegion>, Owners, Owners), String> {
    let mut parents: Vec<_> = (0..graph.pistons.len() + graph.resources.len()).collect();
    let mut contact_actors = vec![None; graph.contacts.len()];
    for (index, actor) in graph.pistons.iter().enumerate() {
        check_cancelled(monitor)?;
        let far = match actor.lane {
            PistonLane::SingleResource { far } => far,
            PistonLane::Empty => actor.near,
        };
        for contact in [actor.base, actor.near, far] {
            if let Some(other) = contact_actors[contact.index()] {
                join(&mut parents, index, other);
            } else {
                contact_actors[contact.index()] = Some(index);
            }
        }
    }
    for (index, resource) in graph.resources.iter().enumerate() {
        check_cancelled(monitor)?;
        let token = graph.pistons.len() + index;
        for &contact in &resource.contacts {
            if let Some(other) = contact_actors[contact.index()] {
                join(&mut parents, token, other);
            } else {
                contact_actors[contact.index()] = Some(token);
            }
        }
    }
    for mechanism in mechanisms {
        join(&mut parents, mechanism.piston.index(), mechanism.watched_actor.index());
    }
    let mut active = vec![false; parents.len()];
    for mechanism in mechanisms {
        active[root(&mut parents, mechanism.piston.index())] = true;
    }
    let mut root_regions = vec![None; parents.len()];
    let mut regions = Vec::new();
    for index in 0..parents.len() {
        let owner = root(&mut parents, index);
        if active[owner] && root_regions[owner].is_none() {
            root_regions[owner] = Some(regions.len());
            regions.push(InstantRegion::default());
        }
    }
    let actor_regions: Vec<_> = (0..graph.pistons.len()).map(|i| root_regions[root(&mut parents, i)]).collect();
    for (index, &owner) in actor_regions.iter().enumerate() {
        if let Some(owner) = owner {
            regions[owner].actors.push(PistonId::new(index as u32));
        }
    }
    let mut contact_regions: Vec<_> = contact_actors
        .iter()
        .map(|actor| actor.and_then(|a| root_regions[root(&mut parents, a)]))
        .collect();
    for (index, resource) in graph.resources.iter().enumerate() {
        let owner = resource.contacts.iter().find_map(|c| contact_regions[c.index()]);
        if let Some(owner) = owner {
            regions[owner].resources.push(ResourceId::new(index as u32));
            for &contact in &resource.contacts {
                contact_regions[contact.index()] = Some(owner);
            }
        }
    }
    Ok((regions, actor_regions, contact_regions))
}

fn bind_owned_sites(
    graph: &CompileGraph,
    mechanisms: &[InstantMechanism],
    actor_regions: &[Option<usize>],
    contact_regions: &[Option<usize>],
    regions: &mut [InstantRegion],
) -> (Owners, Owners) {
    let mut node_regions = vec![None; graph.nodes.len()];
    for (index, actor) in graph.pistons.iter().enumerate() {
        if let Some(owner) = actor_regions[index] {
            node_regions[actor.node.index()] = Some(owner);
        }
    }
    for mechanism in mechanisms {
        let owner = actor_regions[mechanism.piston.index()].expect("recognized actor has a region");
        node_regions[mechanism.observer.index()] = Some(owner);
        if !regions[owner].resets.contains(&mechanism.observer) {
            regions[owner].resets.push(mechanism.observer);
        }
    }
    for (index, contact) in graph.contacts.iter().enumerate() {
        if let Some(owner) = contact_regions[index] {
            regions[owner].contacts.push(ContactId::new(index as u32));
            node_regions[contact.node.index()] = Some(owner);
        }
    }
    let site_regions: Vec<_> = graph
        .sites
        .iter()
        .map(|site| match site.source {
            SiteSource::Component(node) => node_regions[node.index()],
            SiteSource::Contact(contact) => contact_regions[contact.index()],
            SiteSource::Constant(_) => None,
        })
        .collect();
    for (index, &owner) in site_regions.iter().enumerate() {
        if let Some(owner) = owner {
            regions[owner].readable_sites.push(SiteId::new(index as u32));
        }
    }
    (node_regions, site_regions)
}

fn collect_power_boundary(graph: &CompileGraph, owners: &Ownership, regions: &mut [InstantRegion], monitor: &TaskMonitor) -> Result<(), String> {
    for (index, link) in graph.power_links.iter().enumerate() {
        check_cancelled(monitor)?;
        let target = match link.to {
            PowerTarget::Input { node, .. } => owners.nodes[node.index()],
            PowerTarget::Conductor { site, .. } => owners.sites[site.index()],
        };
        let source = owners.sites[link.from.index()];
        if let Some(owner) = target.filter(|&owner| source != Some(owner)) {
            regions[owner].power_inputs.push(index as u32);
        }
        if let Some(owner) = source {
            if target != Some(owner) {
                regions[owner].power_readers.push(index as u32);
            }
        }
    }
    Ok(())
}

fn collect_callback_boundary(graph: &CompileGraph, owners: &Ownership, regions: &mut [InstantRegion], monitor: &TaskMonitor) -> Result<(), String> {
    // Scan large route tables once; retain references, not copies of their deliveries.
    for (index, route) in graph.routes.iter().enumerate() {
        check_cancelled(monitor)?;
        let source = owners.nodes[route.source.index()];
        let id = RouteId::new(index as u32);
        if let Some(owner) = source {
            regions[owner].routes.push(id);
        }
        for (ordinal, delivery) in route.deliveries.iter().enumerate() {
            let target = match delivery.receiver {
                Some(CallbackReceiver::Component(node)) => owners.nodes[node.index()],
                Some(CallbackReceiver::Contact(contact)) => owners.contacts[contact.index()],
                Some(CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor)) => owners.actors[actor.index()],
                None => owners.sites[delivery.target.index()],
            };
            let occurrence = RouteOccurrence {
                route: id,
                ordinal: ordinal as u32,
            };
            if target != source {
                if let Some(owner) = source {
                    regions[owner].outgoing.push(occurrence);
                }
                if let Some(owner) = target {
                    regions[owner].incoming.push(occurrence);
                }
            }
        }
    }
    Ok(())
}

fn collect_geometry_readers(graph: &CompileGraph, owners: &Ownership, regions: &mut [InstantRegion], monitor: &TaskMonitor) -> Result<(), String> {
    for wire in &graph.wires {
        check_cancelled(monitor)?;
        let mut read_regions: Vec<_> = wire.canonical_neighbors.iter().filter_map(|site| owners.sites[site.index()]).collect();
        read_regions.sort_unstable();
        read_regions.dedup();
        for owner in read_regions {
            if owners.nodes[wire.node.index()] != Some(owner) {
                regions[owner].wire_readers.push(wire.node);
            }
        }
    }
    for (index, node) in graph.nodes.iter().enumerate() {
        if let CircuitNodeKind::Observer { watched, .. } | CircuitNodeKind::BuiltinReset { watched, .. } = node.kind {
            if let Some(owner) = owners.sites[watched.index()] {
                if owners.nodes[index] != Some(owner) {
                    regions[owner].observers.push(NodeId::new(index as u32));
                }
            }
        }
    }
    Ok(())
}

/// Compile ordered instructions; shared cells require an explicit composed-region contract.
pub(crate) fn infer_proxies(graph: &CompileGraph, regions: &[InstantRegion], monitor: &TaskMonitor) -> Result<Vec<InstantProxy>, String> {
    use rustc_hash::{FxHashMap, FxHashSet};
    let ownership;
    let private_regions = if regions.iter().any(|region| region.stable_inputs) {
        ownership = infer_regions(graph, monitor)?;
        &ownership
    } else {
        regions
    };
    let private_cells: FxHashMap<_, _> = private_regions
        .iter()
        .filter_map(|region| private_cell(graph, region))
        .map(|cell| (cell.actor, cell))
        .collect();
    let mut turbo = vec![false; graph.routes.len()];
    for wire in &graph.wires {
        if let Some(route) = wire.turbo_route {
            turbo[route.index()] = true;
        }
    }
    let mut proxies = Vec::new();
    for (region_index, region) in regions.iter().enumerate() {
        check_cancelled(monitor)?;
        if !region.stable_inputs && !(region.actors.len() == 1 && private_cells.contains_key(&region.actors[0])) {
            continue;
        }
        let actors: FxHashSet<_> = region.actors.iter().copied().collect();
        let resets: FxHashMap<_, _> = region
            .resets
            .iter()
            .filter_map(|&reset| match graph.nodes[reset.index()].kind {
                CircuitNodeKind::BuiltinReset { facing, .. } => Some((reset, facing)),
                _ => None,
            })
            .collect();
        let mut routes = Vec::new();
        let mut has_reset = false;
        for &route in &region.routes {
            if turbo[route.index()] {
                continue;
            }
            let steps: Vec<_> = graph.routes[route.index()]
                .deliveries
                .iter()
                .map(|&delivery| InstantStep {
                    delivery,
                    kind: classify_internal_delivery(graph, delivery, &actors, &resets),
                })
                .collect();
            has_reset |= steps.iter().any(|step| matches!(step.kind, InstantStepKind::ArmReset(_)));
            if steps.iter().any(|step| step.kind != InstantStepKind::Dispatch) {
                routes.push(InstantRoute {
                    route,
                    steps: steps.into_boxed_slice(),
                });
            }
        }
        if has_reset {
            proxies.push(InstantProxy {
                region: region_index,
                private_cells: region.actors.iter().filter_map(|actor| private_cells.get(actor).copied()).collect(),
                routes: routes.into_boxed_slice(),
            });
        }
    }
    check_cancelled(monitor)?;
    Ok(proxies)
}

fn private_cell(graph: &CompileGraph, region: &InstantRegion) -> Option<PrivateInstantCell> {
    let ([actor], [resource], [reset]) = (region.actors.as_slice(), region.resources.as_slice(), region.resets.as_slice()) else {
        return None;
    };
    let spec = graph.pistons[actor.index()];
    let PistonLane::SingleResource { far } = spec.lane else {
        return None;
    };
    let contacts = &graph.resources[resource.index()].contacts;
    if !spec.sticky
        || contacts.len() != 2
        || !contacts.contains(&spec.near)
        || !contacts.contains(&far)
        || spec.base == spec.near
        || spec.base == far
        || spec.near == far
    {
        return None;
    }
    let CircuitNodeKind::BuiltinReset { watched, .. } = graph.nodes[reset.index()].kind else {
        return None;
    };
    if graph.nodes[reset.index()].output_route.is_some() || graph.sites[watched.index()].source != SiteSource::Contact(spec.base) {
        return None;
    }
    Some(PrivateInstantCell {
        actor: *actor,
        resource: *resource,
        reset: *reset,
    })
}

fn classify_internal_delivery(
    graph: &CompileGraph,
    delivery: RouteDelivery,
    actors: &rustc_hash::FxHashSet<PistonId>,
    resets: &rustc_hash::FxHashMap<NodeId, mchprs_blocks::BlockFace>,
) -> InstantStepKind {
    if delivery.view != OperandView::Live {
        return InstantStepKind::Dispatch;
    }
    let receiver = delivery
        .receiver
        .or_else(|| match (graph.sites[delivery.target.index()].source, delivery.kind) {
            (SiteSource::Component(node), DeliveryKind::Update { .. }) => Some(CallbackReceiver::Component(node)),
            _ => None,
        });
    if let Some(CallbackReceiver::Component(reset)) = receiver {
        if let Some(&facing) = resets.get(&reset) {
            if delivery.kind == (DeliveryKind::Update { direction: Some(facing) }) && graph.guards[delivery.guard.index()] == Guard::Always {
                return InstantStepKind::ArmReset(reset);
            }
        }
    }
    if let Some(CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor)) = receiver {
        if matches!(delivery.kind, DeliveryKind::Update { .. }) && actors.contains(&actor) {
            return InstantStepKind::Sample(actor);
        }
    }
    InstantStepKind::Dispatch
}

#[cfg(test)]
mod tests {
    use super::*;
    use mchprs_blocks::{BlockFace, BlockPos};

    #[test]
    fn power_boundary_excludes_internal_links_and_keeps_cross_region_links() {
        let graph = CompileGraph {
            nodes: vec![CircuitNodeDraft {
                position: None,
                raw_state: None,
                kind: CircuitNodeKind::Contact(ContactId::new(0)),
                state: ComponentState::Specialized,
                output_route: None,
            }],
            links: vec![],
            power_links: [0, 1, 2]
                .map(|site| PowerLink {
                    from: SiteId::new(site),
                    from_port: PowerPort::Output,
                    to: PowerTarget::Input {
                        node: NodeId::new(0),
                        port: InputPort::Main,
                    },
                    face: BlockFace::North,
                    attenuation: 0,
                    guard: GuardId::new(0),
                })
                .to_vec(),
            comparator_inputs: vec![],
            profiles: vec![],
            guards: vec![Guard::Always],
            contacts: vec![],
            routes: vec![],
            wires: vec![],
            wire_relations: vec![],
            sites: (0..3)
                .map(|_| SiteSpec {
                    position: BlockPos::new(0, 0, 0),
                    source: SiteSource::Constant(ProfileId::new(0)),
                    source_strength: 0,
                })
                .collect(),
            pistons: vec![],
            resources: vec![],
            initial_pistons: vec![],
            initial_resources: vec![],
            plans: ExecutionPlans::default(),
        };
        let owners = Ownership {
            actors: vec![],
            contacts: vec![],
            nodes: vec![Some(0)],
            sites: vec![Some(0), None, Some(1)],
        };
        let mut regions = vec![InstantRegion::default(), InstantRegion::default()];

        collect_power_boundary(&graph, &owners, &mut regions, &TaskMonitor::default()).unwrap();

        assert_eq!(regions[0].power_inputs, [1, 2]);
        assert_eq!(regions[1].power_readers, [2]);
    }
}
