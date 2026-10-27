//! Extract causal order from actual callback rows for connected cells.
//! Internal Turbo paths still require a separate arrival certificate; no discovery
//! order is substituted for one. Each outside route is a separate possible entry.
use super::*;

enum Failure {
    /// This whole region lacks a representable source or observation witness.
    Unsupported(LayerWitness, LayerDeclineReason),
    /// Optional symbolic construction exhausted its per-region proof budget.
    BudgetExceeded,
    /// This graph or cancellation prevents publication.
    Invalid(String),
}
impl From<String> for Failure {
    fn from(value: String) -> Self {
        Self::Invalid(value)
    }
}

impl From<ConstructionError> for Failure {
    fn from(error: ConstructionError) -> Self {
        match error {
            ConstructionError::BudgetExceeded => Self::BudgetExceeded,
            ConstructionError::Unsupported(message) => Self::Invalid(message),
            ConstructionError::FeedbackRequiresRegister(source) => Self::Invalid(format!("logical feedback requires a register: {source:?}")),
            ConstructionError::Invalid(message) => Self::Invalid(message),
        }
    }
}

pub(in crate::redpiler::analysis::instant::layers) fn compile_connected(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    component: &RegionComponent,
    region_index: usize,
    partition: &regions::RegionPartition,
    power: &PowerIndex,
    perimeter: &FxHashSet<(BoundaryDirection, BoundaryPortKind)>,
    monitor: &TaskMonitor,
) -> Result<Result<CompiledRegion, LayerDecline>, ConstructionError> {
    match compile(graph, catalog, component, region_index, partition, power, perimeter, monitor) {
        Ok((region, arrivals)) => Ok(Ok(CompiledRegion {
            region,
            arrivals: Some(arrivals),
        })),
        Err(Failure::Unsupported(witness, reason)) => Ok(Err(decline(component, witness, reason))),
        Err(Failure::BudgetExceeded) => Err(ConstructionError::BudgetExceeded),
        Err(Failure::Invalid(message)) => Err(ConstructionError::Invalid(message)),
    }
}

struct Routes<'a> {
    /// These original route rows provide callback order and receivers.
    graph: &'a CompileGraph,
    /// This maps owned actors to their local equation contracts.
    actors: FxHashMap<PistonId, u32>,
    /// These scheduler-owned reset observers are lifecycle effects rather than computation uses.
    resets: FxHashSet<NodeId>,
    /// These externally exposed callback roots preserve actual phase order and duplicate occurrences.
    outputs: Vec<LayerOutput>,
    /// These unchanged outside Graph readers retain live geometry and captured snapshots.
    graph_outputs: Vec<u32>,
    /// These catalog ports identify the original outside recipients of each effect.
    ports: &'a [BoundaryPort],
    /// These are the published port ordinals for this connected component.
    region_ports: &'a [u32],
    /// Only these reads/effects cross the merged region boundary.
    perimeter: &'a FxHashSet<(BoundaryDirection, BoundaryPortKind)>,
    /// These monotone cable certificates replace complete internal Turbo responses.
    arrivals: Vec<super::corridors::Arrival>,
}

