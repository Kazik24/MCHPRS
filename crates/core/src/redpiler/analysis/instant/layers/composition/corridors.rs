//! First arrival certificate: a private redstone source feeds one flat straight cable
//! and one computation actor. Source removal is monotone; the only possible queued
//! computation is the consumer's unpowered request. Reset observers coalesce their
//! wire notifications with the same-phase guaranteed base notification (or are powered).
use super::*;
use mchprs_blocks::blocks::Block;

pub(in crate::redpiler::analysis::instant::layers) struct Arrival {
    /// This exact unconditional source-Far delivery starts the synchronous wire response.
    pub(in crate::redpiler::analysis::instant::layers) start: RouteOccurrence,
    /// This original wire delivery proves that the consumer receives a qualifying update.
    pub(in crate::redpiler::analysis::instant::layers) finish: RouteOccurrence,
    /// This actor owns the private source removed by the start event.
    pub(in crate::redpiler::analysis::instant::layers) producer: PistonId,
    /// This actor is the cable's only potentially active computation receiver.
    pub(in crate::redpiler::analysis::instant::layers) consumer: PistonId,
    /// These wires have no other observable electrical or callback consumers.
    pub(in crate::redpiler::analysis::instant::layers) wires: FxHashSet<NodeId>,
}

pub(in crate::redpiler::analysis::instant::layers) fn certify(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    component: &RegionComponent,
    power: &PowerIndex,
    monitor: &TaskMonitor,
) -> Result<Option<Vec<Arrival>>, ConstructionError> {
    if component.corridors.is_empty() {
        return Ok(Some(Vec::new()));
    }
    // Each cable admits only its producer and one consumer; all requests still enter the
    // enclosing FIFO after the synchronous walk, even when the component has more cells.
    let mut private = Vec::new();
    for &cell in &component.cells {
        let contract = &catalog.cells[cell as usize];
        if !super::super::partition::private_observer_cell(graph, contract) {
            return Ok(None);
        }
        let resource = &contract.resources[0];
        let use_ = &contract.uses[0];
        private.push((use_, resource));
    }
    let all_wires: FxHashSet<_> = component
        .corridors
        .iter()
        .flat_map(|&id| catalog.corridors[id as usize].wires.iter().copied())
        .collect();
    let mut arrivals = Vec::new();
    for &corridor in &component.corridors {
        check_cancelled(monitor)?;
        let wires: FxHashSet<_> = catalog.corridors[corridor as usize].wires.iter().copied().collect();
        if !closed_straight_cable(graph, &wires) {
            return Ok(None);
        }
        let mut ordered_wires: Vec<_> = wires.iter().copied().collect();
        ordered_wires.sort_unstable();
        let mut builder = LayerBuilder::new(monitor)?;
        let state = FxHashMap::default();
        let mut tracer = signals::SignalTracer::new(graph, power, &all_wires, &state, LayerInputRole::RequestSample);
        let mut producer = None;
        for &(use_, resource) in &private {
            let enabled = builder.input(
                LayerInputSource::State(StateTest::ResourceEnabled(resource.resource)),
                LayerInputRole::RequestSample,
            )?;
            let enabled = builder.input_expr(enabled)?;
            let at = builder.input(
                LayerInputSource::State(StateTest::ResourceAt {
                    resource: resource.resource,
                    contact: resource.source,
                }),
                LayerInputRole::RequestSample,
            )?;
            let at = builder.input_expr(at)?;
            let present = builder.all([enabled, at])?;
            let mut matches = true;
            for &wire in &ordered_wires {
                let root = tracer.query(
                    PowerTarget::Input {
                        node: wire,
                        port: InputPort::Main,
                    },
                    0,
                    &mut builder,
                )?;
                matches &= root == present;
            }
            if matches {
                if producer.is_some() {
                    return Ok(None);
                }
                producer = Some((use_, resource, present));
            }
        }
        let Some((producer, source, present)) = producer else {
            return Ok(None);
        };
        let mut consumer = None;
        for &(use_, _) in &private {
            if use_.actor == producer.actor {
                continue;
            }
            let node = graph.pistons[use_.actor.index()].node;
            let root = tracer.query(PowerTarget::Input { node, port: InputPort::Main }, 0, &mut builder)?;
            if root != present {
                continue;
            }
            // Quasi power may only be the existing reset clock; other inhibits need another proof.
            if power
                .get(&PowerTarget::Input {
                    node,
                    port: InputPort::Quasi,
                })
                .into_iter()
                .flatten()
                .any(|link| local_reset_output_link(graph, power, use_, *link).is_none())
            {
                continue;
            }
            if consumer.is_some() {
                return Ok(None);
            }
            consumer = Some(use_);
        }
        let Some(consumer) = consumer else {
            return Ok(None);
        };
        let mut finish = None;
        for &wire in &ordered_wires {
            let CircuitNodeKind::Wire(index) = graph.nodes[wire.index()].kind else {
                return Ok(None);
            };
            for route in [graph.wires[index].shape_route, graph.wires[index].turbo_route].into_iter().flatten() {
                for (ordinal, delivery) in graph.routes[route.index()].deliveries.iter().enumerate() {
                    let receiver = delivery
                        .receiver
                        .or_else(|| match (graph.sites[delivery.target.index()].source, delivery.kind) {
                            (SiteSource::Component(node), DeliveryKind::Update { .. }) => Some(CallbackReceiver::Component(node)),
                            _ => None,
                        });
                    match receiver {
                        Some(CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor)) => {
                            if actor != consumer.actor && actor != producer.actor {
                                return Ok(None);
                            }
                            if actor == consumer.actor
                                && matches!(delivery.kind, DeliveryKind::Update { .. })
                                && consumer.activations.contains(&RouteOccurrence {
                                    route,
                                    ordinal: ordinal as u32,
                                })
                                && callback_guard(graph, delivery.guard, producer, consumer, 0) == Some(true)
                            {
                                // The first active consumer update suffices; its stationary receiver exists throughout this walk.
                                finish.get_or_insert(RouteOccurrence {
                                    route,
                                    ordinal: ordinal as u32,
                                });
                            }
                        }
                        Some(CallbackReceiver::Component(node)) => {
                            if !wires.contains(&node) && Some(node) != producer.reset.observer() && Some(node) != consumer.reset.observer() {
                                return Ok(None);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        let Some(finish) = finish else {
            return Ok(None);
        };
        let Some(route) = producer.retraction.routes.far_changed else {
            return Ok(None);
        };
        let start = graph.routes[route.index()].deliveries.iter().enumerate().find_map(|(ordinal, delivery)| {
            let node = match delivery.receiver {
                Some(CallbackReceiver::Component(node)) => node,
                _ => return None,
            };
            (wires.contains(&node)
                && delivery.view == OperandView::Live
                && matches!(delivery.kind, DeliveryKind::Update { .. })
                && matches!(graph.guards[delivery.guard.index()], Guard::Always))
            .then_some(RouteOccurrence {
                route,
                ordinal: ordinal as u32,
            })
        });
        let Some(start) = start else {
            return Ok(None);
        };
        // The source is adjacent to the cable; every wire starts at its uniquely derived 15-distance level.
        let source_pos = graph.contacts[source.source.index()].position;
        for &wire in &ordered_wires {
            let pos = graph.nodes[wire.index()].position.expect("wire position");
            let distance = (pos.x - source_pos.x).abs() + (pos.z - source_pos.z).abs();
            let ComponentState::Wire(saved) = graph.nodes[wire.index()].state else {
                return Ok(None);
            };
            if pos.y != source_pos.y || distance < 1 || distance > 15 || saved.power != 16 - distance as u8 || crate::redstone::wire::is_dot(saved) {
                return Ok(None);
            }
        }
        arrivals.push(Arrival {
            start,
            finish,
            producer: producer.actor,
            consumer: consumer.actor,
            wires,
        });
    }
    Ok(Some(arrivals))
}

pub(in crate::redpiler::analysis::instant::layers) fn closed_straight_cable(graph: &CompileGraph, wires: &FxHashSet<NodeId>) -> bool {
    if wires.is_empty() || wires.len() > 15 {
        return false;
    }
    let mut positions: Vec<_> = wires
        .iter()
        .map(|wire| graph.nodes[wire.index()].position.expect("wire position was validated"))
        .collect();
    positions.sort_by_key(|p| (p.x, p.z));
    let first = positions[0];
    let straight_x = positions
        .iter()
        .enumerate()
        .all(|(i, p)| p.y == first.y && p.z == first.z && p.x == first.x + i as i32);
    let straight_z = positions
        .iter()
        .enumerate()
        .all(|(i, p)| p.y == first.y && p.x == first.x && p.z == first.z + i as i32);
    if !straight_x && !straight_z {
        return false;
    }
    // A diagonal or nearby independent wire can introduce captured callbacks or another walk.
    for &wire in wires {
        let CircuitNodeKind::Wire(index) = graph.nodes[wire.index()].kind else {
            return false;
        };
        for &site in &graph.wires[index].canonical_neighbors {
            if let SiteSource::Component(other) = graph.sites[site.index()].source {
                if matches!(graph.nodes[other.index()].kind, CircuitNodeKind::Wire(_)) && !wires.contains(&other) {
                    return false;
                }
            }
        }
    }
    true
}

/// During the certified walk only wire strengths change; captured block properties equal live properties.
fn callback_guard(graph: &CompileGraph, id: GuardId, producer: &CellUse, consumer: &CellUse, depth: usize) -> Option<bool> {
    if depth >= 128 {
        return None;
    }
    let state = |test: StateTest| -> Option<bool> {
        match test {
            StateTest::PistonExtended(actor) if actor == producer.actor || actor == consumer.actor => Some(true),
            StateTest::PistonHeadPresent(actor) if actor == producer.actor => Some(false),
            StateTest::PistonHeadPresent(actor) if actor == consumer.actor => Some(true),
            StateTest::PistonAvailability { piston, is } if piston == producer.actor => Some(is == PistonAvailability::BasePending),
            StateTest::PistonAvailability { piston, is } if piston == consumer.actor => Some(is == PistonAvailability::Ready),
            StateTest::ResourceEnabled(resource) if producer.retraction.transfer.is_some_and(|t| t.resource == resource) => Some(false),
            StateTest::ResourceEnabled(resource) if consumer.retraction.transfer.is_some_and(|t| t.resource == resource) => Some(true),
            StateTest::ResourceAt { resource, contact } => {
                if let Some(t) = producer.retraction.transfer.filter(|t| t.resource == resource) {
                    Some(contact == t.destination)
                } else {
                    consumer
                        .retraction
                        .transfer
                        .filter(|t| t.resource == resource)
                        .map(|t| contact == t.source)
                }
            }
            _ => None,
        }
    };
    match &graph.guards[id.index()] {
        Guard::Always => Some(true),
        Guard::State(test) => state(*test),
        Guard::Not(child) => callback_guard(graph, *child, producer, consumer, depth + 1).map(|v| !v),
        Guard::All(children) => {
            let values = children
                .iter()
                .map(|&g| callback_guard(graph, g, producer, consumer, depth + 1))
                .collect::<Option<Vec<_>>>()?;
            Some(values.into_iter().all(|v| v))
        }
        Guard::Any(children) => {
            let values = children
                .iter()
                .map(|&g| callback_guard(graph, g, producer, consumer, depth + 1))
                .collect::<Option<Vec<_>>>()?;
            Some(values.into_iter().any(|v| v))
        }
        Guard::Site { site, test, .. } => match graph.sites[site.index()].source {
            SiteSource::Constant(profile) => Some(signals::profile_matches(graph.profiles[profile.index()], Some(profile), *test)),
            SiteSource::Component(node) => {
                let spec = &graph.nodes[node.index()];
                let block = match spec.state {
                    ComponentState::Wire(wire) => Block::RedstoneWire { wire },
                    _ => Block::from_id(spec.raw_state?),
                };
                Some(match test {
                    SiteTest::Solid => block.is_solid(),
                    SiteTest::Transparent => block.is_transparent(),
                    SiteTest::IsWire => matches!(block, Block::RedstoneWire { .. }),
                    SiteTest::Profile(_) => false,
                })
            }
            SiteSource::Contact(contact) => {
                for variant in &graph.contacts[contact.index()].variants {
                    if callback_guard(graph, variant.when, producer, consumer, depth + 1)? {
                        return Some(signals::profile_matches(
                            graph.profiles[variant.profile.index()],
                            Some(variant.profile),
                            *test,
                        ));
                    }
                }
                None
            }
        },
    }
}
