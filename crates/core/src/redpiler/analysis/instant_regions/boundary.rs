//! Derive compact boundary ports from shared graph occurrences.
use crate::redpiler::program_graph::*;

#[derive(Clone, Copy, Default)]
struct GuardSupport {
    /// This guard can read data owned outside the selected region.
    external: bool,
    /// This guard can read data owned by the selected region.
    internal: bool,
}

struct BoundaryBuilder<'a> {
    /// This is the validated graph whose rows remain the canonical event data.
    graph: &'a CompileGraph,
    /// This is the actor/resource ownership closure receiving the derived ports.
    region: &'a InstantRegion,
    /// This marks every mutable node owned by the region.
    owned_nodes: Vec<bool>,
    /// This marks every dynamic site owned by the region.
    owned_sites: Vec<bool>,
    /// This marks actor identities whose pose is retained by the region.
    owned_actors: Vec<bool>,
    /// This marks payload identities whose location is retained by the region.
    owned_resources: Vec<bool>,
    /// This marks contacts whose geometry is retained by the region.
    owned_contacts: Vec<bool>,
    /// This memoizes each shared guard's ownership support for this region.
    guard_support: Vec<Option<GuardSupport>>,
    /// These ports are external reads and delivered callback entries.
    inputs: Vec<KernelInput>,
    /// These ports are external observations of region-owned values.
    outputs: Vec<KernelOutput>,
}

impl<'a> BoundaryBuilder<'a> {
    fn new(graph: &'a CompileGraph, region: &'a InstantRegion) -> Self {
        let mut builder = Self {
            graph,
            region,
            owned_nodes: vec![false; graph.nodes.len()],
            owned_sites: vec![false; graph.sites.len()],
            owned_actors: vec![false; graph.pistons.len()],
            owned_resources: vec![false; graph.resources.len()],
            owned_contacts: vec![false; graph.contacts.len()],
            guard_support: vec![None; graph.guards.len()],
            inputs: region.power_inputs.iter().copied().map(KernelInput::Power).collect(),
            outputs: region.power_readers.iter().copied().map(KernelOutput::Power).collect(),
        };
        builder.inputs.extend(region.incoming.iter().copied().map(KernelInput::Callback));
        builder.outputs.extend(region.outgoing.iter().copied().map(KernelOutput::Callback));
        builder.outputs.extend(region.observers.iter().copied().map(KernelOutput::Observer));
        builder.index_ownership();
        builder
    }

    fn index_ownership(&mut self) {
        for &actor in &self.region.actors {
            self.owned_actors[actor.index()] = true;
            self.owned_nodes[self.graph.pistons[actor.index()].node.index()] = true;
        }
        for &resource in &self.region.resources {
            self.owned_resources[resource.index()] = true;
        }
        for &contact in &self.region.contacts {
            self.owned_contacts[contact.index()] = true;
            self.owned_nodes[self.graph.contacts[contact.index()].node.index()] = true;
        }
        for &node in self.region.resets.iter().chain(&self.region.wires) {
            self.owned_nodes[node.index()] = true;
        }
        for &site in &self.region.readable_sites {
            self.owned_sites[site.index()] = true;
        }
    }

    fn derive(mut self) -> KernelBoundary {
        self.collect_power_guard_uses();
        self.collect_route_guard_uses();
        self.collect_contact_guard_uses();
        self.collect_wire_guard_uses();
        self.collect_comparator_guard_uses();
        self.collect_wire_neighbors();
        self.collect_wire_readers();
        self.inputs.sort_by_key(input_key);
        deduplicate_non_events(&mut self.inputs);
        self.outputs.sort_by_key(output_key);
        deduplicate_non_events(&mut self.outputs);

        let mut state = Vec::new();
        state.extend(self.region.resets.iter().copied().map(KernelStateSlot::Reset));
        state.extend(self.region.actors.iter().copied().map(KernelStateSlot::Piston));
        state.extend(self.region.resources.iter().copied().map(KernelStateSlot::Resource));
        state.extend(self.region.wires.iter().copied().map(KernelStateSlot::Wire));
        let mut projection_roots = self.region.readable_sites.clone();
        projection_roots.sort_unstable();
        projection_roots.dedup();
        KernelBoundary {
            inputs: self.inputs,
            state,
            outputs: self.outputs,
            effects: self.region.outgoing.clone(),
            projection_roots,
        }
    }