fn compile(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    component: &RegionComponent,
    region_index: usize,
    partition: &regions::RegionPartition,
    power: &PowerIndex,
    perimeter: &FxHashSet<(BoundaryDirection, BoundaryPortKind)>,
    monitor: &TaskMonitor,
) -> Result<(LayeredRegion, Vec<super::corridors::Arrival>), Failure> {
    let mut routes = Routes {
        graph,
        actors: FxHashMap::default(),
        resets: FxHashSet::default(),
        outputs: Vec::new(),
        graph_outputs: Vec::new(),
        ports: &catalog.ports,
        region_ports: &component.ports,
        perimeter,
        arrivals: Vec::new(),
    };
    let Some(arrivals) = super::corridors::certify(graph, catalog, component, power, monitor)? else {
        let witness = first_unproven_power(
            graph,
            catalog,
            component,
            region_index,
            &partition.owners,
            &partition.owner_region,
            monitor,
        )?
        .unwrap_or_else(|| first_witness(graph, catalog, component));
        return Err(Failure::Unsupported(witness, LayerDeclineReason::SignalArrivalUnproven));
    };
    routes.arrivals = arrivals;
    for &cell in &component.cells {
        for use_ in &catalog.cells[cell as usize].uses {
            routes.actors.insert(use_.actor, cell);
            if let Some(reset) = use_.reset.observer() {
                routes.resets.insert(reset);
            }
        }
    }
    let mut entries = FxHashSet::default();
    for &cell in &component.cells {
        for use_ in &catalog.cells[cell as usize].uses {
            for occurrence in &use_.activations {
                let source = graph.routes[occurrence.route.index()].source;
                if !routes.owns_node(source) {
                    entries.insert(occurrence.route);
                }
            }
        }
    }
    if entries.is_empty() {
        return Err(Failure::Unsupported(
            first_witness(graph, catalog, component),
            LayerDeclineReason::CallbackSemanticsUnsupported,
        ));
    }
    let mut entries: Vec<_> = entries.into_iter().collect();
    entries.sort_unstable();
    let mut composition = Composition::new(graph, catalog, power, component, monitor)?;
    let no = composition.builder.constant(false)?;
    let mut entry_roots = Vec::new();
    for &cell in &component.cells {
        let actor = catalog.cells[cell as usize].uses[0].actor;
        let pins = composition.pins(cell, actor, no)?;
        entry_roots.push(pins[catalog.cells[cell as usize].stationary_entry.index()]);
    }
    let mut quiet_reset_sites = FxHashSet::default();
    for arrival in &routes.arrivals {
        quiet_reset_sites.extend(arrival.quiet_reset_sites.iter().copied());
    }
    let mut quiet_reset_sites: Vec<_> = quiet_reset_sites.into_iter().collect();
    quiet_reset_sites.sort_unstable();
    for site in quiet_reset_sites {
        let powered = composition.builder.input(
            LayerInputSource::SourceAbove {
                site,
                port: PowerPort::Output,
                threshold: 0,
            },
            LayerInputRole::AcceptanceRevalidation,
        )?;
        let powered = composition.builder.input_expr(powered)?;
        entry_roots.push(composition.builder.not(powered)?);
    }
    let stationary_entry = composition.builder.all(entry_roots)?;
    // Only the actual entry route is active. Sorting keeps construction deterministic;
    // it does not prescribe an execution order between independent outside events.
    for entry in entries {
        routes.entry(entry, &mut composition)?;
    }
    routes.compose_wave(catalog, &mut composition)?;
    let lifecycle = lifecycle_occurrences(graph, catalog, component)?;
    routes.record_final_values(catalog, component, &mut composition)?;
    if !routes.bind_boundary_state(catalog, component, &mut composition.builder.inputs) {
        return Err(Failure::Unsupported(
            first_witness(graph, catalog, component),
            LayerDeclineReason::BoundaryEffectUnsupported,
        ));
    }
    let region = LayeredRegion {
        id: LayerRegionId(region_index as u32),
        cells: component.cells.clone(),
        corridors: component.corridors.clone(),
        stationary_entry,
        inputs: composition.builder.inputs.into(),
        expressions: composition.builder.expressions.into(),
        layers: composition.layers.into(),
        resource_outputs: composition.resources.into(),
        outputs: routes.outputs.into(),
        graph_outputs: routes.graph_outputs.into(),
        cold_lifecycle_occurrences: lifecycle.into(),
        pin_values: Box::new([]),
    };
    Ok((region, routes.arrivals))
}

fn lifecycle_occurrences(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    component: &RegionComponent,
) -> Result<Vec<LayerLifecycleOccurrence>, Failure> {
    let mut lifecycle = Vec::new();
    for &cell in &component.cells {
        for use_ in &catalog.cells[cell as usize].uses {
            let spec = graph
                .pistons
                .get(use_.actor.index())
                .ok_or_else(|| Failure::Invalid("cell actor has no piston specification".into()))?;
            for (route, phase) in [
                (Some(use_.retraction.wake.base_restored), LayerLifecyclePhase::DeferredBaseRestoration),
                (Some(spec.routes.head_restored), LayerLifecyclePhase::DeferredHeadRestoration),
                (
                    use_.retraction.wake.resource.map(|wake| wake.route),
                    LayerLifecyclePhase::DeferredResourceRestoration,
                ),
                (
                    use_.reset.observer().and_then(|node| graph.nodes[node.index()].output_route),
                    LayerLifecyclePhase::ResetOutput,
                ),
                (spec.routes.far_shape_changed, LayerLifecyclePhase::ResetFarShapeChange),
                (spec.routes.resource_restored_far, LayerLifecyclePhase::DeferredResourceRestoration),
            ] {
                if let Some(route) = route {
                    for ordinal in 0..graph.routes[route.index()].deliveries.len() {
                        lifecycle.push(LayerLifecycleOccurrence {
                            actor: use_.actor,
                            occurrence: RouteOccurrence {
                                route,
                                ordinal: ordinal as u32,
                            },
                            phase,
                            reason: LayerLifecycleReason::OrdinaryGraphLifecycle,
                        });
                    }
                }
            }
            if let (Some(reset_node), Some(wake)) = (use_.reset.observer(), use_.reset.observer_wake()) {
                for &occurrence in &wake.on_base_changed {
                    lifecycle.push(LayerLifecycleOccurrence {
                        actor: use_.actor,
                        occurrence,
                        phase: LayerLifecyclePhase::ResetObserverLifecycle,
                        reason: LayerLifecycleReason::ResetObserverUpdate { observer: reset_node },
                    });
                }
                for &occurrence in &wake.on_base_restored {
                    lifecycle.push(LayerLifecycleOccurrence {
                        actor: wake.watched_actor,
                        occurrence,
                        phase: LayerLifecyclePhase::ResetObserverLifecycle,
                        reason: LayerLifecycleReason::ResetObserverUpdate { observer: reset_node },
                    });
                }
                continue;
            }
            let reset_occurrences = match &use_.reset {
                CellReset::Payload { notification, sample, .. } | CellReset::InputRestoration { notification, sample, .. } => {
                    Some([*notification, *sample])
                }
                CellReset::Observer { .. } => None,
            };
            if let Some(occurrences) = reset_occurrences {
                for occurrence in occurrences {
                    let row = LayerLifecycleOccurrence {
                        actor: use_.actor,
                        occurrence,
                        phase: LayerLifecyclePhase::ResetInputLifecycle,
                        reason: LayerLifecycleReason::OrdinaryGraphLifecycle,
                    };
                    if !lifecycle.contains(&row) {
                        lifecycle.push(row);
                    }
                }
            }
        }
    }
    Ok(lifecycle)
}

