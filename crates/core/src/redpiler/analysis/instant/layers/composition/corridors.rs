//! First arrival certificate: a private redstone source feeds one flat straight cable
//! and one computation actor. Source removal is monotone; the only possible queued
//! computation is the consumer's unpowered request. Reset observers coalesce their
//! wire notifications with the same-phase guaranteed base notification (or are powered).
use super::*;
use mchprs_blocks::blocks::Block;

#[derive(Clone)]
pub(in crate::redpiler::analysis::instant::layers) struct Arrival {
    /// This exact unconditional source-Far delivery starts the synchronous wire response.
    pub(in crate::redpiler::analysis::instant::layers) start: RouteOccurrence,
    /// This original wire delivery proves that the consumer receives a qualifying update.
    pub(in crate::redpiler::analysis::instant::layers) finishes: Box<[ArrivalFinish]>,
    /// The accepted actor whose Far change produced this source occurrence.
    pub(in crate::redpiler::analysis::instant::layers) producer: PistonId,
    /// These wires have no other observable electrical or callback consumers.
    pub(in crate::redpiler::analysis::instant::layers) wires: FxHashSet<NodeId>,
    /// Proof-derived power levels at the stationary hot-entry cut, independent of
    /// the schematic's saved initial input vector.
    pub(in crate::redpiler::analysis::instant::layers) expected_wire_levels: Box<[(NodeId, u8)]>,
    /// Reset observers that receive an unordered captured wire update are inert only
    /// while their existing native output is off at the entry cut.
    pub(in crate::redpiler::analysis::instant::layers) quiet_reset_sites: Box<[SiteId]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::redpiler::analysis::instant::layers) struct ArrivalFinish {
    /// This piston is sampled by the captured receiver at this point in native callback order.
    pub(in crate::redpiler::analysis::instant::layers) actor: PistonId,
    pub(in crate::redpiler::analysis::instant::layers) occurrence: RouteOccurrence,
    /// This captured callback predicate is evaluated at the composed FIFO prefix.
    pub(in crate::redpiler::analysis::instant::layers) guard: GuardId,
    /// Earlier eligible route rows for this same captured target win before this one.
    pub(in crate::redpiler::analysis::instant::layers) prior_guards: Box<[GuardId]>,
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
    if let Some(arrivals) = certify_single_wire_component_arrivals(graph, catalog, component, power, monitor)? {
        return Ok(Some(arrivals));
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
        let Some(resource_spec) = graph.resources.get(source.resource.index()) else {
            return Ok(None);
        };
        let Some(material) = graph.profiles.get(resource_spec.material.index()) else {
            return Ok(None);
        };
        if Block::from_id(material.raw_state) != Block::RedstoneBlock {
            return Ok(None);
        }
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
        let mut expected_wire_levels = Vec::with_capacity(ordered_wires.len());
        for &wire in &ordered_wires {
            let pos = graph.nodes[wire.index()].position.expect("wire position");
            let distance = (pos.x - source_pos.x).abs() + (pos.z - source_pos.z).abs();
            let ComponentState::Wire(saved) = graph.nodes[wire.index()].state else {
                return Ok(None);
            };
            if pos.y != source_pos.y || distance < 1 || distance > 15 || crate::redstone::wire::is_dot(saved) {
                return Ok(None);
            }
            expected_wire_levels.push((wire, 16 - distance as u8));
        }
        arrivals.push(Arrival {
            start,
            finishes: vec![ArrivalFinish {
                actor: consumer.actor,
                occurrence: finish,
                guard: graph.routes[finish.route.index()].deliveries[finish.ordinal as usize].guard,
                prior_guards: Box::new([]),
            }]
            .into(),
            producer: producer.actor,
            wires,
            expected_wire_levels: expected_wire_levels.into_boxed_slice(),
            quiet_reset_sites: Box::new([]),
        });
    }
    Ok(Some(arrivals))
}