    fn collect_power_guard_uses(&mut self) {
        for (index, link) in self.graph.power_links.iter().enumerate() {
            let consumer_owned = match link.to {
                PowerTarget::Input { node, .. } => self.owns_node(node),
                PowerTarget::Conductor { site, .. } => self.owns_site(site),
            };
            self.add_guard_use(KernelGuardUse::Power(index as u32), link.guard, consumer_owned);
        }
    }

    fn collect_route_guard_uses(&mut self) {
        for (route_index, route) in self.graph.routes.iter().enumerate() {
            for (ordinal, delivery) in route.deliveries.iter().enumerate() {
                let consumer_owned = delivery
                    .receiver
                    .map_or_else(|| self.owns_site(delivery.target), |receiver| self.owns_receiver(receiver));
                let event = RouteOccurrence {
                    route: RouteId::new(route_index as u32),
                    ordinal: ordinal as u32,
                };
                self.add_guard_use(KernelGuardUse::Callback(event), delivery.guard, consumer_owned);
                if !consumer_owned && self.owns_site(delivery.target) {
                    self.outputs.push(KernelOutput::CallbackSite {
                        event,
                        site: delivery.target,
                        view: delivery.view,
                    });
                }
            }
        }
    }

    fn collect_contact_guard_uses(&mut self) {
        for (contact_index, contact) in self.graph.contacts.iter().enumerate() {
            let contact_id = ContactId::new(contact_index as u32);
            for (ordinal, variant) in contact.variants.iter().enumerate() {
                self.add_guard_use(
                    KernelGuardUse::ContactVariant {
                        contact: contact_id,
                        ordinal: ordinal as u32,
                    },
                    variant.when,
                    self.owns_contact(contact_id),
                );
            }
        }
    }

    fn collect_wire_guard_uses(&mut self) {
        for (index, relation) in self.graph.wire_relations.iter().enumerate() {
            self.add_guard_use(KernelGuardUse::WireRelation(index as u32), relation.guard, self.owns_node(relation.from));
        }
    }

    fn collect_comparator_guard_uses(&mut self) {
        for (index, plan) in self.graph.comparator_inputs.iter().enumerate() {
            let Some((_, _, guard)) = plan.far_override else {
                continue;
            };
            self.add_guard_use(KernelGuardUse::ComparatorOverride(index as u32), guard, self.owns_node(plan.node));
        }
    }

    fn collect_wire_neighbors(&mut self) {
        for &node in &self.region.wires {
            let Some(plan) = self.wire_plan(node) else { continue };
            for (slot, &site) in plan.canonical_neighbors.iter().enumerate() {
                if !self.owns_site(site) && self.is_dynamic_site(site) {
                    self.inputs.push(KernelInput::WireNeighbor {
                        wire: node,
                        slot: slot as u8,
                        site,
                    });
                }
            }
        }
    }

    fn collect_wire_readers(&mut self) {
        for &node in &self.region.wire_readers {
            let Some(plan) = self.wire_plan(node) else { continue };
            for (slot, &site) in plan.canonical_neighbors.iter().enumerate() {
                if self.owns_site(site) {
                    self.outputs.push(KernelOutput::WireReader {
                        wire: node,
                        slot: slot as u8,
                        site,
                    });
                }
            }
        }
    }

    fn add_guard_use(&mut self, owner: KernelGuardUse, guard: GuardId, consumer_owned: bool) {
        let support = self.support(guard);
        let read = KernelGuardRead { owner, guard };
        if consumer_owned && support.external {
            self.inputs.push(KernelInput::Guard(read));
        } else if !consumer_owned && support.internal {
            self.outputs.push(KernelOutput::Guard(read));
        }
    }

