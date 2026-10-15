//! Shared discovery and certificate checking for private instant reset cells.
use crate::redpiler::program_graph::{
    CallbackReceiver, CircuitNodeKind, CompileGraph, ComponentState, ContactId, DeliveryKind, Guard, GuardId, InputPort, InstantResetPlan, NodeId,
    OperandView, PistonId, PistonLane, PowerLink, PowerPort, PowerTarget, ProfileElectrical, ResourceId, RouteId, SiteId, SiteSource, SiteTest,
    StateTest,
};
use mchprs_blocks::blocks::Block;
use mchprs_blocks::{BlockFace, BlockPos};
use rustc_hash::FxHashMap;

/// Transient structural witnesses; existing graph tables remain authoritative.
#[derive(Debug, Clone)]
pub(crate) struct InstantResetCandidate {
    /// This binds the existing actor whose two power inputs can be simplified.
    pub(crate) piston: PistonId,
    /// This binds its private redstone token without introducing another owner.
    pub(crate) resource: ResourceId,
    /// This binds the downward observer watching the actor's base.
    pub(crate) observer: NodeId,
    /// These identify the retained output wires in source-to-consumer order.
    pub(crate) output_wires: [NodeId; 2],
    /// This identifies the actor's original Main input occurrence.
    pub(crate) main_edge: usize,
    /// This identifies the actor's original Quasi input occurrence.
    pub(crate) quasi_edge: usize,
    /// This identifies the normal conductor's single live Strong contributor.
    pub(crate) normal_edge: usize,
    /// This identifies the QC cap's single live observer contributor.
    pub(crate) qc_edge: usize,
}

/// Compile-time reverse indexes, built once rather than rescanned per candidate.
#[derive(Default)]
pub(crate) struct DiscoveryIndex {
    /// This groups incoming power occurrences by their exact target and dust policy.
    power_to: FxHashMap<PowerTarget, Vec<usize>>,
    /// This groups outgoing power occurrences across all aliases of a component.
    power_from_component: FxHashMap<NodeId, Vec<usize>>,
    /// This binds each compiled component position to its possible node identities.
    nodes_at: FxHashMap<BlockPos, Vec<NodeId>>,
    /// This groups observers by the contact they watch.
    observers: FxHashMap<ContactId, Vec<NodeId>>,
    /// This records every possible resource owner, independent of its initial selection.
    resources: FxHashMap<ContactId, Vec<ResourceId>>,
    /// This records all actors capable of touching a contact.
    actors: FxHashMap<ContactId, Vec<PistonId>>,
    /// This binds canonical sampled positions without rescanning the site table.
    sites_at: FxHashMap<BlockPos, SiteId>,
    /// This counts component aliases whose distinct capture identities affect heading.
    component_sites: FxHashMap<NodeId, Vec<SiteId>>,
    /// This indexes every effective component Update ingress, including fallback rows.
    updates_to: FxHashMap<NodeId, Vec<RouteId>>,
    /// This records component state observed outside the admitted pair.
    watched_components: rustc_hash::FxHashSet<NodeId>,
    /// This records wires already using a different certified power kernel.
    dust_path_wires: rustc_hash::FxHashSet<NodeId>,
}

