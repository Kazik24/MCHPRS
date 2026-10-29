//! Experimental, isolated event-wave runtime. Not installed in the Graph compiler.
use std::collections::VecDeque;

#[derive(Clone, Copy)]
pub(crate) struct Link {
    pub source: usize,
    pub target: usize,
    pub attenuation: u8,
    pub contact: Option<(usize, usize)>,
}

#[derive(Clone, Copy)]
pub(crate) struct Action {
    pub input: usize,
    pub threshold: u8,
    pub when_powered: bool,
    pub resource: usize,
    pub from: usize,
    pub to: usize,
}

// shortcut: links read external sources only, extend after graph-derived event routing exists.
#[derive(Clone)]
pub(crate) struct EventNetlist {
    sources: Vec<u8>,
    binary_sources: bool,
    contacts: Vec<usize>,
    links: Vec<Link>,
    actions: Vec<Action>,
    incoming: Vec<Vec<usize>>,
    contact_watchers: Vec<Vec<usize>>,
}

/// One observed clock request; pending Graph work remains with the caller.
pub(crate) struct ObservedClockEdge {
    pub source: mchprs_blocks::BlockPos,
    pub route: usize,
    pub actor: mchprs_blocks::BlockPos,
    pub from: mchprs_blocks::BlockPos,
    pub to: mchprs_blocks::BlockPos,
    pub window_end_pending: super::backend::graph::piston_probe::PendingWork,
    net: EventNetlist,
}

/// A single piston compiled from physical lane geometry and callback routes.
/// Power and callback occurrences are supplied by the owning circuit executor.
#[derive(Clone)]
pub(crate) struct PhysicalPiston {
    routes: Vec<usize>,
    positions: [mchprs_blocks::BlockPos; 2],
    net: EventNetlist,
}

/// Explicitly driven piston and delayed output; the caller owns source routing.
pub(crate) struct PhysicalScope {
    pub(crate) piston: PhysicalPiston,
    pub(crate) output: super::event_repeater::PhysicalRepeater,
}

impl PhysicalScope {
    pub(crate) fn notify(&mut self, route: usize, piston_power: u8, output_input: u8, locked: bool) -> Result<bool, &'static str> {
        if output_input > 15 {
            return Err("invalid output strength");
        }
        let moved = self.piston.fire(route, piston_power)?;
        self.output.notify(output_input, locked)?;
        Ok(moved)
    }

    pub(crate) fn tick(&mut self) -> bool {
        self.output.tick()
    }
}

impl PhysicalPiston {
    pub(crate) fn compile(
        program: &super::program_graph::CircuitProgram,
        state: &super::program_graph::CircuitState,
        actor: mchprs_blocks::BlockPos,
    ) -> Result<Self, &'static str> {
        use super::program_graph::{CallbackReceiver, PistonLane};
        let (piston_index, piston) = program
            .pistons()
            .iter()
            .enumerate()
            .find(|(_, piston)| piston.position == actor)
            .ok_or("piston missing")?;
        if !piston.sticky || !matches!(piston.lane, PistonLane::SingleResource { .. }) {
            return Err("unsupported piston lane");
        }
        let mut resources =
            program.resources().iter().enumerate().filter(|(index, spec)| {
                state.resources[*index].enabled && spec.contacts.contains(&piston.near) && spec.contacts.contains(&piston.far)
            });
        let resource = resources.next().ok_or("piston lane resource missing")?;
        if resources.next().is_some() {
            return Err("piston lane has multiple resources");
        }
        let at = state.resources[resource.0].at;
        let initial = if at == piston.near {
            0
        } else if at == piston.far {
            1
        } else {
            return Err("resource outside piston lane");
        };
        if state.pistons[piston_index].extended != (initial == 1) {
            return Err("piston and resource entry states disagree");
        }
        let routes = program
            .routes()
            .iter()
            .enumerate()
            .filter_map(|(index, route)| {
                route
                    .deliveries
                    .iter()
                    .any(|delivery| match delivery.receiver {
                        Some(CallbackReceiver::PistonBase(id) | CallbackReceiver::PistonHead(id)) => id.index() == piston_index,
                        _ => false,
                    })
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        if routes.is_empty() {
            return Err("piston has no callback routes");
        }
        let links = vec![Link {
            source: 0,
            target: 0,
            attenuation: 0,
            contact: None,
        }];
        let actions = vec![
            Action {
                input: 0,
                threshold: 0,
                when_powered: true,
                resource: 0,
                from: 0,
                to: 1,
            },
            Action {
                input: 0,
                threshold: 0,
                when_powered: false,
                resource: 0,
                from: 1,
                to: 0,
            },
        ];
        Ok(Self {
            routes,
            positions: [
                program.contacts()[piston.near.index()].position,
                program.contacts()[piston.far.index()].position,
            ],
            net: EventNetlist::new(vec![0], vec![initial], 2, 1, links, actions, &[])?,
        })
    }

    /// Route membership is necessary but callers must establish the delivery's live guard.
    pub(crate) fn fire(&mut self, route: usize, power: u8) -> Result<bool, &'static str> {
        if !self.routes.contains(&route) {
            return Err("unrepresented piston route");
        }
        let before = self.net.contacts[0];
        let action = if power > 0 { 0 } else { 1 };
        self.net.frame(&[power], &[action], &[])?;
        Ok(self.net.contacts[0] != before)
    }

