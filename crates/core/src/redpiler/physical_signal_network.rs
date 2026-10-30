//! Compiled source-derived signal updates for bounded physical circuits.
#[path = "physical_signal_network/dust_corridor.rs"]
mod dust_corridor;
#[path = "physical_signal_network/entry_work.rs"]
mod entry_work;
#[path = "physical_signal_network/input_links.rs"]
mod input_links;
#[cfg(test)]
#[path = "physical_signal_network/repeater_tests.rs"]
mod repeater_tests;
pub(crate) use entry_work::EntryWork;
#[derive(Clone, Copy)]
struct WireSample {
    site: super::program_graph::SiteId,
    face: mchprs_blocks::BlockFace,
    dust: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct SignalEvent {
    /// The exact piston receiver selected by this callback delivery.
    pub(crate) piston: super::program_graph::PistonId,
    /// Unique occurrence within this signal network; route alone is not unique.
    pub(crate) occurrence: u64,
    pub(crate) route: usize,
    pub(crate) action: mchprs_world::PistonAction,
    pub(crate) piston_input: u8,
    pub(crate) captures: Vec<CapturedSite>,
    pub(crate) piston_guards: Vec<(super::program_graph::GuardId, bool)>,
}

#[derive(Clone, Debug)]
pub(crate) struct CapturedSite {
    pub(crate) site: super::program_graph::SiteId,
    pub(crate) profile: Option<super::program_graph::ProfileId>,
    pub(crate) block: mchprs_blocks::blocks::Block,
    pub(crate) wire: Option<mchprs_blocks::blocks::RedstoneWire>,
}

/// Source-derived lever, torch, wire, and callback fanout for bounded physical circuits.
#[derive(Clone)]
pub(crate) struct PhysicalSignalNetwork {
    program: std::sync::Arc<super::program_graph::CircuitProgram>,
    state: super::program_graph::CircuitState,
    wire_samples: std::sync::Arc<[(usize, Vec<WireSample>)]>,
    input_links: std::sync::Arc<input_links::InputLinks>,
    dust_corridor: Option<dust_corridor::Corridor>,
    physical_settles: u64,
    torches: std::sync::Arc<[usize]>,
    observers: std::sync::Arc<[usize]>,
    comparators: std::sync::Arc<[usize]>,
    scheduler: super::TickScheduler<usize>,
    detached_output_repeaters: Vec<usize>,
    tick: u64,
    next_occurrence: u64,
}

impl PhysicalSignalNetwork {
    pub(crate) fn compile(
        program: &super::program_graph::CircuitProgram,
        state: &super::program_graph::CircuitState,
        entry: &EntryWork,
    ) -> Result<Self, String> {
        entry.validate(program, state)?;
        let mut network = Self::compile_state(program, state).map_err(str::to_owned)?;
        network.tick = entry.logical_tick();
        Ok(network)
    }

