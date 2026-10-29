use super::*;

/// Shared physical graph inventory. Resource locations live only in `signals.state()`;
/// the consumed vector is indexed by ResourceId and is cleared only by the caller.
#[derive(Clone)]
pub(crate) struct CompiledPistonActors {
    pub(super) signals: crate::redpiler::physical_signal_network::PhysicalSignalNetwork,
    pub(super) actors: Vec<Option<CompiledPistonActor>>,
    pub(super) consumed: Vec<bool>,
    pub(super) successful_moves: Vec<u64>,
    pub(super) successful_actions: Vec<u64>,
    pub(super) deferred: Vec<(usize, DeferredPistonMove, bool)>,
    pub(super) pending: Vec<crate::redpiler::physical_signal_network::SignalEvent>,
}

#[derive(Clone)]
pub(super) struct CompiledPistonActor {
    pub(super) id: crate::redpiler::program_graph::PistonId,
    pub(super) resource: crate::redpiler::program_graph::ResourceId,
    pub(super) position: mchprs_blocks::BlockPos,
    pub(super) near: crate::redpiler::program_graph::ContactId,
    pub(super) far: crate::redpiler::program_graph::ContactId,
    pub(super) routes: Vec<usize>,
    pub(super) movement: crate::redpiler::program_graph::PistonRoutes,
}

