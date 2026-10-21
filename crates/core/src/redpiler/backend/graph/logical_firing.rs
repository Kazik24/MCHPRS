//! Direct execution for the first source-certified one-shot instant cell.
use super::pistons::DeferredResourceEvent;
use super::*;

pub(super) struct LogicalFiringRuntime {
    circuit: SettledCircuit,
    evaluation: SettledEvaluation,
    inputs: Vec<bool>,
    use_by_actor: Box<[Option<usize>]>,
    fired: Box<[bool]>,
    retained_callbacks: Box<[RouteOccurrence]>,
    observer: NodeId,
    reset_pending: bool,
}

impl LogicalFiringRuntime {
    pub(super) fn new(backend: &GraphBackend) -> Option<Self> {
        let circuit = backend.program.settled_circuit()?.clone();
        if circuit.timing.is_some() || backend.fixed_classic.is_some() {
            return None;
        }
        let plan = circuit.firing.as_ref()?;
        if plan.uses.len() != 1 || !circuit.declines.is_empty() {
            return None;
        }
        let use_ = &plan.uses[0];
        if !matches!(use_.reset, CellReset::Observer { .. }) || use_.effect.transfer.is_none() {
            return None;
        }
        let actor = use_.actor;
        if backend.logical_bud_owner_by_actor.get(actor.index()).is_some_and(Option::is_some) {
            return None;
        }
        if use_.activation_routes.is_empty() || use_.effect.actor_revision_increment != 1 {
            return None;
        }
        if backend.program.cpu_kernel_entry(actor).is_some() {
            return None;
        }
        let transfer = use_.effect.transfer?;
        let piston = backend.state.pistons.get(actor.index())?;
        let resource = backend.state.resources.get(transfer.resource.index())?;
        let observer = use_.reset.observer()?;
        let wake = use_.reset.observer_wake()?;
        if !piston.extended
            || piston.availability.base_pending()
            || !resource.enabled
            || resource.at != transfer.source
            || backend.is_pending(backend.program.pistons()[actor.index()].node)
            || backend.is_pending(observer)
            || wake.watched_actor != actor
            || wake.on_base_changed.is_empty()
            || wake.on_base_restored.is_empty()
        {
            return None;
        }
        if use_.activation_routes.iter().any(|occurrence| {
            let Some(delivery) = backend
                .program
                .routes()
                .get(occurrence.route.index())
                .and_then(|route| route.deliveries.get(occurrence.ordinal as usize))
            else {
                return true;
            };
            let receiver = delivery
                .receiver
                .or_else(|| match (backend.program.sites()[delivery.target.index()].source, delivery.kind) {
                    (SiteSource::Component(node), DeliveryKind::Update { .. }) => Some(CallbackReceiver::Component(node)),
                    _ => None,
                });
            delivery.view == OperandView::Captured
                || !matches!(receiver,
                    Some(CallbackReceiver::PistonBase(candidate) | CallbackReceiver::PistonHead(candidate)) if candidate == actor
                ) && !matches!(receiver, Some(CallbackReceiver::Component(node)) if node == backend.program.pistons()[actor.index()].node)
        }) {
            return None;
        }
        let wake_rows = use_.reset.observer_wake()?;
        if wake_rows
            .on_base_changed
            .iter()
            .chain(wake_rows.on_base_restored.iter())
            .any(|occurrence| {
                backend
                    .program
                    .routes()
                    .get(occurrence.route.index())
                    .and_then(|route| route.deliveries.get(occurrence.ordinal as usize))
                    .is_none_or(|delivery| delivery.view == OperandView::Captured)
            })
        {
            return None;
        }
        let mut lamp_outputs = Vec::new();
        for port in circuit.retained_effects.iter().filter(|port| port.direction == BoundaryDirection::Output) {
            match port.kind {
                BoundaryPortKind::Callback { occurrence } => {
                    let delivery = backend
                        .program
                        .routes()
                        .get(occurrence.route.index())?
                        .deliveries
                        .get(occurrence.ordinal as usize)?;
                    let receiver = delivery
                        .receiver
                        .or_else(|| match backend.program.sites()[delivery.target.index()].source {
                            SiteSource::Component(node) if matches!(delivery.kind, DeliveryKind::Update { .. }) => {
                                Some(CallbackReceiver::Component(node))
                            }
                            _ => None,
                        });
                    if !matches!(receiver, Some(CallbackReceiver::Component(node)) if matches!(&backend.program.nodes()[node.index()].kind, CircuitNodeKind::Electrical(NodeType::Lamp)))
                        || delivery.view == OperandView::Captured
                    {
                        return None;
                    }
                    lamp_outputs.push(occurrence);
                }
                BoundaryPortKind::Power { link } => {
                    let Some(PowerTarget::Input { node, .. }) = backend.program.power_links().get(link as usize).map(|link| link.to) else {
                        return None;
                    };
                    if !matches!(&backend.program.nodes()[node.index()].kind, CircuitNodeKind::Electrical(NodeType::Lamp)) {
                        return None;
                    }
                }
                BoundaryPortKind::GuardState { test, .. } => {
                    let belongs = match test {
                        StateTest::PistonExtended(candidate)
                        | StateTest::PistonHeadPresent(candidate)
                        | StateTest::PistonAvailability { piston: candidate, .. } => candidate == actor,
                        StateTest::ResourceAt { resource, .. } | StateTest::ResourceEnabled(resource) => resource == transfer.resource,
                    };
                    if !belongs {
                        return None;
                    }
                }
                _ => return None,
            }
        }
        if lamp_outputs.is_empty() {
            return None;
        }
        lamp_outputs.sort_unstable_by_key(|occurrence| (occurrence.route, occurrence.ordinal));
        let mut inputs = Vec::with_capacity(circuit.program.inputs.len());
        for input in circuit.program.inputs.iter() {
            match input.source {
                LayerInputSource::InstantFired { actor: input_actor } if input_actor == actor => inputs.push(false),
                LayerInputSource::InstantFired { .. } => return None,
                source => inputs.push(backend.sample_settled_input(source).ok()?),
            }
        }
        let evaluation = SettledEvaluation::new(&circuit).ok()?;
        let mut use_by_actor = vec![None; backend.program.pistons().len()];
        use_by_actor.get_mut(actor.index())?.replace(0);
        Some(Self {
            circuit,
            evaluation,
            inputs,
            use_by_actor: use_by_actor.into_boxed_slice(),
            fired: vec![false; backend.program.pistons().len()].into_boxed_slice(),
            retained_callbacks: lamp_outputs.into_boxed_slice(),
            observer,
            reset_pending: false,
        })
    }