    fn compile_state(program: &super::program_graph::CircuitProgram, state: &super::program_graph::CircuitState) -> Result<Self, &'static str> {
        use super::components::NodeType;
        use super::program_graph::{CircuitNodeKind, ComponentState, PowerTarget, SiteSource};
        if state
            .components
            .iter()
            .any(|component| matches!(component, ComponentState::Electrical(node) if node.pending_tick))
        {
            return Err("scheduled component entry work is unsupported");
        }
        let mut wire_samples = Vec::new();
        let mut torches = Vec::new();
        let mut observers = Vec::new();
        let mut comparators = Vec::new();
        for (node_index, node) in program.nodes().iter().enumerate() {
            match node.kind {
                CircuitNodeKind::Electrical(NodeType::Lamp) => {
                    if !matches!(&state.components[node_index], ComponentState::Electrical(state) if !state.pending_tick) {
                        return Err("lamp entry state has unavailable or pending work");
                    }
                }
                CircuitNodeKind::Electrical(NodeType::Torch) => torches.push(node_index),
                CircuitNodeKind::Electrical(NodeType::Repeater { .. }) => {}
                CircuitNodeKind::Electrical(NodeType::Comparator { .. }) => {
                    if program.comparator_input(super::program_graph::NodeId::new(node_index as u32)).is_none() {
                        return Err("comparator input plan is unavailable");
                    }
                    comparators.push(node_index);
                }
                CircuitNodeKind::Observer { .. } => {
                    if !matches!(state.components[node_index], ComponentState::Electrical(_)) {
                        return Err("observer state is unavailable");
                    }
                    observers.push(node_index);
                }
                CircuitNodeKind::Wire(wire_index) => {
                    if !matches!(state.components[node_index], ComponentState::Wire(_)) {
                        return Err("wire state is unavailable");
                    }
                    let wire = &program.wires()[wire_index];
                    let mut samples = Vec::new();
                    for (face, slot) in [
                        (mchprs_blocks::BlockFace::West, 0),
                        (mchprs_blocks::BlockFace::East, 1),
                        (mchprs_blocks::BlockFace::Bottom, 2),
                        (mchprs_blocks::BlockFace::Top, 3),
                        (mchprs_blocks::BlockFace::North, 4),
                        (mchprs_blocks::BlockFace::South, 5),
                    ] {
                        let site = wire.canonical_neighbors[slot];
                        let source = program.sites()[site.index()].source;
                        let block = site_block(program, state, site)?;
                        let dust = matches!(block, mchprs_blocks::blocks::Block::RedstoneWire { .. });
                        if dust
                            || !block.is_solid()
                            || matches!(source, SiteSource::Contact(_))
                            || !program.power_links_to(PowerTarget::Conductor { site, include_dust: false }).is_empty()
                        {
                            if let SiteSource::Component(source) = source
                                && !matches!(
                                    program.nodes()[source.index()].kind,
                                    CircuitNodeKind::Wire(_)
                                        | CircuitNodeKind::Electrical(
                                            NodeType::Lever | NodeType::Torch | NodeType::Repeater { .. } | NodeType::Constant
                                        )
                                )
                                && (dust
                                    || crate::redpiler::power_rules::emits_weak(block, face)
                                    || crate::redpiler::power_rules::emits_strong(block, face))
                            {
                                return Err("wire depends on an unsupported dynamic source");
                            }
                            if !dust && block.is_solid() {
                                validate_power_source(program, site, super::program_graph::PowerPort::Conducted { include_dust: false }, 0)?;
                            } else {
                                validate_power_source(program, site, super::program_graph::PowerPort::Weak(face), 0)?;
                            }
                            samples.push(WireSample { site, face, dust });
                        }
                    }
                    wire_samples.push((node_index, samples));
                }
                _ => {}
            }
        }
        for &node_index in &torches {
            for link in program.power_links_to(PowerTarget::Input {
                node: super::program_graph::NodeId::new(node_index as u32),
                port: super::program_graph::InputPort::Main,
            }) {
                if !matches!(
                    program.guards().get(link.guard.index()),
                    Some(super::program_graph::Guard::Always | super::program_graph::Guard::State(_))
                ) {
                    return Err("torch input has an unsupported guard");
                }
                validate_power_source(program, link.from, link.from_port, 0)?;
                let site = link.from;
                if let SiteSource::Component(source) = program.sites()[site.index()].source
                    && !matches!(
                        program.nodes()[source.index()].kind,
                        CircuitNodeKind::Wire(_)
                            | CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Torch | NodeType::Repeater { .. } | NodeType::Constant)
                    )
                    && signal_site_may_emit(program, state, site, link.from_port)?
                {
                    return Err("torch depends on an unsupported dynamic source");
                }
            }
        }
        for piston in program.pistons() {
            for port in [super::program_graph::InputPort::Main, super::program_graph::InputPort::Quasi] {
                for link in program.power_links_to(PowerTarget::Input { node: piston.node, port }) {
                    if !state_only_guard(link.guard, program.guards()) {
                        return Err("piston input has an unsupported site guard");
                    }
                    validate_power_source(program, link.from, link.from_port, 0)?;
                }
            }
        }
        Ok(Self {
            program: std::sync::Arc::new(program.clone()),
            state: state.clone(),
            wire_samples: wire_samples.into(),
            input_links: std::sync::Arc::new(input_links::InputLinks::compile(program)),
            dust_corridor: None,
            physical_settles: 0,
            torches: torches.into(),
            observers: observers.into(),
            comparators: comparators.into(),
            scheduler: super::TickScheduler::default(),
            detached_output_repeaters: Vec::new(),
            tick: 0,
            next_occurrence: 0,
        })
    }

    /// Change one declared lever, update wires, schedule torches, and return changed output routes.
    pub(crate) fn set_lever(&mut self, position: mchprs_blocks::BlockPos, powered: bool) -> Result<Vec<SignalEvent>, &'static str> {
        let before = self.clone();
        let result = self.set_lever_in_transaction(position, powered);
        if result.is_err() {
            *self = before;
        }
        result
    }

    pub(super) fn set_lever_in_transaction(&mut self, position: mchprs_blocks::BlockPos, powered: bool) -> Result<Vec<SignalEvent>, &'static str> {
        use super::components::NodeType;
        use super::program_graph::{CircuitNodeKind, ComponentState};
        let (index, _) = self
            .program
            .nodes()
            .iter()
            .enumerate()
            .find(|(_, node)| node.position == Some(position) && matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Lever)))
            .ok_or("lever is not in the compiled signal network")?;
        let ComponentState::Electrical(source) = &self.state.components[index] else {
            return Err("lever state is unavailable");
        };
        if source.powered == powered {
            return Ok(Vec::new());
        }
        let ComponentState::Electrical(source) = &mut self.state.components[index] else {
            return Err("lever state is unavailable");
        };
        source.powered = powered;
        source.output_strength = if powered { 15 } else { 0 };
        let result = (|| {
            let mut changed = self.update_wires()?;
            changed.push(index);
            self.schedule_torches()?;
            let routes = self.output_routes(changed);
            self.deliver_routes(&routes)
        })();
        result
    }

    pub(crate) fn state(&self) -> &super::program_graph::CircuitState {
        &self.state
    }

    pub(crate) fn current_tick(&self) -> u64 {
        self.tick
    }

    pub(crate) fn route_guard_matches(&self, guard: super::program_graph::GuardId) -> Result<bool, &'static str> {
        guard_matches(guard, &self.program, &self.state, 0)
    }

    pub(crate) fn route_event_guard_matches(&self, guard: super::program_graph::GuardId, event: &SignalEvent) -> Result<bool, &'static str> {
        guard_matches_event(guard, &self.program, &self.state, &event.captures, 0)
    }

    pub(crate) fn set_resource_contact(
        &mut self,
        resource: usize,
        contact: super::program_graph::ContactId,
        enabled: bool,
    ) -> Result<Vec<SignalEvent>, &'static str> {
        let spec = self.program.resources().get(resource).ok_or("resource is unavailable")?;
        if !spec.contacts.contains(&contact) {
            return Err("resource cannot occupy the requested contact");
        }
        let old = *self.state.resources.get(resource).ok_or("resource state is unavailable")?;
        if old.at == contact && old.enabled == enabled {
            return Ok(Vec::new());
        }
        let state = self.state.resources.get_mut(resource).ok_or("resource state is unavailable")?;
        state.at = contact;
        state.enabled = enabled;
        state.revision += 1;
        // Graph piston transitions publish neighbor changes through PistonRoutes. The
        // caller delivers those ordered routes; the contact mutation itself is not a route.
        Ok(Vec::new())
    }

    pub(crate) fn notify_resource_observers(&mut self, resource: usize, contact: super::program_graph::ContactId) -> Result<(), &'static str> {
        let Some(corridor) = &self.dust_corridor else { return Ok(()) };
        for record in corridor
            .plan
            .resource_observers
            .iter()
            .filter(|record| record.resource.index() == resource && record.contact == contact)
        {
            let node = record.observer.index();
            if !self.scheduler.contains(&node) {
                self.scheduler.schedule_half_tick(node, 2, mchprs_world::TickPriority::Normal);
                let super::program_graph::ComponentState::Electrical(observer) = &mut self.state.components[node] else {
                    return Err("observer state is unavailable");
                };
                observer.pending_tick = true;
            }
        }
        Ok(())
    }

    pub(crate) fn resource_destination_available(
        &self,
        resource: super::program_graph::ResourceId,
        contact: super::program_graph::ContactId,
        vacated_head: Option<super::program_graph::PistonId>,
    ) -> Result<bool, &'static str> {
        let Some(spec) = self.program.resources().get(resource.index()) else {
            return Ok(false);
        };
        if !spec.contacts.contains(&contact)
            || self
                .program
                .resources_at_contact(contact)
                .iter()
                .any(|other| *other != resource && self.state.resources[other.index()].at == contact)
        {
            return Ok(false);
        }
        if self
            .program
            .base_pistons_at_contact(contact)
            .iter()
            .any(|id| !self.state.pistons[id.index()].availability.base_pending())
        {
            return Ok(false);
        }
        if self.program.head_pistons_at_contact(contact).iter().any(|id| {
            let state = self.state.pistons[id.index()];
            state.head_present
                && matches!(
                    state.availability,
                    super::program_graph::PistonAvailability::Ready
                        | super::program_graph::PistonAvailability::HeadPending
                        | super::program_graph::PistonAvailability::BasePendingHeadPresent
                )
                && Some(*id) != vacated_head
        }) {
            return Ok(false);
        }
        if vacated_head.is_some_and(|id| self.program.pistons()[id.index()].near == contact) {
            return Ok(true);
        }
        for variant in &self.program.contacts()[contact.index()].variants {
            if guard_matches(variant.when, &self.program, &self.state, 0)? {
                return Ok(self.program.profile_block(variant.profile).ok_or("contact profile unavailable")? == mchprs_blocks::blocks::Block::Air);
            }
        }
        Err("contact has no active profile")
    }

    pub(crate) fn set_piston_state(&mut self, piston: usize, state: super::program_graph::PistonState) -> Result<(), &'static str> {
        *self.state.pistons.get_mut(piston).ok_or("piston state is unavailable")? = state;
        Ok(())
    }

    pub(crate) fn deliver_callback_routes(&mut self, routes: &[usize]) -> Result<Vec<SignalEvent>, &'static str> {
        self.deliver_routes(routes)
    }

    pub(crate) fn advance_tick(&mut self) -> Result<Vec<SignalEvent>, &'static str> {
        let previous = self.clone();
        let result = self.advance_tick_inner();
        if result.is_err() {
            *self = previous;
        }
        result
    }

    pub(super) fn advance_tick_inner(&mut self) -> Result<Vec<SignalEvent>, &'static str> {
        self.tick += 1;
        let mut events = self.drain_scheduled()?;
        self.scheduler.end_last_tick_move_next();
        events.extend(self.drain_scheduled()?);
        Ok(events)
    }

    fn drain_scheduled(&mut self) -> Result<Vec<SignalEvent>, &'static str> {
        use super::components::NodeType;
        use super::program_graph::{CircuitNodeKind, ComponentState, InputPort};
        use mchprs_world::TickPriority;
        let mut events = Vec::new();
        while let Some(node) = self.scheduler.this_tick().pop_first() {
            let kind = self.program.nodes()[node].kind.clone();
            let ComponentState::Electrical(state) = &mut self.state.components[node] else {
                return Err("scheduled component state unavailable");
            };
            state.pending_tick = false;
            let mut reschedule = None;
            let changed = match kind {
                CircuitNodeKind::Electrical(NodeType::Torch) => {
                    let input = self.input_power(node, InputPort::Main)?;
                    let ComponentState::Electrical(state) = &mut self.state.components[node] else {
                        unreachable!()
                    };
                    let powered = input == 0;
                    let changed = state.powered != powered;
                    state.powered = powered;
                    state.output_strength = u8::from(powered) * 15;
                    changed
                }
                CircuitNodeKind::Observer { .. } => {
                    let ComponentState::Electrical(state) = &mut self.state.components[node] else {
                        unreachable!()
                    };
                    state.powered = !state.powered;
                    state.output_strength = u8::from(state.powered) * 15;
                    if state.powered {
                        reschedule = Some((2, TickPriority::Normal));
                    }
                    true
                }
                CircuitNodeKind::Electrical(NodeType::Repeater { .. }) => {
                    let input = self.input_power(node, InputPort::Main)?;
                    let delay = self.repeater_delay(node) as usize;
                    let ComponentState::Electrical(state) = &mut self.state.components[node] else {
                        unreachable!()
                    };
                    if state.repeater_locked {
                        false
                    } else if state.powered && input == 0 {
                        state.powered = false;
                        state.output_strength = 0;
                        true
                    } else if !state.powered {
                        if input == 0 {
                            reschedule = Some((delay, TickPriority::Higher));
                        }
                        state.powered = true;
                        state.output_strength = 15;
                        true
                    } else {
                        false
                    }
                }
                CircuitNodeKind::Electrical(NodeType::Comparator { mode, .. }) => {
                    let strength = self.comparator_output(node)?;
                    let powered = self.comparator_powered(node)?;
                    let ComponentState::Electrical(state) = &mut self.state.components[node] else {
                        unreachable!()
                    };
                    if strength != state.output_strength || mode == mchprs_blocks::blocks::ComparatorMode::Compare {
                        state.powered = powered;
                        state.output_strength = strength;
                        true
                    } else {
                        false
                    }
                }
                CircuitNodeKind::Electrical(NodeType::Lamp) => {
                    let input = self.input_power(node, InputPort::Main)?;
                    let ComponentState::Electrical(state) = &mut self.state.components[node] else {
                        unreachable!()
                    };
                    if input == 0 {
                        state.powered = false;
                        state.output_strength = 0;
                    }
                    false
                }
                _ => return Err("unsupported scheduled component"),
            };
            if let Some((delay, priority)) = reschedule {
                self.scheduler.schedule_half_tick(node, delay, priority);
                let ComponentState::Electrical(state) = &mut self.state.components[node] else {
                    unreachable!()
                };
                state.pending_tick = true;
            }
            if changed {
                let mut nodes = self.update_wires()?;
                nodes.push(node);
                self.schedule_torches()?;
                events.extend(self.deliver_routes(&self.output_routes(nodes))?);
            }
        }
        Ok(events)
    }

    pub(crate) fn piston_input(&self, position: mchprs_blocks::BlockPos) -> Result<u8, &'static str> {
        let piston = self
            .program
            .pistons()
            .iter()
            .find(|piston| piston.position == position)
            .ok_or("piston is not in the signal network")?;
        self.piston_input_by_node(piston.node)
    }

    pub(super) fn piston_input_by_id(&self, id: super::program_graph::PistonId) -> Result<u8, &'static str> {
        let piston = self.program.pistons().get(id.index()).ok_or("piston is not in the signal network")?;
        self.piston_input_by_node(piston.node)
    }

    fn piston_input_by_node(&self, node: super::program_graph::NodeId) -> Result<u8, &'static str> {
        Ok(self
            .input_power(node.index(), super::program_graph::InputPort::Main)?
            .max(self.input_power(node.index(), super::program_graph::InputPort::Quasi)?))
    }

    pub(crate) fn piston_signal_nodes(&self, position: mchprs_blocks::BlockPos) -> Result<Vec<usize>, &'static str> {
        use super::program_graph::{CircuitNodeKind, PowerTarget, SiteSource};
        let piston = self
            .program
            .pistons()
            .iter()
            .find(|piston| piston.position == position)
            .ok_or("piston is not in the signal network")?;
        let mut pending_sites = [super::program_graph::InputPort::Main, super::program_graph::InputPort::Quasi]
            .into_iter()
            .flat_map(|port| {
                self.program
                    .power_links_to(PowerTarget::Input { node: piston.node, port })
                    .iter()
                    .map(|link| link.from)
            })
            .collect::<Vec<_>>();
        let mut pending = Vec::new();
        let mut visited_sites = Vec::new();
        let mut nodes = Vec::new();
        while !pending.is_empty() || !pending_sites.is_empty() {
            if let Some(site) = pending_sites.pop() {
                if visited_sites.contains(&site) {
                    continue;
                }
                visited_sites.push(site);
                if let SiteSource::Component(source) = self.program.sites()[site.index()].source {
                    pending.push(source.index());
                }
                for include_dust in [false, true] {
                    pending_sites.extend(
                        self.program
                            .power_links_to(PowerTarget::Conductor { site, include_dust })
                            .iter()
                            .map(|link| link.from),
                    );
                }
                continue;
            }
            let Some(node) = pending.pop() else { continue };
            if nodes.contains(&node) {
                continue;
            }
            nodes.push(node);
            match self.program.nodes()[node].kind {
                CircuitNodeKind::Wire(_) => {
                    if let Some((_, samples)) = self.wire_samples.iter().find(|(target, _)| *target == node) {
                        pending_sites.extend(samples.iter().map(|sample| sample.site));
                    }
                }
                CircuitNodeKind::Electrical(super::components::NodeType::Torch) => {
                    pending_sites.extend(
                        self.program
                            .power_links_to(PowerTarget::Input {
                                node: super::program_graph::NodeId::new(node as u32),
                                port: super::program_graph::InputPort::Main,
                            })
                            .iter()
                            .map(|link| link.from),
                    );
                }
                CircuitNodeKind::Electrical(super::components::NodeType::Repeater { .. }) => {
                    for port in [super::program_graph::InputPort::Main, super::program_graph::InputPort::Side] {
                        pending_sites.extend(
                            self.program
                                .power_links_to(PowerTarget::Input {
                                    node: super::program_graph::NodeId::new(node as u32),
                                    port,
                                })
                                .iter()
                                .map(|link| link.from),
                        );
                    }
                }
                CircuitNodeKind::Observer { watched, .. } => {
                    pending_sites.push(watched);
                }
                _ => {}
            }
        }
        Ok(nodes)
    }

    pub(crate) fn has_pending_torch_work(&self) -> bool {
        self.scheduler.iter().any(|(node, _, _)| self.torches.contains(node))
    }

    pub(crate) fn has_pending_observer_work(&self) -> bool {
        self.scheduler.iter().any(|(node, _, _)| self.observers.contains(node))
    }

    pub(crate) fn has_pending_repeater_work(&self) -> bool {
        self.scheduler.iter().any(|(node, _, _)| {
            matches!(
                self.program.nodes()[*node].kind,
                super::program_graph::CircuitNodeKind::Electrical(super::components::NodeType::Repeater { .. })
            )
        })
    }

    pub(crate) fn has_pending_comparator_work(&self) -> bool {
        self.scheduler.iter().any(|(node, _, _)| self.comparators.contains(node))
    }

    pub(crate) fn repeater_inputs(&self, position: mchprs_blocks::BlockPos) -> Result<(u8, bool), &'static str> {
        use super::components::NodeType;
        let node = self
            .program
            .nodes()
            .iter()
            .position(|node| {
                node.position == Some(position) && matches!(node.kind, super::program_graph::CircuitNodeKind::Electrical(NodeType::Repeater { .. }))
            })
            .ok_or("output is not a compiled repeater")?;
        Ok((
            self.input_power(node, super::program_graph::InputPort::Main)?,
            self.input_power(node, super::program_graph::InputPort::Side)? > 0,
        ))
    }

    pub(crate) fn detach_repeater_for_adapter(&mut self, position: mchprs_blocks::BlockPos) -> Result<(), &'static str> {
        use super::components::NodeType;
        use super::program_graph::CircuitNodeKind;
        let node = self
            .program
            .nodes()
            .iter()
            .position(|node| node.position == Some(position) && matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Repeater { .. })))
            .ok_or("output is not a compiled repeater")?;
        if self.detached_output_repeaters.contains(&node) {
            return Err("repeater is already owned by an output adapter");
        }
        if self.scheduler.contains(&node) {
            return Err("repeater has scheduled work");
        }
        self.detached_output_repeaters.push(node);
        Ok(())
    }

    pub(crate) fn publish_repeater_output(&mut self, position: mchprs_blocks::BlockPos, powered: bool) -> Result<Vec<SignalEvent>, &'static str> {
        use super::components::NodeType;
        use super::program_graph::{CircuitNodeKind, ComponentState};
        let node = self
            .program
            .nodes()
            .iter()
            .position(|node| node.position == Some(position) && matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Repeater { .. })))
            .ok_or("output is not a compiled repeater")?;
        if !self.detached_output_repeaters.contains(&node) {
            return Err("repeater output is not owned by the output adapter");
        }
        let ComponentState::Electrical(state) = &mut self.state.components[node] else {
            return Err("repeater state is unavailable");
        };
        if state.powered == powered {
            return Ok(Vec::new());
        }
        state.powered = powered;
        state.output_strength = if powered { 15 } else { 0 };
        let route = self.program.nodes()[node].output_route;
        match route {
            Some(route) => self.deliver_routes(&[route.index()]),
            None => Ok(Vec::new()),
        }
    }

    fn deliver_routes(&mut self, routes: &[usize]) -> Result<Vec<SignalEvent>, &'static str> {
        let mut delivered = Vec::new();
        let mut route_count = 0;
        for &route in routes {
            self.deliver_route(route, &mut delivered, &mut route_count, 0)?;
        }
        Ok(delivered)
    }

    fn deliver_route(
        &mut self,
        route_index: usize,
        delivered: &mut Vec<SignalEvent>,
        route_count: &mut usize,
        depth: usize,
    ) -> Result<(), &'static str> {
        use super::components::NodeType;
        use super::program_graph::{CallbackReceiver, CircuitNodeKind, ComponentState, DeliveryKind, InputPort};
        *route_count += 1;
        if *route_count > 4096 || depth > 64 {
            return Err("signal callback fanout exceeded its event bound");
        }
        let route = self.program.routes().get(route_index).ok_or("signal callback route is unavailable")?;
        let mut captures = Vec::new();
        if matches!(self.program.nodes()[route.source.index()].kind, CircuitNodeKind::Wire(_)) {
            for delivery in route
                .deliveries
                .iter()
                .filter(|delivery| delivery.view == super::program_graph::OperandView::Captured)
            {
                if !captures.iter().any(|capture: &CapturedSite| capture.site == delivery.target) {
                    captures.push(self.capture_site(delivery.target)?);
                }
            }
        }
        let deliveries = route.deliveries.to_vec();
        for delivery in deliveries {
            if !guard_matches_event(delivery.guard, &self.program, &self.state, &captures, 0)? {
                continue;
            }
            let captured = captures.iter().find(|capture| capture.site == delivery.target);
            if delivery.view == super::program_graph::OperandView::Captured && captured.is_none() {
                continue;
            }
            let receiver = match delivery.receiver {
                Some(CallbackReceiver::Component(receiver)) => receiver,
                Some(CallbackReceiver::PistonBase(id) | CallbackReceiver::PistonHead(id)) => {
                    let piston = self.program.pistons().get(id.index()).ok_or("piston receiver is unavailable")?;
                    let input = self
                        .input_power(piston.node.index(), InputPort::Main)?
                        .max(self.input_power(piston.node.index(), InputPort::Quasi)?);
                    let occurrence = self.next_occurrence;
                    self.next_occurrence = self.next_occurrence.wrapping_add(1);
                    delivered.push(SignalEvent {
                        piston: id,
                        occurrence,
                        route: route_index,
                        action: if input > 0 {
                            mchprs_world::PistonAction::Extend
                        } else {
                            mchprs_world::PistonAction::Retract
                        },
                        piston_input: input,
                        captures: captures.clone(),
                        piston_guards: vec![(delivery.guard, true)],
                    });
                    continue;
                }
                // Graph contact deliveries intentionally carry no component update.
                Some(CallbackReceiver::Contact(_)) | None => continue,
            };
            if let CircuitNodeKind::Wire(wire_index) = self.program.nodes()[receiver.index()].kind {
                match delivery.kind {
                    DeliveryKind::Update { .. } => {
                        let changed = self.update_wires()?;
                        self.schedule_torches()?;
                        for followup in self.output_routes(changed) {
                            self.deliver_route(followup, delivered, route_count, depth + 1)?;
                        }
                    }
                    DeliveryKind::Shape { direction } => {
                        let current = match captured.and_then(|capture| capture.wire) {
                            Some(wire) => wire,
                            None => match self.state.components[receiver.index()] {
                                ComponentState::Wire(wire) => wire,
                                _ => return Err("wire state is unavailable"),
                            },
                        };
                        let plan = &self.program.wires()[wire_index];
                        let raw = crate::redstone::wire::get_raw_sides_from_neighbor_read(current, |slot| {
                            site_block(&self.program, &self.state, plan.canonical_neighbors[slot])
                                .expect("validated wire neighbor block remains available")
                        });
                        let updated = crate::redstone::wire::on_neighbor_changed_from(current, raw, direction);
                        if updated != current {
                            let ComponentState::Wire(wire) = &mut self.state.components[receiver.index()] else {
                                return Err("wire state is unavailable");
                            };
                            *wire = updated;
                            if let Some(route) = plan.shape_route {
                                self.deliver_route(route.index(), delivered, route_count, depth + 1)?;
                            }
                        }
                    }
                }
                continue;
            }
            let direction = match delivery.kind {
                DeliveryKind::Update { direction } => direction,
                DeliveryKind::Shape { .. } => continue,
            };
            if matches!(self.program.nodes()[receiver.index()].kind, CircuitNodeKind::Electrical(NodeType::Torch)) {
                let input = self.input_power(receiver.index(), InputPort::Main)?;
                let ComponentState::Electrical(torch) = &mut self.state.components[receiver.index()] else {
                    return Err("torch state is unavailable");
                };
                if torch.powered == (input > 0) && !self.scheduler.contains(&receiver.index()) {
                    self.scheduler.schedule_half_tick(receiver.index(), 2, mchprs_world::TickPriority::Normal);
                    torch.pending_tick = true;
                }
                continue;
            }
            if matches!(
                self.program.nodes()[receiver.index()].kind,
                CircuitNodeKind::Electrical(NodeType::Repeater { .. })
            ) {
                if self.detached_output_repeaters.contains(&receiver.index()) {
                    continue;
                }
                let main = self.input_power(receiver.index(), InputPort::Main)? > 0;
                let side = self.input_power(receiver.index(), InputPort::Side)? > 0;
                let delay = self.repeater_delay(receiver.index());
                let priority = match self.program.nodes()[receiver.index()].kind {
                    CircuitNodeKind::Electrical(NodeType::Repeater { facing_diode: true, .. }) => mchprs_world::TickPriority::Highest,
                    _ if !main => mchprs_world::TickPriority::Higher,
                    _ => mchprs_world::TickPriority::High,
                };
                let ComponentState::Electrical(repeater) = &mut self.state.components[receiver.index()] else {
                    return Err("repeater state is unavailable");
                };
                repeater.repeater_locked = side;
                if !side && !repeater.pending_tick && main != repeater.powered {
                    repeater.pending_tick = true;
                    self.scheduler.schedule_half_tick(receiver.index(), delay as usize, priority);
                }
                continue;
            }
            if matches!(
                self.program.nodes()[receiver.index()].kind,
                CircuitNodeKind::Electrical(NodeType::Comparator { .. })
            ) {
                if !self.scheduler.contains(&receiver.index()) {
                    let strength = self.comparator_output(receiver.index())?;
                    let powered = self.comparator_powered(receiver.index())?;
                    let ComponentState::Electrical(comparator) = &mut self.state.components[receiver.index()] else {
                        return Err("comparator state is unavailable");
                    };
                    if strength != comparator.output_strength || powered != comparator.powered {
                        comparator.pending_tick = true;
                        let priority = if matches!(
                            self.program.nodes()[receiver.index()].kind,
                            CircuitNodeKind::Electrical(NodeType::Comparator { facing_diode: true, .. })
                        ) {
                            mchprs_world::TickPriority::High
                        } else {
                            mchprs_world::TickPriority::Normal
                        };
                        self.scheduler.schedule_half_tick(receiver.index(), 2, priority);
                    }
                }
                continue;
            }
            if matches!(
                self.program.nodes()[receiver.index()].kind,
                CircuitNodeKind::Electrical(NodeType::CopperBulb)
            ) {
                let input = self.input_power(receiver.index(), InputPort::Main)? > 0;
                let route = self.program.nodes()[receiver.index()].output_route;
                let ComponentState::Electrical(bulb) = &mut self.state.components[receiver.index()] else {
                    return Err("copper bulb state is unavailable");
                };
                if bulb.powered != input {
                    let lit = if input { bulb.output_strength == 0 } else { bulb.output_strength > 0 };
                    bulb.powered = input;
                    bulb.output_strength = if lit { 15 } else { 0 };
                    if let Some(route) = route {
                        self.deliver_route(route.index(), delivered, route_count, depth + 1)?;
                    }
                }
                continue;
            }
            if matches!(self.program.nodes()[receiver.index()].kind, CircuitNodeKind::Electrical(NodeType::Lamp)) {
                let input = self.input_power(receiver.index(), InputPort::Main)? > 0;
                let ComponentState::Electrical(lamp) = &mut self.state.components[receiver.index()] else {
                    return Err("lamp state is unavailable");
                };
                if input && !lamp.powered {
                    lamp.powered = true;
                    lamp.output_strength = 0;
                } else if !input && lamp.powered && !self.scheduler.contains(&receiver.index()) {
                    self.scheduler.schedule_half_tick(receiver.index(), 4, mchprs_world::TickPriority::Normal);
                    lamp.pending_tick = true;
                }
                continue;
            }
            if matches!(
                self.program.nodes()[receiver.index()].kind,
                CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::PressurePlate | NodeType::Constant)
            ) {
                continue;
            }
            let CircuitNodeKind::Observer { facing, .. } = self.program.nodes()[receiver.index()].kind else {
                return Err(match self.program.nodes()[receiver.index()].kind {
                    CircuitNodeKind::Electrical(NodeType::Hopper) => "signal callback reaches an unsupported hopper",
                    CircuitNodeKind::Electrical(NodeType::NoteBlock { .. }) => "signal callback reaches an unsupported note block",
                    CircuitNodeKind::Electrical(NodeType::CommandBlock { .. }) => "signal callback reaches an unsupported command block",
                    CircuitNodeKind::Electrical(NodeType::Trapdoor) => "signal callback reaches an unsupported trapdoor",
                    _ => "signal callback reaches an unsupported component",
                });
            };
            let ComponentState::Electrical(observer) = &mut self.state.components[receiver.index()] else {
                return Err("observer state is unavailable");
            };
            let qualifies = direction.map_or(observer.powered, |direction| direction == facing);
            if qualifies && !self.scheduler.contains(&receiver.index()) {
                self.scheduler.schedule_half_tick(receiver.index(), 2, mchprs_world::TickPriority::Normal);
                observer.pending_tick = true;
            }
        }
        Ok(())
    }

    fn capture_site(&self, site: super::program_graph::SiteId) -> Result<CapturedSite, &'static str> {
        use super::program_graph::SiteSource;
        let spec = self.program.sites().get(site.index()).ok_or("captured site is unavailable")?;
        let profile = match spec.source {
            SiteSource::Contact(contact) => {
                let mut selected = None;
                for variant in &self.program.contacts()[contact.index()].variants {
                    if guard_matches(variant.when, &self.program, &self.state, 0)? {
                        selected = Some(variant.profile);
                        break;
                    }
                }
                selected
            }
            SiteSource::Constant(profile) => Some(profile),
            SiteSource::Component(_) => None,
        };
        let block = match profile {
            Some(profile) => self.program.profile_block(profile).ok_or("captured profile is unavailable")?,
            None => site_block(&self.program, &self.state, site)?,
        };
        let wire = match spec.source {
            SiteSource::Component(node) => match self.state.components.get(node.index()) {
                Some(super::program_graph::ComponentState::Wire(wire)) => Some(*wire),
                _ => None,
            },
            _ => None,
        };
        Ok(CapturedSite { site, profile, block, wire })
    }

    fn settle_wires(&mut self) -> Result<Vec<usize>, &'static str> {
        self.physical_settles += 1;
        use super::program_graph::ComponentState;
        let mut changed_nodes = Vec::new();
        for _ in 0..self.state.components.len().saturating_mul(16).max(1) {
            let mut changed = false;
            for &(node_index, ref samples) in self.wire_samples.iter() {
                let mut block_power = 0;
                let mut wire_power = 0;
                for sample in samples {
                    let strength = if sample.dust {
                        self.site_power(sample.site, super::program_graph::PowerPort::Stored, 0)?
                    } else if site_block(&self.program, &self.state, sample.site)?.is_solid() {
                        self.site_power(sample.site, super::program_graph::PowerPort::Conducted { include_dust: false }, 0)?
                    } else {
                        self.site_power(sample.site, super::program_graph::PowerPort::Weak(sample.face), 0)?
                    };
                    if sample.dust {
                        wire_power = wire_power.max(strength);
                    } else {
                        block_power = block_power.max(strength);
                    }
                }
                let power = block_power.max(wire_power.saturating_sub(1));
                let ComponentState::Wire(wire) = &mut self.state.components[node_index] else {
                    return Err("wire state is unavailable");
                };
                if wire.power != power {
                    wire.power = power;
                    changed = true;
                    changed_nodes.push(node_index);
                }
            }
            if !changed {
                return Ok(changed_nodes);
            }
        }
        Err("source signal feedback did not settle")
    }

    fn update_wires(&mut self) -> Result<Vec<usize>, &'static str> {
        if self.dust_corridor.is_none() {
            return self.settle_wires();
        }
        let mut corridor = self.dust_corridor.take().unwrap();
        let result = (|| {
            for (index, source) in corridor.plan.sources.iter().enumerate() {
                let value = self.site_power(source.site, source.port, 0)?;
                if corridor.values[index] != value {
                    corridor.values[index] = value;
                    for &(node, _) in &corridor.plan.outgoing[corridor.plan.source_ranges[index].clone()] {
                        if !corridor.affected[node] {
                            corridor.affected[node] = true;
                            corridor.affected_nodes.push(node);
                        }
                        corridor.operations += 1;
                    }
                }
            }
            let mut changed = Vec::new();
            corridor.affected_nodes.sort_unstable();
            for node in corridor.affected_nodes.drain(..) {
                corridor.affected[node] = false;
                let mut power = 0;
                for &(source, attenuation) in &corridor.plan.incoming[corridor.plan.receiver_ranges[node].clone()] {
                    power = power.max(corridor.values[source].saturating_sub(attenuation));
                    corridor.operations += 1;
                }
                let super::program_graph::ComponentState::Wire(wire) = &mut self.state.components[node] else {
                    return Err("wire state is unavailable");
                };
                if wire.power != power {
                    wire.power = power;
                    changed.push(node);
                }
            }
            Ok(changed)
        })();
        self.dust_corridor = Some(corridor);
        result
    }

    pub(crate) fn enable_dust_corridor(&mut self) -> Result<(), &'static str> {
        if self.dust_corridor.is_some() {
            return Err("dust corridor already enabled");
        }
        let corridor = dust_corridor::Corridor::compile(&self.program, &self.state, &self.wire_samples)?;
        self.dust_corridor = Some(corridor);
        let before = self.state.clone();
        if let Err(error) = self.update_wires() {
            self.state = before;
            self.dust_corridor = None;
            return Err(error);
        }
        if self.state != before {
            self.state = before;
            self.dust_corridor = None;
            return Err("dust entry power is not settled");
        }
        Ok(())
    }

    pub(crate) fn lever_power(&self, position: mchprs_blocks::BlockPos) -> Option<bool> {
        use super::components::NodeType;
        use super::program_graph::{CircuitNodeKind, ComponentState};
        let index = self
            .program
            .nodes()
            .iter()
            .position(|node| node.position == Some(position) && matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Lever)))?;
        match self.state.components.get(index)? {
            ComponentState::Electrical(state) => Some(state.powered),
            _ => None,
        }
    }

    pub(crate) fn dust_corridor_work(&self) -> (u64, u64) {
        (self.dust_corridor.as_ref().map_or(0, |plan| plan.operations), self.physical_settles)
    }

    #[cfg(test)]
    pub(crate) fn dust_corridor_resource_observers(&self) -> usize {
        self.dust_corridor.as_ref().map_or(0, |corridor| corridor.plan.resource_observers.len())
    }

    fn schedule_torches(&mut self) -> Result<(), &'static str> {
        use super::program_graph::ComponentState;
        for &node_index in self.torches.iter() {
            let input = self.input_power(node_index, super::program_graph::InputPort::Main)?;
            let ComponentState::Electrical(state) = &self.state.components[node_index] else {
                return Err("torch state is unavailable");
            };
            if state.powered != (input == 0) {
                if !self.scheduler.contains(&node_index) {
                    self.scheduler.schedule_half_tick(node_index, 2, mchprs_world::TickPriority::Normal);
                    let ComponentState::Electrical(state) = &mut self.state.components[node_index] else {
                        return Err("torch state is unavailable");
                    };
                    state.pending_tick = true;
                }
            }
        }
        Ok(())
    }

    fn output_routes(&self, nodes: Vec<usize>) -> Vec<usize> {
        // These are Graph-style node output notifications (`fire_route(node)` with no captured
        // site buffer); callback guards must use that exact no-capture context.
        let mut routes = Vec::new();
        for node in nodes {
            let route = match self.program.nodes()[node].kind {
                super::program_graph::CircuitNodeKind::Wire(index) => self.program.wires()[index].turbo_route,
                _ => self.program.nodes()[node].output_route,
            };
            if let Some(route) = route.map(|route| route.index()) {
                routes.push(route);
            }
        }
        routes
    }

    fn input_power(&self, node: usize, port: super::program_graph::InputPort) -> Result<u8, &'static str> {
        if let Some(links) = self.input_links.get(node, port) {
            let mut maximum = 0;
            for link in links {
                let mut enabled = true;
                for &guard in &link.guards {
                    if !guard_matches(guard, &self.program, &self.state, 0)? {
                        enabled = false;
                        break;
                    }
                }
                if enabled {
                    maximum = maximum.max(self.site_power(link.source, link.port, 0)?.saturating_sub(link.attenuation));
                }
            }
            return Ok(maximum);
        }
        self.input_power_physical(node, port)
    }

    #[cfg(test)]
    pub(crate) fn diagnostic_input_power(&self, node: usize, port: super::program_graph::InputPort) -> Result<u8, &'static str> {
        self.input_power(node, port)
    }

    #[cfg(test)]
    pub(crate) fn diagnostic_scheduled(&self, node: usize) -> Vec<(usize, mchprs_world::TickPriority)> {
        self.scheduler
            .iter()
            .filter_map(|(item, delay, priority)| (*item == node).then_some((delay, priority)))
            .collect()
    }

    fn input_power_physical(&self, node: usize, port: super::program_graph::InputPort) -> Result<u8, &'static str> {
        use super::program_graph::PowerTarget;
        self.program
            .power_links_to(PowerTarget::Input {
                node: super::program_graph::NodeId::new(node as u32),
                port,
            })
            .iter()
            .try_fold(0, |maximum, link| {
                if !guard_matches(link.guard, &self.program, &self.state, 0)? {
                    return Ok(maximum);
                }
                self.site_power(link.from, link.from_port, 0)
                    .map(|power| maximum.max(power.saturating_sub(link.attenuation)))
            })
    }

    fn site_power(&self, site: super::program_graph::SiteId, port: super::program_graph::PowerPort, depth: usize) -> Result<u8, &'static str> {
        use super::program_graph::{PowerPort, PowerTarget, SiteSource};
        if depth > 32 {
            return Err("conducted power dependency is cyclic");
        }
        if let PowerPort::Conducted { include_dust } = port {
            return self
                .program
                .power_links_to(PowerTarget::Conductor { site, include_dust })
                .iter()
                .try_fold(0, |maximum, link| {
                    if !guard_matches(link.guard, &self.program, &self.state, 0)? {
                        return Ok(maximum);
                    }
                    self.site_power(link.from, link.from_port, depth + 1)
                        .map(|power| maximum.max(power.saturating_sub(link.attenuation)))
                });
        }
        let spec = &self.program.sites()[site.index()];
        let (mut block, strength) = match spec.source {
            SiteSource::Component(node) => (self.component_block(node.index())?, self.component_strength(node.index())?),
            SiteSource::Constant(profile) => (
                self.program.profile_block(profile).ok_or("constant profile is unavailable")?,
                spec.source_strength,
            ),
            SiteSource::Contact(_) => {
                let block = site_block(&self.program, &self.state, site)?;
                let strength = if matches!(block, mchprs_blocks::blocks::Block::RedstoneBlock) {
                    15
                } else {
                    0
                };
                (block, strength)
            }
        };
        use super::program_graph::PowerPort::*;
        if matches!(port, Weak(_) | Strong(_))
            && let (SiteSource::Component(node), mchprs_blocks::blocks::Block::RedstoneWire { wire }) = (spec.source, block)
            && let super::program_graph::CircuitNodeKind::Wire(index) = self.program.nodes()[node.index()].kind
        {
            let plan = &self.program.wires()[index];
            let mut neighbors = [mchprs_blocks::blocks::Block::Air; 24];
            for (slot, site) in plan.canonical_neighbors.iter().enumerate() {
                neighbors[slot] = site_block(&self.program, &self.state, *site)?;
            }
            let raw = crate::redstone::wire::get_raw_sides_from_neighbor_read(wire, |slot| neighbors[slot]);
            block = mchprs_blocks::blocks::Block::RedstoneWire {
                wire: crate::redstone::wire::regulate_sides(wire, raw),
            };
        }
        Ok(match port {
            Output | Stored => strength,
            Weak(face) if crate::redpiler::power_rules::emits_weak(block, face) => strength,
            Strong(face) if crate::redpiler::power_rules::emits_strong(block, face) => strength,
            _ => 0,
        })
    }

    fn component_block(&self, node: usize) -> Result<mchprs_blocks::blocks::Block, &'static str> {
        use super::program_graph::{CircuitNodeKind, ComponentState};
        let spec = self.program.nodes().get(node).ok_or("component source is unavailable")?;
        if matches!(spec.kind, CircuitNodeKind::Wire(_)) {
            let ComponentState::Wire(wire) = self.state.components[node] else {
                return Err("wire state is unavailable");
            };
            return Ok(mchprs_blocks::blocks::Block::RedstoneWire { wire });
        }
        spec.block.ok_or("component block state is unavailable")
    }

    fn component_strength(&self, node: usize) -> Result<u8, &'static str> {
        use super::program_graph::ComponentState;
        match self.state.components.get(node).ok_or("component state is unavailable")? {
            ComponentState::Electrical(state) => Ok(state.output_strength),
            ComponentState::Wire(wire) => Ok(wire.power),
            ComponentState::Specialized => Ok(0),
        }
    }

    fn repeater_delay(&self, node: usize) -> u64 {
        match self.program.nodes()[node].kind {
            super::program_graph::CircuitNodeKind::Electrical(super::components::NodeType::Repeater { delay, .. }) => u64::from(delay) * 2,
            _ => 1,
        }
    }

    fn comparator_input(&self, node: usize) -> Result<u8, &'static str> {
        let id = super::program_graph::NodeId::new(node as u32);
        let plan = self.program.comparator_input(id).ok_or("comparator input plan is unavailable")?;
        if let Some(source) = plan.direct_override {
            return self.comparator_override(source);
        }
        let rear = self.input_power(node, super::program_graph::InputPort::Main)?;
        if rear >= plan.far_threshold {
            return Ok(rear);
        }
        if let Some((_, source, guard)) = plan.far_override
            && guard_matches(guard, &self.program, &self.state, 0)?
        {
            return self.comparator_override(source);
        }
        Ok(rear)
    }

    fn comparator_override(&self, source: super::program_graph::ComparatorOverride) -> Result<u8, &'static str> {
        use super::program_graph::{ComparatorOverride, ComponentState};
        match source {
            ComparatorOverride::Literal(strength) => Ok(strength),
            ComparatorOverride::LiveComponentOutput(node) => match self.state.components.get(node.index()) {
                Some(ComponentState::Electrical(state)) => Ok(state.output_strength),
                Some(ComponentState::Wire(wire)) => Ok(wire.power),
                Some(ComponentState::Specialized) => Err("comparator override uses an unsupported specialized source"),
                None => Err("comparator override source is unavailable"),
            },
        }
    }

    fn comparator_output(&self, node: usize) -> Result<u8, &'static str> {
        let input = self.comparator_input(node)?;
        let side = self.input_power(node, super::program_graph::InputPort::Side)?;
        let super::program_graph::CircuitNodeKind::Electrical(super::components::NodeType::Comparator { mode, .. }) = self.program.nodes()[node].kind
        else {
            return Err("comparator node kind is unavailable");
        };
        Ok(crate::redpiler::power_rules::comparator_output_strength(input, side, mode))
    }

    fn comparator_powered(&self, node: usize) -> Result<bool, &'static str> {
        let input = self.comparator_input(node)?;
        let side = self.input_power(node, super::program_graph::InputPort::Side)?;
        let super::program_graph::CircuitNodeKind::Electrical(super::components::NodeType::Comparator { mode, .. }) = self.program.nodes()[node].kind
        else {
            return Err("comparator node kind is unavailable");
        };
        Ok(crate::redpiler::power_rules::comparator_is_powered(input, side, mode))
    }
}