impl DiscoveryIndex {
    pub(crate) fn build(graph: &CompileGraph, monitor: &crate::redpiler::TaskMonitor) -> Result<Self, String> {
        let mut index = Self::default();
        for path in &graph.plans.dust_paths {
            check_cancelled(monitor)?;
            index.dust_path_wires.extend(path.wires.iter().copied());
        }
        for (id, site) in graph.sites.iter().enumerate() {
            check_cancelled(monitor)?;
            index.sites_at.insert(site.position, SiteId::new(id as u32));
            if let SiteSource::Component(node) = site.source {
                index.component_sites.entry(node).or_default().push(SiteId::new(id as u32));
            }
        }
        for (edge, link) in graph.power_links.iter().enumerate() {
            check_cancelled(monitor)?;
            index.power_to.entry(link.to).or_default().push(edge);
            if let SiteSource::Component(node) = graph.sites[link.from.index()].source {
                index.power_from_component.entry(node).or_default().push(edge);
            }
        }
        for (id, node) in graph.nodes.iter().enumerate() {
            check_cancelled(monitor)?;
            let id = NodeId::new(id as u32);
            if let Some(position) = node.position {
                index.nodes_at.entry(position).or_default().push(id);
            }
            if let CircuitNodeKind::Observer { watched, .. } | CircuitNodeKind::BuiltinReset { watched, .. } = node.kind
                && let SiteSource::Contact(contact) = graph.sites[watched.index()].source
            {
                index.observers.entry(contact).or_default().push(id);
            }
            if let CircuitNodeKind::Observer { watched, .. } | CircuitNodeKind::BuiltinReset { watched, .. } = node.kind
                && let SiteSource::Component(component) = graph.sites[watched.index()].source
            {
                index.watched_components.insert(component);
            }
        }
        for (id, resource) in graph.resources.iter().enumerate() {
            check_cancelled(monitor)?;
            for contact in &resource.contacts {
                index.resources.entry(*contact).or_default().push(ResourceId::new(id as u32));
            }
        }
        for (id, piston) in graph.pistons.iter().enumerate() {
            check_cancelled(monitor)?;
            for contact in [piston.base, piston.near, piston.far] {
                index.actors.entry(contact).or_default().push(PistonId::new(id as u32));
            }
        }
        for (id, route) in graph.routes.iter().enumerate() {
            check_cancelled(monitor)?;
            for delivery in &route.deliveries {
                if !matches!(delivery.kind, DeliveryKind::Update { .. }) {
                    continue;
                }
                let receiver = delivery.receiver.or_else(|| match graph.sites[delivery.target.index()].source {
                    SiteSource::Component(node) => Some(CallbackReceiver::Component(node)),
                    _ => None,
                });
                if let Some(CallbackReceiver::Component(node)) = receiver {
                    index.updates_to.entry(node).or_default().push(RouteId::new(id as u32));
                }
            }
        }
        Ok(index)
    }

    fn sole_power(&self, target: PowerTarget) -> Option<usize> {
        let [edge] = self.power_to.get(&target)?.as_slice() else {
            return None;
        };
        Some(*edge)
    }

    fn wire_at(&self, graph: &CompileGraph, position: BlockPos) -> Option<NodeId> {
        let mut wires = self
            .nodes_at
            .get(&position)?
            .iter()
            .copied()
            .filter(|node| matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Wire(_)));
        let wire = wires.next()?;
        wires.next().is_none().then_some(wire)
    }
}

/// Finds only the closed private reset motif; this helper does not mutate it.
pub(crate) fn discover_candidates(graph: &CompileGraph, monitor: &crate::redpiler::TaskMonitor) -> Result<Vec<InstantResetCandidate>, String> {
    let index = DiscoveryIndex::build(graph, monitor)?;
    discover_indexed_candidates(graph, &index, monitor)
}

fn discover_indexed_candidates(
    graph: &CompileGraph,
    index: &DiscoveryIndex,
    monitor: &crate::redpiler::TaskMonitor,
) -> Result<Vec<InstantResetCandidate>, String> {
    let mut candidates = Vec::new();
    for id in 0..graph.pistons.len() {
        check_cancelled(monitor)?;
        if let Some(candidate) = check_candidate(graph, &index, PistonId::new(id as u32)) {
            candidates.push(candidate);
        }
    }
    check_cancelled(monitor)?;
    Ok(candidates)
}