    pub(super) fn owns_actor(&self, actor: PistonId) -> bool {
        self.use_by_actor.get(actor.index()).is_some_and(Option::is_some)
    }

    pub(super) fn owns_activation(&self, occurrence: RouteOccurrence) -> Option<PistonId> {
        self.circuit
            .firing
            .as_ref()?
            .uses
            .iter()
            .find(|use_| use_.activation_routes.contains(&occurrence))
            .map(|use_| use_.actor)
    }

    pub(super) fn owns_route(&self, route: RouteId) -> bool {
        self.circuit
            .firing
            .as_ref()
            .is_some_and(|plan| plan.uses.iter().any(|use_| use_.activation_routes.iter().any(|row| row.route == route)))
    }

    pub(super) fn owns_observer(&self, node: NodeId) -> bool {
        self.observer == node
    }

    pub(super) fn sample_action(&mut self, backend: &GraphBackend, actor: PistonId) -> Option<(Option<mchprs_world::PistonAction>, u64)> {
        let use_index = self.use_by_actor.get(actor.index()).copied().flatten()?;
        for (slot, input) in self.circuit.program.inputs.iter().enumerate() {
            self.inputs[slot] = match input.source {
                LayerInputSource::InstantFired { actor: input_actor } if input_actor == actor => self.fired[actor.index()],
                LayerInputSource::InstantFired { .. } => return None,
                source => backend.sample_settled_input(source).ok()?,
            };
        }
        let operations = self.evaluation.evaluate(&self.inputs).ok()? as u64;
        let use_ = &self.circuit.firing.as_ref()?.uses[use_index];
        let eligible = self.evaluation.outputs().get(use_.power_eligible_output as usize).copied()? != 0;
        let powered = self
            .circuit
            .outputs
            .iter()
            .enumerate()
            .filter_map(|(index, output)| match output.target {
                SettledTarget::Power(PowerTarget::Input {
                    node,
                    port: InputPort::Main | InputPort::Quasi,
                }) if node == backend.program.pistons()[actor.index()].node => self.evaluation.outputs().get(index).map(|value| *value != 0),
                _ => None,
            })
            .any(|value| value);
        let state = backend.state.pistons[actor.index()];
        let action = match (powered, state.extended) {
            (true, false) if state.availability == PistonAvailability::Ready && backend.piston_lane_available(actor) => {
                Some(mchprs_world::PistonAction::Extend)
            }
            (false, true) if eligible && !state.availability.base_pending() => Some(mchprs_world::PistonAction::Retract),
            _ => None,
        };
        Some((action, operations))
    }

