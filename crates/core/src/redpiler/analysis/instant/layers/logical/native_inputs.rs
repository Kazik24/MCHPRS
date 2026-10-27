//! Source-certified native ownership around a settled computation's value boundary.
//!
//! Ordinary Graph remains authoritative for native actor/resource lifecycle. This
//! module closes the physical ownership groups and their actual power/callback
//! dependencies so frame execution never suppresses an uncovered native effect.
use super::*;
use crate::redpiler::analysis::instant::ownership::OwnershipClosure;

#[derive(Default)]
pub(super) struct NativeInputOwnership {
    pub(super) nodes: FxHashSet<NodeId>,
    pub(super) sites: FxHashSet<SiteId>,
    pub(super) contacts: FxHashSet<ContactId>,
    pub(super) resources: FxHashSet<ResourceId>,
}

pub(super) struct RetainedCellBoundary {
    pub(super) actors: FxHashSet<PistonId>,
    pub(super) resources: FxHashSet<ResourceId>,
    pub(super) contacts: FxHashSet<ContactId>,
    pub(super) nodes: FxHashSet<NodeId>,
    pub(super) activations: FxHashMap<RouteOccurrence, FxHashSet<PistonId>>,
    pub(super) restoration_notifications: FxHashMap<RouteOccurrence, FxHashSet<PistonId>>,
    pub(super) restoration_samples: FxHashMap<RouteOccurrence, FxHashSet<PistonId>>,
}

impl RetainedCellBoundary {
    pub(super) fn contains_source(&self, source: SiteSource) -> bool {
        match source {
            SiteSource::Component(node) => self.nodes.contains(&node),
            SiteSource::Contact(contact) => self.contacts.contains(&contact),
            SiteSource::Constant(_) => false,
        }
    }
}

pub(super) fn retained_cell_boundary(
    graph: &CompileGraph,
    circuit: &SettledCircuit,
    physical_ownership: &OwnershipClosure,
    timing: &SettledTimingIsland,
) -> Option<RetainedCellBoundary> {
    let mut boundary = RetainedCellBoundary {
        actors: FxHashSet::default(),
        resources: FxHashSet::default(),
        contacts: FxHashSet::default(),
        nodes: FxHashSet::default(),
        activations: FxHashMap::default(),
        restoration_notifications: FxHashMap::default(),
        restoration_samples: FxHashMap::default(),
    };
    let mut retained_observers = FxHashSet::default();
    for use_ in circuit.retained_cells.iter() {
        if !boundary.actors.insert(use_.actor) {
            return None;
        }
        if let Some(transfer) = use_.retraction.transfer {
            boundary.resources.insert(transfer.resource);
            boundary.contacts.extend([transfer.source, transfer.destination]);
        }
        match &use_.reset {
            CellReset::Observer { node, .. } => {
                retained_observers.insert(*node);
            }
            CellReset::Payload { resource, .. } => {
                boundary.resources.insert(*resource);
            }
            CellReset::InputRestoration { resource, contact, .. } => {
                boundary.resources.insert(*resource);
                boundary.contacts.insert(*contact);
            }
        }
        for &occurrence in &use_.activations {
            boundary.activations.entry(occurrence).or_default().insert(use_.actor);
        }
        if let CellReset::InputRestoration { notification, sample, .. } = &use_.reset {
            boundary.restoration_notifications.entry(*notification).or_default().insert(use_.actor);
            boundary.restoration_samples.entry(*sample).or_default().insert(use_.actor);
        }
    }

    let mut resource_groups = vec![None; graph.resources.len()];
    for (group_index, group) in physical_ownership.groups.iter().enumerate() {
        for &resource in &group.resources {
            *resource_groups.get_mut(resource.index())? = Some(group_index);
        }
    }
    let mut pending_groups = Vec::new();
    for &actor in &boundary.actors {
        pending_groups.push(physical_ownership.actors.get(actor.index()).copied().flatten()?);
    }
    for &resource in &boundary.resources {
        let group = resource_groups.get(resource.index()).copied().flatten()?;
        pending_groups.push(group);
    }
    for &contact in &boundary.contacts {
        let group = physical_ownership.contacts.get(contact.index()).copied().flatten()?;
        pending_groups.push(group);
    }
    let mut groups = FxHashSet::default();
    while let Some(group_index) = pending_groups.pop() {
        if !groups.insert(group_index) {
            continue;
        }
        let group = physical_ownership.groups.get(group_index)?;
        if group.actors.iter().any(|actor| !boundary.actors.contains(actor))
            || group.resources.iter().any(|resource| !boundary.resources.contains(resource))
        {
            return None;
        }
        for &actor in &group.actors {
            let piston = graph.pistons.get(actor.index())?;
            let node = piston.node;
            if timing.nodes.contains(&node) || timing.actor_specs.iter().any(|spec| spec.node == node) {
                return None;
            }
            let far = match piston.lane {
                PistonLane::SingleResource { far } => far,
                PistonLane::Empty => piston.near,
            };
            boundary.contacts.extend([piston.base, piston.near, far]);
            boundary.nodes.insert(node);
        }
        for &resource in &group.resources {
            boundary.contacts.extend(graph.resources.get(resource.index())?.contacts.iter().copied());
        }
    }
    for observer in retained_observers {
        if timing.nodes.contains(&observer) || timing.actor_specs.iter().any(|spec| spec.node == observer) {
            return None;
        }
        if !matches!(
            graph.nodes.get(observer.index())?.kind,
            CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. }
        ) {
            return None;
        }
        boundary.nodes.insert(observer);
    }
    Some(boundary)
}

