//! Conservative ownership closure and private-cell certificates, independent of pass order.
//! All owned sites and state remain observable; no reader is declared dead by this analysis.
use crate::redpiler::TaskMonitor;
use crate::redpiler::analysis::instant::boundaries;
use crate::redpiler::analysis::instant::ownership::{self, join, root};
use crate::redpiler::passes::utils::instant_mechanisms::{InstantMechanism, discover_mechanisms};
use crate::redpiler::program_graph::*;
mod composition;
pub(crate) use composition::infer_stable_regions;

fn check_cancelled(monitor: &TaskMonitor) -> Result<(), String> {
    if monitor.cancelled() {
        Err("cancelled while analyzing instant regions".into())
    } else {
        Ok(())
    }
}

type Owners = Vec<Option<usize>>;

/// Index ownership once, then collect query and ordered event boundaries in separate scans.
pub(crate) fn infer_regions(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<Vec<InstantRegion>, String> {
    let mechanisms = discover_mechanisms(graph, monitor)?;
    if mechanisms.is_empty() {
        return Ok(Vec::new());
    }
    let (mut regions, actor_regions, contact_regions) = close_ownership(graph, &mechanisms, monitor)?;
    let resets = bind_legacy_sites(graph, &mechanisms, &actor_regions, &contact_regions, &mut regions);
    let owners = boundaries::bind_owners(graph, actor_regions, vec![None; graph.resources.len()], contact_regions, &resets);
    for (index, owner) in owners.sites.iter().enumerate() {
        if let Some(owner) = owner {
            regions[*owner].readable_sites.push(SiteId::new(index as u32));
        }
    }
    let scan = boundaries::scan_legacy_boundaries(graph, &owners, monitor)?;
    project_legacy_scan(scan, &mut regions);
    for region in &mut regions {
        region.resets.sort_unstable();
    }
    check_cancelled(monitor)?;
    Ok(regions)
}

fn close_ownership(
    graph: &CompileGraph,
    mechanisms: &[InstantMechanism],
    monitor: &TaskMonitor,
) -> Result<(Vec<InstantRegion>, Owners, Owners), String> {
    let closure = ownership::close_ownership(graph, mechanisms, monitor)?;
    let regions = closure
        .groups
        .into_iter()
        .map(|group| InstantRegion {
            actors: group.actors,
            resources: group.resources,
            ..Default::default()
        })
        .collect();
    Ok((regions, closure.actors, closure.contacts))
}

fn bind_legacy_sites(
    graph: &CompileGraph,
    mechanisms: &[InstantMechanism],
    actor_regions: &[Option<usize>],
    contact_regions: &[Option<usize>],
    regions: &mut [InstantRegion],
) -> Vec<(NodeId, usize)> {
    let mut resets = Vec::new();
    for mechanism in mechanisms {
        let owner = actor_regions[mechanism.piston.index()].expect("recognized actor has a region");
        resets.push((mechanism.observer, owner));
        if !regions[owner].resets.contains(&mechanism.observer) {
            regions[owner].resets.push(mechanism.observer);
        }
    }
    for (index, _) in graph.contacts.iter().enumerate() {
        if let Some(owner) = contact_regions[index] {
            regions[owner].contacts.push(ContactId::new(index as u32));
        }
    }
    resets
}

fn project_legacy_scan(scan: boundaries::BoundaryScan, regions: &mut [InstantRegion]) {
    use crate::redpiler::program_graph::{BoundaryDirection, BoundaryPortKind};
    use rustc_hash::FxHashSet;

    for (owner, route) in scan.owned_routes {
        regions[owner].routes.push(route);
    }
    let mut wire_readers = FxHashSet::default();
    for port in scan.ports {
        match (port.direction, port.kind) {
            (BoundaryDirection::Input, BoundaryPortKind::Power { link }) => regions[port.owner].power_inputs.push(link),
            (BoundaryDirection::Output, BoundaryPortKind::Power { link }) => regions[port.owner].power_readers.push(link),
            (BoundaryDirection::Input, BoundaryPortKind::Callback { occurrence }) => regions[port.owner].incoming.push(occurrence),
            (BoundaryDirection::Output, BoundaryPortKind::Callback { occurrence }) => regions[port.owner].outgoing.push(occurrence),
            (BoundaryDirection::Output, BoundaryPortKind::WireNeighborhood { wire, .. }) => {
                if wire_readers.insert((port.owner, wire)) {
                    regions[port.owner].wire_readers.push(wire);
                }
            }
            (BoundaryDirection::Output, BoundaryPortKind::ObserverWatch { observer, .. }) => regions[port.owner].observers.push(observer),
            _ => {}
        }
    }
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
    use crate::redpiler::analysis::instant::boundaries::OwnerMap;
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
        let owners = OwnerMap {
            actors: vec![],
            resources: vec![],
            contacts: vec![],
            nodes: vec![Some(0)],
            sites: vec![Some(0), None, Some(1)],
        };
        let mut regions = vec![InstantRegion::default(), InstantRegion::default()];

        let scan = boundaries::scan_legacy_boundaries(&graph, &owners, &TaskMonitor::default()).unwrap();
        project_legacy_scan(scan, &mut regions);

        assert_eq!(regions[0].power_inputs, [1, 2]);
        assert_eq!(regions[1].power_readers, [2]);
    }
}