    pub(super) fn set_fired(&mut self, actor: PistonId, value: bool) {
        self.fired[actor.index()] = value;
    }

    pub(super) fn set_reset_pending(&mut self, pending: bool) {
        self.reset_pending = pending;
    }

    pub(super) fn reset_pending(&self) -> bool {
        self.reset_pending
    }

    pub(super) fn observer(&self) -> NodeId {
        self.observer
    }

    pub(super) fn actor(&self) -> PistonId {
        self.circuit.firing.as_ref().expect("validated firing plan").uses[0].actor
    }

    pub(super) fn activation_count(&self, actor: PistonId) -> usize {
        self.effect(actor).map_or(0, |use_| use_.activation_routes.len())
    }

    pub(super) fn activation_at(&self, actor: PistonId, index: usize) -> Option<RouteOccurrence> {
        self.effect(actor)?.activation_routes.get(index).copied()
    }

    pub(super) fn effect(&self, actor: PistonId) -> Option<&LogicalFireUse> {
        let index = self.use_by_actor.get(actor.index()).copied().flatten()?;
        self.circuit.firing.as_ref()?.uses.get(index)
    }

    pub(super) fn retained_count(&self, route: RouteId) -> usize {
        self.retained_callbacks.iter().filter(|row| row.route == route).count()
    }

    pub(super) fn retained_at(&self, route: RouteId, index: usize) -> Option<RouteOccurrence> {
        self.retained_callbacks.iter().filter(|row| row.route == route).nth(index).copied()
    }
}

impl GraphBackend {
    pub(super) fn execute_logical_firing_activation(
        &mut self,
        occurrence: RouteOccurrence,
        delivery: RouteDelivery,
        captured: Option<&[Option<ObservedSite>]>,
    ) -> Result<bool, GraphExecutionError> {
        let Some(actor) = self.logical_firing.as_ref().and_then(|runtime| runtime.owns_activation(occurrence)) else {
            return Ok(false);
        };
        if matches!(delivery.kind, DeliveryKind::Update { .. }) && delivery.receiver.is_some_and(|receiver| self.instant_event_noop(receiver)) {
            return Ok(true);
        }
        if !self.guard_matches(delivery.guard, captured)
            || (delivery.view == OperandView::Captured
                && captured
                    .and_then(|sites| sites.get(delivery.target.index()))
                    .and_then(Option::as_ref)
                    .is_none())
        {
            return Ok(true);
        }
        let mut runtime = self.logical_firing.take().expect("checked logical firing runtime");
        let evaluation = runtime.sample_action(self, actor);
        self.logical_firing = Some(runtime);
        let Some((action, operations)) = evaluation else {
            return Ok(false);
        };
        self.counts.logical_kernel_evaluations += 1;
        self.counts.logical_kernel_operations += operations;
        self.queue_logical_firing_action(actor, occurrence.route, action);
        Ok(true)
    }