struct Builder<'a> {
    graph: &'a CompileGraph,
    island: &'a LogicalIsland,
    expanded_components: &'a FxHashSet<NodeId>,
    physical_ownership: &'a OwnershipClosure,
    retained_cells: &'a RetainedCellBoundary,
    monitor: &'a TaskMonitor,
    ownership: NativeInputOwnership,
    resource_groups: Vec<Option<usize>>,
    node_routes: Vec<Vec<RouteId>>,
    checked_groups: FxHashSet<usize>,
    pending_nodes: Vec<NodeId>,
    checked_nodes: FxHashSet<NodeId>,
    pending_guards: Vec<GuardId>,
    checked_guards: FxHashSet<GuardId>,
    traced_targets: FxHashSet<PowerTarget>,
    pending_trace_targets: Vec<PowerTarget>,
    work: usize,
}

impl<'a> Builder<'a> {
    fn new(
        graph: &'a CompileGraph,
        island: &'a LogicalIsland,
        expanded_components: &'a FxHashSet<NodeId>,
        physical_ownership: &'a OwnershipClosure,
        retained_cells: &'a RetainedCellBoundary,
        monitor: &'a TaskMonitor,
    ) -> Option<Self> {
        let mut resource_groups = vec![None; graph.resources.len()];
        for (group_index, group) in physical_ownership.groups.iter().enumerate() {
            for &resource in &group.resources {
                *resource_groups.get_mut(resource.index())? = Some(group_index);
            }
        }
        let mut node_routes = vec![Vec::new(); graph.nodes.len()];
        for (index, route) in graph.routes.iter().enumerate() {
            if index & 0x3fff == 0 && monitor.cancelled() {
                return None;
            }
            node_routes.get_mut(route.source.index())?.push(RouteId::new(index as u32));
        }
        Some(Self {
            graph,
            island,
            expanded_components,
            physical_ownership,
            retained_cells,
            monitor,
            ownership: NativeInputOwnership::default(),
            resource_groups,
            node_routes,
            checked_groups: FxHashSet::default(),
            pending_nodes: Vec::new(),
            checked_nodes: FxHashSet::default(),
            pending_guards: Vec::new(),
            checked_guards: FxHashSet::default(),
            traced_targets: FxHashSet::default(),
            pending_trace_targets: Vec::new(),
            work: 0,
        })
    }

    fn charge(&mut self) -> Option<()> {
        self.work = self.work.checked_add(1)?;
        if self.work > super::MAX_LAYER_WORK {
            return None;
        }
        if self.work & 0x3fff == 0 && self.monitor.cancelled() {
            return None;
        }
        Some(())
    }

    fn add_node(&mut self, node: NodeId) -> Option<()> {
        if self.island.nodes.binary_search(&node).is_err() {
            return None;
        }
        if !self.checked_nodes.contains(&node) && self.ownership.nodes.insert(node) {
            self.pending_nodes.push(node);
        }
        Some(())
    }

    fn add_contact_node(&mut self, contact: ContactId) -> Option<()> {
        self.ownership.contacts.insert(contact);
        let node = self.graph.contacts.get(contact.index())?.node;
        self.add_node(node)
    }