fn check_candidate(graph: &CompileGraph, index: &DiscoveryIndex, piston: PistonId) -> Option<InstantResetCandidate> {
    let actor = graph.pistons[piston.index()];
    if !actor.sticky || matches!(actor.facing, BlockFace::Top | BlockFace::Bottom) {
        return None;
    }
    let PistonLane::SingleResource { far } = actor.lane else {
        return None;
    };
    if graph.contacts[actor.near.index()].position != actor.position.offset(actor.facing)
        || graph.contacts[far.index()].position != actor.position.offset(actor.facing).offset(actor.facing)
        || [actor.near, far]
            .iter()
            .any(|contact| index.actors.get(contact).map(Vec::as_slice) != Some(&[piston]))
    {
        return None;
    }
    let [resource] = index.resources.get(&actor.near)?.as_slice() else {
        return None;
    };
    let resource = *resource;
    if index.resources.get(&far).map(Vec::as_slice) != Some(&[resource]) {
        return None;
    }
    let token = &graph.resources[resource.index()];
    let material = graph.profiles[token.material.index()];
    if token.contacts.len() != 2
        || !token.contacts.contains(&far)
        || Block::from_id(material.raw_state) != Block::RedstoneBlock
        || material.electrical != ProfileElectrical::RedstoneSource
    {
        return None;
    }
    let [observer] = index.observers.get(&actor.base)?.as_slice() else {
        return None;
    };
    let observer = *observer;
    let observed = &graph.nodes[observer.index()];
    let (CircuitNodeKind::Observer {
        watched,
        facing: BlockFace::Bottom,
    }
    | CircuitNodeKind::BuiltinReset {
        watched,
        facing: BlockFace::Bottom,
    }) = observed.kind
    else {
        return None;
    };
    if observed.position != Some(actor.position.offset(BlockFace::Top)) || graph.sites[watched.index()].position != actor.position {
        return None;
    }
    // The projected block retains raw facing; the update handler uses typed facing.
    let Block::Observer { observer: raw_observer } = Block::from_id(observed.raw_state?) else {
        return None;
    };
    if raw_observer.facing != BlockFace::Bottom.into() || index.observers.contains_key(&actor.near) || index.observers.contains_key(&far) {
        return None;
    }

    let main_edge = sole_nonzero_input(
        graph,
        index,
        PowerTarget::Input {
            node: actor.node,
            port: InputPort::Main,
        },
    )?;
    let quasi_edge = sole_nonzero_input(
        graph,
        index,
        PowerTarget::Input {
            node: actor.node,
            port: InputPort::Quasi,
        },
    )?;
    let main = graph.power_links[main_edge];
    let quasi = graph.power_links[quasi_edge];
    let normal_input = *index.sites_at.get(&actor.position.offset(actor.facing.opposite()))?;
    let cap = *index.sites_at.get(&actor.position.offset(BlockFace::Top).offset(BlockFace::Top))?;
    let normal_edge = sole_conductor_input(graph, index, normal_input)?;
    let qc_edge = sole_conductor_input(graph, index, cap)?;
    let normal = graph.power_links[normal_edge];
    let qc = graph.power_links[qc_edge];
    if !matches_original_or_rewritten_input(graph, main, normal_input, normal, normal.from_port)
        || !matches_original_or_rewritten_input(graph, quasi, cap, qc, PowerPort::Output)
    {
        return None;
    }
    let normal_wire = index.wire_at(graph, actor.position.offset(actor.facing.opposite()).offset(actor.facing.opposite()))?;
    if graph.sites[normal.from.index()].source != SiteSource::Component(normal_wire)
        || graph.sites[normal.from.index()].position != graph.nodes[normal_wire.index()].position?
        || normal.from_port != PowerPort::Strong(actor.facing.opposite())
        || graph.sites[qc.from.index()].source != SiteSource::Component(observer)
        || graph.sites[qc.from.index()].position != observed.position?
        || !(qc.from_port == PowerPort::Strong(BlockFace::Bottom)
            || (matches!(observed.kind, CircuitNodeKind::BuiltinReset { .. }) && qc.from_port == PowerPort::Output))
        || index
            .power_from_component
            .get(&observer)?
            .iter()
            .any(|edge| *edge != qc_edge && *edge != quasi_edge)
    {
        return None;
    }
    if !has_base_observer_callback(graph, actor.routes.base_changed, observer)
        || !has_base_observer_callback(graph, actor.routes.base_restored, observer)
        || !observer_has_no_output_receiver(graph, observer)
    {
        return None;
    }
    let first_output = graph.contacts[far.index()].position.offset(actor.facing);
    let output_wires = [
        index.wire_at(graph, first_output)?,
        index.wire_at(graph, first_output.offset(actor.facing))?,
    ];
    Some(InstantResetCandidate {
        piston,
        resource,
        observer,
        output_wires,
        main_edge,
        quasi_edge,
        normal_edge,
        qc_edge,
    })
}

fn sole_conductor_input(graph: &CompileGraph, index: &DiscoveryIndex, conductor: SiteId) -> Option<usize> {
    let site = graph.sites[conductor.index()];
    let SiteSource::Constant(profile) = site.source else {
        return None;
    };
    let profile = graph.profiles[profile.index()];
    let block = Block::from_id(profile.raw_state);
    if !profile.solid
        || !profile.cube
        || profile.transparent
        || profile.electrical != ProfileElectrical::None
        || !block.is_solid()
        || !block.is_cube()
        || block.is_transparent()
    {
        return None;
    }
    let edge = index.sole_power(PowerTarget::Conductor {
        site: conductor,
        include_dust: true,
    })?;
    unconditional_zero_link(graph, graph.power_links[edge]).then_some(edge)
}