    fn queue_logical_firing_action(&mut self, actor: PistonId, route: RouteId, action: Option<mchprs_world::PistonAction>) {
        if let Some(action) = action {
            self.enqueue_piston(PistonRequest {
                piston: actor,
                action,
                source: Some(route),
            });
        }
    }

    pub(super) fn sample_logical_firing_actor(&mut self, actor: PistonId, route: RouteId) -> Result<bool, GraphExecutionError> {
        if !self.logical_firing.as_ref().is_some_and(|runtime| runtime.owns_actor(actor)) {
            return Ok(false);
        }
        let mut runtime = self.logical_firing.take().unwrap();
        let sample = runtime.sample_action(self, actor);
        self.logical_firing = Some(runtime);
        let Some((action, operations)) = sample else { return Ok(false) };
        self.counts.logical_kernel_evaluations += 1;
        self.counts.logical_kernel_operations += operations;
        self.queue_logical_firing_action(actor, route, action);
        Ok(true)
    }

    fn deliver_logical_firing_outputs(&mut self, route: RouteId) -> Result<(), GraphExecutionError> {
        let count = self.logical_firing.as_ref().map_or(0, |runtime| runtime.retained_count(route));
        for index in 0..count {
            let occurrence = self
                .logical_firing
                .as_ref()
                .and_then(|runtime| runtime.retained_at(route, index))
                .expect("retained occurrence index");
            let delivery = self.program.routes()[route.index()].deliveries[occurrence.ordinal as usize];
            self.deliver_callback(route, delivery, None, Some(occurrence))?;
        }
        Ok(())
    }

    pub(super) fn apply_logical_firing_action(&mut self, request: PistonRequest) -> Result<Option<bool>, GraphExecutionError> {
        let actor = request.piston;
        if !self.logical_firing.as_ref().is_some_and(|runtime| runtime.owns_actor(actor)) {
            return Ok(None);
        }
        let mut runtime = self.logical_firing.take().expect("checked logical firing runtime");
        let sample = runtime.sample_action(self, actor);
        self.logical_firing = Some(runtime);
        let accepted = sample.is_some_and(|(action, _)| action == Some(request.action))
            && match request.action {
                mchprs_world::PistonAction::Extend => self.state.pistons[actor.index()].availability == PistonAvailability::Ready,
                mchprs_world::PistonAction::Retract => {
                    self.state.pistons[actor.index()].extended
                        && !self.state.pistons[actor.index()].availability.base_pending()
                        && self.piston_retraction_available(actor, false)
                }
                mchprs_world::PistonAction::RetractWithoutPull => false,
            };
        #[cfg(test)]
        self.record_logical_firing_accept(request, accepted);
        if !accepted {
            return Ok(Some(false));
        }
        let extended_before = self.state.pistons[actor.index()].extended;
        match request.action {
            mchprs_world::PistonAction::Retract => {
                let use_ = self
                    .logical_firing
                    .as_ref()
                    .and_then(|runtime| runtime.effect(actor))
                    .cloned()
                    .expect("owned firing use");
                let transfer = use_.effect.transfer.expect("runtime admitted a resource transfer");
                let resource = self.state.resources[transfer.resource.index()];
                if !resource.enabled || resource.at != transfer.source {
                    return Ok(Some(false));
                }
                self.logical_firing.as_mut().unwrap().set_fired(actor, true);
                self.logical_firing.as_mut().unwrap().set_reset_pending(true);
                let revision = {
                    let piston = &mut self.state.pistons[actor.index()];
                    piston.revision = piston.revision.wrapping_add(u64::from(use_.effect.actor_revision_increment));
                    piston.availability = PistonAvailability::BasePendingHeadPresent;
                    piston.revision
                };
                self.deliver_logical_firing_outputs(use_.effect.routes.base_changed)?;
                let (_, resource_revision) = self
                    .set_resource_contact(transfer.resource, transfer.destination, false)
                    .expect("validated one-shot transfer changes the selected resource contact");
                if let Some(route) = use_.effect.routes.far_changed {
                    self.deliver_logical_firing_outputs(route)?;
                }
                self.state.pistons[actor.index()].availability = PistonAvailability::BasePending;
                self.state.pistons[actor.index()].head_present = false;
                self.deliver_logical_firing_outputs(use_.effect.routes.near_changed)?;
                self.future.deferred_pistons.push_back(DeferredPistonEvent {
                    piston: actor,
                    revision,
                    resource: Some(DeferredResourceEvent {
                        resource: transfer.resource,
                        revision: resource_revision,
                        contact: transfer.destination,
                    }),
                    due_tick: self.future.clock.logical_tick + u64::from(use_.effect.wake.ticks_after_acceptance),
                    extending: false,
                });
                let wake = use_.reset.observer_wake().expect("observer reset wake");
                self.wake_logical_observer(wake.on_base_changed.as_ref())?;
            }
            mchprs_world::PistonAction::Extend => self.extend_logical_firing(actor)?,
            mchprs_world::PistonAction::RetractWithoutPull => unreachable!(),
        }
        self.publish_piston_action(request, extended_before);
        Ok(Some(true))
    }