    fn add_group(&mut self, group_index: usize) -> Option<()> {
        if !self.checked_groups.insert(group_index) {
            return Some(());
        }
        self.charge()?;
        let group = self.physical_ownership.groups.get(group_index)?;
        let actors = group.actors.clone();
        let resources = group.resources.clone();
        for actor in actors {
            self.charge()?;
            let piston = *self.graph.pistons.get(actor.index())?;
            self.add_node(piston.node)?;
            let far = match piston.lane {
                PistonLane::SingleResource { far } => far,
                PistonLane::Empty => piston.near,
            };
            for contact in [piston.base, piston.near, far] {
                self.add_contact_node(contact)?;
            }
        }
        for resource in resources {
            self.charge()?;
            self.ownership.resources.insert(resource);
            let contacts = self.graph.resources.get(resource.index())?.contacts.clone();
            for contact in contacts.iter().copied() {
                self.add_contact_node(contact)?;
            }
        }
        Some(())
    }

    fn add_actor(&mut self, actor: PistonId) -> Option<()> {
        if self.retained_cells.actors.contains(&actor) {
            return Some(());
        }
        let group_index = self.physical_ownership.actors.get(actor.index()).copied().flatten()?;
        self.add_group(group_index)
    }

    fn add_resource(&mut self, resource: ResourceId) -> Option<()> {
        if self.retained_cells.resources.contains(&resource) {
            return Some(());
        }
        let group_index = self.resource_groups.get(resource.index()).copied().flatten()?;
        self.add_group(group_index)
    }

    fn add_contact(&mut self, contact: ContactId) -> Option<()> {
        if self.retained_cells.contacts.contains(&contact) {
            return Some(());
        }
        let group_index = self.physical_ownership.contacts.get(contact.index()).copied().flatten();
        if let Some(group_index) = group_index {
            self.add_group(group_index)?;
        }
        self.add_contact_node(contact)
    }

    fn add_site(&mut self, site: SiteId) -> Option<()> {
        match self.graph.sites.get(site.index())?.source {
            SiteSource::Component(node) => {
                if self.retained_cells.nodes.contains(&node) {
                    return Some(());
                }
                self.ownership.sites.insert(site);
                self.add_source_node(node)
            }
            SiteSource::Contact(contact) => {
                if self.retained_cells.contacts.contains(&contact) {
                    return Some(());
                }
                self.ownership.sites.insert(site);
                self.add_contact(contact)
            }
            SiteSource::Constant(_) => Some(()),
        }
    }

    fn add_guard(&mut self, root: GuardId) -> Option<()> {
        self.pending_guards.push(root);
        while let Some(id) = self.pending_guards.pop() {
            if !self.checked_guards.insert(id) {
                continue;
            }
            self.charge()?;
            match self.graph.guards.get(id.index())?.clone() {
                Guard::Always => {}
                Guard::State(test) => self.add_state(test)?,
                Guard::Site { site, .. } => self.add_site(site)?,
                Guard::Not(child) => self.pending_guards.push(child),
                Guard::All(children) | Guard::Any(children) => self.pending_guards.extend(children.iter().copied()),
            }
        }
        Some(())
    }

    fn add_state(&mut self, test: StateTest) -> Option<()> {
        match test {
            StateTest::PistonExtended(actor) | StateTest::PistonHeadPresent(actor) | StateTest::PistonAvailability { piston: actor, .. } => {
                self.add_actor(actor)
            }
            StateTest::ResourceAt { resource, contact } => {
                self.add_resource(resource)?;
                self.add_contact(contact)
            }
            StateTest::ResourceEnabled(resource) => self.add_resource(resource),
        }
    }

    fn add_power_trace(&mut self, target: PowerTarget) -> Option<()> {
        if self.traced_targets.insert(target) {
            self.pending_trace_targets.push(target);
        }
        Some(())
    }

    fn drain_power_traces(&mut self) -> Option<()> {
        let targets = std::mem::take(&mut self.pending_trace_targets);
        let trace = match super::trace_native_power_targets(self.graph, &targets, self.monitor) {
            Ok(trace) => trace,
            Err(_) => return None,
        };
        for site in trace.source_sites {
            self.add_site(site)?;
        }
        for input in trace.inputs {
            match input {
                LayerInputSource::SourceAbove { site, .. } => self.add_site(site)?,
                LayerInputSource::WireDot { node } if matches!(self.graph.nodes.get(node.index())?.kind, CircuitNodeKind::Wire(_)) => {
                    self.add_node(node)?;
                }
                LayerInputSource::State(test) | LayerInputSource::BoundaryState { test, .. } => self.add_state(test)?,
                _ => return None,
            }
        }
        for node in trace.wires {
            if !matches!(self.graph.nodes.get(node.index())?.kind, CircuitNodeKind::Wire(_)) {
                return None;
            }
            self.add_node(node)?;
        }
        Some(())
    }