fn guard_matches(
    id: super::program_graph::GuardId,
    program: &super::program_graph::CircuitProgram,
    state: &super::program_graph::CircuitState,
    depth: usize,
) -> Result<bool, &'static str> {
    use super::program_graph::{Guard, OperandView, SiteTest};
    if depth > 32 {
        return Err("signal guard is cyclic");
    }
    Ok(match &program.guards()[id.index()] {
        Guard::Always => true,
        Guard::State(test) => state.matches(*test),
        Guard::Not(inner) => !guard_matches(*inner, program, state, depth + 1)?,
        Guard::All(items) => {
            let mut matches = true;
            for item in items.iter() {
                matches &= guard_matches(*item, program, state, depth + 1)?;
            }
            matches
        }
        Guard::Any(items) => {
            let mut matches = false;
            for item in items.iter() {
                matches |= guard_matches(*item, program, state, depth + 1)?;
            }
            matches
        }
        Guard::Site { site, view, test } => {
            if *view == OperandView::Captured {
                // These node-output routes carry no observed-site capture, matching Graph's
                // missing-capture semantics for source and scheduled component notifications.
                false
            } else {
                let block = site_block(program, state, *site)?;
                match test {
                    SiteTest::Solid => block.is_solid(),
                    SiteTest::Transparent => block.is_transparent(),
                    SiteTest::IsWire => matches!(block, mchprs_blocks::blocks::Block::RedstoneWire { .. }),
                    SiteTest::Profile(profile) => program.profile_block(*profile).is_some_and(|profile| profile == block),
                }
            }
        }
    })
}

