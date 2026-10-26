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
    #[cfg(test)]
    pub(super) diagnostic_parents: FxHashMap<NodeId, (Option<NodeId>, &'static str)>,
    #[cfg(test)]
    pub(super) diagnostic_trace_batches: Vec<DiagnosticTraceBatch>,
    #[cfg(test)]
    pub(super) diagnostic_trace_wire_batches: FxHashMap<NodeId, usize>,
}

#[cfg(test)]
pub(super) struct DiagnosticTraceBatch {
    pub(super) targets: Vec<PowerTarget>,
    pub(super) origins: FxHashMap<PowerTarget, FxHashSet<NodeId>>,
}

struct Builder<'a> {
    graph: &'a CompileGraph,
    island: &'a LogicalIsland,
    expanded_components: &'a FxHashSet<NodeId>,
    physical_ownership: &'a OwnershipClosure,
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
    #[cfg(test)]
    pending_trace_origins: FxHashMap<PowerTarget, FxHashSet<NodeId>>,
    #[cfg(test)]
    diagnostic_parent: Option<NodeId>,
    #[cfg(test)]
    diagnostic_reason: &'static str,
}

impl<'a> Builder<'a> {
    fn new(
        graph: &'a CompileGraph,
        island: &'a LogicalIsland,
        expanded_components: &'a FxHashSet<NodeId>,
        physical_ownership: &'a OwnershipClosure,
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
            #[cfg(test)]
            pending_trace_origins: FxHashMap::default(),
            #[cfg(test)]
            diagnostic_parent: None,
            #[cfg(test)]
            diagnostic_reason: "unclassified native ownership",
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
            #[cfg(test)]
            eprintln!(
                "native input decline: node {node:?} at {:?} is outside interaction island {}",
                self.graph.nodes.get(node.index()).and_then(|node| node.position),
                self.island.id
            );
            return None;
        }
        if !self.checked_nodes.contains(&node) && self.ownership.nodes.insert(node) {
            #[cfg(test)]
            {
                self.ownership
                    .diagnostic_parents
                    .entry(node)
                    .or_insert((self.diagnostic_parent, self.diagnostic_reason));
            }
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
        let group_index = self.physical_ownership.actors.get(actor.index()).copied().flatten()?;
        self.add_group(group_index)
    }

    fn add_resource(&mut self, resource: ResourceId) -> Option<()> {
        let group_index = self.resource_groups.get(resource.index()).copied().flatten()?;
        self.add_group(group_index)
    }

    fn add_contact(&mut self, contact: ContactId) -> Option<()> {
        let group_index = self.physical_ownership.contacts.get(contact.index()).copied().flatten();
        if let Some(group_index) = group_index {
            self.add_group(group_index)?;
        }
        self.add_contact_node(contact)
    }