    fn add_source_node(&mut self, node: NodeId) -> Option<()> {
        if self.retained_cells.nodes.contains(&node) {
            return Some(());
        }
        self.add_node(node)?;
        match self.graph.nodes.get(node.index())?.kind.clone() {
            CircuitNodeKind::Piston(actor) => self.add_actor(actor),
            CircuitNodeKind::Contact(contact) => self.add_contact(contact),
            CircuitNodeKind::Wire(_) => self.add_power_trace(PowerTarget::Input { node, port: InputPort::Main }),
            CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. } => Some(()),
            CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::PressurePlate | NodeType::Constant) => Some(()),
            CircuitNodeKind::Electrical(_) => {
                for port in [InputPort::Main, InputPort::Side, InputPort::Quasi] {
                    self.add_power_trace(PowerTarget::Input { node, port })?;
                }
                Some(())
            }
        }
    }

    fn add_delivery(&mut self, occurrence: RouteOccurrence, delivery: &RouteDelivery) -> Option<()> {
        let receiver = delivery
            .receiver
            .or_else(|| match (self.graph.sites.get(delivery.target.index())?.source, delivery.kind) {
                (SiteSource::Component(node), DeliveryKind::Update { .. }) => Some(CallbackReceiver::Component(node)),
                _ => None,
            });
        let Some(receiver) = receiver else { return Some(()) };
        let node = match receiver {
            CallbackReceiver::Component(node) => node,
            CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor) => self.graph.pistons.get(actor.index())?.node,
            CallbackReceiver::Contact(contact) => self.graph.contacts.get(contact.index())?.node,
        };
        if matches!(delivery.kind, DeliveryKind::Shape { .. })
            && matches!(&self.graph.nodes.get(node.index())?.kind, CircuitNodeKind::Wire(_))
            && crate::redpiler::program_graph::WireShape::shape_callbacks_are_inert(self.graph, node)
        {
            return Some(());
        }
        if let Some(actors) = self.retained_cells.restoration_samples.get(&occurrence) {
            if !matches!(delivery.kind, DeliveryKind::Update { .. }) {
                return None;
            }
            let receiver_is_sampled_actor = match receiver {
                CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor) => actors.contains(&actor),
                CallbackReceiver::Component(node) => {
                    matches!(self.graph.nodes.get(node.index())?.kind, CircuitNodeKind::Piston(actor) if actors.contains(&actor))
                }
                CallbackReceiver::Contact(_) => false,
            };
            if !receiver_is_sampled_actor {
                return None;
            }
            self.add_guard(delivery.guard)?;
            return Some(());
        }
        if let Some(actors) = self.retained_cells.restoration_notifications.get(&occurrence) {
            if !matches!(delivery.kind, DeliveryKind::Update { .. }) {
                return None;
            }
            let is_cell_sample = match receiver {
                CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor) => actors.contains(&actor),
                CallbackReceiver::Component(node) => {
                    matches!(self.graph.nodes.get(node.index())?.kind, CircuitNodeKind::Piston(actor) if actors.contains(&actor))
                }
                CallbackReceiver::Contact(_) => false,
            } && self
                .retained_cells
                .restoration_samples
                .get(&occurrence)
                .is_some_and(|samples| actors.iter().any(|actor| samples.contains(actor)));
            let notification_is_wire =
                matches!(receiver, CallbackReceiver::Component(wire) if matches!(self.graph.nodes.get(wire.index())?.kind, CircuitNodeKind::Wire(_)));
            if !is_cell_sample && !notification_is_wire {
                return None;
            }
            self.add_guard(delivery.guard)?;
            if notification_is_wire {
                self.add_source_node(node)?;
            }
            return Some(());
        }
        if let Some(actors) = self.retained_cells.activations.get(&occurrence) {
            if !matches!(delivery.kind, DeliveryKind::Update { .. }) {
                return None;
            }
            let certified_receiver = match receiver {
                CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor) => actors.contains(&actor),
                CallbackReceiver::Component(node) => {
                    matches!(self.graph.nodes.get(node.index())?.kind, CircuitNodeKind::Piston(actor) if actors.contains(&actor))
                }
                CallbackReceiver::Contact(contact) => self.retained_cells.contacts.contains(&contact),
            };
            if !certified_receiver {
                return None;
            }
            // This exact callback is a retained-cell activation. Graph still
            // checks its guard/capture and samples the physical actor; only the
            // typed cell contract authorizes stopping the ownership walk here.
            self.add_guard(delivery.guard)?;
            if let CallbackReceiver::Contact(contact) = receiver {
                self.add_contact(contact)?;
            }
            return Some(());
        }
        let retained_receiver = match receiver {
            CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor) => self.retained_cells.actors.contains(&actor),
            CallbackReceiver::Component(node) => self.retained_cells.nodes.contains(&node),
            CallbackReceiver::Contact(contact) => self.retained_cells.contacts.contains(&contact),
        };
        if retained_receiver {
            return None;
        }
        if !self.ownership.nodes.contains(&node) && matches!(delivery.kind, DeliveryKind::Update { .. }) && self.expanded_components.contains(&node) {
            // This exact Update is represented by a compiled equation input.
            // Graph consumes it without executing its guard, so that guard is
            // not part of the native dependency closure.
            return Some(());
        }
        self.add_guard(delivery.guard)?;
        match receiver {
            CallbackReceiver::Component(node) => self.add_source_node(node),
            CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor) => self.add_actor(actor),
            CallbackReceiver::Contact(contact) => self.add_contact(contact),
        }
    }

    fn finish(
        mut self,
        seeds: &FxHashSet<NodeId>,
        seed_sites: &[SiteId],
        seed_states: &[StateTest],
        equation_clock_sites: &FxHashSet<SiteId>,
    ) -> Option<NativeInputOwnership> {
        for &node in seeds {
            self.add_source_node(node)?;
        }
        for &site in seed_sites {
            if equation_clock_sites.contains(&site) {
                continue;
            }
            self.add_site(site)?;
        }
        for &state in seed_states {
            self.add_state(state)?;
        }
        loop {
            while let Some(node) = self.pending_nodes.pop() {
                if !self.checked_nodes.insert(node) {
                    continue;
                }
                self.charge()?;
                match self.graph.nodes.get(node.index())?.kind.clone() {
                    CircuitNodeKind::Observer { watched, .. } | CircuitNodeKind::BuiltinReset { watched, .. } => {
                        self.add_site(watched)?;
                    }
                    CircuitNodeKind::Piston(actor) => {
                        self.add_actor(actor)?;
                        for port in [InputPort::Main, InputPort::Quasi] {
                            self.add_power_trace(PowerTarget::Input { node, port })?;
                        }
                    }
                    CircuitNodeKind::Contact(contact) => self.add_contact(contact)?,
                    CircuitNodeKind::Wire(_) => self.add_power_trace(PowerTarget::Input { node, port: InputPort::Main })?,
                    CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::PressurePlate | NodeType::Constant) => {}
                    CircuitNodeKind::Electrical(_) => {
                        for port in [InputPort::Main, InputPort::Side, InputPort::Quasi] {
                            self.add_power_trace(PowerTarget::Input { node, port })?;
                        }
                    }
                }
                let routes = self.node_routes.get(node.index())?.clone();
                for route_id in routes {
                    let deliveries = self.graph.routes.get(route_id.index())?.deliveries.to_vec();
                    for (ordinal, delivery) in deliveries.into_iter().enumerate() {
                        self.charge()?;
                        self.add_delivery(
                            RouteOccurrence {
                                route: route_id,
                                ordinal: u32::try_from(ordinal).ok()?,
                            },
                            &delivery,
                        )?;
                    }
                }
            }
            if self.pending_trace_targets.is_empty() {
                break;
            }
            self.drain_power_traces()?;
        }
        Some(self.ownership)
    }
}

pub(super) fn discover(
    graph: &CompileGraph,
    island: &LogicalIsland,
    expanded_components: &FxHashSet<NodeId>,
    physical_ownership: &OwnershipClosure,
    retained_cells: &RetainedCellBoundary,
    seeds: &FxHashSet<NodeId>,
    seed_sites: &[SiteId],
    seed_states: &[StateTest],
    equation_clock_sites: &FxHashSet<SiteId>,
    monitor: &TaskMonitor,
) -> Option<NativeInputOwnership> {
    Builder::new(graph, island, expanded_components, physical_ownership, retained_cells, monitor)?.finish(
        seeds,
        seed_sites,
        seed_states,
        equation_clock_sites,
    )
}