fn matches_original_or_rewritten_input(
    graph: &CompileGraph,
    input: PowerLink,
    conductor: SiteId,
    contributor: PowerLink,
    direct_port: PowerPort,
) -> bool {
    unconditional_zero_link(graph, input)
        && ((input.from == conductor && input.from_port == (PowerPort::Conducted { include_dust: true }))
            || (input.from == contributor.from && input.from_port == direct_port))
}

fn unconditional_zero_link(graph: &CompileGraph, link: PowerLink) -> bool {
    link.attenuation == 0 && matches!(graph.guards[link.guard.index()], Guard::Always)
}

fn sole_nonzero_input(graph: &CompileGraph, index: &DiscoveryIndex, target: PowerTarget) -> Option<usize> {
    let mut edges = index.power_to.get(&target)?.iter().copied().filter(|edge| {
        let link = graph.power_links[*edge];
        // Preparation may retain a Weak query on the same immutable stone as its
        // conducted query. Stone has no weak emission, irrespective of strength.
        !matches!((link.from_port, graph.sites[link.from.index()].source), (PowerPort::Weak(_), SiteSource::Constant(profile))
            if matches!(Block::from_id(graph.profiles[profile.index()].raw_state), Block::Stone {} | Block::Air))
    });
    let edge = edges.next()?;
    edges.next().is_none().then_some(edge)
}

fn has_base_observer_callback(graph: &CompileGraph, route: crate::redpiler::program_graph::RouteId, observer: NodeId) -> bool {
    graph.routes[route.index()].deliveries.iter().any(|delivery| {
        delivery.receiver == Some(CallbackReceiver::Component(observer))
            && delivery.kind
                == (DeliveryKind::Update {
                    direction: Some(BlockFace::Bottom),
                })
            && delivery.view == OperandView::Live
            && matches!(graph.guards[delivery.guard.index()], Guard::Always)
            && graph.sites[delivery.target.index()].source == SiteSource::Component(observer)
    })
}

fn observer_has_no_output_receiver(graph: &CompileGraph, observer: NodeId) -> bool {
    let Some(route) = graph.nodes[observer.index()].output_route else {
        return true;
    };
    graph.routes[route.index()].deliveries.iter().all(|delivery| {
        delivery.receiver.is_none()
            && !matches!(
                (graph.sites[delivery.target.index()].source, delivery.kind),
                (SiteSource::Component(_), DeliveryKind::Update { .. })
            )
    })
}

fn check_cancelled(monitor: &crate::redpiler::TaskMonitor) -> Result<(), String> {
    if monitor.cancelled() {
        Err("cancelled while simplifying instant resets".into())
    } else {
        Ok(())
    }
}

/// Re-derives wire certificates once for validation, including after power rewriting.
pub(crate) fn infer_instant_resets(graph: &CompileGraph) -> Vec<(usize, InstantResetPlan)> {
    infer_with_monitor(graph, &crate::redpiler::TaskMonitor::default()).expect("a fresh validation monitor is not cancelled")
}

pub(crate) fn infer_with_monitor(graph: &CompileGraph, monitor: &crate::redpiler::TaskMonitor) -> Result<Vec<(usize, InstantResetPlan)>, String> {
    let index = DiscoveryIndex::build(graph, monitor)?;
    let candidates = discover_indexed_candidates(graph, &index, monitor)?;
    infer_kernels(graph, &index, &candidates, monitor)
}

fn infer_kernels(
    graph: &CompileGraph,
    index: &DiscoveryIndex,
    candidates: &[InstantResetCandidate],
    monitor: &crate::redpiler::TaskMonitor,
) -> Result<Vec<(usize, InstantResetPlan)>, String> {
    let mut plans = Vec::new();
    for candidate in candidates {
        check_cancelled(monitor)?;
        if let Some(wire) = check_two_wire_kernel(graph, index, candidate) {
            plans.push((
                wire,
                InstantResetPlan {
                    piston: candidate.piston,
                    resource: candidate.resource,
                    observer: candidate.observer,
                    partner: candidate.output_wires[1],
                },
            ));
        }
    }
    Ok(plans)
}