impl Routes<'_> {
    fn bind_boundary_state(&self, catalog: &BoundaryCatalog, component: &RegionComponent, inputs: &mut [LayerInput]) -> bool {
        let resources: FxHashSet<_> = component
            .cells
            .iter()
            .flat_map(|&cell| catalog.cells[cell as usize].resources.iter().map(|pin| pin.resource))
            .collect();
        for input in inputs {
            let LayerInputSource::State(test) = input.source else { continue };
            let owned = match test {
                StateTest::PistonExtended(actor) | StateTest::PistonHeadPresent(actor) | StateTest::PistonAvailability { piston: actor, .. } => {
                    self.actors.contains_key(&actor)
                }
                StateTest::ResourceEnabled(resource) | StateTest::ResourceAt { resource, .. } => resources.contains(&resource),
            };
            if owned {
                continue;
            }
            let Some(&port) = self.region_ports.iter().find(|&&port| {
                let row = self.ports[port as usize];
                row.direction == BoundaryDirection::Input
                    && matches!(row.kind, BoundaryPortKind::GuardState { test: candidate, .. } if candidate == test)
            }) else {
                return false;
            };
            input.source = LayerInputSource::BoundaryState { port, test };
        }
        true
    }

    fn compose_wave(&mut self, catalog: &BoundaryCatalog, composition: &mut Composition<'_>) -> Result<(), Failure> {
        while let Some(acceptance) = composition.begin_next()? {
            let actor = acceptance.actor;
            let accepted = acceptance.accepted;
            let ancestors = acceptance.ancestors.clone();
            let use_ = catalog.cells[self.actors[&actor] as usize]
                .uses
                .iter()
                .find(|use_| use_.actor == actor)
                .expect("owned use");
            self.emit(use_.retraction.routes.base_changed, actor, accepted, &ancestors, composition)?;
            let transfer = composition.finish_base(acceptance)?;
            if let Some(route) = use_.retraction.routes.far_changed {
                self.emit(route, actor, transfer, &ancestors, composition)?;
            }
            self.emit(use_.retraction.routes.near_changed, actor, accepted, &ancestors, composition)?;
        }
        Ok(())
    }

    fn record_final_values(
        &mut self,
        catalog: &BoundaryCatalog,
        component: &RegionComponent,
        composition: &mut Composition<'_>,
    ) -> Result<(), Failure> {
        let graph = self.graph;
        let power = composition.power;
        let perimeter = self.perimeter;
        let mut tracer = signals::SignalTracer::new(graph, power, &composition.wires, &composition.state, LayerInputRole::RequestSample);
        for &port in &component.ports {
            if catalog.ports[port as usize].direction != BoundaryDirection::Output {
                continue;
            }
            let kind = catalog.ports[port as usize].kind;
            if !perimeter.contains(&(BoundaryDirection::Output, kind)) {
                continue;
            }
            let value = match kind {
                BoundaryPortKind::Callback { .. } => continue,
                BoundaryPortKind::Power { link } => tracer.contribution(graph.power_links[link as usize], 0, &mut composition.builder)?,
                BoundaryPortKind::GuardState { test, .. } => {
                    if let Some(&value) = composition.state.get(&test) {
                        value
                    } else {
                        let input = composition.builder.input(LayerInputSource::State(test), LayerInputRole::RequestSample)?;
                        composition.builder.input_expr(input)?
                    }
                }
                // These readers still execute on Graph against the original authoritative
                // contacts. A Boolean final value cannot replace an earlier capture.
                BoundaryPortKind::GuardSite { .. }
                | BoundaryPortKind::WireRelation { .. }
                | BoundaryPortKind::WireNeighborhood { .. }
                | BoundaryPortKind::CapturedWire { .. } => {
                    self.graph_outputs.push(port);
                    continue;
                }
                BoundaryPortKind::ContactAlternative { contact, ordinal } => {
                    tracer.guard(graph.contacts[contact.index()].variants[ordinal as usize].when, &mut composition.builder)?
                }
                _ => {
                    return Err(Failure::Unsupported(
                        first_witness(graph, catalog, component),
                        LayerDeclineReason::BoundaryEffectUnsupported,
                    ));
                }
            };
            self.outputs.push(LayerOutput { port, emitted_when: value });
        }
        Ok(())
    }

    fn owns_node(&self, node: NodeId) -> bool {
        self.resets.contains(&node)
            || self.arrivals.iter().any(|arrival| arrival.wires.contains(&node))
            || matches!(self.graph.nodes[node.index()].kind,CircuitNodeKind::Piston(actor) if self.actors.contains_key(&actor))
    }

    fn entry(&mut self, route: RouteId, composition: &mut Composition<'_>) -> Result<(), Failure> {
        for ordinal in 0..self.graph.routes[route.index()].deliveries.len() {
            let occurrence = RouteOccurrence {
                route,
                ordinal: ordinal as u32,
            };
            let ports: Vec<_> = self
                .region_ports
                .iter()
                .copied()
                .filter(|&port| {
                    self.ports[port as usize].direction == BoundaryDirection::Input
                        && self.ports[port as usize].kind == BoundaryPortKind::Callback { occurrence }
                })
                .collect();
            let Some(&port) = ports.first() else { continue };
            let delivery = self.graph.routes[route.index()].deliveries[ordinal];
            if delivery.view == OperandView::Captured {
                // The external callback already ran before the action FIFO; do not
                // re-resolve a captured contact or replay its non-piston effects.
                for port in ports {
                    let BoundaryOwner::Cell(cell) = self.ports[port as usize].owner else {
                        continue;
                    };
                    let actors: Vec<_> = composition.catalog.cells[cell as usize]
                        .uses
                        .iter()
                        .filter(|use_| use_.activations.contains(&occurrence))
                        .map(|use_| use_.actor)
                        .collect();
                    for actor in actors {
                        let input = composition.builder.input(
                            LayerInputSource::BoundaryRequest { port, occurrence, actor },
                            LayerInputRole::ActivationEvent,
                        )?;
                        let requested = composition.builder.input_expr(input)?;
                        composition.queued_sample(cell, actor, requested, occurrence)?;
                    }
                }
                continue;
            }
            let input = composition
                .builder
                .input(LayerInputSource::BoundaryEvent { port, occurrence }, LayerInputRole::ActivationEvent)?;
            let event = composition.builder.input_expr(input)?;
            self.delivery(occurrence, None, event, &FxHashSet::default(), composition)?;
        }
        Ok(())
    }

    fn emit(
        &mut self,
        route: RouteId,
        emitter: PistonId,
        when: LayerExprId,
        ancestors: &FxHashSet<PistonId>,
        composition: &mut Composition<'_>,
    ) -> Result<(), Failure> {
        if matches!(composition.builder.expression(when)?, LayerExpr::Constant(false)) {
            return Ok(());
        }
        for ordinal in 0..self.graph.routes[route.index()].deliveries.len() {
            self.delivery(
                RouteOccurrence {
                    route,
                    ordinal: ordinal as u32,
                },
                Some(emitter),
                when,
                ancestors,
                composition,
            )?;
        }
        Ok(())
    }

    fn delivery(
        &mut self,
        occurrence: RouteOccurrence,
        emitter: Option<PistonId>,
        when: LayerExprId,
        ancestors: &FxHashSet<PistonId>,
        composition: &mut Composition<'_>,
    ) -> Result<(), Failure> {
        let delivery = self.graph.routes[occurrence.route.index()].deliveries[occurrence.ordinal as usize];
        if delivery.view != OperandView::Live {
            return Err(Failure::Unsupported(
                LayerWitness::Callback(occurrence),
                LayerDeclineReason::CallbackSemanticsUnsupported,
            ));
        }
        let mut tracer = signals::SignalTracer::new(
            self.graph,
            composition.power,
            &composition.wires,
            &composition.state,
            LayerInputRole::RequestSample,
        );
        let guard = tracer.guard(delivery.guard, &mut composition.builder)?;
        let when = composition.builder.all([when, guard])?;
        if matches!(composition.builder.expression(when)?, LayerExpr::Constant(false)) {
            return Ok(());
        }
        let receiver = delivery
            .receiver
            .or_else(|| match (self.graph.sites[delivery.target.index()].source, delivery.kind) {
                (SiteSource::Component(node), DeliveryKind::Update { .. }) => Some(CallbackReceiver::Component(node)),
                _ => None,
            });
        let actor = match receiver {
            Some(CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor)) => Some(actor),
            Some(CallbackReceiver::Component(node)) => match self.graph.nodes[node.index()].kind {
                CircuitNodeKind::Piston(actor) => Some(actor),
                CircuitNodeKind::Wire(_) => {
                    if self.arrivals.iter().any(|arrival| arrival.wires.contains(&node)) {
                        if let Some(arrival) = self
                            .arrivals
                            .iter()
                            .find(|arrival| arrival.wires.contains(&node) && occurrence == arrival.start && emitter == Some(arrival.producer))
                        {
                            if !ancestors.contains(&arrival.producer) {
                                return Err(Failure::Invalid("corridor arrival has no accepted source provenance".into()));
                            }
                            for finish in arrival.finishes.iter() {
                                let Some(callback_guard) = composition.captured_guard(finish.guard)? else {
                                    return Err(Failure::Unsupported(
                                        LayerWitness::Callback(finish.occurrence),
                                        LayerDeclineReason::CallbackSemanticsUnsupported,
                                    ));
                                };
                                let mut callback_terms = Vec::with_capacity(finish.prior_guards.len() + 2);
                                callback_terms.push(when);
                                for &prior in finish.prior_guards.iter() {
                                    let Some(prior_guard) = composition.captured_guard(prior)? else {
                                        return Err(Failure::Unsupported(
                                            LayerWitness::Callback(finish.occurrence),
                                            LayerDeclineReason::CallbackSemanticsUnsupported,
                                        ));
                                    };
                                    callback_terms.push(composition.builder.not(prior_guard)?);
                                }
                                callback_terms.push(callback_guard);
                                let callback_when = composition.builder.all(callback_terms)?;
                                // Each captured target chooses its first guard-qualified row.
                                // `sample_after` then preserves piston request de-duplication
                                // across the repeated Turbo directions.
                                composition.sample_after_from(
                                    self.actors[&finish.actor],
                                    finish.actor,
                                    callback_when,
                                    vec![finish.occurrence].into(),
                                    vec![arrival.start].into(),
                                    ancestors,
                                )?;
                            }
                        }
                        return Ok(());
                    }
                    if self.actors.len() > 1 {
                        return Err(Failure::Unsupported(
                            LayerWitness::Callback(occurrence),
                            LayerDeclineReason::SignalArrivalUnproven,
                        ));
                    }
                    // A singleton retains the outside walk on Graph.
                    None
                }
                _ => None,
            },
            _ => None,
        };
        if matches!(delivery.kind, DeliveryKind::Update { .. }) {
            if let Some(actor) = actor {
                if let Some(&cell) = self.actors.get(&actor) {
                    let consumed = composition.consumed.get(&actor).copied().unwrap_or(composition.builder.constant(false)?);
                    let unused = composition.builder.not(consumed)?;
                    let when = composition.builder.all([when, unused])?;
                    if !matches!(composition.builder.expression(when)?, LayerExpr::Constant(false)) {
                        composition.sample_after(cell, actor, when, vec![occurrence].into(), ancestors)?;
                    }
                    return Ok(());
                }
            }
        } else if actor.is_some_and(|actor| self.actors.contains_key(&actor)) {
            return Err(Failure::Unsupported(
                LayerWitness::Callback(occurrence),
                LayerDeclineReason::CallbackSemanticsUnsupported,
            ));
        }
        if self.owns_node(self.graph.routes[occurrence.route.index()].source) {
            for &port in self.region_ports {
                let row = self.ports[port as usize];
                if row.direction == BoundaryDirection::Output
                    && row.kind == (BoundaryPortKind::Callback { occurrence })
                    && self.perimeter.contains(&(row.direction, row.kind))
                {
                    self.outputs.push(LayerOutput { port, emitted_when: when });
                }
            }
        }
        Ok(())
    }
}
