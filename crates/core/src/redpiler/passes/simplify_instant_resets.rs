//! Simplifies power reads in a private redstone-token/base-observer reset family.
//!
//! Recognition follows one sticky horizontal actor, its private Near/Far token,
//! downward base observer, fixed normal/QC conductors and two output wires. Each
//! piston input and conductor must have exactly one unconditional, unattenuated
//! contributor. The rewrite substitutes that contributor at the original input;
//! the observer's correctly oriented Strong query becomes its live Output query.
//! A singleton max with zero attenuation is an identity for every strength, even
//! when an observer's powered bit and stored output disagree. Wire Strong faces,
//! input ports and guards remain exact. Sampling and FIFO acceptance therefore
//! see the same power without traversing the two conductors.
//!
//! A stricter south-facing two-wire grammar also receives the existing runtime's
//! instant-reset certificate. It proves the private alternating walk and retains
//! captured repeater/head callbacks; it does not contract the actor lifecycle.
//! State, ticks, resource ownership, lane checks and projection remain general.
//! Unsupported motifs remain general. Discovery indexes tables once, then reads
//! incident edges per actor; memory is linear in those tables. Replacements are
//! planned before a final cancellation check, and repeated application is inert.

use super::{GraphPass, PassContext};
use crate::redpiler::program_graph::{
    CallbackReceiver, CircuitNodeKind, CompileGraph, ComponentState, ContactId, DeliveryKind, Guard, GuardId, InputPort, InstantResetPlan, NodeId,
    OperandView, PistonId, PistonLane, PowerLink, PowerPort, PowerTarget, ProfileElectrical, ResourceId, RouteId, SiteId, SiteSource, SiteTest,
    StateTest,
};
use mchprs_blocks::blocks::Block;
use mchprs_blocks::{BlockFace, BlockPos};
use rustc_hash::FxHashMap;

/// Removes two proved conductor traversals from a private instant-reset motif.
pub(crate) struct SimplifyInstantResets;

impl GraphPass for SimplifyInstantResets {
    fn name(&self) -> &'static str {
        "simplify-instant-resets"
    }

    fn run(&self, graph: &mut CompileGraph, context: &PassContext<'_>) -> Result<(), String> {
        let index = DiscoveryIndex::build(graph, context.monitor)?;
        let candidates = discover_indexed_candidates(graph, &index, context.monitor)?;
        let kernels = infer_kernels(graph, &index, &candidates, context.monitor)?;
        let recognized = candidates.len();
        let certified = kernels.len();
        let mut replacements = Vec::with_capacity(candidates.len() * 2);
        for candidate in candidates {
            check_cancelled(context.monitor)?;
            for (outer, inner) in [(candidate.main_edge, candidate.normal_edge), (candidate.quasi_edge, candidate.qc_edge)] {
                let mut replacement = graph.power_links[outer];
                replacement.from = graph.power_links[inner].from;
                replacement.from_port = if inner == candidate.qc_edge {
                    PowerPort::Output
                } else {
                    graph.power_links[inner].from_port
                };
                replacements.push((outer, replacement));
            }
        }
        check_cancelled(context.monitor)?;
        for (edge, replacement) in replacements {
            graph.power_links[edge] = replacement;
        }
        for (wire, plan) in kernels {
            graph.wires[wire].instant_reset = Some(plan);
        }
        context.monitor.record_preparation_tables(
            [("recognized reset cells", recognized), ("certified output pairs", certified)]
                .into_iter()
                .map(|(table, entries)| crate::redpiler::PreparationTableStatistics {
                    stage: "instant reset specialization",
                    table: table.into(),
                    entries,
                    capacity: entries,
                    estimated_bytes: 0,
                })
                .collect(),
        );
        Ok(())
    }

    fn preserves_dust_paths(&self) -> bool {
        true
    }
    fn preserves_turbo_callback_indices(&self) -> bool {
        true
    }
    fn preserves_wire_query_facts(&self) -> bool {
        true
    }
    fn preserves_instant_resets(&self) -> bool {
        true
    }
}

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
    main_edge: usize,
    /// This identifies the actor's original Quasi input occurrence.
    quasi_edge: usize,
    /// This identifies the normal conductor's single live Strong contributor.
    normal_edge: usize,
    /// This identifies the QC cap's single live observer contributor.
    qc_edge: usize,
}