/// Certify each isolated dust node in a multi-cell component independently. The
/// resulting arrivals retain the real source actor and captured callback order;
/// Composition then samples every receiver's full cell contract at its FIFO prefix.
fn certify_single_wire_component_arrivals(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    component: &RegionComponent,
    power: &PowerIndex,
    monitor: &TaskMonitor,
) -> Result<Option<Vec<Arrival>>, ConstructionError> {
    if component.corridors.iter().any(|&id| catalog.corridors[id as usize].wires.len() != 1) {
        return Ok(None);
    }

    let mut uses = FxHashMap::<PistonId, (u32, &CellUse)>::default();
    for &cell in &component.cells {
        for use_ in &catalog.cells[cell as usize].uses {
            if uses.insert(use_.actor, (cell, use_)).is_some() {
                return Ok(None);
            }
        }
    }
    if uses.is_empty() {
        return Ok(None);
    }

    let all_wires: FxHashSet<_> = component
        .corridors
        .iter()
        .flat_map(|&id| catalog.corridors[id as usize].wires.iter().copied())
        .collect();
    let mut arrivals = Vec::new();
    for &corridor_id in &component.corridors {
        check_cancelled(monitor)?;
        let corridor = &catalog.corridors[corridor_id as usize];
        let wire = corridor.wires[0];
        if !closed_straight_cable(graph, &FxHashSet::from_iter([wire])) {
            return Ok(None);
        }
        let CircuitNodeKind::Wire(wire_index) = graph.nodes[wire.index()].kind else {
            return Ok(None);
        };
        let wire_spec = &graph.wires[wire_index];
        let (Some(instant), Some(turbo_route)) = (wire_spec.execution.instant_node, wire_spec.turbo_route) else {
            return Ok(None);
        };
        let ComponentState::Wire(saved_wire) = graph.nodes[wire.index()].state else {
            return Ok(None);
        };
        if crate::redstone::wire::is_dot(saved_wire) {
            return Ok(None);
        }

        // A corridor source is one exact movable Far contact. Do not infer a
        // producer from wire strength alone: the equation root must name the
        // complete resource/source state, and every accepted producer must have
        // its own witnessed unconditional Far-change occurrence.
        let wire_pos = graph.nodes[wire.index()].position.expect("wire position was validated");
        let mut source_candidates = Vec::new();
        let mut checked_resources = FxHashSet::default();
        for &cell in &component.cells {
            for resource in &catalog.cells[cell as usize].resources {
                if !checked_resources.insert(resource.resource) {
                    continue;
                }
                let Some(resource_spec) = graph.resources.get(resource.resource.index()) else {
                    continue;
                };
                let Some(material) = graph.profiles.get(resource_spec.material.index()) else {
                    continue;
                };
                if Block::from_id(material.raw_state) != Block::RedstoneBlock {
                    continue;
                }
                let source_pos = graph.contacts[resource.source.index()].position;
                let distance = (wire_pos.x - source_pos.x).abs() + (wire_pos.y - source_pos.y).abs() + (wire_pos.z - source_pos.z).abs();
                if distance != 1 {
                    continue;
                }

                let mut producers: Vec<_> = uses
                    .values()
                    .map(|(_, use_)| *use_)
                    .filter(|use_| {
                        use_.retraction
                            .transfer
                            .is_some_and(|transfer| transfer.resource == resource.resource && transfer.source == resource.source)
                    })
                    .collect();
                producers.sort_unstable_by_key(|producer| producer.actor);
                if producers.is_empty() {
                    continue;
                }
                let mut producer_actors: Vec<_> = producers.iter().map(|producer| producer.actor).collect();
                producer_actors.sort_unstable();
                let mut source_owners = resource.owners.to_vec();
                source_owners.sort_unstable();
                if producer_actors != source_owners {
                    continue;
                }
                let starts = producers
                    .into_iter()
                    .map(|producer| {
                        let route = producer.retraction.routes.far_changed?;
                        if graph.routes[route.index()].source != graph.pistons[producer.actor.index()].node {
                            return None;
                        }
                        let matches: Vec<_> = graph.routes[route.index()]
                            .deliveries
                            .iter()
                            .enumerate()
                            .filter_map(|(ordinal, delivery)| {
                                let SiteSource::Component(node) = graph.sites[delivery.target.index()].source else {
                                    return None;
                                };
                                (node == wire
                                    && delivery.view == OperandView::Live
                                    && matches!(delivery.kind, DeliveryKind::Update { .. })
                                    && matches!(graph.guards[delivery.guard.index()], Guard::Always))
                                .then_some(RouteOccurrence {
                                    route,
                                    ordinal: ordinal as u32,
                                })
                            })
                            .collect();
                        if matches.len() == 1 { Some((matches[0], producer)) } else { None }
                    })
                    .collect::<Option<Vec<_>>>();
                let Some(starts) = starts else {
                    continue;
                };
                let mut builder = LayerBuilder::new(monitor)?;
                let state = FxHashMap::default();
                let mut tracer = signals::SignalTracer::new(graph, power, &all_wires, &state, LayerInputRole::RequestSample);
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
                let wire_power = tracer.query(
                    PowerTarget::Input {
                        node: wire,
                        port: InputPort::Main,
                    },
                    0,
                    &mut builder,
                )?;
                if wire_power == present {
                    source_candidates.push((resource, starts));
                }
            }
        }
        if source_candidates.len() != 1 {
            return Ok(None);
        }
        let (_, starts) = source_candidates.pop().expect("one source candidate");

        let mut producer_arrivals = Vec::with_capacity(starts.len());
        for (start, producer) in starts {
            check_cancelled(monitor)?;
            let mut finishes = Vec::new();
            let mut quiet_reset_sites = FxHashSet::default();
            for rows in captured_turbo_slots(graph, wire_spec, instant, turbo_route) {
                let mut target_actor = None;
                let mut prior_guards = Vec::new();
                for (occurrence, row) in rows {
                    let actor = match row.receiver {
                        Some(CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor)) => actor,
                        Some(CallbackReceiver::Component(node)) => match graph.nodes[node.index()].kind {
                            CircuitNodeKind::Piston(actor) => actor,
                            CircuitNodeKind::BuiltinReset { facing, .. } => {
                                if !matches!(row.kind, DeliveryKind::Update { .. }) {
                                    return Ok(None);
                                }
                                let DeliveryKind::Update { direction } = row.kind else { unreachable!() };
                                match direction {
                                    Some(direction) if direction != facing => {
                                        prior_guards.push(row.guard);
                                        continue;
                                    }
                                    Some(_) => return Ok(None),
                                    None => {
                                        let belongs_to_cell = uses.values().any(|(_, use_)| use_.reset.observer() == Some(node));
                                        if !belongs_to_cell {
                                            return Ok(None);
                                        }
                                        if graph.sites[row.target.index()].source != SiteSource::Component(node) {
                                            return Ok(None);
                                        }
                                        quiet_reset_sites.insert(row.target);
                                        prior_guards.push(row.guard);
                                        continue;
                                    }
                                }
                            }
                            _ => return Ok(None),
                        },
                        _ => return Ok(None),
                    };
                    let Some((_, consumer)) = uses.get(&actor).copied() else {
                        return Ok(None);
                    };
                    if target_actor.is_some_and(|selected| selected != actor) {
                        // Conditional alternatives may select a different actor;
                        // that needs a route-level first-match port certificate.
                        return Ok(None);
                    }
                    target_actor = Some(actor);
                    if !matches!(row.kind, DeliveryKind::Update { .. }) || !consumer.activations.contains(&occurrence) {
                        return Ok(None);
                    }
                    finishes.push(ArrivalFinish {
                        actor,
                        occurrence,
                        guard: row.guard,
                        prior_guards: prior_guards.clone().into_boxed_slice(),
                    });
                    prior_guards.push(row.guard);
                }
            }
            if finishes.is_empty() {
                return Ok(None);
            }
            producer_arrivals.push(Arrival {
                start,
                finishes: finishes.into_boxed_slice(),
                producer: producer.actor,
                wires: FxHashSet::from_iter([wire]),
                expected_wire_levels: vec![(wire, 15)].into_boxed_slice(),
                quiet_reset_sites: {
                    let mut sites: Vec<_> = quiet_reset_sites.into_iter().collect();
                    sites.sort_unstable();
                    sites.into_boxed_slice()
                },
            });
        }
        arrivals.extend(producer_arrivals);
    }
    Ok((!arrivals.is_empty()).then_some(arrivals))
}