    fn support(&mut self, id: GuardId) -> GuardSupport {
        if let Some(support) = self.guard_support[id.index()] {
            return support;
        }
        let guard = self.graph.guards[id.index()].clone();
        let support = match guard {
            Guard::State(test) => {
                if self.owns_state(test) {
                    GuardSupport {
                        internal: true,
                        ..Default::default()
                    }
                } else {
                    GuardSupport {
                        external: true,
                        ..Default::default()
                    }
                }
            }
            Guard::Site { site, .. } if self.is_dynamic_site(site) => {
                if self.owns_site(site) {
                    GuardSupport {
                        internal: true,
                        ..Default::default()
                    }
                } else {
                    GuardSupport {
                        external: true,
                        ..Default::default()
                    }
                }
            }
            Guard::Not(child) => self.support(child),
            Guard::All(children) | Guard::Any(children) => {
                let mut combined = GuardSupport::default();
                for child in children.iter().copied() {
                    let child = self.support(child);
                    combined.internal |= child.internal;
                    combined.external |= child.external;
                }
                combined
            }
            Guard::Always | Guard::Site { .. } => GuardSupport::default(),
        };
        self.guard_support[id.index()] = Some(support);
        support
    }

    fn owns_state(&self, test: StateTest) -> bool {
        match test {
            StateTest::PistonExtended(actor) | StateTest::PistonHeadPresent(actor) | StateTest::PistonAvailability { piston: actor, .. } => {
                self.owned_actors[actor.index()]
            }
            StateTest::ResourceAt { resource, .. } | StateTest::ResourceEnabled(resource) => self.owned_resources[resource.index()],
        }
    }

    fn owns_node(&self, node: NodeId) -> bool {
        self.owned_nodes[node.index()]
    }

    fn owns_site(&self, site: SiteId) -> bool {
        self.owned_sites[site.index()]
    }

    fn owns_contact(&self, contact: ContactId) -> bool {
        self.owned_contacts[contact.index()]
    }

    fn owns_receiver(&self, receiver: CallbackReceiver) -> bool {
        match receiver {
            CallbackReceiver::Component(node) => self.owns_node(node),
            CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor) => self.owned_actors[actor.index()],
            CallbackReceiver::Contact(contact) => self.owns_contact(contact),
        }
    }

    fn is_dynamic_site(&self, site: SiteId) -> bool {
        !matches!(self.graph.sites[site.index()].source, SiteSource::Constant(_))
    }

    fn wire_plan(&self, node: NodeId) -> Option<&'a WirePlan> {
        let CircuitNodeKind::Wire(index) = self.graph.nodes.get(node.index())?.kind else {
            return None;
        };
        self.graph.wires.get(index).filter(|plan| plan.node == node)
    }
}

/// Derive ports from a graph that has passed full graph validation.
pub(crate) fn derive(graph: &CompileGraph, region: &InstantRegion) -> KernelBoundary {
    BoundaryBuilder::new(graph, region).derive()
}

/// Validate references and ownership before comparing a stored boundary with its derivation.
pub(crate) fn validate(boundary: &KernelBoundary, graph: &CompileGraph, region: &InstantRegion) -> Result<(), &'static str> {
    validate_region(graph, region)?;
    if boundary != &derive(graph, region) {
        return Err("boundary ports do not match graph ownership");
    }
    Ok(())
}