fn guard_matches_event(
    id: super::program_graph::GuardId,
    program: &super::program_graph::CircuitProgram,
    state: &super::program_graph::CircuitState,
    captures: &[CapturedSite],
    depth: usize,
) -> Result<bool, &'static str> {
    use super::program_graph::{Guard, OperandView, SiteTest};
    if depth > 32 {
        return Err("signal guard is cyclic");
    }
    Ok(match &program.guards()[id.index()] {
        Guard::Always => true,
        Guard::State(test) => state.matches(*test),
        Guard::Not(inner) => !guard_matches_event(*inner, program, state, captures, depth + 1)?,
        Guard::All(items) => {
            let mut matches = true;
            for item in items.iter() {
                matches &= guard_matches_event(*item, program, state, captures, depth + 1)?;
            }
            matches
        }
        Guard::Any(items) => {
            let mut matches = false;
            for item in items.iter() {
                matches |= guard_matches_event(*item, program, state, captures, depth + 1)?;
            }
            matches
        }
        Guard::Site { site, view, test } => {
            let captured = if *view == OperandView::Captured {
                captures.iter().find(|capture| capture.site == *site)
            } else {
                None
            };
            if *view == OperandView::Captured && captured.is_none() {
                false
            } else {
                let block = match captured {
                    Some(capture) => capture.block,
                    None => site_block(program, state, *site)?,
                };
                match test {
                    SiteTest::Solid => block.is_solid(),
                    SiteTest::Transparent => block.is_transparent(),
                    SiteTest::IsWire => matches!(block, mchprs_blocks::blocks::Block::RedstoneWire { .. }),
                    SiteTest::Profile(profile) => captured.is_some_and(|capture| capture.profile == Some(*profile)),
                }
            }
        }
    })
}
fn state_only_guard(id: super::program_graph::GuardId, guards: &[super::program_graph::Guard]) -> bool {
    use super::program_graph::Guard;
    match &guards[id.index()] {
        Guard::Always | Guard::State(_) => true,
        Guard::Not(inner) => state_only_guard(*inner, guards),
        Guard::All(items) | Guard::Any(items) => items.iter().all(|item| state_only_guard(*item, guards)),
        Guard::Site { .. } => false,
    }
}