/// Structural gate for joining a shared cell to one isolated, preplanned dust node.
/// The full source, callback-order, and owner proof remains in the certificate below.
pub(in crate::redpiler::analysis::instant::layers) fn may_join_single_wire_shared_arrival(
    graph: &CompileGraph,
    cell: &CellContract,
    corridor: &WireCorridor,
) -> bool {
    if !super::super::partition::direct_shared_observer_cell(graph, cell) || cell.resources.len() != 1 || corridor.wires.len() != 1 {
        return false;
    }
    let wire = corridor.wires[0];
    if !closed_straight_cable(graph, &FxHashSet::from_iter([wire])) {
        return false;
    }
    let CircuitNodeKind::Wire(index) = graph.nodes[wire.index()].kind else {
        return false;
    };
    let spec = &graph.wires[index];
    if spec.execution.instant_node.is_none() || spec.turbo_route.is_none() {
        return false;
    }
    let Some(resource) = graph.resources.get(cell.resources[0].resource.index()) else {
        return false;
    };
    let Some(material) = graph.profiles.get(resource.material.index()) else {
        return false;
    };
    if Block::from_id(material.raw_state) != Block::RedstoneBlock {
        return false;
    }
    let source = cell.resources[0].source;
    let source_position = graph.contacts[source.index()].position;
    let wire_position = graph.nodes[wire.index()].position.expect("wire position was validated");
    let distance =
        (wire_position.x - source_position.x).abs() + (wire_position.y - source_position.y).abs() + (wire_position.z - source_position.z).abs();
    if !matches!(graph.nodes[wire.index()].state, ComponentState::Wire(saved)
        if distance == 1 && !crate::redstone::wire::is_dot(saved))
    {
        return false;
    }
    true
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
                    // GraphBackend keeps component sites profile-less; only constants
                    // and contact variants can satisfy a Profile guard.
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

/// Captured receivers are visited in this exact canonical Turbo order, including
/// the repeated first four directions. Both private and multi-cell certificates
/// consume this common ordered walk rather than route or actor sorting.
fn captured_turbo_slots(
    graph: &CompileGraph,
    wire: &crate::redpiler::program_graph::WirePlan,
    instant: crate::redpiler::program_graph::InstantWireNode,
    route: RouteId,
) -> Vec<Vec<(RouteOccurrence, crate::redpiler::program_graph::RouteDelivery)>> {
    let mut slots = Vec::new();
    for &slot in crate::redstone::wire::TURBO_ORDER[3]
        .iter()
        .chain(crate::redstone::wire::TURBO_ORDER[3][..4].iter())
    {
        if instant.receivers & (1 << slot) == 0 {
            continue;
        }
        let target = wire.canonical_neighbors[slot];
        let rows = graph.routes[route.index()]
            .deliveries
            .iter()
            .enumerate()
            .filter(|(_, row)| row.target == target && row.receiver.is_some() && row.view == OperandView::Captured)
            .map(|(ordinal, row)| {
                (
                    RouteOccurrence {
                        route,
                        ordinal: ordinal as u32,
                    },
                    *row,
                )
            })
            .collect();
        slots.push(rows);
    }
    slots
}