impl CompiledPistonActors {
    pub(crate) fn compile(
        program: &crate::redpiler::program_graph::CircuitProgram,
        state: &crate::redpiler::program_graph::CircuitState,
        entry: &crate::redpiler::physical_signal_network::EntryWork,
    ) -> Result<Self, String> {
        use crate::redpiler::program_graph::{CallbackReceiver, PistonLane};
        let mut actors = vec![None; program.pistons().len()];
        for (index, spec) in program.pistons().iter().enumerate() {
            if !spec.sticky || !matches!(spec.lane, PistonLane::SingleResource { .. }) {
                return Err("unsupported piston actor geometry".into());
            }
            let id = crate::redpiler::program_graph::PistonId::new(index as u32);
            let resource = program.resource_for_piston(id).ok_or("piston has no resource identity")?;
            let resource_state = state.resources.get(resource.index()).ok_or("piston resource state is missing")?;
            if !program.resources()[resource.index()].contacts.contains(&spec.near)
                || !program.resources()[resource.index()].contacts.contains(&spec.far)
                || (resource_state.enabled && resource_state.at != spec.near && resource_state.at != spec.far)
            {
                return Err("resource cannot occupy the piston lane".into());
            }
            let routes = program
                .routes()
                .iter()
                .enumerate()
                .filter_map(|(route, item)| {
                    item.deliveries
                        .iter()
                        .any(|delivery| {
                            matches!(delivery.receiver,
                                Some(CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor)) if actor == id
                            )
                        })
                        .then_some(route)
                })
                .collect::<Vec<_>>();
            if routes.is_empty() {
                return Err("piston has no callback routes".into());
            }
            actors[index] = Some(CompiledPistonActor {
                id,
                resource,
                position: spec.position,
                near: spec.near,
                far: spec.far,
                routes,
                movement: spec.routes,
            });
        }
        Ok(Self {
            signals: crate::redpiler::physical_signal_network::PhysicalSignalNetwork::compile(program, state, entry)?,
            actors,
            consumed: vec![false; program.resources().len()],
            successful_moves: vec![0; program.pistons().len()],
            successful_actions: vec![0; program.pistons().len()],
            deferred: Vec::new(),
            pending: Vec::new(),
        })
    }

    /// Caller-defined frame boundary for computation resources. Physical clock/reset
    /// actors should remain on the dedicated physical path.
    pub(crate) fn begin_computation_cycle(&mut self) {
        self.consumed.fill(false);
    }

    pub(crate) fn set_lever(&mut self, position: mchprs_blocks::BlockPos, powered: bool) -> Result<(), &'static str> {
        let before = self.clone();
        match self.signals.set_lever(position, powered) {
            Ok(events) => {
                self.pending.extend(events);
                Ok(())
            }
            Err(error) => {
                *self = before;
                Err(error)
            }
        }
    }

    #[cfg(test)]
    pub(super) fn resource_for_actor(&self, piston: crate::redpiler::program_graph::PistonId) -> Option<crate::redpiler::program_graph::ResourceId> {
        self.actors.get(piston.index())?.as_ref().map(|actor| actor.resource)
    }

    #[cfg(test)]
    pub(super) fn resource_state(&self, resource: crate::redpiler::program_graph::ResourceId) -> crate::redpiler::program_graph::ResourceState {
        self.signals.state().resources[resource.index()]
    }

    pub(crate) fn drain(&mut self, requests: Vec<crate::redpiler::physical_signal_network::SignalEvent>) -> Result<usize, &'static str> {
        let before = self.clone();
        match self.drain_inner(requests) {
            Ok(moved) => Ok(moved),
            Err(error) => {
                *self = before;
                Err(error)
            }
        }
    }

    fn drain_inner(&mut self, requests: Vec<crate::redpiler::physical_signal_network::SignalEvent>) -> Result<usize, &'static str> {
        let mut requests: VecDeque<_> = requests.into();
        let mut moved = 0;
        let mut budget = 0;
        while let Some(request) = requests.pop_front() {
            budget += 1;
            if budget > 4096 {
                return Err("piston callback queue exceeded its event bound");
            }
            let Some(actor) = self.actors.get(request.piston.index()).and_then(Option::as_ref) else {
                continue;
            };
            if !belongs_to_piston(&request, actor.id, &actor.routes) || !request.piston_guards.iter().any(|(_, passed)| *passed) {
                continue;
            }
            let power = self.signals.piston_input(actor.position)?;
            let state = self.signals.state().pistons[actor.id.index()];
            let resource = self.signals.state().resources[actor.resource.index()];
            let extending = request.action == mchprs_world::PistonAction::Extend;
            if request.action == mchprs_world::PistonAction::RetractWithoutPull {
                continue;
            }
            let source_contact = if extending { actor.near } else { actor.far };
            let destination = if extending { actor.far } else { actor.near };
            let moving_resource = resource.at == source_contact && resource.enabled;
            if (power > 0) != extending
                || (request.piston_input > 0) != (power > 0)
                || (extending && resource.at == source_contact && !resource.enabled)
                || (!extending && resource.at == source_contact && !resource.enabled)
                || (extending && (state.extended || state.availability != crate::redpiler::program_graph::PistonAvailability::Ready))
                || (!extending && (!state.extended || state.availability.base_pending()))
            {
                continue;
            }
            if extending && !moving_resource && !self.signals.resource_destination_available(actor.resource, actor.near, None)? {
                continue;
            }
            if moving_resource
                && (self.consumed[actor.resource.index()]
                    || !self
                        .signals
                        .resource_destination_available(actor.resource, destination, (!extending).then_some(actor.id))?)
            {
                continue;
            }
            let mut next = state;
            next.head_present = true;
            next.availability = if extending {
                crate::redpiler::program_graph::PistonAvailability::HeadPending
            } else {
                crate::redpiler::program_graph::PistonAvailability::BasePendingHeadPresent
            };
            next.revision = next.revision.wrapping_add(1);
            let revision = next.revision;
            self.signals.set_piston_state(actor.id.index(), next)?;
            if extending && moving_resource {
                self.signals.set_resource_contact(actor.resource.index(), destination, false)?;
            }
            let mut callbacks = Vec::new();
            if !extending {
                callbacks.extend(self.signals.deliver_callback_routes(&[actor.movement.base_changed.index()])?);
                next.availability = crate::redpiler::program_graph::PistonAvailability::BasePending;
                next.head_present = false;
                self.signals.set_piston_state(actor.id.index(), next)?;
                if moving_resource {
                    self.signals.set_resource_contact(actor.resource.index(), destination, false)?;
                }
            }
            self.deferred.push((
                actor.id.index(),
                DeferredPistonMove {
                    due_tick: self.signals.current_tick() + 2,
                    revision,
                    extending,
                    resource_revision: self.signals.state().resources[actor.resource.index()].revision,
                    resource_contact: destination,
                },
                moving_resource,
            ));
            if extending {
                if moving_resource && let Some(route) = actor.movement.far_shape_changed {
                    callbacks.extend(self.signals.deliver_callback_routes(&[route.index()])?);
                }
                if moving_resource {
                    callbacks.extend(self.signals.deliver_callback_routes(&[actor.movement.near_changed.index()])?);
                }
                callbacks.extend(self.signals.deliver_callback_routes(&[actor.movement.near_changed.index()])?);
                next.extended = true;
                self.signals.set_piston_state(actor.id.index(), next)?;
                callbacks.extend(self.signals.deliver_callback_routes(&[actor.movement.base_changed.index()])?);
            } else {
                if moving_resource && let Some(route) = actor.movement.far_changed {
                    callbacks.extend(self.signals.deliver_callback_routes(&[route.index()])?);
                }
                callbacks.extend(self.signals.deliver_callback_routes(&[actor.movement.near_changed.index()])?);
            }
            requests.extend(callbacks);
            self.successful_actions[actor.id.index()] += 1;
            if moving_resource {
                self.consumed[actor.resource.index()] = true;
                self.successful_moves[actor.id.index()] += 1;
                moved += 1;
            }
        }
        Ok(moved)
    }

    pub(crate) fn advance_tick(&mut self) -> Result<usize, &'static str> {
        let before = self.clone();
        let result = self.advance_tick_inner();
        if result.is_err() {
            *self = before;
        }
        result
    }

    fn advance_tick_inner(&mut self) -> Result<usize, &'static str> {
        let mut events = std::mem::take(&mut self.pending);
        events.extend(self.signals.advance_tick()?);
        let moved = self.drain_inner(events)?;
        let now = self.signals.current_tick();
        let mut index = 0;
        while index < self.deferred.len() {
            if self.deferred[index].1.due_tick > now {
                index += 1;
                continue;
            }
            let (id, work, moving_resource) = self.deferred.remove(index);
            let actor = self.actors[id].as_ref().ok_or("deferred piston missing")?;
            let resource = self.signals.state().resources[actor.resource.index()];
            let mut piston = self.signals.state().pistons[id];
            let expected = if work.extending {
                crate::redpiler::program_graph::PistonAvailability::HeadPending
            } else {
                crate::redpiler::program_graph::PistonAvailability::BasePending
            };
            if piston.revision != work.revision
                || piston.availability != expected
                || (moving_resource && (resource.revision != work.resource_revision || resource.enabled || resource.at != work.resource_contact))
            {
                continue;
            }
            let routes = actor.movement;
            if !work.extending {
                piston.extended = false;
                piston.head_present = false;
                piston.availability = crate::redpiler::program_graph::PistonAvailability::Ready;
                self.signals.set_piston_state(id, piston)?;
                self.pending
                    .extend(self.signals.deliver_callback_routes(&[routes.base_restored.index()])?);
            }
            if moving_resource {
                self.signals.set_resource_contact(actor.resource.index(), work.resource_contact, true)?;
            }
            let route = if work.extending {
                routes.resource_restored_far
            } else {
                routes.resource_restored_near
            };
            if moving_resource && let Some(route) = route {
                self.pending.extend(self.signals.deliver_callback_routes(&[route.index()])?);
            }
            if work.extending {
                piston.head_present = true;
                piston.availability = crate::redpiler::program_graph::PistonAvailability::Ready;
                self.signals.set_piston_state(id, piston)?;
                self.pending
                    .extend(self.signals.deliver_callback_routes(&[routes.head_restored.index()])?);
            }
        }
        Ok(moved)
    }
}