    fn extend_logical_firing(&mut self, actor: PistonId) -> Result<(), GraphExecutionError> {
        let spec = self.program.pistons()[actor.index()];
        let PistonLane::SingleResource { far } = spec.lane else {
            return Err(self.unsupported(spec.node, "logical firing piston lane"));
        };
        let resource = self.piston_resource_at(actor, spec.near);
        let revision = {
            let piston = &mut self.state.pistons[actor.index()];
            piston.revision = piston.revision.wrapping_add(1);
            piston.availability = PistonAvailability::HeadPending;
            piston.head_present = true;
            piston.revision
        };
        let deferred_resource = if let Some(resource) = resource {
            if !self.state.resources[resource.index()].enabled {
                return Err(self.unsupported(spec.node, "disabled logical firing payload"));
            }
            self.set_resource_contact(resource, far, false)
                .map(|(resource, revision)| DeferredResourceEvent {
                    resource,
                    revision,
                    contact: far,
                })
        } else {
            None
        };
        if deferred_resource.is_some()
            && let Some(route) = spec.routes.far_shape_changed
        {
            self.deliver_logical_firing_outputs(route)?;
        }
        self.deliver_logical_firing_outputs(spec.routes.near_changed)?;
        if deferred_resource.is_some() {
            self.deliver_logical_firing_outputs(spec.routes.near_changed)?;
        }
        self.future.deferred_pistons.push_back(DeferredPistonEvent {
            piston: actor,
            revision,
            resource: deferred_resource,
            due_tick: self.future.clock.logical_tick + 2,
            extending: true,
        });
        self.state.pistons[actor.index()].extended = true;
        self.deliver_logical_firing_outputs(spec.routes.base_changed)?;
        Ok(())
    }

    fn wake_logical_observer(&mut self, occurrences: &[RouteOccurrence]) -> Result<(), GraphExecutionError> {
        let observer = self.logical_firing.as_ref().expect("logical firing runtime").observer();
        let facing = match &self.program.nodes()[observer.index()].kind {
            CircuitNodeKind::Observer { facing, .. } => *facing,
            _ => return Err(self.unsupported(observer, "logical firing reset observer")),
        };
        for occurrence in occurrences {
            let Some(delivery) = self
                .program
                .routes()
                .get(occurrence.route.index())
                .and_then(|route| route.deliveries.get(occurrence.ordinal as usize))
            else {
                return Err(self.unsupported(observer, "logical firing observer wake occurrence"));
            };
            if !self.guard_matches(delivery.guard, None) {
                continue;
            }
            let direction = match delivery.kind {
                DeliveryKind::Update { direction } => direction,
                DeliveryKind::Shape { direction } => Some(direction),
            };
            let qualifies = match direction {
                Some(direction) => facing == direction.into(),
                None => self.node_state(observer).powered,
            };
            if qualifies && !self.is_pending(observer) {
                self.schedule(observer, 1, TickPriority::Normal);
            }
        }
        Ok(())
    }