    pub(crate) fn position(&self) -> mchprs_blocks::BlockPos {
        self.positions[self.net.contacts[0]]
    }
}

/// Source-derived piston actor for the experimental signal network.
/// The network owns component schedulers; this actor only owns piston movement publications.
#[derive(Clone)]
pub(crate) struct AutonomousPiston {
    piston: PhysicalPiston,
    signals: super::physical_signal_network::PhysicalSignalNetwork,
    actor: mchprs_blocks::BlockPos,
    piston_index: usize,
    resource: usize,
    near: super::program_graph::ContactId,
    far: super::program_graph::ContactId,
    far_position: mchprs_blocks::BlockPos,
    movement_routes: super::program_graph::PistonRoutes,
    deferred: Vec<DeferredPistonMove>,
    successful_moves: u64,
}

#[derive(Clone)]
struct DeferredPistonMove {
    due_tick: u64,
    revision: u64,
    extending: bool,
    resource_revision: u64,
    resource_contact: super::program_graph::ContactId,
}

impl AutonomousPiston {
    pub(crate) fn compile(
        program: &super::program_graph::CircuitProgram,
        state: &super::program_graph::CircuitState,
        actor: mchprs_blocks::BlockPos,
    ) -> Result<Self, &'static str> {
        let piston = PhysicalPiston::compile(program, state, actor)?;
        let piston_index = program.pistons().iter().position(|p| p.position == actor).ok_or("piston missing")?;
        for &route in &piston.routes {
            if program.routes()[route].deliveries.iter().any(|delivery| {
                matches!(delivery.receiver,
                    Some(super::program_graph::CallbackReceiver::PistonBase(other)
                        | super::program_graph::CallbackReceiver::PistonHead(other))
                        if other.index() != piston_index)
            }) {
                return Err("shared piston callback route is unsupported");
            }
        }
        let spec = program.pistons()[piston_index];
        let resource = program
            .resource_for_piston(super::program_graph::PistonId::new(piston_index as u32))
            .ok_or("piston resource is unavailable")?;
        if program.pistons().iter().enumerate().any(|(other, _)| {
            other != piston_index && program.resource_for_piston(super::program_graph::PistonId::new(other as u32)) == Some(resource)
        }) {
            return Err("piston resource has multiple owners");
        }
        let signals = super::physical_signal_network::PhysicalSignalNetwork::compile(program, state)?;
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
            successful_moves: 0,
        })
    }

    pub(crate) fn set_lever(&mut self, position: mchprs_blocks::BlockPos, powered: bool) -> Result<usize, &'static str> {
        let before = self.clone();
        match self.signals.set_lever(position, powered).and_then(|events| self.dispatch(events)) {
            Ok(moved) => Ok(moved),
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
        let events = self.signals.advance_tick()?;
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
                        ready.availability = super::program_graph::PistonAvailability::Ready;
                        ready.head_present = true;
                        self.signals.set_piston_state(self.piston_index, ready)?;
                        moved += self.deliver(self.movement_routes.head_restored.index())?;
                    }
                } else {
                    if state.revision == publication.revision {
                        let mut ready = state;
                        ready.extended = false;
                        ready.head_present = false;
                        ready.availability = super::program_graph::PistonAvailability::Ready;
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
        contact: super::program_graph::ContactId,
        route: Option<super::program_graph::RouteId>,
    ) -> Result<usize, &'static str> {
        let events = self.signals.set_resource_contact(self.resource, contact, true)?;
        let mut moved = self.dispatch(events)?;
        if let Some(route) = route {
            moved += self.deliver(route.index())?;
        }
        Ok(moved)
    }

    fn deliver(&mut self, route: usize) -> Result<usize, &'static str> {
        let events = self.signals.deliver_callback_routes(&[route])?;
        self.dispatch(events)
    }

    fn dispatch(&mut self, initial: Vec<super::physical_signal_network::SignalEvent>) -> Result<usize, &'static str> {
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
            if !piston_routes.contains(&event.route) || !event.piston_guards.iter().any(|(_, passed)| *passed) {
                continue;
            }
            let power = self.signals.piston_input(self.actor)?;
            let piston_state = self.signals.state().pistons[self.piston_index];
            let resource_state = self.signals.state().resources[self.resource];
            let eligible = if power > 0 {
                !piston_state.extended
                    && piston_state.availability == super::program_graph::PistonAvailability::Ready
                    && resource_state.enabled
                    && resource_state.at == self.near
            } else {
                piston_state.extended && !piston_state.availability.base_pending() && resource_state.enabled && resource_state.at == self.far
            };
            if !eligible || (event.piston_input > 0) != (power > 0) {
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
                piston_state.availability = super::program_graph::PistonAvailability::HeadPending;
            } else {
                piston_state.availability = super::program_graph::PistonAvailability::BasePendingHeadPresent;
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
                piston_state.availability = super::program_graph::PistonAvailability::BasePending;
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

    pub(crate) fn state(&self) -> &super::program_graph::CircuitState {
        self.signals.state()
    }

    pub(crate) fn successful_moves(&self) -> u64 {
        self.successful_moves
    }
}

