use super::*;

/// Source-derived piston actor for the experimental signal network.
/// The network owns component schedulers; this actor only owns piston movement publications.
#[derive(Clone)]
pub(crate) struct AutonomousPiston {
    pub(super) piston: PhysicalPiston,
    pub(super) signals: crate::redpiler::physical_signal_network::PhysicalSignalNetwork,
    pub(super) actor: mchprs_blocks::BlockPos,
    pub(super) piston_index: usize,
    pub(super) resource: usize,
    pub(super) near: crate::redpiler::program_graph::ContactId,
    pub(super) far: crate::redpiler::program_graph::ContactId,
    pub(super) far_position: mchprs_blocks::BlockPos,
    pub(super) movement_routes: crate::redpiler::program_graph::PistonRoutes,
    pub(super) deferred: Vec<DeferredPistonMove>,
    // Restoration callbacks occur after the piston phase and wait for the next tick.
    pub(super) pending: Vec<crate::redpiler::physical_signal_network::SignalEvent>,
    pub(super) successful_moves: u64,
}

#[derive(Clone)]
pub(super) struct DeferredPistonMove {
    pub(super) due_tick: u64,
    pub(super) revision: u64,
    pub(super) extending: bool,
    pub(super) resource_revision: u64,
    pub(super) resource_contact: crate::redpiler::program_graph::ContactId,
}

pub(super) fn belongs_to_piston(
    event: &crate::redpiler::physical_signal_network::SignalEvent,
    piston: crate::redpiler::program_graph::PistonId,
    routes: &[usize],
) -> bool {
    event.piston == piston && routes.contains(&event.route)
}

impl AutonomousPiston {
    pub(crate) fn compile(
        program: &crate::redpiler::program_graph::CircuitProgram,
        state: &crate::redpiler::program_graph::CircuitState,
        entry: &crate::redpiler::physical_signal_network::EntryWork,
        actor: mchprs_blocks::BlockPos,
    ) -> Result<Self, String> {
        let piston = PhysicalPiston::compile(program, state, actor)?;
        let piston_index = program.pistons().iter().position(|p| p.position == actor).ok_or("piston missing")?;
        let spec = program.pistons()[piston_index];
        let resource = program
            .resource_for_piston(crate::redpiler::program_graph::PistonId::new(piston_index as u32))
            .ok_or("piston resource is unavailable")?;
        if program.pistons().iter().enumerate().any(|(other, _)| {
            other != piston_index && program.resource_for_piston(crate::redpiler::program_graph::PistonId::new(other as u32)) == Some(resource)
        }) {
            return Err("piston resource has multiple owners".into());
        }
        let signals = crate::redpiler::physical_signal_network::PhysicalSignalNetwork::compile(program, state, entry)?;
        Ok(Self {
            piston,
            signals,
            actor,
            piston_index,
            resource: resource.index(),
            near: spec.near,
            far: spec.far,
            far_position: program.contacts()[spec.far.index()].position,
            movement_routes: spec.routes,
            deferred: Vec::new(),
            pending: Vec::new(),
            successful_moves: 0,
        })
    }