    fn add_site(&mut self, site: SiteId) -> Option<()> {
        match self.graph.sites.get(site.index())?.source {
            SiteSource::Component(node) => {
                self.ownership.sites.insert(site);
                self.add_source_node(node)
            }
            SiteSource::Contact(contact) => {
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
                Guard::State(StateTest::PistonExtended(actor) | StateTest::PistonHeadPresent(actor))
                | Guard::State(StateTest::PistonAvailability { piston: actor, .. }) => self.add_actor(actor)?,
                Guard::State(StateTest::ResourceAt { resource, contact }) => {
                    self.add_resource(resource)?;
                    self.add_contact(contact)?;
                }
                Guard::State(StateTest::ResourceEnabled(resource)) => self.add_resource(resource)?,
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

    fn add_power_trace(&mut self, target: PowerTarget, origin: NodeId) -> Option<()> {
        if self.traced_targets.insert(target) {
            self.pending_trace_targets.push(target);
        }
        #[cfg(test)]
        self.pending_trace_origins.entry(target).or_default().insert(origin);
        Some(())
    }

    fn drain_power_traces(&mut self) -> Option<()> {
        let targets = std::mem::take(&mut self.pending_trace_targets);
        #[cfg(test)]
        let batch_origins = targets
            .iter()
            .filter_map(|target| self.pending_trace_origins.remove(target).map(|origins| (*target, origins)))
            .collect::<FxHashMap<_, _>>();
        let trace = match super::trace_native_power_targets(self.graph, &targets, self.monitor) {
            Ok(trace) => trace,
            Err(reason) => {
                #[cfg(test)]
                eprintln!(
                    "native input decline: power trace for {} targets beginning {:?} failed: {reason}",
                    targets.len(),
                    &targets[..targets.len().min(8)]
                );
                return None;
            }
        };
        #[cfg(test)]
        {
            let batch_index = self.ownership.diagnostic_trace_batches.len();
            for &wire in &trace.wires {
                self.ownership.diagnostic_trace_wire_batches.entry(wire).or_insert(batch_index);
            }
            self.ownership.diagnostic_trace_batches.push(DiagnosticTraceBatch {
                targets: targets.clone(),
                origins: batch_origins,
            });
        }
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
                unsupported => {
                    #[cfg(test)]
                    eprintln!("native input decline: unsupported power trace input {unsupported:?}");
                    return None;
                }
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
        self.add_node(node)?;
        match self.graph.nodes.get(node.index())?.kind.clone() {
            CircuitNodeKind::Piston(actor) => self.add_actor(actor),
            CircuitNodeKind::Contact(contact) => self.add_contact(contact),
            CircuitNodeKind::Wire(_) => self.add_power_trace(PowerTarget::Input { node, port: InputPort::Main }, node),
            CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. } => Some(()),
            CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::PressurePlate | NodeType::Constant) => Some(()),
            CircuitNodeKind::Electrical(_) => {
                for port in [InputPort::Main, InputPort::Side, InputPort::Quasi] {
                    self.add_power_trace(PowerTarget::Input { node, port }, node)?;
                }
                Some(())
            }
        }
    }

    fn add_delivery(&mut self, delivery: &RouteDelivery) -> Option<()> {
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
        #[cfg(test)]
        {
            self.diagnostic_parent = None;
            self.diagnostic_reason = "equation/source input seed";
        }
        for &node in seeds {
            self.add_source_node(node)?;
        }
        #[cfg(test)]
        {
            self.diagnostic_reason = "equation/source site seed";
        }
        for &site in seed_sites {
            if equation_clock_sites.contains(&site) {
                continue;
            }
            self.add_site(site)?;
        }
        #[cfg(test)]
        {
            self.diagnostic_reason = "equation/state seed";
        }
        for &state in seed_states {
            self.add_state(state)?;
        }
        loop {
            while let Some(node) = self.pending_nodes.pop() {
                if !self.checked_nodes.insert(node) {
                    continue;
                }
                #[cfg(test)]
                {
                    self.diagnostic_parent = Some(node);
                    self.diagnostic_reason = "native route, power, state, or owner closure";
                }
                self.charge()?;
                match self.graph.nodes.get(node.index())?.kind.clone() {
                    CircuitNodeKind::Observer { watched, .. } | CircuitNodeKind::BuiltinReset { watched, .. } => {
                        self.add_site(watched)?;
                    }
                    CircuitNodeKind::Piston(actor) => {
                        self.add_actor(actor)?;
                        for port in [InputPort::Main, InputPort::Quasi] {
                            self.add_power_trace(PowerTarget::Input { node, port }, node)?;
                        }
                    }
                    CircuitNodeKind::Contact(contact) => self.add_contact(contact)?,
                    CircuitNodeKind::Wire(_) => self.add_power_trace(PowerTarget::Input { node, port: InputPort::Main }, node)?,
                    CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::PressurePlate | NodeType::Constant) => {}
                    CircuitNodeKind::Electrical(_) => {
                        for port in [InputPort::Main, InputPort::Side, InputPort::Quasi] {
                            self.add_power_trace(PowerTarget::Input { node, port }, node)?;
                        }
                    }
                }
                let routes = self.node_routes.get(node.index())?.clone();
                for route_id in routes {
                    let deliveries = self.graph.routes.get(route_id.index())?.deliveries.to_vec();
                    for delivery in deliveries {
                        self.charge()?;
                        self.add_delivery(&delivery)?;
                    }
                }
            }
            if self.pending_trace_targets.is_empty() {
                break;
            }
            #[cfg(test)]
            {
                self.diagnostic_parent = None;
                self.diagnostic_reason = "native power trace result";
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
    seeds: &FxHashSet<NodeId>,
    seed_sites: &[SiteId],
    seed_states: &[StateTest],
    equation_clock_sites: &FxHashSet<SiteId>,
    monitor: &TaskMonitor,
) -> Option<NativeInputOwnership> {
    Builder::new(graph, island, expanded_components, physical_ownership, monitor)?.finish(seeds, seed_sites, seed_states, equation_clock_sites)
}
