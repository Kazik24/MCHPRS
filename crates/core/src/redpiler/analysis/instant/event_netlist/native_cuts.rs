//! Recompute greedy regions until unsupported exposed resource owners stay native.
use super::discovery::{EventCandidates, discover};
use crate::redpiler::TaskMonitor;
use crate::redpiler::analysis::instant::boundaries::{self, BoundaryScan};
use crate::redpiler::analysis::instant::ownership;
use crate::redpiler::program_graph::*;
use rustc_hash::FxHashMap;

#[derive(Debug, Clone, Copy)]
pub(crate) enum CutReason {
    /// The complete physical group included an actor or resource outside this region.
    IncompleteGroup {
        actor: PistonId,
        resource: Option<ResourceId>,
        contact: Option<ContactId>,
    },
    /// This original outside read needs a destination the current state model cannot represent.
    ResourceDestination { resource: ResourceId, crossing: BoundaryPortKind },
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct NativeCut {
    pub(crate) owner: usize,
    pub(crate) reason: CutReason,
}

pub(crate) struct ResourceCuts {
    pub(crate) candidates: EventCandidates,
    pub(crate) scan: BoundaryScan,
    pub(crate) cuts: Vec<NativeCut>,
    pub(crate) rounds: usize,
}

/// Each successful round disables at least one complete catalog owner. This is
/// research lowering only; remaining callback/geometry effects are not certified.
pub(crate) fn cut_unsupported_resources(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    eligible: &[bool],
    monitor: &TaskMonitor,
) -> Result<ResourceCuts, String> {
    let mut eligible = eligible.to_vec();
    let mut cuts = Vec::new();
    let max_rounds = 64usize.saturating_mul(monitor.budget_multiplier());
    for round in 0..max_rounds {
        if monitor.cancelled() {
            return Err("cancelled while cutting resource owners".into());
        }
        let candidates = discover(graph, catalog, &eligible, monitor)?;
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
                    return Err("cancelled while mapping resource-cut owners".into());
                }
                *owner = match *owner {
                    Some(id) if candidates.selected.get(id).copied().unwrap_or(false) => {
                        Some(*candidates.partition.owner_region.get(id).ok_or("missing owner region")? as usize)
                    }
                    _ => None,
                };
            }
        }
        let mut actors = Vec::new();
        let mut resources = FxHashMap::default();
        let mut contacts = FxHashMap::<ContactId, Vec<ResourceId>>::default();
        for (cell_id, cell) in catalog.cells.iter().enumerate() {
            if !candidates.selected[cell_id] {
                continue;
            }
            actors.extend(cell.uses.iter().map(|use_| use_.actor));
            for pin in cell.resources.iter() {
                let spec = graph.resources.get(pin.resource.index()).ok_or("missing candidate resource")?;
                let supported = (1..=5).contains(&pin.owners.len())
                    && spec.contacts.contains(&pin.source)
                    && pin.owners.iter().all(|actor| {
                        graph.pistons.get(actor.index()).is_some_and(|piston| {
                            piston.sticky && piston.lane == (PistonLane::SingleResource { far: pin.source }) && spec.contacts.contains(&piston.near)
                        })
                    })
                    && spec
                        .contacts
                        .iter()
                        .all(|contact| *contact == pin.source || pin.owners.iter().any(|actor| graph.pistons[actor.index()].near == *contact));
                resources.insert(pin.resource, (cell_id, supported));
                for &contact in spec.contacts.iter() {
                    contacts.entry(contact).or_default().push(pin.resource);
                }
            }
        }
        let closure = ownership::close_seeded_ownership(graph, &[], &actors, monitor)?;
        let mut pending = FxHashMap::<usize, CutReason>::default();
        for group in &closure.groups {
            let region = group.actors.iter().find_map(|actor| owners.actors.get(actor.index()).copied().flatten());
            let Some(region) = region else { continue };
            let outside_actor = group
                .actors
                .iter()
                .copied()
                .find(|actor| owners.actors.get(actor.index()).copied().flatten() != Some(region));
            let mut outside_resource = None;
            let mut outside_contact = None;
            for &resource in &group.resources {
                if owners.resources.get(resource.index()).copied().flatten() != Some(region) {
                    outside_resource = Some(resource);
                    break;
                }
                let spec = graph.resources.get(resource.index()).ok_or("missing physical resource")?;
                if let Some(contact) = spec
                    .contacts
                    .iter()
                    .copied()
                    .find(|contact| owners.contacts.get(contact.index()).copied().flatten() != Some(region))
                {
                    outside_resource = Some(resource);
                    outside_contact = Some(contact);
                    break;
                }
            }
            if outside_actor.is_some() || outside_resource.is_some() {
                let actor = outside_actor
                    .or_else(|| group.actors.first().copied())
                    .ok_or("physical ownership group has no actor")?;
                for member in &group.actors {
                    if let Some(owner) = candidates.partition.owners.actors[member.index()] {
                        if candidates.selected[owner] {
                            pending.entry(owner).or_insert(CutReason::IncompleteGroup {
                                actor,
                                resource: outside_resource,
                                contact: outside_contact,
                            });
                        }
                    }
                }
            }
        }
        let scan = boundaries::scan_boundaries(graph, &owners, monitor)?;
        for port in &scan.ports {
            if monitor.cancelled() {
                return Err("cancelled while inspecting resource crossings".into());
            }
            if port.direction != BoundaryDirection::Output {
                continue;
            }
            let mut consider = |resource: ResourceId| {
                if let Some(&(owner, supported)) = resources.get(&resource) {
                    if !supported && candidates.partition.owner_region[owner] as usize == port.owner {
                        pending.entry(owner).or_insert(CutReason::ResourceDestination {
                            resource,
                            crossing: port.kind,
                        });
                    }
                }
            };
            match port.kind {
                BoundaryPortKind::GuardState {
                    test: StateTest::ResourceAt { resource, .. } | StateTest::ResourceEnabled(resource),
                    ..
                } => consider(resource),
                BoundaryPortKind::ContactAlternative { contact, .. } => {
                    if let Some(resources) = contacts.get(&contact) {
                        for &resource in resources {
                            consider(resource);
                        }
                    }
                }
                _ => {}
            }
        }
        if pending.is_empty() {
            return Ok(ResourceCuts {
                candidates,
                scan,
                cuts,
                rounds: round + 1,
            });
        }
        let mut new_cuts = pending.into_iter().collect::<Vec<_>>();
        new_cuts.sort_unstable_by_key(|(owner, _)| *owner);
        for (owner, reason) in new_cuts {
            if !eligible.get(owner).copied().unwrap_or(false) {
                return Err("resource cut did not reduce eligibility".into());
            }
            eligible[owner] = false;
            cuts.push(NativeCut { owner, reason });
        }
    }
    Err("resource-cut fixed point exceeded its round budget".into())
}
