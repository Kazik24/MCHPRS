//! Close actual payload/reset identities without joining through outside signal fanout.
use crate::redpiler::TaskMonitor;
use crate::redpiler::passes::utils::instant_mechanisms::InstantMechanism;
use crate::redpiler::program_graph::{CompileGraph, PistonId, PistonLane, ResourceId};

#[derive(Debug, Default)]
pub(crate) struct OwnershipGroup {
    /// These actors can share contacts, payloads or a reset's watched base.
    pub(crate) actors: Vec<PistonId>,
    /// These conserved tokens belong to this complete ownership group.
    pub(crate) resources: Vec<ResourceId>,
}

pub(crate) struct OwnershipClosure {
    /// These groups contain at least one recognized reset mechanism.
    pub(crate) groups: Vec<OwnershipGroup>,
    /// This maps each original actor to its complete group, including unrecognized co-owners.
    pub(crate) actors: Vec<Option<usize>>,
    /// This maps each possible occupied contact to the same exclusive group.
    pub(crate) contacts: Vec<Option<usize>>,
}

pub(crate) fn root(parents: &mut [usize], mut index: usize) -> usize {
    while parents[index] != index {
        parents[index] = parents[parents[index]];
        index = parents[index];
    }
    index
}

pub(crate) fn join(parents: &mut [usize], first: usize, second: usize) {
    let first = root(parents, first);
    let second = root(parents, second);
    parents[first.max(second)] = first.min(second);
}

/// Recognition seeds ownership; it does not certify the other actors in the closure.
pub(crate) fn close_ownership(graph: &CompileGraph, mechanisms: &[InstantMechanism], monitor: &TaskMonitor) -> Result<OwnershipClosure, String> {
    close_seeded_ownership(graph, mechanisms, &[], monitor)
}

pub(crate) fn close_seeded_ownership(
    graph: &CompileGraph,
    mechanisms: &[InstantMechanism],
    additional_actors: &[PistonId],
    monitor: &TaskMonitor,
) -> Result<OwnershipClosure, String> {
    let mut parents: Vec<_> = (0..graph.pistons.len() + graph.resources.len()).collect();
    let mut contact_owners = vec![None; graph.contacts.len()];
    for (index, actor) in graph.pistons.iter().enumerate() {
        check_cancelled(monitor)?;
        let far = match actor.lane {
            PistonLane::SingleResource { far } => far,
            PistonLane::Empty => actor.near,
        };
        for contact in [actor.base, actor.near, far] {
            if let Some(other) = contact_owners[contact.index()] {
                join(&mut parents, index, other);
            } else {
                contact_owners[contact.index()] = Some(index);
            }
        }
    }
    for (index, resource) in graph.resources.iter().enumerate() {
        check_cancelled(monitor)?;
        let token = graph.pistons.len() + index;
        for &contact in &resource.contacts {
            if let Some(other) = contact_owners[contact.index()] {
                join(&mut parents, token, other);
            } else {
                contact_owners[contact.index()] = Some(token);
            }
        }
    }
    for mechanism in mechanisms {
        check_cancelled(monitor)?;
        join(&mut parents, mechanism.piston.index(), mechanism.watched_actor.index());
    }
    let mut active = vec![false; parents.len()];
    for mechanism in mechanisms {
        active[root(&mut parents, mechanism.piston.index())] = true;
    }
    for actor in additional_actors {
        check_cancelled(monitor)?;
        active[root(&mut parents, actor.index())] = true;
    }
    let mut root_groups = vec![None; parents.len()];
    let mut groups = Vec::new();
    for index in 0..parents.len() {
        check_cancelled(monitor)?;
        let owner = root(&mut parents, index);
        if active[owner] && root_groups[owner].is_none() {
            root_groups[owner] = Some(groups.len());
            groups.push(OwnershipGroup::default());
        }
    }
    let actors: Vec<_> = (0..graph.pistons.len()).map(|index| root_groups[root(&mut parents, index)]).collect();
    for (index, &owner) in actors.iter().enumerate() {
        if let Some(owner) = owner {
            groups[owner].actors.push(PistonId::new(index as u32));
        }
    }
    let mut contacts: Vec<_> = contact_owners
        .iter()
        .map(|actor| actor.and_then(|actor| root_groups[root(&mut parents, actor)]))
        .collect();
    for (index, resource) in graph.resources.iter().enumerate() {
        check_cancelled(monitor)?;
        if let Some(owner) = resource.contacts.iter().find_map(|contact| contacts[contact.index()]) {
            groups[owner].resources.push(ResourceId::new(index as u32));
            for &contact in &resource.contacts {
                contacts[contact.index()] = Some(owner);
            }
        }
    }
    Ok(OwnershipClosure { groups, actors, contacts })
}

fn check_cancelled(monitor: &TaskMonitor) -> Result<(), String> {
    if monitor.cancelled() {
        Err("cancelled while closing instant ownership".into())
    } else {
        Ok(())
    }
}