pub(crate) fn check_two_wire_kernel(graph: &CompileGraph, index: &DiscoveryIndex, candidate: &InstantResetCandidate) -> Option<usize> {
    use crate::redpiler::components::NodeType;
    let actor = graph.pistons[candidate.piston.index()];
    // The proved heading/order grammar is initially the south-facing family only.
    if actor.facing != BlockFace::South {
        return None;
    }
    let [a, b] = candidate.output_wires;
    let CircuitNodeKind::Wire(ai) = graph.nodes[a.index()].kind else {
        return None;
    };
    let CircuitNodeKind::Wire(bi) = graph.nodes[b.index()].kind else {
        return None;
    };
    let ap = &graph.wires[ai];
    let bp = &graph.wires[bi];
    let source = ap.canonical_neighbors[4];
    let head = ap.canonical_neighbors[22];
    if graph.sites[source.index()].source != SiteSource::Contact(actor.far)
        || graph.sites[head.index()].source != SiteSource::Contact(actor.near)
        || ap.canonical_neighbors[23] != bp.canonical_neighbors[5]
        || source != bp.canonical_neighbors[22]
    {
        return None;
    }
    let component = |site: SiteId| match graph.sites[site.index()].source {
        SiteSource::Component(node) => Some(node),
        _ => None,
    };
    if component(ap.canonical_neighbors[5]) != Some(b) || component(bp.canonical_neighbors[4]) != Some(a) {
        return None;
    }
    // Turbo's visited set is keyed by SiteId, not component identity. A second
    // root alias would change whether B sees A as visited and alter its heading.
    if index.component_sites.get(&a).map(Vec::as_slice) != Some(&[bp.canonical_neighbors[4]])
        || index.component_sites.get(&b).map(Vec::as_slice) != Some(&[ap.canonical_neighbors[5]])
    {
        return None;
    }
    let r = component(ap.canonical_neighbors[23])?;
    let t = component(bp.canonical_neighbors[23])?;
    for node in [r, t] {
        if !matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Electrical(NodeType::Repeater { .. })) {
            return None;
        }
        let Block::RedstoneRepeater { repeater } = Block::from_id(graph.nodes[node.index()].raw_state?) else {
            return None;
        };
        if repeater.facing.block_face() != BlockFace::North || index.power_to.contains_key(&PowerTarget::Input { node, port: InputPort::Side }) {
            return None;
        }
    }
    if graph.nodes[r.index()].position != graph.nodes[b.index()].position.map(|p| p.offset(BlockFace::South))
        || graph.nodes[t.index()].position != graph.nodes[r.index()].position.map(|p| p.offset(BlockFace::South))
        || !native_straight_wire(graph, a)
        || !native_straight_wire(graph, b)
        || !native_far_contact(graph, actor.far, candidate.resource)
        || !native_near_contact(graph, actor.near, candidate.piston)
    {
        return None;
    }
    for plan in [ap, bp] {
        let position = graph.nodes[plan.node.index()].position?;
        let expected = crate::world::wire_cache::positions(position);
        let mut distinct = rustc_hash::FxHashSet::default();
        for (slot, site) in plan.canonical_neighbors.iter().copied().enumerate() {
            if !distinct.insert(site)
                || graph.sites[site.index()].position != expected[slot]
                || !closed_kernel_site(graph, index, site, [a, b, r, t], [actor.near, actor.far])
            {
                return None;
            }
        }
    }
    let ar = ap.turbo_route?;
    let br = bp.turbo_route?;
    if !native_kernel_route(
        graph,
        ar,
        &[
            (ap.canonical_neighbors[5], CallbackReceiver::Component(b)),
            (ap.canonical_neighbors[23], CallbackReceiver::Component(r)),
        ],
        Some((head, candidate.piston)),
    ) || !native_kernel_route(
        graph,
        br,
        &[
            (bp.canonical_neighbors[4], CallbackReceiver::Component(a)),
            (bp.canonical_neighbors[5], CallbackReceiver::Component(r)),
            (bp.canonical_neighbors[23], CallbackReceiver::Component(t)),
        ],
        None,
    ) || index
        .updates_to
        .get(&b)?
        .iter()
        .any(|route| *route != ar && Some(*route) != ap.shape_route && Some(*route) != bp.shape_route)
        || index.watched_components.contains(&a)
        || index.watched_components.contains(&b)
        || index.dust_path_wires.contains(&a)
        || index.dust_path_wires.contains(&b)
        || !native_wire_inputs(graph, index, a, b, Some((source, candidate.resource, actor.far)))
        || !native_wire_inputs(graph, index, b, a, None)
        || !single_repeater_input(graph, index, r, b, true)
        || !single_repeater_input(graph, index, t, r, false)
    {
        return None;
    }
    // External power readers could observe intermediate writes hidden by the kernel.
    for wire in [a, b] {
        for edge in index.power_from_component.get(&wire).into_iter().flatten() {
            if !matches!(graph.power_links[*edge].to, PowerTarget::Input { node, port: InputPort::Main } if node == a || node == b || node == r) {
                return None;
            }
        }
    }
    Some(ai)
}