    pub(super) fn tick_logical_firing_observer(&mut self, observer: NodeId) -> Result<(), GraphExecutionError> {
        if !self.logical_firing.as_ref().is_some_and(|runtime| runtime.owns_observer(observer)) {
            return Err(self.unsupported(observer, "unowned logical observer tick"));
        }
        let powered = self.node_state(observer).powered;
        if !powered {
            self.schedule(observer, 1, TickPriority::Normal);
        }
        self.set_node(observer, !powered, 15 * u8::from(!powered), None, false)?;
        let actor = self.logical_firing.as_ref().expect("checked logical firing runtime").actor();
        let activation_count = self.logical_firing.as_ref().unwrap().activation_count(actor);
        for index in 0..activation_count {
            let Some(occurrence) = self.logical_firing.as_ref().unwrap().activation_at(actor, index) else {
                continue;
            };
            if self.program.routes()[occurrence.route.index()].source != observer {
                continue;
            }
            let delivery = self.program.routes()[occurrence.route.index()].deliveries[occurrence.ordinal as usize];
            self.execute_logical_firing_activation(occurrence, delivery, None)?;
        }
        Ok(())
    }

    pub(super) fn apply_logical_firing_deferred(&mut self, event: DeferredPistonEvent) -> Result<(), GraphExecutionError> {
        let Some(use_) = self.logical_firing.as_ref().and_then(|runtime| runtime.effect(event.piston)).cloned() else {
            return Ok(());
        };
        let current = self.state.pistons[event.piston.index()].revision == event.revision;
        self.counts.deferred_publications += 1;
        let spec = self.program.pistons()[event.piston.index()];
        if event.extending {
            if let Some(resource_event) = event.resource {
                let resource = self.state.resources[resource_event.resource.index()];
                let valid = resource.revision == resource_event.revision
                    && resource.at == resource_event.contact
                    && !resource.enabled
                    && self.resource_contact_available(resource_event.resource, resource_event.contact, None);
                if valid {
                    self.set_resource_contact(resource_event.resource, resource_event.contact, true);
                    if let Some(route) = spec.routes.resource_restored_far {
                        self.deliver_logical_firing_outputs(route)?;
                    }
                }
            }
            if current {
                let piston = &mut self.state.pistons[event.piston.index()];
                piston.availability = PistonAvailability::Ready;
                piston.head_present = true;
                self.deliver_logical_firing_outputs(spec.routes.head_restored)?;
            }
            let observer = self.logical_firing.as_ref().unwrap().observer();
            if current
                && self.logical_firing.as_ref().is_some_and(|runtime| runtime.reset_pending())
                && matches!(self.state.components.get(observer.index()), Some(ComponentState::Electrical(state)) if !state.powered && !state.pending_tick)
            {
                self.logical_firing.as_mut().unwrap().set_fired(event.piston, false);
                self.logical_firing.as_mut().unwrap().set_reset_pending(false);
            }
        } else {
            if current {
                let piston = &mut self.state.pistons[event.piston.index()];
                piston.extended = false;
                piston.head_present = false;
                piston.availability = PistonAvailability::Ready;
                self.deliver_logical_firing_outputs(use_.effect.wake.base_restored)?;
                let wake = use_.reset.observer_wake().expect("observer reset wake");
                self.wake_logical_observer(wake.on_base_restored.as_ref())?;
            }
            if let Some(resource_event) = event.resource {
                let resource = self.state.resources[resource_event.resource.index()];
                if resource.revision == resource_event.revision
                    && resource.at == resource_event.contact
                    && !resource.enabled
                    && self.resource_contact_available(resource_event.resource, resource_event.contact, None)
                {
                    self.set_resource_contact(resource_event.resource, resource_event.contact, true);
                    if let Some(wake) = use_.effect.wake.resource {
                        self.deliver_logical_firing_outputs(wake.route)?;
                    }
                }
            }
        }
        Ok(())
    }
}
