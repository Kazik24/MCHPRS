//! Pair a restored power input with original notifications, without traversing observer ticks.
use super::*;

#[derive(Clone, Copy)]
pub(crate) struct RestorationSource {
    /// This upstream actor must already have a reset provenance witness.
    pub(crate) driver: PistonId,
    /// This is the single conserved token restored by the original route.
    pub(crate) resource: ResourceId,
    /// This is that token's original restoration route at the indexed contact.
    pub(crate) route: RouteId,
}

pub(crate) fn inputs(
    graph: &CompileGraph,
    power: &PowerIndex,
    actor: PistonId,
    sources: &FxHashMap<ContactId, Vec<RestorationSource>>,
    monitor: &TaskMonitor,
) -> Result<Vec<(PistonId, CellReset)>, String> {
    let mut found = Vec::new();
    let mut updates = Notifications::new(graph, actor);
    trace(graph, power, graph.pistons[actor.index()].node, monitor, |link, queries| {
        if matches!(link.from_port, PowerPort::Strong(_) | PowerPort::Conducted { .. }) {
            return None::<()>;
        }
        let SiteSource::Contact(contact) = graph.sites[link.from.index()].source else {
            return None;
        };
        for source in sources.get(&contact).into_iter().flatten() {
            if let Some((notification, sample)) = updates.trigger(source.route, queries) {
                let reset = CellReset::InputRestoration {
                    resource: source.resource,
                    contact,
                    notification,
                    sample,
                };
                if !found.contains(&(source.driver, reset.clone())) {
                    found.push((source.driver, reset));
                }
            }
        }
        None
    })?;
    Ok(found)
}

pub(super) struct Notifications<'a> {
    /// These original routes supply every retained notification witness.
    graph: &'a CompileGraph,
    /// This is the candidate receiver at its stationary reset cut.
    actor: PistonId,
    /// These guard facts share that one immutable reset cut.
    facts: guards::Facts<'a>,
    /// Each wire's qualifying receiver is searched once per candidate actor.
    samples: FxHashMap<NodeId, Option<RouteOccurrence>>,
}

impl<'a> Notifications<'a> {
    pub(super) fn new(graph: &'a CompileGraph, actor: PistonId) -> Self {
        Self {
            graph,
            actor,
            facts: guards::Facts::ready(graph, actor),
            samples: FxHashMap::default(),
        }
    }

    /// A power corridor only qualifies when its own callback cone reaches the actor.
    pub(super) fn trigger(&mut self, route: RouteId, queries: &[PowerTarget]) -> Option<(RouteOccurrence, RouteOccurrence)> {
        let graph = self.graph;
        let actor = self.actor;
        for (ordinal, delivery) in graph.routes[route.index()].deliveries.iter().enumerate() {
            if delivery.view != OperandView::Live
                || !matches!(delivery.kind, DeliveryKind::Update { .. })
                || self.facts.value(delivery.guard) != Some(true)
            {
                continue;
            }
            let notification = RouteOccurrence {
                route,
                ordinal: ordinal as u32,
            };
            if samples(graph, delivery, actor) {
                return Some((notification, notification));
            }
            let Some(CallbackReceiver::Component(wire)) = delivery.receiver else {
                continue;
            };
            if !matches!(graph.nodes[wire.index()].kind, CircuitNodeKind::Wire(_))
                || !queries.contains(&PowerTarget::Input {
                    node: wire,
                    port: InputPort::Main,
                })
            {
                continue;
            }
            for target in queries {
                let PowerTarget::Input { node, port: InputPort::Main } = *target else {
                    continue;
                };
                if let Some(sample) = self.wire_sample(node) {
                    return Some((notification, sample));
                }
            }
        }
        None
    }

    fn wire_sample(&mut self, node: NodeId) -> Option<RouteOccurrence> {
        if let Some(&sample) = self.samples.get(&node) {
            return sample;
        }
        let CircuitNodeKind::Wire(index) = self.graph.nodes[node.index()].kind else {
            return None;
        };
        let route = self.graph.wires[index].turbo_route?;
        let sample = self.graph.routes[route.index()]
            .deliveries
            .iter()
            .enumerate()
            .find_map(|(ordinal, delivery)| {
                (matches!(delivery.kind, DeliveryKind::Update { .. })
                    && samples(self.graph, delivery, self.actor)
                    && self.facts.value(delivery.guard) == Some(true))
                .then_some(RouteOccurrence {
                    route,
                    ordinal: ordinal as u32,
                })
            });
        self.samples.insert(node, sample);
        sample
    }
}

fn samples(graph: &CompileGraph, delivery: &RouteDelivery, actor: PistonId) -> bool {
    match delivery.receiver {
        Some(CallbackReceiver::PistonBase(target) | CallbackReceiver::PistonHead(target)) => target == actor,
        Some(CallbackReceiver::Component(node)) => graph.nodes[node.index()].kind == CircuitNodeKind::Piston(actor),
        _ => false,
    }
}