/// Compile-time reverse indexes, built once rather than rescanned per candidate.
#[derive(Default)]
struct DiscoveryIndex {
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
    fn build(graph: &CompileGraph, monitor: &crate::redpiler::TaskMonitor) -> Result<Self, String> {
        let mut index = Self::default();
        for path in &graph.dust_paths {
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
            if let CircuitNodeKind::Observer { watched, .. } = node.kind
                && let SiteSource::Contact(contact) = graph.sites[watched.index()].source
            {
                index.observers.entry(contact).or_default().push(id);
            }
            if let CircuitNodeKind::Observer { watched, .. } = node.kind
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
#[cfg(test)]
fn discover_candidates(graph: &CompileGraph, monitor: &crate::redpiler::TaskMonitor) -> Result<Vec<InstantResetCandidate>, String> {
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
    let CircuitNodeKind::Observer {
        watched,
        facing: BlockFace::Bottom,
    } = observed.kind
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
        || qc.from_port != PowerPort::Strong(BlockFace::Bottom)
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
    let monitor = crate::redpiler::TaskMonitor::default();
    let index = DiscoveryIndex::build(graph, &monitor).expect("a fresh validation monitor is not cancelled");
    let candidates = discover_indexed_candidates(graph, &index, &monitor).expect("a fresh validation monitor is not cancelled");
    infer_kernels(graph, &index, &candidates, &monitor).expect("a fresh validation monitor is not cancelled")
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

fn check_two_wire_kernel(graph: &CompileGraph, index: &DiscoveryIndex, candidate: &InstantResetCandidate) -> Option<usize> {
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::plot::PlotWorld;
    use crate::plot::worldedit::{load_schematic, paste_clipboard};
    use crate::redpiler::analysis::AnalysisLimits;
    use crate::redpiler::backend::graph::GraphBackend;
    use crate::redpiler::passes::rich;
    use crate::redpiler::program_graph::{ComponentState, EntryState};
    use crate::redpiler::{CompilerOptions, TaskMonitor};
    use crate::world::World;
    use crate::world::storage::Chunk;
    use sha2::{Digest, Sha256};

    const ORIGIN: BlockPos = BlockPos::new(8, 12, 8);

    pub(crate) fn fixture() -> (PlotWorld, (BlockPos, BlockPos)) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/piston-research/test-prefix-20261008/TEST_PISTION_2.schem");
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            "567f61a69ff36e693916ea6069241ba2b56882f384b022fc1d88dd710b4985df"
        );
        let clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
        let mut world = PlotWorld::from_chunks(
            0,
            0,
            (0..3).flat_map(|x| (0..3).map(move |z| Chunk::empty(x, z))).collect(),
            Default::default(),
        );
        paste_clipboard(
            &mut world,
            &clipboard,
            ORIGIN + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z),
            false,
        );
        let bounds = (
            ORIGIN - BlockPos::new(2, 2, 2),
            ORIGIN + BlockPos::new(clipboard.size_x as i32 + 1, clipboard.size_y as i32 + 1, clipboard.size_z as i32 + 1),
        );
        (world, bounds)
    }

    fn graph() -> CompileGraph {
        let (world, bounds) = fixture();
        rich::prepare(&world, bounds, vec![], &TaskMonitor::default(), AnalysisLimits::default())
            .unwrap()
            .graph
    }

    fn apply(graph: &mut CompileGraph, monitor: &TaskMonitor) -> Result<(), String> {
        SimplifyInstantResets.run(
            graph,
            &PassContext {
                monitor,
                options: &CompilerOptions::default(),
                entry: &EntryState {
                    logical_tick: 0,
                    pending_ticks: vec![],
                },
            },
        )
    }

    fn candidate(graph: &CompileGraph) -> InstantResetCandidate {
        let mut found = discover_candidates(graph, &TaskMonitor::default()).unwrap();
        assert_eq!(found.len(), 1, "the pinned original fixture must match structurally");
        found.pop().unwrap()
    }

    #[test]
    fn original_fixture_rewrites_only_two_input_occurrences_and_is_idempotent() {
        let mut graph = graph();
        graph.validate().unwrap();
        let witness = candidate(&graph);
        assert_eq!(graph.resources[witness.resource.index()].contacts.len(), 2);
        assert_eq!(
            graph.nodes[witness.output_wires[0].index()]
                .position
                .unwrap()
                .offset(graph.pistons[witness.piston.index()].facing),
            graph.nodes[witness.output_wires[1].index()].position.unwrap()
        );
        assert_eq!(
            graph.sites[graph.power_links[witness.main_edge].from.index()].position,
            graph.pistons[witness.piston.index()].position.offset(BlockFace::North)
        );
        let before = graph.clone();
        let mut expected = before.clone();
        expected.power_links[witness.main_edge].from = before.power_links[witness.normal_edge].from;
        expected.power_links[witness.main_edge].from_port = before.power_links[witness.normal_edge].from_port;
        expected.power_links[witness.quasi_edge].from = before.power_links[witness.qc_edge].from;
        expected.power_links[witness.quasi_edge].from_port = PowerPort::Output;
        let certificates = infer_instant_resets(&graph);
        assert_eq!(certificates.len(), 1, "the original two-wire boundary must certify");
        for (wire, plan) in certificates {
            expected.wires[wire].instant_reset = Some(plan);
        }
        apply(&mut graph, &TaskMonitor::default()).unwrap();
        assert!(graph == expected, "only the input sources and kernel certificate may change");
        graph.validate().unwrap();
        apply(&mut graph, &TaskMonitor::default()).unwrap();
        assert_eq!(graph, expected);
    }

    #[test]
    fn declines_shared_resource_extra_qc_and_guarded_or_attenuated_inputs() {
        let original = graph();
        let witness = candidate(&original);
        for case in 0..8 {
            let mut changed = original.clone();
            match case {
                0 => {
                    changed.pistons.push(changed.pistons[witness.piston.index()]);
                }
                1 => {
                    changed.power_links.push(changed.power_links[witness.qc_edge]);
                }
                2 => {
                    changed.power_links[witness.main_edge].attenuation = 1;
                }
                3 => {
                    changed.power_links[witness.normal_edge].attenuation = 1;
                }
                4 => {
                    changed.power_links[witness.qc_edge].attenuation = 1;
                }
                5 => {
                    changed.power_links[witness.qc_edge].from_port = PowerPort::Strong(BlockFace::Top);
                }
                6 => {
                    let guard = crate::redpiler::program_graph::GuardId::new(changed.guards.len() as u32);
                    changed
                        .guards
                        .push(Guard::State(crate::redpiler::program_graph::StateTest::PistonExtended(witness.piston)));
                    changed.power_links[witness.quasi_edge].guard = guard;
                }
                7 => {
                    let actor = changed.pistons[witness.piston.index()];
                    let mut foreign = changed.resources[witness.resource.index()].clone();
                    foreign.contacts = vec![actor.base, actor.far].into_boxed_slice();
                    changed.resources.push(foreign);
                    let mut state = changed.initial_resources[witness.resource.index()];
                    state.at = actor.base;
                    changed.initial_resources.push(state);
                }
                _ => unreachable!(),
            }
            let before = changed.clone();
            apply(&mut changed, &TaskMonitor::default()).unwrap();
            assert_eq!(changed, before, "unsafe near-match {case} must remain general");
        }
    }

    #[test]
    fn declines_wrong_watch_typed_or_raw_orientation_and_selectable_sources() {
        let original = graph();
        let witness = candidate(&original);
        for case in 0..6 {
            let mut changed = original.clone();
            match case {
                0 => {
                    let CircuitNodeKind::Observer { watched, .. } = &mut changed.nodes[witness.observer.index()].kind else {
                        unreachable!()
                    };
                    *watched = changed.power_links[witness.main_edge].from;
                }
                1 => {
                    let CircuitNodeKind::Observer { facing, .. } = &mut changed.nodes[witness.observer.index()].kind else {
                        unreachable!()
                    };
                    *facing = BlockFace::Top;
                }
                2 => {
                    let node = &mut changed.nodes[witness.observer.index()];
                    let Block::Observer { mut observer } = Block::from_id(node.raw_state.unwrap()) else {
                        unreachable!()
                    };
                    observer.facing = BlockFace::Top.into();
                    node.raw_state = Some(Block::Observer { observer }.get_id());
                }
                3 => {
                    let site = changed.power_links[witness.qc_edge].from;
                    changed.sites[site.index()].source = SiteSource::Contact(changed.pistons[witness.piston.index()].near);
                }
                4 => {
                    let site = changed.power_links[witness.main_edge].from;
                    changed.sites[site.index()].source = SiteSource::Contact(changed.pistons[witness.piston.index()].near);
                }
                5 => {
                    let mut extra = changed.power_links[witness.qc_edge];
                    let mut alias = changed.sites[extra.from.index()];
                    alias.position = alias.position.offset(BlockFace::East);
                    extra.from = SiteId::new(changed.sites.len() as u32);
                    changed.sites.push(alias);
                    changed.power_links.push(extra);
                }
                _ => unreachable!(),
            }
            let before = changed.clone();
            apply(&mut changed, &TaskMonitor::default()).unwrap();
            assert_eq!(changed, before, "unclosed or wrongly oriented near-match {case}");
        }
    }

    #[test]
    fn cancellation_before_discovery_or_publication_does_not_mutate() {
        let mut graph = graph();
        let before = graph.clone();
        let monitor = TaskMonitor::default();
        monitor.cancel();
        assert!(apply(&mut graph, &monitor).is_err());
        assert_eq!(graph, before);
    }

    #[test]
    fn kernel_declines_changed_capture_boundaries_emitters_and_site_aliases() {
        let original = graph();
        let certificates = infer_instant_resets(&original);
        let [(wire, plan)] = certificates.as_slice() else {
            panic!("the original fixture must have one certificate")
        };
        let (wire, plan) = (*wire, *plan);
        let a = original.wires[wire].node;
        let b = plan.partner;
        let ar = original.wires[wire].turbo_route.unwrap();
        let east = original.wires[wire].canonical_neighbors[0];
        let actor = original.pistons[plan.piston.index()];
        for case in 0..11 {
            let mut changed = original.clone();
            match case {
                0 => {
                    // A constant emitter cannot masquerade as inert through electrical labels.
                    let block = Block::RedstoneBlock;
                    let profile = crate::redpiler::program_graph::ProfileId::new(changed.profiles.len() as u32);
                    changed.profiles.push(crate::redpiler::program_graph::Profile {
                        raw_state: block.get_id(),
                        registry_type: block.registry_id(),
                        solid: block.is_solid(),
                        transparent: block.is_transparent(),
                        cube: block.is_cube(),
                        electrical: ProfileElectrical::None,
                    });
                    changed.sites[east.index()].source = SiteSource::Constant(profile);
                }
                1 => {
                    // A contact alias at another position adds a second source to B.
                    let CircuitNodeKind::Wire(bi) = changed.nodes[b.index()].kind else {
                        unreachable!()
                    };
                    let site = changed.wires[bi].canonical_neighbors[0];
                    changed.sites[site.index()].source = SiteSource::Contact(actor.far);
                }
                2 => {
                    // Distinct same-position capture roots change visited-heading state.
                    let site = changed.wires[wire].canonical_neighbors[5];
                    changed.sites.push(changed.sites[site.index()]);
                }
                3 => {
                    // Repeater admission must be captured, never a live operand.
                    let mut rows = changed.routes[ar.index()].deliveries.to_vec();
                    rows.iter_mut()
                        .find(|row| row.receiver == Some(CallbackReceiver::Component(b)))
                        .unwrap()
                        .view = OperandView::Live;
                    changed.routes[ar.index()].deliveries = rows.into_boxed_slice();
                }
                4 => {
                    // A second incoming callback from a retained input is outside closure.
                    let mut extra = changed.routes[ar.index()]
                        .deliveries
                        .iter()
                        .find(|row| row.receiver == Some(CallbackReceiver::Component(b)))
                        .copied()
                        .unwrap();
                    extra.sequence = u32::MAX;
                    let route = changed
                        .nodes
                        .iter()
                        .find(|node| matches!(node.kind, CircuitNodeKind::Electrical(crate::redpiler::components::NodeType::Lever)))
                        .unwrap()
                        .output_route
                        .unwrap();
                    let mut rows = changed.routes[route.index()].deliveries.to_vec();
                    rows.push(extra);
                    changed.routes[route.index()].deliveries = rows.into_boxed_slice();
                }
                5 => {
                    let ComponentState::Wire(state) = &mut changed.nodes[a.index()].state else {
                        unreachable!()
                    };
                    state.east = mchprs_blocks::blocks::RedstoneWireSide::Side;
                }
                6 => {
                    let air = changed.contacts[actor.far.index()]
                        .variants
                        .iter()
                        .find(|variant| Block::from_id(changed.profiles[variant.profile.index()].raw_state) == Block::Air)
                        .unwrap()
                        .profile;
                    changed.profiles[air.index()].solid = true;
                }
                7 => {
                    let mut link = changed.power_links[0];
                    link.to = PowerTarget::Conductor {
                        site: east,
                        include_dust: false,
                    };
                    changed.power_links.push(link);
                }
                8 => {
                    let mut rows = changed.routes[ar.index()].deliveries.to_vec();
                    let duplicate = *rows.iter().find(|row| row.receiver == Some(CallbackReceiver::Component(b))).unwrap();
                    rows.push(duplicate);
                    changed.routes[ar.index()].deliveries = rows.into_boxed_slice();
                }
                9 => {
                    changed.dust_paths.push(crate::redpiler::program_graph::DustPathPlan {
                        input_node: plan.observer,
                        input_site: east,
                        input_face: BlockFace::North,
                        wires: vec![a, b].into_boxed_slice(),
                    });
                }
                10 => {
                    let mut rows = changed.routes[ar.index()].deliveries.to_vec();
                    rows.iter_mut()
                        .find(|row| row.receiver == Some(CallbackReceiver::PistonHead(plan.piston)))
                        .unwrap()
                        .guard = crate::redpiler::program_graph::GuardId::new(0);
                    changed.routes[ar.index()].deliveries = rows.into_boxed_slice();
                }
                _ => unreachable!(),
            }
            assert!(infer_instant_resets(&changed).is_empty(), "kernel near-match {case} must decline");
        }
    }

    #[test]
    fn two_wire_heading_grammar_declines_every_non_south_facing() {
        let original = graph();
        let witness = candidate(&original);
        for facing in [BlockFace::North, BlockFace::East, BlockFace::West, BlockFace::Top, BlockFace::Bottom] {
            let mut changed = original.clone();
            changed.pistons[witness.piston.index()].facing = facing;
            let index = DiscoveryIndex::build(&changed, &TaskMonitor::default()).unwrap();
            assert!(check_two_wire_kernel(&changed, &index, &witness).is_none(), "unproved heading {facing:?}");
        }
    }

    fn assert_cells_equal(left: &PlotWorld, right: &PlotWorld, bounds: (BlockPos, BlockPos)) {
        for y in bounds.0.y..=bounds.1.y {
            for z in bounds.0.z..=bounds.1.z {
                for x in bounds.0.x..=bounds.1.x {
                    let position = BlockPos::new(x, y, z);
                    assert_eq!(left.get_block(position), right.get_block(position), "cell {position:?}");
                    assert_eq!(
                        serde_json::to_value(left.get_block_entity(position)).unwrap(),
                        serde_json::to_value(right.get_block_entity(position)).unwrap(),
                        "entity {position:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn all_strengths_and_observer_power_disagreement_preserve_execution_and_reset() {
        let original = graph();
        let witness = candidate(&original);
        let input_site = original.power_links[witness.normal_edge].from;
        let SiteSource::Component(input_wire) = original.sites[input_site.index()].source else {
            unreachable!()
        };
        for strength in 0..=15 {
            for powered in [false, true] {
                let mut general = original.clone();
                let ComponentState::Electrical(observer) = &mut general.nodes[witness.observer.index()].state else {
                    unreachable!()
                };
                observer.powered = powered;
                observer.output_strength = strength;
                let ComponentState::Wire(wire) = &mut general.nodes[input_wire.index()].state else {
                    unreachable!()
                };
                wire.power = 15 - strength;
                let mut simplified = general.clone();
                apply(&mut simplified, &TaskMonitor::default()).unwrap();
                assert_ne!(general.power_links, simplified.power_links);
                let (program, state) = general.lower().unwrap();
                let mut baseline = GraphBackend::new(program, state, 0).unwrap();
                let (program, state) = simplified.lower().unwrap();
                let mut optimized = GraphBackend::new(program, state, 0).unwrap();
                let (mut baseline_world, bounds) = fixture();
                let (mut optimized_world, _) = fixture();
                for tick in 0..12 {
                    if [0, 1, 4, 5, 8].contains(&tick) {
                        baseline.use_block(ORIGIN + BlockPos::new(0, 2, 0)).unwrap();
                        optimized.use_block(ORIGIN + BlockPos::new(0, 2, 0)).unwrap();
                    }
                    baseline.tick().unwrap();
                    optimized.tick().unwrap();
                    baseline.flush(&mut baseline_world, false);
                    optimized.flush(&mut optimized_world, false);
                    assert_cells_equal(&baseline_world, &optimized_world, bounds);
                    assert_eq!(baseline.execution_counts().piston_actions, optimized.execution_counts().piston_actions);
                    assert_eq!(baseline.execution_counts().piston_samples, optimized.execution_counts().piston_samples);
                }
                baseline.reset(&mut baseline_world);
                optimized.reset(&mut optimized_world);
                assert_cells_equal(&baseline_world, &optimized_world, bounds);
                assert_eq!(
                    format!("{:?}", baseline_world.scheduler().iter_entries().collect::<Vec<_>>()),
                    format!("{:?}", optimized_world.scheduler().iter_entries().collect::<Vec<_>>())
                );
            }
        }
    }
}