impl ObservedClockEdge {
    /// Accept only the observed callback occurrence; live power is revalidated.
    pub(crate) fn fire(&mut self, source: mchprs_blocks::BlockPos, route: usize, power: u8) -> Result<bool, &'static str> {
        if source != self.source || route != self.route {
            return Err("unrepresented clock request");
        }
        let before = self.net.contacts[0];
        self.net.frame(&[power], &[0], &[])?;
        Ok(self.net.contacts[0] != before)
    }
}

impl EventNetlist {
    /// Build one falling-edge transfer from a bounded physical observation.
    /// The caller must handle pending Graph work; the recorded pending state is at the window end, not the edge.
    pub(crate) fn from_clock_edge(trial: &super::backend::graph::piston_probe::PistonTrial) -> Result<ObservedClockEdge, &'static str> {
        let index = *trial.clock_retractions.first().ok_or("no observed falling clock event")?;
        let attempt = trial.attempts.get(index).ok_or("invalid clock event index")?;
        let before = attempt.before.as_ref().ok_or("clock event has no resource")?;
        let after = attempt.after.as_ref().ok_or("clock event has no resource result")?;
        if trial.clock != Some(attempt.actor)
            || trial.input_events.len() != 1
            || !attempt.accepted
            || attempt.powered
            || attempt.action != mchprs_world::PistonAction::Retract
            || !before.enabled
            || before.id != after.id
            || before.position == after.position
            || trial.attempts[..index].iter().any(|earlier| earlier.accepted)
        {
            return Err("unsupported clock edge");
        }
        let net = Self::new(
            vec![0],
            vec![0],
            2,
            1,
            vec![Link {
                source: 0,
                target: 0,
                attenuation: 0,
                contact: None,
            }],
            vec![Action {
                input: 0,
                threshold: 0,
                when_powered: false,
                resource: 0,
                from: 0,
                to: 1,
            }],
            &[],
        )?;
        Ok(ObservedClockEdge {
            source: attempt.source,
            route: attempt.route,
            actor: attempt.actor,
            from: before.position,
            to: after.position,
            window_end_pending: trial.pending.clone(),
            net,
        })
    }

    /// Replay one observed, source-qualified transfer; this does not generalize the trial to other states.
    pub(crate) fn from_piston_trial(trial: &super::backend::graph::piston_probe::PistonTrial) -> Result<Self, &'static str> {
        if !trial.settled
            || trial.input_events.len() != 1
            || trial.initial_resources.len() != 1
            || trial.final_resources.len() != 1
            || trial.attempts.len() != 1
        {
            return Err("piston trial is not a single-resource single-request transition");
        }
        let attempt = &trial.attempts[0];
        let before = attempt.before.as_ref().ok_or("piston request has no resource")?;
        let after = attempt.after.as_ref().ok_or("piston request has no resource result")?;
        let initial = &trial.initial_resources[0];
        let final_state = &trial.final_resources[0];
        if !attempt.accepted
            || !before.enabled
            || !final_state.enabled
            || before != initial
            || before.id != after.id
            || after.id != final_state.id
            || before.position == after.position
            || after.position != final_state.position
            || !matches!(
                (attempt.action, attempt.powered),
                (mchprs_world::PistonAction::Extend, true) | (mchprs_world::PistonAction::Retract, false)
            )
        {
            return Err("piston trial is not a settled resource transfer");
        }
        Self::new(
            vec![u8::from(attempt.powered) * 15],
            vec![0],
            2,
            1,
            vec![Link {
                source: 0,
                target: 0,
                attenuation: 0,
                contact: None,
            }],
            vec![Action {
                input: 0,
                threshold: 0,
                when_powered: attempt.powered,
                resource: 0,
                from: 0,
                to: 1,
            }],
            &[],
        )
    }
    /// Admit only a physical probe whose exhaustive samples are a max-of-attenuated-sources net.
    pub(crate) fn from_probe(probe: &super::backend::graph::wire_probe::WireNetlist) -> Result<Self, &'static str> {
        let mut net = Self::from_samples(probe.sources().len(), probe.receivers().len(), probe.samples())?;
        for (index, source) in net.sources.iter_mut().enumerate() {
            *source = if probe.input_mask() & (1 << index) != 0 { 15 } else { 0 };
        }
        Ok(net)
    }

    fn from_samples(source_count: usize, node_count: usize, samples: &[Vec<u8>]) -> Result<Self, &'static str> {
        if source_count > 8
            || samples.len() != 1usize << source_count
            || samples.iter().any(|row| row.len() != node_count || row.iter().any(|&value| value > 15))
            || samples[0].iter().any(|&value| value != 0)
        {
            return Err("invalid or nonzero probe baseline");
        }
        let mut links = Vec::new();
        for source in 0..source_count {
            for (target, &value) in samples[1 << source].iter().enumerate() {
                if value != 0 {
                    links.push(Link {
                        source,
                        target,
                        attenuation: 15 - value,
                        contact: None,
                    });
                }
            }
        }
        for (mask, row) in samples.iter().enumerate() {
            for (target, &actual) in row.iter().enumerate() {
                let expected = links
                    .iter()
                    .filter(|link| link.target == target && mask & (1 << link.source) != 0)
                    .map(|link| 15 - link.attenuation)
                    .max()
                    .unwrap_or(0);
                if expected != actual {
                    return Err("probe is not a weighted max net");
                }
            }
        }
        let mut net = Self::new(vec![0; source_count], vec![], 0, node_count, links, vec![], &[])?;
        net.binary_sources = true;
        Ok(net)
    }
    pub(crate) fn new(
        sources: Vec<u8>,
        contacts: Vec<usize>,
        contact_count: usize,
        node_count: usize,
        links: Vec<Link>,
        actions: Vec<Action>,
        notifications: &[(usize, usize)],
    ) -> Result<Self, &'static str> {
        if sources.iter().any(|&strength| strength > 15) || contacts.iter().any(|&contact| contact >= contact_count) {
            return Err("invalid initial source or contact");
        }
        let mut incoming = vec![Vec::new(); node_count];
        for (index, link) in links.iter().enumerate() {
            if link.source >= sources.len()
                || link.target >= node_count
                || link.attenuation > 15
                || link
                    .contact
                    .is_some_and(|(resource, contact)| resource >= contacts.len() || contact >= contact_count)
            {
                return Err("invalid link");
            }
            incoming[link.target].push(index);
        }
        for action in &actions {
            if action.input >= node_count
                || action.threshold >= 15
                || action.resource >= contacts.len()
                || action.from >= contact_count
                || action.to >= contact_count
                || action.from == action.to
            {
                return Err("invalid action");
            }
        }
        let mut contact_watchers = vec![Vec::new(); contacts.len()];
        for &(resource, action) in notifications {
            if resource >= contacts.len() || action >= actions.len() {
                return Err("invalid notification");
            }
            contact_watchers[resource].push(action);
        }
        Ok(Self {
            sources,
            binary_sources: false,
            contacts,
            links,
            actions,
            incoming,
            contact_watchers,
        })
    }

    fn power(&self, target: usize) -> u8 {
        self.incoming[target]
            .iter()
            .map(|&index| self.links[index])
            .filter(|link| link.contact.is_none_or(|(resource, contact)| self.contacts[resource] == contact))
            .map(|link| self.sources[link.source].saturating_sub(link.attenuation))
            .max()
            .unwrap_or(0)
    }

    /// Validate inputs before mutation, drain the supplied FIFO, and sample at the frame boundary.
    pub(crate) fn frame(&mut self, sources: &[u8], requests: &[usize], outputs: &[usize]) -> Result<Vec<u8>, &'static str> {
        if sources.len() != self.sources.len()
            || sources
                .iter()
                .any(|&strength| strength > 15 || (self.binary_sources && strength != 0 && strength != 15))
            || requests.iter().any(|&index| index >= self.actions.len())
            || outputs.iter().any(|&index| index >= self.incoming.len())
        {
            return Err("invalid frame input");
        }
        self.sources.copy_from_slice(sources);
        let mut pending: VecDeque<_> = requests.iter().copied().collect();
        let mut consumed = vec![false; self.contacts.len()];
        while let Some(index) = pending.pop_front() {
            let action = self.actions[index];
            if consumed[action.resource]
                || self.contacts[action.resource] != action.from
                || (self.power(action.input) > action.threshold) != action.when_powered
            {
                continue;
            }
            self.contacts[action.resource] = action.to;
            consumed[action.resource] = true;
            // A contact change is an event even when an input's final power stays the same.
            pending.extend(self.contact_watchers[action.resource].iter().copied());
        }
        Ok(outputs.iter().map(|&target| self.power(target)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pistion_source_topology_is_available_for_cycle_compilation() {
        use crate::redpiler::backend::graph::{piston_probe::tests::test_pistion_fixture, wire_probe::physical_program};
        use crate::redpiler::program_graph::{CallbackReceiver, PowerTarget};
        let (world, bounds, _) = test_pistion_fixture();
        let (program, state, _) = physical_program(&world, bounds).unwrap();
        assert_eq!(program.pistons().len(), 1);
        let piston = &program.pistons()[0];
        let resource = state
            .resources
            .iter()
            .enumerate()
            .find(|(_, state)| state.enabled && (state.at == piston.near || state.at == piston.far))
            .expect("clock lane contains a resource");
        let routes: Vec<_> = program
            .routes()
            .iter()
            .enumerate()
            .filter(|(_, route)| {
                route
                    .deliveries
                    .iter()
                    .any(|delivery| matches!(delivery.receiver, Some(CallbackReceiver::PistonBase(_) | CallbackReceiver::PistonHead(_))))
            })
            .map(|(index, _)| index)
            .collect();
        let power: Vec<_> = program
            .power_links()
            .iter()
            .filter(|link| {
                matches!(link.to,
            PowerTarget::Input { node, .. } if node == piston.node)
            })
            .collect();
        assert!(!routes.is_empty(), "clock has source-qualified callback routes");
        assert!(!power.is_empty(), "clock has represented power dependencies");
        println!(
            "clock {:?}, lane {:?}, resource {:?}, callback routes {:?}, power links {:?}",
            piston.position, piston.lane, resource, routes, power
        );
        for link in &power {
            println!(
                "power site {:?}, guard {:?}",
                program.sites()[link.from.index()],
                program.guards()[link.guard.index()]
            );
            for child in program
                .power_links()
                .iter()
                .filter(|child| matches!(child.to, PowerTarget::Conductor { site, .. } if site == link.from))
            {
                println!(
                    "conductor input {:?}, site {:?}, guard {:?}",
                    child,
                    program.sites()[child.from.index()],
                    program.guards()[child.guard.index()]
                );
            }
        }
        let compiled = PhysicalPiston::compile(&program, &state, piston.position).unwrap();
        let (world, bounds, sources) = test_pistion_fixture();
        use crate::redpiler::components::NodeType;
        use crate::redpiler::program_graph::CircuitNodeKind;
        let output = program
            .nodes()
            .iter()
            .find_map(|node| match node.kind {
                CircuitNodeKind::Electrical(NodeType::Repeater { .. }) => node.position,
                _ => None,
            })
            .expect("fixture has a repeater");
        let repeater = super::super::event_repeater::PhysicalRepeater::compile(&program, &state, output, 0).unwrap();
        let mut scope = PhysicalScope {
            piston: compiled,
            output: repeater,
        };
        let trial =
            &crate::redpiler::backend::graph::piston_probe::probe_clock_outputs(&world, bounds, &sources, piston.position, &[output], 12).unwrap()[0];
        for attempt in trial.attempts.iter().filter(|attempt| attempt.accepted) {
            let observed = &attempt.outputs[0];
            assert!(
                scope
                    .notify(attempt.route, u8::from(attempt.powered) * 15, observed.input_strength, observed.locked)
                    .unwrap()
            );
            assert_eq!(scope.piston.position(), attempt.after.as_ref().unwrap().position);
        }
        let _ = scope.tick();
    }

    #[test]
    fn autonomous_piston_owns_its_repeater_scheduler_and_keeps_clock_running() {
        use crate::redpiler::backend::graph::{
            piston_probe::{probe_clock_outputs, tests::test_pistion_fixture},
            wire_probe::physical_program,
        };
        use crate::redpiler::components::NodeType;
        use crate::redpiler::program_graph::{CircuitNodeKind, ComponentState};

        let (world, bounds, sources) = test_pistion_fixture();
        let (program, state, _) = physical_program(&world, bounds).unwrap();
        let clock = program.pistons()[0].position;
        let outputs = program
            .nodes()
            .iter()
            .filter_map(|node| match node.kind {
                CircuitNodeKind::Electrical(NodeType::Repeater { .. }) => node.position,
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(!outputs.is_empty());
        let expected = probe_clock_outputs(&world, bounds, &sources, clock, &outputs, 12).unwrap().remove(0);
        let falling = expected.clock_retractions.len();
        assert!(falling >= 2, "fixture oracle has repeated falling edges");

        let source_node = program.position_index()[&sources[0]].index();
        let source_powered = matches!(&state.components[source_node], ComponentState::Electrical(source) if source.powered);
        let mut runtime = AutonomousPiston::compile(&program, &state, clock).unwrap();
        runtime.set_lever(sources[0], !source_powered).unwrap();
        for _ in 0..12 {
            runtime.advance_tick().unwrap();
        }

        let native_moves = expected
            .attempts
            .iter()
            .filter(|attempt| attempt.actor == clock && attempt.accepted)
            .count();
        assert_eq!(
            runtime.successful_moves() as usize,
            native_moves,
            "the actor processes each accepted move once"
        );
        assert!(
            runtime.successful_moves() >= (falling * 2) as u64,
            "the active source keeps the clock cycling"
        );
        let final_edge = expected
            .attempts
            .iter()
            .rev()
            .find(|attempt| attempt.actor == clock && attempt.accepted)
            .unwrap();
        for observation in &final_edge.outputs {
            let node = program.position_index()[&observation.position].index();
            let ComponentState::Electrical(output) = &runtime.state().components[node] else {
                panic!("readout state is unavailable")
            };
            assert_eq!(output.powered, observation.powered, "clock readout at {}", observation.position);
        }
        assert!(
            runtime.state().resources[runtime.resource].enabled,
            "resource is restored at the final tick boundary"
        );
    }

    #[test]
    fn observed_physical_piston_request_moves_resource_in_event_runtime() {
        use crate::redpiler::backend::graph::piston_probe::{
            probe,
            tests::{ACTOR, BOUNDS, FAR, LEVER, NEAR, fixture},
        };
        let world = fixture();
        let trials = probe(&world, BOUNDS, &[LEVER], 4).unwrap();
        let trial = &trials[0];
        let mut runtime = EventNetlist::from_piston_trial(trial).unwrap();
        assert_eq!(trial.attempts[0].actor, ACTOR);
        assert_eq!(trial.initial_resources[0].position, NEAR);
        assert_eq!(trial.final_resources[0].position, FAR);
        let power = u8::from(trial.attempts[0].powered) * 15;
        runtime.frame(&[power], &[], &[]).unwrap();
        assert_eq!(runtime.contacts, [0], "no represented request means no movement");
        runtime.frame(&[power], &[0], &[]).unwrap();
        assert_eq!(
            runtime.contacts,
            [1],
            "observed request moves the same resource to the observed destination"
        );
    }
    #[test]
    fn test_pistion_clock_window_is_not_a_settled_single_transfer() {
        use crate::redpiler::backend::graph::piston_probe::{probe_clock_window, tests::test_pistion_fixture};
        use crate::world::World;
        use mchprs_blocks::{BlockPos, blocks::Block};
        let (world, bounds, sources) = test_pistion_fixture();
        let mut pistons = Vec::new();
        for y in bounds.0.y..=bounds.1.y {
            for z in bounds.0.z..=bounds.1.z {
                for x in bounds.0.x..=bounds.1.x {
                    let pos = BlockPos::new(x, y, z);
                    if matches!(world.get_block(pos), Block::Piston { .. }) {
                        pistons.push(pos);
                    }
                }
            }
        }
        assert_eq!(pistons.len(), 1);
        let trials = probe_clock_window(&world, bounds, &sources, pistons[0], 12).unwrap();
        let trial = &trials[0];
        assert!(!trial.settled);
        assert!(trial.clock_retractions.len() >= 2);
        assert!(
            EventNetlist::from_piston_trial(trial).is_err(),
            "an active clock is not a settled single-transfer replay"
        );
        let first = &trial.attempts[trial.clock_retractions[0]];
        let mut edge = EventNetlist::from_clock_edge(trial).expect("first observed clock edge is representable");
        assert_eq!(edge.window_end_pending, trial.pending, "window-end pending work remains visible");
        assert_eq!((edge.source, edge.route, edge.actor), (first.source, first.route, first.actor));
        assert_eq!(edge.from, first.before.as_ref().unwrap().position);
        assert_eq!(edge.to, first.after.as_ref().unwrap().position);
        assert!(
            edge.fire(edge.source, edge.route + 1, 0).is_err(),
            "an unrepresented source route cannot trigger the move"
        );
        assert!(
            !edge.fire(edge.source, edge.route, 15).unwrap(),
            "live powered state rejects a stale retraction"
        );
        assert!(
            edge.fire(edge.source, edge.route, 0).unwrap(),
            "observed unpowered request moves at the falling edge"
        );
    }
    #[test]
    fn physical_graph_probe_builds_weighted_runtime() {
        use crate::redpiler::backend::graph::wire_probe::{
            probe,
            tests::{BOUNDS, LEFT, RECEIVER, RIGHT, fixture},
        };
        let world = fixture();
        let table = probe(&world, BOUNDS, &[LEFT, RIGHT], &[RECEIVER]).unwrap();
        let mut runtime = EventNetlist::from_probe(&table).unwrap();
        assert_eq!(runtime.power(0), table.outputs()[0], "probe entry state is retained");
        for (mask, expected) in table.samples().iter().enumerate() {
            let inputs = [(mask & 1 != 0) as u8 * 15, (mask & 2 != 0) as u8 * 15];
            assert_eq!(&runtime.frame(&inputs, &[], &[0]).unwrap(), expected, "mask {mask}");
        }
    }
    #[test]
    fn probe_conversion_rejects_interacting_source_tables() {
        let samples = vec![vec![0], vec![13], vec![11], vec![13]];
        let mut net = EventNetlist::from_samples(2, 1, &samples).unwrap();
        for (mask, expected) in samples.iter().enumerate() {
            let inputs = [(mask & 1 != 0) as u8 * 15, (mask & 2 != 0) as u8 * 15];
            assert_eq!(&net.frame(&inputs, &[], &[0]).unwrap(), expected);
        }
        assert!(net.frame(&[7, 0], &[], &[0]).is_err(), "physical probe covers binary sources only");
        let mut nonlinear = samples;
        nonlinear[3][0] = 14;
        assert!(EventNetlist::from_samples(2, 1, &nonlinear).is_err());
    }
    #[test]
    fn contact_event_is_delivered_when_input_strength_does_not_change() {
        let links = vec![
            Link {
                source: 0,
                target: 0,
                attenuation: 0,
                contact: None,
            },
            Link {
                source: 0,
                target: 1,
                attenuation: 0,
                contact: None,
            },
            Link {
                source: 0,
                target: 1,
                attenuation: 1,
                contact: Some((0, 1)),
            },
        ];
        let actions = vec![
            Action {
                input: 0,
                threshold: 0,
                when_powered: true,
                resource: 0,
                from: 0,
                to: 1,
            },
            Action {
                input: 1,
                threshold: 0,
                when_powered: true,
                resource: 1,
                from: 0,
                to: 1,
            },
        ];
        let mut net = EventNetlist::new(vec![15], vec![0, 0], 2, 2, links, actions, &[(0, 1)]).unwrap();
        assert_eq!(net.power(1), 15);
        assert_eq!(net.frame(&[15], &[0], &[1]).unwrap(), [15]);
        assert_eq!(net.contacts, [1, 1], "contact update delivers a request even though power stays 15");
    }
    #[test]
    fn unpowered_bud_needs_a_request_and_revalidates_at_execution() {
        let links = vec![Link {
            source: 0,
            target: 0,
            attenuation: 0,
            contact: None,
        }];
        let actions = vec![Action {
            input: 0,
            threshold: 0,
            when_powered: false,
            resource: 0,
            from: 1,
            to: 0,
        }];
        let mut net = EventNetlist::new(vec![0], vec![1], 2, 1, links, actions, &[]).unwrap();
        assert_eq!(net.frame(&[0], &[], &[0]).unwrap(), [0]);
        assert_eq!(net.contacts, [1], "unpowered state alone cannot queue a retraction");
        assert_eq!(net.frame(&[15], &[0], &[0]).unwrap(), [15]);
        assert_eq!(net.contacts, [1], "live powered state rejects a stale request");
        assert_eq!(net.frame(&[0], &[0], &[0]).unwrap(), [0]);
        assert_eq!(net.contacts, [0], "a notified, unpowered resource moves once");
    }
    #[test]
    fn falling_edge_and_explicit_rearm_are_separate_cycles() {
        let links = vec![Link {
            source: 0,
            target: 0,
            attenuation: 0,
            contact: None,
        }];
        let actions = vec![
            Action {
                input: 0,
                threshold: 0,
                when_powered: false,
                resource: 0,
                from: 1,
                to: 0,
            },
            Action {
                input: 0,
                threshold: 0,
                when_powered: true,
                resource: 0,
                from: 0,
                to: 1,
            },
        ];
        let mut net = EventNetlist::new(vec![15], vec![1], 2, 1, links, actions, &[]).unwrap();
        net.frame(&[0], &[0], &[]).unwrap();
        assert_eq!(net.contacts, [0], "falling-edge request pulls the resource");
        net.frame(&[15], &[1], &[]).unwrap();
        assert_eq!(net.contacts, [1], "explicit rearm moves it back on a later cycle");
        net.frame(&[0], &[], &[]).unwrap();
        assert_eq!(net.contacts, [1], "unpowered input without an event remains BUD");
        net.frame(&[0], &[0], &[]).unwrap();
        assert_eq!(net.contacts, [0]);
    }
    #[test]
    fn fifo_revalidates_and_consumes_shared_resources_once_per_frame() {
        let links = vec![
            Link {
                source: 0,
                target: 0,
                attenuation: 0,
                contact: None,
            },
            Link {
                source: 1,
                target: 1,
                attenuation: 2,
                contact: Some((0, 1)),
            },
            Link {
                source: 0,
                target: 2,
                attenuation: 0,
                contact: Some((0, 0)),
            },
            Link {
                source: 0,
                target: 3,
                attenuation: 0,
                contact: None,
            },
            Link {
                source: 1,
                target: 4,
                attenuation: 3,
                contact: Some((1, 1)),
            },
        ];
        let actions = vec![
            Action {
                input: 0,
                threshold: 0,
                when_powered: true,
                resource: 0,
                from: 0,
                to: 1,
            },
            Action {
                input: 1,
                threshold: 0,
                when_powered: true,
                resource: 1,
                from: 0,
                to: 1,
            },
            Action {
                input: 2,
                threshold: 0,
                when_powered: true,
                resource: 1,
                from: 0,
                to: 2,
            },
            Action {
                input: 3,
                threshold: 0,
                when_powered: true,
                resource: 1,
                from: 1,
                to: 2,
            },
        ];
        let mut net = EventNetlist::new(vec![15, 12], vec![0, 0], 3, 5, links.clone(), actions.clone(), &[(0, 1), (0, 2)]).unwrap();
        assert_eq!(net.frame(&[15, 12], &[0, 2, 3], &[4]).unwrap(), [9]);
        assert_eq!(net.contacts, [1, 1]);
        assert!(net.frame(&[15, 16], &[3], &[4]).is_err());
        assert!(net.frame(&[0, 12], &[999], &[4]).is_err());
        assert!(net.frame(&[0, 12], &[3], &[999]).is_err());
        assert_eq!(net.contacts, [1, 1]);
        assert_eq!(net.sources, [15, 12]);
        assert_eq!(net.frame(&[15, 12], &[3], &[4]).unwrap(), [0]);
        assert_eq!(net.contacts, [1, 2]);
        assert!(EventNetlist::new(vec![16], vec![], 0, 0, vec![], vec![], &[]).is_err());
        let mut bad = links;
        bad[0].source = 9;
        assert!(EventNetlist::new(vec![15, 12], vec![0, 0], 3, 5, bad, actions, &[]).is_err());
    }
}