fn native_straight_wire(graph: &CompileGraph, node: NodeId) -> bool {
    use mchprs_blocks::blocks::RedstoneWireSide;
    let ComponentState::Wire(wire) = graph.nodes[node.index()].state else {
        return false;
    };
    wire.north == RedstoneWireSide::Side
        && wire.south == RedstoneWireSide::Side
        && wire.east == RedstoneWireSide::None
        && wire.west == RedstoneWireSide::None
        && matches!(graph.nodes[node.index()].raw_state.map(Block::from_id), Some(Block::RedstoneWire { .. }))
}

fn resource_selected_guard(graph: &CompileGraph, guard: GuardId, resource: ResourceId, far: ContactId) -> bool {
    let Guard::All(children) = &graph.guards[guard.index()] else {
        return false;
    };
    children.len() == 2
        && children
            .iter()
            .any(|id| graph.guards[id.index()] == Guard::State(StateTest::ResourceAt { resource, contact: far }))
        && children
            .iter()
            .any(|id| graph.guards[id.index()] == Guard::State(StateTest::ResourceEnabled(resource)))
}

fn native_far_contact(graph: &CompileGraph, far: ContactId, resource: ResourceId) -> bool {
    let variants = &graph.contacts[far.index()].variants;
    variants.len() == 2
        && variants.iter().all(|variant| {
            let profile = graph.profiles[variant.profile.index()];
            if variant.receiver.is_some() || !native_profile(profile) {
                return false;
            }
            match Block::from_id(profile.raw_state) {
                Block::RedstoneBlock => {
                    profile.electrical == ProfileElectrical::RedstoneSource && resource_selected_guard(graph, variant.when, resource, far)
                }
                Block::Air => {
                    profile.electrical == ProfileElectrical::None
                        && matches!(graph.guards[variant.when.index()], Guard::Not(inner) if resource_selected_guard(graph, inner, resource, far))
                }
                _ => false,
            }
        })
}

fn native_near_contact(graph: &CompileGraph, near: ContactId, piston: PistonId) -> bool {
    graph.contacts[near.index()].variants.iter().all(|variant| {
        let profile = graph.profiles[variant.profile.index()];
        if !native_profile(profile) {
            return false;
        }
        match Block::from_id(profile.raw_state) {
            Block::PistonHead { head } => head.facing == BlockFace::South.into() && variant.receiver == Some(CallbackReceiver::PistonHead(piston)),
            Block::Air | Block::MovingPiston { .. } | Block::RedstoneBlock => variant.receiver.is_none(),
            _ => false,
        }
    })
}

fn closed_kernel_site(graph: &CompileGraph, index: &DiscoveryIndex, site: SiteId, components: [NodeId; 4], contacts: [ContactId; 2]) -> bool {
    let spec = graph.sites[site.index()];
    match spec.source {
        SiteSource::Component(node) => components.contains(&node) && graph.nodes[node.index()].position == Some(spec.position),
        SiteSource::Contact(contact) => contacts.contains(&contact) && graph.contacts[contact.index()].position == spec.position,
        SiteSource::Constant(profile) => {
            let profile = graph.profiles[profile.index()];
            let block = Block::from_id(profile.raw_state);
            profile.electrical == ProfileElectrical::None
                && spec.source_strength == 0
                && matches!(block, Block::Air | Block::Stone {} | Block::Sandstone {})
                && profile.solid == block.is_solid()
                && profile.transparent == block.is_transparent()
                && profile.cube == block.is_cube()
                && !index.power_to.contains_key(&PowerTarget::Conductor { site, include_dust: true })
                && !index.power_to.contains_key(&PowerTarget::Conductor { site, include_dust: false })
        }
    }
}