#[cfg(test)]
mod state_guard_tests {
    use super::state_only_guard;
    use crate::redpiler::program_graph::{Guard, GuardId, OperandView, PistonId, SiteId, SiteTest, StateTest};

    #[test]
    fn composed_state_guards_are_admitted_but_site_guards_are_not() {
        let guards = vec![
            Guard::Always,
            Guard::State(StateTest::PistonExtended(PistonId::new(0))),
            Guard::All(vec![GuardId::new(0), GuardId::new(1)].into_boxed_slice()),
            Guard::Not(GuardId::new(2)),
            Guard::Site {
                site: SiteId::new(0),
                view: OperandView::Live,
                test: SiteTest::Solid,
            },
            Guard::Any(vec![GuardId::new(3), GuardId::new(4)].into_boxed_slice()),
        ];
        assert!(state_only_guard(GuardId::new(2), &guards));
        assert!(state_only_guard(GuardId::new(3), &guards));
        assert!(!state_only_guard(GuardId::new(5), &guards));
    }
}

fn site_block(
    program: &super::program_graph::CircuitProgram,
    state: &super::program_graph::CircuitState,
    site: super::program_graph::SiteId,
) -> Result<mchprs_blocks::blocks::Block, &'static str> {
    use super::program_graph::{CircuitNodeKind, ComponentState, SiteSource};
    let spec = &program.sites()[site.index()];
    match spec.source {
        SiteSource::Component(node) => {
            if matches!(program.nodes()[node.index()].kind, CircuitNodeKind::Wire(_)) {
                let ComponentState::Wire(wire) = state.components[node.index()] else {
                    return Err("wire state is unavailable");
                };
                Ok(mchprs_blocks::blocks::Block::RedstoneWire { wire })
            } else {
                program.nodes()[node.index()].block.ok_or("component block state is unavailable")
            }
        }
        SiteSource::Constant(profile) => program.profile_block(profile).ok_or("constant profile is unavailable"),
        SiteSource::Contact(contact) => {
            for &resource in program.resources_at_contact(contact) {
                let resource_state = state.resources[resource.index()];
                if resource_state.enabled && resource_state.at == contact {
                    return program
                        .profile_block(program.resources()[resource.index()].material)
                        .ok_or("resource profile is unavailable");
                }
            }
            let contact = &program.contacts()[contact.index()];
            for variant in &contact.variants {
                if guard_matches(variant.when, program, state, 0)? {
                    return program.profile_block(variant.profile).ok_or("contact profile is unavailable");
                }
            }
            Err("contact has no active profile")
        }
    }
}