fn validate_region(graph: &CompileGraph, region: &InstantRegion) -> Result<(), &'static str> {
    if region.actors.iter().any(|actor| graph.pistons.get(actor.index()).is_none())
        || region.resources.iter().any(|resource| graph.resources.get(resource.index()).is_none())
        || region.contacts.iter().any(|contact| graph.contacts.get(contact.index()).is_none())
        || region.readable_sites.iter().any(|site| graph.sites.get(site.index()).is_none())
        || region.resets.iter().any(|node| graph.nodes.get(node.index()).is_none())
        || region
            .power_inputs
            .iter()
            .chain(&region.power_readers)
            .any(|link| graph.power_links.get(*link as usize).is_none())
        || region.routes.iter().any(|route| graph.routes.get(route.index()).is_none())
        || region
            .incoming
            .iter()
            .chain(&region.outgoing)
            .any(|event| route_delivery(graph, *event).is_none())
        || region
            .wires
            .iter()
            .chain(&region.wire_readers)
            .any(|node| wire_plan(graph, *node).is_none())
        || region.observers.iter().any(|node| graph.nodes.get(node.index()).is_none())
    {
        return Err("region contains a missing graph reference");
    }
    if !unique(&region.actors)
        || !unique(&region.resources)
        || !unique(&region.contacts)
        || !unique(&region.readable_sites)
        || !unique(&region.resets)
        || !unique(&region.power_inputs)
        || !unique(&region.power_readers)
        || !unique(&region.routes)
        || !unique(&region.incoming)
        || !unique(&region.outgoing)
        || !unique(&region.wires)
        || !unique(&region.wire_readers)
        || !unique(&region.observers)
    {
        return Err("region repeats a non-duplicate ownership reference");
    }
    let builder = BoundaryBuilder::new(graph, region);
    let mut expected_contacts = vec![false; graph.contacts.len()];
    for &actor in &region.actors {
        let spec = graph.pistons[actor.index()];
        for contact in [spec.base, spec.near, spec.far] {
            expected_contacts[contact.index()] = true;
        }
    }
    for &resource in &region.resources {
        for &contact in &graph.resources[resource.index()].contacts {
            expected_contacts[contact.index()] = true;
        }
    }
    if expected_contacts != builder.owned_contacts {
        return Err("region contact ownership does not match its actors and resources");
    }
    for (index, actor) in graph.pistons.iter().enumerate() {
        if builder.owned_actors[index] {
            continue;
        }
        if [actor.base, actor.near, actor.far]
            .iter()
            .any(|contact| expected_contacts[contact.index()])
        {
            return Err("region omits an actor sharing an owned contact");
        }
    }
    for (index, resource) in graph.resources.iter().enumerate() {
        if builder.owned_resources[index] {
            continue;
        }
        if resource.contacts.iter().any(|contact| expected_contacts[contact.index()]) {
            return Err("region omits a resource sharing an owned contact");
        }
    }
    for &site in &region.readable_sites {
        match graph.sites[site.index()].source {
            SiteSource::Component(node) if !builder.owns_node(node) => return Err("region claims an external component site"),
            SiteSource::Contact(contact) if !builder.owns_contact(contact) => return Err("region claims an external contact site"),
            SiteSource::Constant(_) | SiteSource::Component(_) | SiteSource::Contact(_) => {}
        }
    }
    for (index, _site) in graph.sites.iter().enumerate() {
        let id = SiteId::new(index as u32);
        let expected = match graph.sites[index].source {
            SiteSource::Component(node) => builder.owns_node(node),
            SiteSource::Contact(contact) => builder.owns_contact(contact),
            SiteSource::Constant(_) => builder.owns_site(id),
        };
        if expected != builder.owns_site(id) {
            return Err("region site ownership disagrees with its source owner");
        }
    }

    let expected_power_inputs: Vec<_> = graph
        .power_links
        .iter()
        .enumerate()
        .filter_map(|(index, link)| (builder.target_owned(link.to) && !builder.owns_site(link.from)).then_some(index as u32))
        .collect();
    let expected_power_readers: Vec<_> = graph
        .power_links
        .iter()
        .enumerate()
        .filter_map(|(index, link)| (builder.owns_site(link.from) && !builder.target_owned(link.to)).then_some(index as u32))
        .collect();
    if region.power_inputs != expected_power_inputs || region.power_readers != expected_power_readers {
        return Err("region power boundary is incomplete or has the wrong owner");
    }

    let mut expected_routes = Vec::new();
    let mut expected_incoming = Vec::new();
    let mut expected_outgoing = Vec::new();
    for (route_index, route) in graph.routes.iter().enumerate() {
        let source_owned = builder.owns_node(route.source);
        if source_owned {
            expected_routes.push(RouteId::new(route_index as u32));
        }
        for (ordinal, delivery) in route.deliveries.iter().enumerate() {
            let target_owned = delivery
                .receiver
                .map_or_else(|| builder.owns_site(delivery.target), |receiver| builder.owns_receiver(receiver));
            if source_owned == target_owned {
                continue;
            }
            let event = RouteOccurrence {
                route: RouteId::new(route_index as u32),
                ordinal: ordinal as u32,
            };
            if source_owned {
                expected_outgoing.push(event);
            } else if target_owned {
                expected_incoming.push(event);
            }
        }
    }
    if region.routes != expected_routes || region.incoming != expected_incoming || region.outgoing != expected_outgoing {
        return Err("region callback boundary is incomplete or has the wrong owner");
    }

    for &reset in &region.resets {
        match graph.nodes[reset.index()].kind {
            CircuitNodeKind::Observer { watched, .. } | CircuitNodeKind::BuiltinReset { watched, .. } if builder.owns_site(watched) => {}
            _ => return Err("region reset does not observe an owned site"),
        }
    }
    let expected_wire_readers: Vec<_> = graph
        .wires
        .iter()
        .filter(|wire| !builder.owns_node(wire.node) && wire.canonical_neighbors.iter().any(|site| builder.owns_site(*site)))
        .map(|wire| wire.node)
        .collect();
    let expected_observers: Vec<_> = graph
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            let watched = match node.kind {
                CircuitNodeKind::Observer { watched, .. } | CircuitNodeKind::BuiltinReset { watched, .. } => watched,
                _ => return None,
            };
            (!builder.owns_node(NodeId::new(index as u32)) && builder.owns_site(watched)).then_some(NodeId::new(index as u32))
        })
        .collect();
    if region.wire_readers != expected_wire_readers || region.observers != expected_observers {
        return Err("region geometry readers are incomplete or have the wrong owner");
    }
    Ok(())
}

