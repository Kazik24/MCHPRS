//! Find potential observer reset paths without crossing scheduled devices, payloads or callbacks.
use super::*;
use std::collections::VecDeque;

mod guards;
mod restoration;
#[cfg(test)]
mod tests;
mod wire_emission;
pub(crate) use restoration::{RestorationSource, inputs as restoration_inputs};

/// Return a reset-power provenance witness, never a certificate to discard a lifecycle.
pub(super) fn find(
    graph: &CompileGraph,
    incoming: &PowerIndex,
    observers: &FxHashMap<NodeId, PistonId>,
    node: NodeId,
    monitor: &TaskMonitor,
) -> Result<Option<(NodeId, PistonId)>, String> {
    trace(graph, incoming, node, monitor, |link, _| {
        let SiteSource::Component(source) = graph.sites[link.from.index()].source else {
            return None;
        };
        let facing = match graph.nodes[source.index()].kind {
            CircuitNodeKind::Observer { facing, .. } | CircuitNodeKind::BuiltinReset { facing, .. } => facing,
            _ => return None,
        };
        let emits =
            matches!(link.from_port, PowerPort::Strong(face) | PowerPort::Weak(face) if face == facing) || link.from_port == PowerPort::Output;
        emits.then(|| observers.get(&source).map(|&actor| (source, actor))).flatten()
    })
}

/// Own-payload feedback is distinct from observing another cell's data or update bus.
pub(crate) fn find_payload(
    graph: &CompileGraph,
    incoming: &PowerIndex,
    actor: PistonId,
    resources: &[ResourceId],
    monitor: &TaskMonitor,
) -> Result<Option<CellReset>, String> {
    let spec = graph.pistons[actor.index()];
    let PistonLane::SingleResource { far } = spec.lane else {
        return Ok(None);
    };
    let Some(route) = spec.routes.resource_restored_near else {
        return Ok(None);
    };
    let resources: Vec<_> = resources
        .iter()
        .copied()
        .filter(|resource| {
            let resource = &graph.resources[resource.index()];
            resource.contacts.contains(&spec.near)
                && resource.contacts.contains(&far)
                && Block::from_id(graph.profiles[resource.material.index()].raw_state) == Block::RedstoneBlock
        })
        .collect();
    if !spec.sticky || resources.is_empty() {
        return Ok(None);
    }
    let mut updates = restoration::Notifications::new(graph, actor);
    trace(graph, incoming, spec.node, monitor, |link, wires| {
        if graph.sites[link.from.index()].source != SiteSource::Contact(spec.near)
            || matches!(link.from_port, PowerPort::Strong(_) | PowerPort::Conducted { .. })
        {
            return None;
        }
        let (notification, sample) = updates.trigger(route, wires)?;
        Some(CellReset::Payload {
            resource: resources[0],
            notification,
            sample,
        })
    })
}

fn trace<T>(
    graph: &CompileGraph,
    incoming: &PowerIndex,
    node: NodeId,
    monitor: &TaskMonitor,
    mut select: impl FnMut(PowerLink, &[PowerTarget]) -> Option<T>,
) -> Result<Option<T>, String> {
    let mut pending = VecDeque::new();
    let mut losses = FxHashMap::<PowerTarget, u8>::default();
    let mut parents = FxHashMap::<PowerTarget, PowerTarget>::default();
    let mut guard_facts = guards::Facts::electrical(graph);
    for port in [InputPort::Quasi, InputPort::Main] {
        enqueue(&mut pending, &mut losses, &mut parents, PowerTarget::Input { node, port }, 0, None);
    }
    while let Some((target, loss)) = pending.pop_front() {
        check_cancelled(monitor)?;
        if losses[&target] < loss {
            continue;
        }
        for link in incoming.get(&target).into_iter().flatten() {
            check_cancelled(monitor)?;
            let loss = loss.saturating_add(link.attenuation);
            if loss >= 15 || guard_facts.value(link.guard) == Some(false) {
                continue;
            }
            if let PowerPort::Conducted { include_dust } = link.from_port {
                // Conditional conductivity stays in the original guard equations, never in a saved pose.
                if may_conduct(graph, link.from) {
                    enqueue(
                        &mut pending,
                        &mut losses,
                        &mut parents,
                        PowerTarget::Conductor {
                            site: link.from,
                            include_dust,
                        },
                        loss,
                        Some(target),
                    );
                }
                continue;
            }
            let path = query_path(target, &parents)?;
            if let Some(source) = select(*link, &path) {
                return Ok(Some(source));
            }
            let SiteSource::Component(source) = graph.sites[link.from.index()].source else {
                continue;
            };
            match graph.nodes[source.index()].kind {
                CircuitNodeKind::Wire(_) if wire_emission::may_emit(graph, source, link.from_port) => {
                    enqueue(
                        &mut pending,
                        &mut losses,
                        &mut parents,
                        PowerTarget::Input {
                            node: source,
                            port: InputPort::Main,
                        },
                        loss,
                        Some(target),
                    );
                }
                _ => {}
            }
        }
    }
    Ok(None)
}

fn may_conduct(graph: &CompileGraph, site: SiteId) -> bool {
    let conducts = |profile: ProfileId| {
        let profile = graph.profiles[profile.index()];
        profile.solid && profile.cube && !profile.transparent
    };
    match graph.sites[site.index()].source {
        SiteSource::Constant(profile) => conducts(profile),
        SiteSource::Contact(contact) => graph.contacts[contact.index()].variants.iter().any(|variant| conducts(variant.profile)),
        SiteSource::Component(_) => false,
    }
}

/// Revisit a zero-loss cycle only when its power budget improves.
fn enqueue(
    pending: &mut VecDeque<(PowerTarget, u8)>,
    losses: &mut FxHashMap<PowerTarget, u8>,
    parents: &mut FxHashMap<PowerTarget, PowerTarget>,
    target: PowerTarget,
    loss: u8,
    parent: Option<PowerTarget>,
) {
    if losses.get(&target).is_none_or(|&previous| loss < previous) {
        losses.insert(target, loss);
        if let Some(parent) = parent {
            parents.insert(target, parent);
        }
        pending.push_back((target, loss));
    }
}

/// Keep separate input cones separate even when both were visited by the same query search.
fn query_path(mut target: PowerTarget, parents: &FxHashMap<PowerTarget, PowerTarget>) -> Result<Vec<PowerTarget>, String> {
    let mut path = Vec::new();
    for _ in 0..=parents.len() {
        path.push(target);
        let Some(&parent) = parents.get(&target) else { return Ok(path) };
        target = parent;
    }
    Err("reset power predecessor chain contains a cycle".into())
}