    pub(crate) fn set_lever(&mut self, position: mchprs_blocks::BlockPos, powered: bool) -> Result<usize, &'static str> {
        let before = self.clone();
        match self.signals.set_lever(position, powered) {
            Ok(events) => {
                self.pending.extend(events);
                Ok(0)
            }
            Err(error) => {
                *self = before;
                Err(error)
            }
        }
    }

    /// One game tick for the source-derived component scheduler and piston publications.
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
        let mut moved = self.dispatch(events)?;
        let now = self.signals.current_tick();
        let mut index = 0;
        while index < self.deferred.len() {
            if self.deferred[index].due_tick > now {
                index += 1;
                continue;
            }
            let publication = self.deferred.remove(index);
            let state = self.signals.state().pistons[self.piston_index];
            let resource = self.signals.state().resources[self.resource];
            if resource.revision == publication.resource_revision && !resource.enabled && resource.at == publication.resource_contact {
                if publication.extending {
                    moved += self.restore_resource(publication.resource_contact, self.movement_routes.resource_restored_far)?;
                    if state.revision == publication.revision {
                        let mut ready = state;
                        ready.availability = crate::redpiler::program_graph::PistonAvailability::Ready;
                        ready.head_present = true;
                        self.signals.set_piston_state(self.piston_index, ready)?;
                        moved += self.deliver(self.movement_routes.head_restored.index())?;
                    }
                } else {
                    if state.revision == publication.revision {
                        let mut ready = state;
                        ready.extended = false;
                        ready.head_present = false;
                        ready.availability = crate::redpiler::program_graph::PistonAvailability::Ready;
                        self.signals.set_piston_state(self.piston_index, ready)?;
                        moved += self.deliver(self.movement_routes.base_restored.index())?;
                    }
                    moved += self.restore_resource(publication.resource_contact, self.movement_routes.resource_restored_near)?;
                }
            }
        }
        Ok(moved)
    }

    fn restore_resource(
        &mut self,
        contact: crate::redpiler::program_graph::ContactId,
        route: Option<crate::redpiler::program_graph::RouteId>,
    ) -> Result<usize, &'static str> {
        let events = self.signals.set_resource_contact(self.resource, contact, true)?;
        self.pending.extend(events);
        if let Some(route) = route {
            self.deliver(route.index())?;
        }
        Ok(0)
    }

    fn deliver(&mut self, route: usize) -> Result<usize, &'static str> {
        let events = self.signals.deliver_callback_routes(&[route])?;
        self.pending.extend(events);
        Ok(0)
    }

    fn dispatch(&mut self, initial: Vec<crate::redpiler::physical_signal_network::SignalEvent>) -> Result<usize, &'static str> {
        use std::collections::VecDeque;
        let piston_routes = self.piston.routes.clone();
        let mut pending: VecDeque<_> = initial.into();
        let mut moved_count = 0;
        let mut budget = 0;
        while let Some(event) = pending.pop_front() {
            budget += 1;
            if budget > 4096 {
                return Err("piston event dispatch exceeded its bound");
            }
            if !belongs_to_piston(
                &event,
                crate::redpiler::program_graph::PistonId::new(self.piston_index as u32),
                &piston_routes,
            ) || !event.piston_guards.iter().any(|(_, passed)| *passed)
            {
                continue;
            }
            let power = self.signals.piston_input(self.actor)?;
            let piston_state = self.signals.state().pistons[self.piston_index];
            let resource_state = self.signals.state().resources[self.resource];
            let eligible = if power > 0 {
                !piston_state.extended
                    && piston_state.availability == crate::redpiler::program_graph::PistonAvailability::Ready
                    && resource_state.enabled
                    && resource_state.at == self.near
            } else {
                piston_state.extended && !piston_state.availability.base_pending() && resource_state.enabled && resource_state.at == self.far
            };
            let live_action = if power > 0 {
                mchprs_world::PistonAction::Extend
            } else {
                mchprs_world::PistonAction::Retract
            };
            if !eligible || event.action != live_action || (event.piston_input > 0) != (power > 0) {
                continue;
            }
            if !self.piston.fire(event.route, power)? {
                continue;
            }
            moved_count += 1;
            self.successful_moves += 1;
            let extending = self.piston.position() == self.far_position;
            let contact = if extending { self.far } else { self.near };
            let enabled = self.signals.state().resources[self.resource].enabled;
            let events = self.signals.set_resource_contact(self.resource, contact, false)?;
            pending.extend(events);

            let mut piston_state = self.signals.state().pistons[self.piston_index];
            piston_state.revision = piston_state.revision.wrapping_add(1);
            let revision = piston_state.revision;
            if extending {
                piston_state.extended = true;
                piston_state.head_present = true;
                piston_state.availability = crate::redpiler::program_graph::PistonAvailability::HeadPending;
            } else {
                piston_state.availability = crate::redpiler::program_graph::PistonAvailability::BasePendingHeadPresent;
            }
            self.signals.set_piston_state(self.piston_index, piston_state)?;

            let routes = self.movement_routes;
            if extending {
                if enabled {
                    if let Some(route) = routes.far_shape_changed {
                        pending.extend(self.signals.deliver_callback_routes(&[route.index()])?);
                    }
                }
                pending.extend(self.signals.deliver_callback_routes(&[routes.near_changed.index()])?);
                if enabled {
                    pending.extend(self.signals.deliver_callback_routes(&[routes.near_changed.index()])?);
                }
                self.signals.set_piston_state(self.piston_index, piston_state)?;
                pending.extend(self.signals.deliver_callback_routes(&[routes.base_changed.index()])?);
            } else {
                pending.extend(self.signals.deliver_callback_routes(&[routes.base_changed.index()])?);
                piston_state.availability = crate::redpiler::program_graph::PistonAvailability::BasePending;
                piston_state.head_present = false;
                self.signals.set_piston_state(self.piston_index, piston_state)?;
                if enabled {
                    if let Some(route) = routes.far_changed {
                        pending.extend(self.signals.deliver_callback_routes(&[route.index()])?);
                    }
                }
                pending.extend(self.signals.deliver_callback_routes(&[routes.near_changed.index()])?);
            }
            if enabled {
                self.deferred.push(DeferredPistonMove {
                    due_tick: self.signals.current_tick() + 2,
                    revision,
                    extending,
                    resource_revision: self.signals.state().resources[self.resource].revision,
                    resource_contact: contact,
                });
            }
        }
        Ok(moved_count)
    }

    pub(crate) fn position(&self) -> mchprs_blocks::BlockPos {
        self.piston.position()
    }

    pub(crate) fn state(&self) -> &crate::redpiler::program_graph::CircuitState {
        self.signals.state()
    }

    pub(crate) fn successful_moves(&self) -> u64 {
        self.successful_moves
    }
}