fn unique<T: Copy + Eq + std::hash::Hash>(items: &[T]) -> bool {
    let mut seen = std::collections::HashSet::with_capacity(items.len());
    items.iter().all(|item| seen.insert(*item))
}

impl BoundaryBuilder<'_> {
    fn target_owned(&self, target: PowerTarget) -> bool {
        match target {
            PowerTarget::Input { node, .. } => self.owns_node(node),
            PowerTarget::Conductor { site, .. } => self.owns_site(site),
        }
    }
}

fn wire_plan(graph: &CompileGraph, node: NodeId) -> Option<&WirePlan> {
    let CircuitNodeKind::Wire(index) = graph.nodes.get(node.index())?.kind else {
        return None;
    };
    graph.wires.get(index).filter(|plan| plan.node == node)
}

fn route_delivery(graph: &CompileGraph, occurrence: RouteOccurrence) -> Option<&RouteDelivery> {
    graph.routes.get(occurrence.route.index())?.deliveries.get(occurrence.ordinal as usize)
}

fn input_key(input: &KernelInput) -> (u8, u8, u32, u32, u32) {
    match input {
        KernelInput::Power(link) => (0, 0, *link, 0, 0),
        KernelInput::Callback(event) => (1, 0, event.route.index() as u32, event.ordinal, 0),
        KernelInput::Guard(read) => guard_key(2, read.owner, read.guard),
        KernelInput::WireNeighbor { wire, slot, site } => (3, 0, wire.index() as u32, *slot as u32, site.index() as u32),
    }
}

fn output_key(output: &KernelOutput) -> (u8, u8, u32, u32, u32) {
    match output {
        KernelOutput::Power(link) => (0, 0, *link, 0, 0),
        KernelOutput::Callback(event) => (1, 0, event.route.index() as u32, event.ordinal, 0),
        KernelOutput::Observer(node) => (2, 0, node.index() as u32, 0, 0),
        KernelOutput::WireReader { wire, slot, site } => (3, 0, wire.index() as u32, *slot as u32, site.index() as u32),
        KernelOutput::Guard(read) => guard_key(4, read.owner, read.guard),
        KernelOutput::CallbackSite { event, site, .. } => (5, 0, event.route.index() as u32, event.ordinal, site.index() as u32),
    }
}

fn guard_key(group: u8, owner: KernelGuardUse, guard: GuardId) -> (u8, u8, u32, u32, u32) {
    let (kind, id, ordinal) = match owner {
        KernelGuardUse::Power(index) => (0, index, 0),
        KernelGuardUse::Callback(event) => (1, event.route.index() as u32, event.ordinal),
        KernelGuardUse::ContactVariant { contact, ordinal } => (2, contact.index() as u32, ordinal),
        KernelGuardUse::WireRelation(index) => (3, index, 0),
        KernelGuardUse::ComparatorOverride(index) => (4, index, 0),
    };
    (group, kind, id, ordinal, guard.index() as u32)
}

fn deduplicate_non_events<T: BoundaryPort>(ports: &mut Vec<T>) {
    ports.dedup_by(|later, earlier| !T::is_event(*later) && !T::is_event(*earlier) && later == earlier);
}

trait BoundaryPort: Copy + Eq {
    fn is_event(self) -> bool;
}

impl BoundaryPort for KernelInput {
    fn is_event(self) -> bool {
        matches!(self, KernelInput::Callback(_))
    }
}

impl BoundaryPort for KernelOutput {
    fn is_event(self) -> bool {
        matches!(self, KernelOutput::Callback(_) | KernelOutput::CallbackSite { .. })
    }
}
