//! Shared graph-only discovery of reset mechanisms; provenance is not a lifecycle proof.
use crate::redpiler::TaskMonitor;
use crate::redpiler::program_graph::*;
use mchprs_blocks::{BlockFace, blocks::Block};
use rustc_hash::FxHashMap;

mod reset_sources;
pub(crate) use reset_sources::find_payload;
pub(crate) use reset_sources::{RestorationSource, restoration_inputs};

/// Graph-derived reset provenance; this is not permission to erase a lifecycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstantMechanism {
    /// This identifies the actuator receiving reset power.
    pub(crate) piston: PistonId,
    /// This identifies the observer whose existing output provides reset power.
    pub(crate) observer: NodeId,
    /// This identifies the base watched by that observer, possibly another actor.
    pub(crate) watched_actor: PistonId,
    /// This retains every allowed payload identity without creating a private copy.
    pub(crate) resources: Box<[ResourceId]>,
}

pub(crate) type PowerIndex = FxHashMap<PowerTarget, Vec<PowerLink>>;

pub(crate) fn index_power(graph: &CompileGraph) -> PowerIndex {
    let mut incoming = FxHashMap::default();
    for &link in &graph.power_links {
        incoming.entry(link.to).or_insert_with(Vec::new).push(link);
    }
    incoming
}

pub(crate) fn discover_mechanisms(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<Vec<InstantMechanism>, String> {
    discover_indexed_mechanisms(graph, &index_power(graph), monitor)
}

pub(crate) fn discover_indexed_mechanisms(
    graph: &CompileGraph,
    incoming: &PowerIndex,
    monitor: &TaskMonitor,
) -> Result<Vec<InstantMechanism>, String> {
    let observers = find_reset_observers(graph, monitor)?;
    let mut resource_lanes = FxHashMap::<_, Vec<ResourceId>>::default();
    for (index, resource) in graph.resources.iter().enumerate() {
        check_cancelled(monitor)?;
        for &contact in &resource.contacts {
            resource_lanes.entry(contact).or_default().push(ResourceId::new(index as u32));
        }
    }
    let mut found = Vec::new();
    for (index, actor) in graph.pistons.iter().enumerate() {
        check_cancelled(monitor)?;
        let PistonLane::SingleResource { far } = actor.lane else {
            continue;
        };
        if !actor.sticky {
            continue;
        }
        let resources: Vec<_> = resource_lanes
            .get(&far)
            .into_iter()
            .flatten()
            .copied()
            .filter(|resource| graph.resources[resource.index()].contacts.contains(&actor.near))
            .collect();
        if resources.is_empty() {
            continue;
        }
        let reset = reset_sources::find(graph, incoming, &observers, actor.node, monitor)?;
        if let Some((observer, watched_actor)) = reset {
            found.push(InstantMechanism {
                piston: PistonId::new(index as u32),
                observer,
                watched_actor,
                resources: resources.into_boxed_slice(),
            });
        }
    }
    Ok(found)
}

fn find_reset_observers(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<FxHashMap<NodeId, PistonId>, String> {
    let bases: FxHashMap<_, _> = graph
        .pistons
        .iter()
        .enumerate()
        .map(|(index, actor)| (actor.base, PistonId::new(index as u32)))
        .collect();
    let mut observers = FxHashMap::default();
    for (index, node) in graph.nodes.iter().enumerate() {
        check_cancelled(monitor)?;
        let (CircuitNodeKind::Observer { watched, facing } | CircuitNodeKind::BuiltinReset { watched, facing }) = node.kind else {
            continue;
        };
        let SiteSource::Contact(base) = graph.sites[watched.index()].source else {
            continue;
        };
        let Some(&actor) = bases.get(&base) else {
            continue;
        };
        let Block::Observer { observer } = node.raw_state.map(Block::from_id).unwrap_or(Block::Air) else {
            continue;
        };
        if BlockFace::from(observer.facing) != facing {
            continue;
        }
        let observer = NodeId::new(index as u32);
        let spec = graph.pistons[actor.index()];
        if has_observer_callback(graph, spec.routes.base_changed, observer, facing)
            && has_observer_callback(graph, spec.routes.base_restored, observer, facing)
        {
            observers.insert(observer, actor);
        }
    }
    Ok(observers)
}

fn has_observer_callback(graph: &CompileGraph, route: RouteId, observer: NodeId, facing: BlockFace) -> bool {
    graph.routes[route.index()].deliveries.iter().any(|delivery| {
        delivery.receiver == Some(CallbackReceiver::Component(observer))
            && delivery.kind == DeliveryKind::Update { direction: Some(facing) }
            && delivery.view == OperandView::Live
            && matches!(graph.guards[delivery.guard.index()], Guard::Always)
    })
}

pub(crate) fn conductor_inputs(graph: &CompileGraph, incoming: &PowerIndex, link: PowerLink) -> Option<Vec<PowerLink>> {
    let PowerPort::Conducted { include_dust } = link.from_port else {
        return None;
    };
    let SiteSource::Constant(profile) = graph.sites[link.from.index()].source else {
        return None;
    };
    let profile = graph.profiles[profile.index()];
    let block = Block::from_id(profile.raw_state);
    if !profile.solid || !profile.cube || profile.transparent || !block.is_solid() || !block.is_cube() || block.is_transparent() {
        return None;
    }
    let contributors = incoming
        .get(&PowerTarget::Conductor {
            site: link.from,
            include_dust,
        })
        .cloned()
        .unwrap_or_default();
    // Keep recursive conductor programs intact rather than introducing repeated pass expansion.
    if contributors.iter().any(|input| matches!(input.from_port, PowerPort::Conducted { .. })) {
        return None;
    }
    Some(contributors)
}

fn check_cancelled(monitor: &TaskMonitor) -> Result<(), String> {
    if monitor.cancelled() {
        Err("cancelled while recognizing instant mechanisms".into())
    } else {
        Ok(())
    }
}