fn signal_site_may_emit(
    program: &super::program_graph::CircuitProgram,
    state: &super::program_graph::CircuitState,
    site: super::program_graph::SiteId,
    port: super::program_graph::PowerPort,
) -> Result<bool, &'static str> {
    use super::program_graph::PowerPort;
    let block = site_block(program, state, site)?;
    Ok(match port {
        PowerPort::Weak(face) => crate::redpiler::power_rules::emits_weak(block, face),
        PowerPort::Strong(face) => crate::redpiler::power_rules::emits_strong(block, face),
        PowerPort::Output | PowerPort::Stored => true,
        PowerPort::Conducted { .. } => false,
    })
}

fn validate_power_source(
    program: &super::program_graph::CircuitProgram,
    site: super::program_graph::SiteId,
    port: super::program_graph::PowerPort,
    depth: usize,
) -> Result<(), &'static str> {
    use super::program_graph::{PowerPort, PowerTarget};
    if depth > 32 {
        return Err("conducted power dependency is cyclic");
    }
    if let super::program_graph::SiteSource::Contact(contact) = program.sites()[site.index()].source {
        if program.contacts()[contact.index()]
            .variants
            .iter()
            .any(|variant| !state_only_guard(variant.when, program.guards()))
        {
            return Err("signal source depends on an unrepresented movable contact");
        }
    }
    if let PowerPort::Conducted { include_dust } = port {
        for link in program.power_links_to(PowerTarget::Conductor { site, include_dust }) {
            if !state_only_guard(link.guard, program.guards()) {
                return Err("conducted power link has an unsupported site guard");
            }
            validate_power_source(program, link.from, link.from_port, depth + 1)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod lamp_tests {
    use super::*;
    use crate::plot::PlotWorld;
    use crate::redpiler::backend::graph::wire_probe::physical_program;
    use crate::redpiler::program_graph::ComponentState;
    use crate::redstone;
    use crate::world::World;
    use crate::world::storage::Chunk;
    use mchprs_blocks::blocks::{Block, Lever, LeverFace};
    use mchprs_blocks::{BlockDirection, BlockFace, BlockPos};

    const SOURCE: BlockPos = BlockPos::new(8, 8, 7);
    const LAMP: BlockPos = BlockPos::new(8, 8, 8);

    fn set_source(native: &mut PlotWorld, network: &mut PhysicalSignalNetwork, powered: bool) {
        network.set_lever(SOURCE, powered).unwrap();
        let Block::Lever { mut lever } = native.get_block(SOURCE) else {
            panic!("missing source")
        };
        lever.powered = powered;
        native.set_block(SOURCE, Block::Lever { lever });
        redstone::update_surrounding_blocks(native, SOURCE);
        redstone::update_surrounding_blocks(native, SOURCE.offset(BlockFace::Bottom));
    }

    fn assert_matches(native: &PlotWorld, network: &PhysicalSignalNetwork, lamp: usize) {
        let Block::RedstoneLamp { lit } = native.get_block(LAMP) else {
            panic!("missing lamp")
        };
        let ComponentState::Electrical(state) = &network.state().components[lamp] else {
            panic!("missing lamp state")
        };
        assert_eq!(state.powered, lit, "lamp at tick {}", network.current_tick());
        assert_eq!(state.pending_tick, native.scheduler().iter_entries().any(|tick| tick.pos == LAMP));
    }

    fn advance(native: &mut PlotWorld, network: &mut PhysicalSignalNetwork, lamp: usize, ticks: usize) {
        for _ in 0..ticks {
            network.advance_tick().unwrap();
            native.tick_interpreted();
            assert_matches(native, network, lamp);
        }
    }

    #[test]
    fn lamp_drop_delay_and_repower_match_native() {
        let mut native = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
        native.set_block(SOURCE.offset(BlockFace::Bottom), Block::Stone {});
        native.set_block(
            SOURCE,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
            },
        );
        native.set_block(LAMP, Block::RedstoneLamp { lit: false });
        let bounds = (BlockPos::new(5, 5, 5), BlockPos::new(11, 10, 11));
        let (program, state, _) = physical_program(&native, bounds).unwrap();
        let lamp = program.position_index()[&LAMP].index();
        let source = program.position_index()[&SOURCE];
        let source_route = program.nodes()[source.index()].output_route.unwrap().index();
        let mut network = PhysicalSignalNetwork::compile(&program, &state, &EntryWork::capture(&native, bounds)).unwrap();

        set_source(&mut native, &mut network, true);
        assert_matches(&native, &network, lamp);
        set_source(&mut native, &mut network, false);
        assert_eq!(
            network.scheduler.iter().find(|(node, _, _)| **node == lamp).map(|(_, delay, _)| delay),
            Some(4)
        );
        advance(&mut native, &mut network, lamp, 1);
        set_source(&mut native, &mut network, true);
        advance(&mut native, &mut network, lamp, 3);
        assert!(matches!(native.get_block(LAMP), Block::RedstoneLamp { lit: true }));

        set_source(&mut native, &mut network, false);
        advance(&mut native, &mut network, lamp, 1);
        network.deliver_callback_routes(&[source_route]).unwrap();
        redstone::update_surrounding_blocks(&mut native, SOURCE);
        assert_eq!(
            network.scheduler.iter().find(|(node, _, _)| **node == lamp).map(|(_, delay, _)| delay),
            Some(3),
            "repeated callbacks preserve the original due time"
        );
        advance(&mut native, &mut network, lamp, 3);
        assert!(matches!(native.get_block(LAMP), Block::RedstoneLamp { lit: false }));
    }
}

#[cfg(test)]
mod comparator_tests {
    use super::*;
    use crate::plot::PlotWorld;
    use crate::redstone;
    use crate::world::World;
    use crate::world::storage::Chunk;
    use mchprs_blocks::block_entities::BlockEntity;
    use mchprs_blocks::blocks::{Block, ComparatorMode, Lever, LeverFace, RedstoneComparator};
    use mchprs_blocks::{BlockFace, BlockPos};

    const COMPARATOR: BlockPos = BlockPos::new(8, 8, 8);
    const SOURCE: BlockPos = BlockPos::new(8, 8, 7);
    const BOUNDS: (BlockPos, BlockPos) = (BlockPos::new(5, 5, 5), BlockPos::new(11, 10, 11));

    fn world(mode: ComparatorMode) -> PlotWorld {
        let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
        world.set_block(COMPARATOR.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(SOURCE.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(
            COMPARATOR,
            Block::RedstoneComparator {
                comparator: RedstoneComparator::new(mchprs_blocks::BlockDirection::North, mode, false),
            },
        );
        world.set_block(COMPARATOR.offset(BlockFace::East), Block::RedstoneBlock {});
        world.set_block(
            SOURCE,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, mchprs_blocks::BlockDirection::North, false),
            },
        );
        world
    }

    fn set_source(world: &mut PlotWorld, powered: bool) {
        let Block::Lever { mut lever } = world.get_block(SOURCE) else {
            panic!("source lever is missing")
        };
        lever.powered = powered;
        world.set_block(SOURCE, Block::Lever { lever });
        redstone::update_surrounding_blocks(world, SOURCE);
        redstone::update_surrounding_blocks(world, SOURCE.offset(BlockFace::Bottom));
    }

    fn comparator_state(network: &PhysicalSignalNetwork, program: &super::super::program_graph::CircuitProgram) -> (bool, u8) {
        use super::super::program_graph::{CircuitNodeKind, ComponentState};
        let node = program
            .nodes()
            .iter()
            .position(|node| {
                node.position == Some(COMPARATOR)
                    && matches!(
                        node.kind,
                        CircuitNodeKind::Electrical(crate::redpiler::components::NodeType::Comparator { .. })
                    )
            })
            .unwrap();
        let ComponentState::Electrical(state) = &network.state().components[node] else {
            panic!("comparator state is missing")
        };
        (state.powered, state.output_strength)
    }

    fn assert_matches_native(network: &PhysicalSignalNetwork, program: &super::super::program_graph::CircuitProgram, native: &PlotWorld) {
        let actual = comparator_state(network, program);
        let Block::RedstoneComparator { comparator } = native.get_block(COMPARATOR) else {
            panic!("comparator is missing")
        };
        let native_output = match native.get_block_entity(COMPARATOR) {
            Some(BlockEntity::Comparator { output_strength }) => *output_strength,
            _ => 0,
        };
        assert_eq!(actual.0, comparator.powered);
        assert_eq!(actual.1, native_output);
    }

    #[test]
    fn comparator_modes_and_short_pulse_match_graph_tick_boundary() {
        use crate::redpiler::backend::graph::wire_probe::physical_program;

        for mode in [ComparatorMode::Compare, ComparatorMode::Subtract] {
            let initial = world(mode);
            let (program, state, _) = physical_program(&initial, BOUNDS).unwrap();
            let mut network = PhysicalSignalNetwork::compile(&program, &state, &EntryWork::capture(&initial, BOUNDS)).unwrap();
            let mut native = initial.activation_snapshot();

            network.set_lever(SOURCE, true).unwrap();
            set_source(&mut native, true);
            let scheduled = native.scheduler().iter_entries().find(|entry| entry.pos == COMPARATOR);
            if mode == ComparatorMode::Compare {
                assert_eq!(scheduled.unwrap().ticks_left, 2);
                assert!(network.has_pending_comparator_work());
            } else {
                assert!(scheduled.is_none(), "equal rear/side strength produces zero subtract output");
                assert!(!network.has_pending_comparator_work());
            }
            for elapsed in 1..=2 {
                network.advance_tick().unwrap();
                native.tick_interpreted();
                if elapsed == 1 && mode == ComparatorMode::Compare {
                    assert_eq!(
                        native
                            .scheduler()
                            .iter_entries()
                            .find(|entry| entry.pos == COMPARATOR)
                            .unwrap()
                            .ticks_left,
                        1
                    );
                    assert!(network.has_pending_comparator_work());
                }
            }
            assert!(!network.has_pending_comparator_work());
            assert_matches_native(&network, &program, &native);

            if mode == ComparatorMode::Compare {
                let mut pulse_network = PhysicalSignalNetwork::compile(&program, &state, &EntryWork::capture(&initial, BOUNDS)).unwrap();
                let mut pulse_native = initial.activation_snapshot();
                pulse_network.set_lever(SOURCE, true).unwrap();
                set_source(&mut pulse_native, true);
                pulse_network.set_lever(SOURCE, false).unwrap();
                set_source(&mut pulse_native, false);
                assert!(pulse_network.has_pending_comparator_work(), "pulse reversal preserves queued work");
                assert_eq!(
                    pulse_native
                        .scheduler()
                        .iter_entries()
                        .find(|entry| entry.pos == COMPARATOR)
                        .unwrap()
                        .ticks_left,
                    2
                );
                for elapsed in 1..=2 {
                    pulse_network.advance_tick().unwrap();
                    pulse_native.tick_interpreted();
                    if elapsed == 1 {
                        assert_eq!(
                            pulse_native
                                .scheduler()
                                .iter_entries()
                                .find(|entry| entry.pos == COMPARATOR)
                                .unwrap()
                                .ticks_left,
                            1
                        );
                        assert!(pulse_network.has_pending_comparator_work());
                    }
                }
                assert!(!pulse_network.has_pending_comparator_work());
                assert_matches_native(&pulse_network, &program, &pulse_native);
            }
        }
    }
}