fn native_profile(profile: crate::redpiler::program_graph::Profile) -> bool {
    let block = Block::from_id(profile.raw_state);
    profile.solid == block.is_solid()
        && profile.transparent == block.is_transparent()
        && profile.cube == block.is_cube()
        && profile.electrical
            == if block == Block::RedstoneBlock {
                ProfileElectrical::RedstoneSource
            } else {
                ProfileElectrical::None
            }
}

fn native_kernel_route(graph: &CompileGraph, route: RouteId, fixed: &[(SiteId, CallbackReceiver)], head: Option<(SiteId, PistonId)>) -> bool {
    let mut seen = vec![false; fixed.len()];
    let mut seen_head = false;
    for delivery in &graph.routes[route.index()].deliveries {
        let Some(receiver) = delivery.receiver else {
            continue;
        };
        if delivery.view != OperandView::Captured || delivery.kind != (DeliveryKind::Update { direction: None }) {
            return false;
        }
        if let Some(index) = fixed.iter().position(|entry| *entry == (delivery.target, receiver)) {
            if seen[index] || !matches!(graph.guards[delivery.guard.index()], Guard::Always) {
                return false;
            }
            seen[index] = true;
        } else if let Some((site, piston)) = head {
            if seen_head || delivery.target != site || receiver != CallbackReceiver::PistonHead(piston) {
                return false;
            }
            let Guard::Site {
                site: guard_site,
                view: OperandView::Captured,
                test: SiteTest::Profile(profile),
            } = graph.guards[delivery.guard.index()]
            else {
                return false;
            };
            if guard_site != site || !matches!(Block::from_id(graph.profiles[profile.index()].raw_state), Block::PistonHead { .. }) {
                return false;
            }
            seen_head = true;
        } else {
            return false;
        }
    }
    seen.into_iter().all(|seen| seen) && (head.is_none() || seen_head)
}

fn native_wire_inputs(
    graph: &CompileGraph,
    index: &DiscoveryIndex,
    wire: NodeId,
    partner: NodeId,
    source: Option<(SiteId, ResourceId, ContactId)>,
) -> bool {
    let Some(edges) = index.power_to.get(&PowerTarget::Input {
        node: wire,
        port: InputPort::Main,
    }) else {
        return false;
    };
    let mut partner_seen = false;
    let mut source_seen = false;
    for edge in edges {
        let link = graph.power_links[*edge];
        if graph.sites[link.from.index()].source == SiteSource::Component(partner)
            && link.from_port == PowerPort::Output
            && link.attenuation == 1
            && matches!(graph.guards[link.guard.index()], Guard::Always)
        {
            if partner_seen {
                return false;
            }
            partner_seen = true;
        } else if let Some((site, resource, far)) = source
            && link.from == site
            && link.from_port == PowerPort::Weak(BlockFace::North)
            && link.attenuation == 0
            && resource_selected_guard(graph, link.guard, resource, far)
        {
            if source_seen {
                return false;
            }
            source_seen = true;
        } else if link.from_port != (PowerPort::Conducted { include_dust: false })
            || !unconditional_zero_link(graph, link)
            || !matches!(graph.sites[link.from.index()].source, SiteSource::Constant(_))
            || !closed_kernel_site(graph, index, link.from, [wire, partner, wire, partner], [ContactId::new(u32::MAX); 2])
        {
            return false;
        }
    }
    partner_seen && source_seen == source.is_some()
}

fn single_repeater_input(graph: &CompileGraph, index: &DiscoveryIndex, node: NodeId, source: NodeId, stored: bool) -> bool {
    let Some(edge) = index.sole_power(PowerTarget::Input { node, port: InputPort::Main }) else {
        return false;
    };
    let link = graph.power_links[edge];
    graph.sites[link.from.index()].source == SiteSource::Component(source)
        && unconditional_zero_link(graph, link)
        && link.from_port == if stored { PowerPort::Stored } else { PowerPort::Weak(BlockFace::North) }
}
