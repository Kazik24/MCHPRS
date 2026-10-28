//! Dense local bindings for Turbo wire corridors.
//!
//! The exact product admits queue-only corridors with no conditional transfer
//! inputs. The guarded product additionally records geometry-dependent transfer
//! slots and receiver classes that require ordered callback execution. Every
//! compiled wire-power input must be a fixed member wire; selectable contacts
//! that may contain wire, external wire inputs, constant dust, and wires owned
//! by another execution plan decline the whole corridor.
//! Non-wire source power is read live by the runtime and is stable during an
//! episode because admitted external receivers only enqueue work. Before
//! admission, the compiler follows the actual no-dust PowerLink dependencies
//! for all six cardinal samples and rejects any path back to a member wire;
//! mixed-classic drive operands receive the same interior-source check. Callback
//! admission itself remains live at each captured sample and is never baked
//! from the component's current powered/resource state.
//!
//! `neighbors` uses the original 24 canonical slots, but stores only sites that
//! can affect a transfer input, wire-visit heading, or captured update receiver.
//! The per-net union of such sites is copied into every row where it occurs:
//! native first-discovery captures and layer suppression are shared by all
//! source wires, including a row whose own callback guard rejects that site.
//! Omitted slots cannot dispatch or be sampled by the admitted wire-power
//! expression. `site_wire` points only at corridor members, preserving each
//! wire's independent state. The exact transport backend runs the original
//! three rotating FIFO queues without physical neighborhood discovery. Guarded
//! execution retains conditional masks and topology sites, keeps delayed
//! receiver operations ordered, and checks repeater emission state before
//! effects. This compiler does not collapse either product to a settled Boolean
//! equation.
use super::*;
use rustc_hash::{FxHashMap, FxHashSet};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct InstantTransport {
    pub(crate) nets: Vec<TransportNet>,
    pub(crate) stats: TransportCompileStats,
    /// This product preserves the topology guards needed by conditional Turbo
    /// transfers. The unguarded product remains the original exact certificate.
    pub(crate) guarded: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct TransportCompileStats {
    pub(crate) corridors: usize,
    pub(crate) admitted: usize,
    pub(crate) too_small: usize,
    pub(crate) owner_overlap: usize,
    pub(crate) no_route_or_facts: usize,
    pub(crate) conditional_input: usize,
    pub(crate) no_dust_dependency: usize,
    pub(crate) unsafe_receiver: usize,
    pub(crate) unsafe_input: usize,
    pub(crate) unsupported: usize,
}

#[derive(Clone, Copy)]
enum DeclineReason {
    NoRouteOrFacts,
    ConditionalInput,
    NoDustDependency,
    UnsafeReceiver,
    UnsafeInput,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TransportNet {
    pub(crate) wires: Box<[TransportWire]>,
    /// Dense site table referenced by `TransportWire::neighbors`.
    pub(crate) sites: Box<[SiteId]>,
    /// Maps captured sites to a local wire index, or `u32::MAX` when non-wire.
    pub(crate) site_wire: Box<[u32]>,
    /// Set when receiver callbacks include stateful delayed devices; these must
    /// be replayed at their original ordered call cuts by the backend.
    pub(crate) ordered_receivers: bool,
    /// Repeater receiver nodes whose emitted-strength state must be canonical
    /// before guarded transport can mutate anything.
    pub(crate) canonical_repeaters: Box<[NodeId]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TransportWire {
    pub(crate) node: NodeId,
    /// Index of this wire's root site in `TransportNet::sites`.
    pub(crate) site: u32,
    /// Its canonical 24 Turbo slots, mapped to dense sites.
    pub(crate) neighbors: [u32; 24],
    /// Slots which contribute a changing internal wire strength to the Turbo query.
    pub(crate) inputs: u32,
    /// Conditional Turbo wire-strength input slots, separate from `inputs`.
    pub(crate) conditional_inputs: u32,
    pub(crate) route: RouteId,
}

pub(crate) fn infer(graph: &CompileGraph, monitor: &crate::redpiler::TaskMonitor) -> Result<InstantTransport, String> {
    infer_mode(graph, monitor, false)
}

pub(crate) fn infer_guarded(graph: &CompileGraph, monitor: &crate::redpiler::TaskMonitor) -> Result<InstantTransport, String> {
    infer_mode(graph, monitor, true)
}

#[cfg(test)]
pub(crate) fn infer_guarded_with_test_corridors(
    graph: &CompileGraph,
    monitor: &crate::redpiler::TaskMonitor,
    corridors: Vec<WireCorridor>,
) -> Result<InstantTransport, String> {
    infer_mode_with_discovery(graph, monitor, true, |_, _| Ok(corridors))
}
fn infer_mode(graph: &CompileGraph, monitor: &crate::redpiler::TaskMonitor, guarded: bool) -> Result<InstantTransport, String> {
    infer_mode_with_discovery(
        graph,
        monitor,
        guarded,
        crate::redpiler::analysis::instant::boundaries::discover_transport_corridors,
    )
}

fn infer_mode_with_discovery(
    graph: &CompileGraph,
    monitor: &crate::redpiler::TaskMonitor,
    guarded: bool,
    discover: impl FnOnce(&CompileGraph, &crate::redpiler::TaskMonitor) -> Result<Vec<WireCorridor>, String>,
) -> Result<InstantTransport, String> {
    if monitor.cancelled() {
        return Err("cancelled while compiling instant transport".into());
    }
    let corridors = match discover(graph, monitor) {
        Ok(corridors) => corridors,
        Err(error)
            if !monitor.cancelled()
                && matches!(
                    error.as_str(),
                    "instant-boundary work budget overflow"
                        | "instant-boundary work budget exceeded"
                        | "instant-boundary byte budget overflow"
                        | "instant-boundary retained-table budget exceeded"
                        | "wire-corridor index exceeds the bounded preparation allocation"
                ) =>
        {
            monitor.record_warning(format!("transport discovery declined: {error}"));
            return Ok(InstantTransport {
                guarded,
                ..Default::default()
            });
        }
        Err(error) => return Err(error),
    };
    let mut output = InstantTransport::default();
    output.guarded = guarded;
    let mut bytes = 0usize;
    let incoming: FxHashMap<_, Vec<_>> = graph.power_links.iter().fold(FxHashMap::default(), |mut index, link| {
        index.entry(link.to).or_default().push(link);
        index
    });
    let mut node_roots = FxHashMap::default();
    for (index, site) in graph.sites.iter().enumerate() {
        if let SiteSource::Component(node) = site.source {
            node_roots.insert(node, SiteId::new(index as u32));
        }
    }
    let mut excluded_wires = FxHashSet::default();
    for net in &graph.plans.classic_nets {
        excluded_wires.extend(net.wires.iter().map(|wire| wire.node));
    }
    for path in &graph.plans.dust_paths {
        excluded_wires.extend(path.wires.iter().copied());
    }
    for proxy in &graph.plans.instant.proxies {
        excluded_wires.extend(proxy.routes.iter().map(|route| graph.routes[route.route.index()].source));
    }
    for wire in &graph.wires {
        if wire.execution.instant_reset.is_some() || wire.execution.instant_node.is_some() {
            excluded_wires.insert(wire.node);
        }
    }
    for corridor in &corridors {
        output.stats.corridors += 1;
        if monitor.cancelled() {
            return Err("cancelled while compiling instant transport".into());
        }
        if corridor.wires.len() < 2 && !guarded {
            output.stats.too_small += 1;
            continue;
        }
        if corridor.wires.iter().any(|node| excluded_wires.contains(node)) {
            output.stats.owner_overlap += 1;
            continue;
        }
        let members: FxHashSet<_> = corridor.wires.iter().copied().collect();
        let mut sites = Vec::new();
        let mut site_index = FxHashMap::default();
        let mut receiver_sites = FxHashSet::default();
        let mut wires = Vec::with_capacity(corridor.wires.len());
        let mut accepted = true;
        let mut decline = None;
        for &node in corridor.wires.iter() {
            let CircuitNodeKind::Wire(index) = graph.nodes[node.index()].kind else {
                accepted = false;
                decline = Some(DeclineReason::Unsupported);
                break;
            };
            let source = &graph.wires[index];
            let Some(route) = source.turbo_route else {
                accepted = false;
                decline = Some(DeclineReason::NoRouteOrFacts);
                break;
            };
            let facts = source.execution.query_facts.unwrap_or_else(|| WireQueryFacts::infer(graph, source));
            let Some(transfer) = facts.transfer else {
                accepted = false;
                decline = Some(DeclineReason::NoRouteOrFacts);
                break;
            };
            if (!guarded && transfer.turbo.conditional != 0) || !safe_geometry(graph, source, &members) {
                accepted = false;
                decline = Some(DeclineReason::ConditionalInput);
                break;
            }
            if !stable_block_power(graph, source, &members, &incoming) {
                accepted = false;
                decline = Some(DeclineReason::NoDustDependency);
                break;
            }
            let Some(&root) = node_roots.get(&node) else {
                accepted = false;
                decline = Some(DeclineReason::Unsupported);
                break;
            };
            let root_local = dense_site(root, &mut sites, &mut site_index);
            let selection = selection_for(graph, source, index);
            let (receiver_mask, receivers_safe) = turbo_receiver_mask(graph, source, route, selection, &members, guarded);
            if !receivers_safe {
                accepted = false;
                decline = Some(DeclineReason::UnsafeReceiver);
            }
            let mut neighbors = [u32::MAX; 24];
            let mut inputs = 0u32;
            let mut conditional_inputs = 0u32;
            for (slot, &site) in source.canonical_neighbors.iter().enumerate() {
                let mut retain = false;
                let input_bit = 1 << slot;
                let direct_input = transfer.turbo.direct & input_bit != 0;
                let conditional_input = transfer.turbo.conditional & input_bit != 0;
                if direct_input || (guarded && conditional_input) {
                    match graph.sites[site.index()].source {
                        SiteSource::Component(upstream) if members.contains(&upstream) => {
                            if direct_input {
                                inputs |= input_bit;
                            }
                            if conditional_input {
                                conditional_inputs |= input_bit;
                            }
                            retain = true;
                        }
                        _ if guarded || site_may_be_wire(graph, site) => {
                            accepted = false;
                            decline = Some(DeclineReason::UnsafeInput);
                        }
                        _ => {}
                    }
                }
                let receiver = receiver_mask & (1 << slot) != 0;
                if receiver && !safe_receiver_site(graph, site, &members, guarded) {
                    accepted = false;
                    decline = Some(DeclineReason::UnsafeReceiver);
                }
                if receiver {
                    retain = true;
                    receiver_sites.insert(site);
                }
                if matches!(graph.sites[site.index()].source, SiteSource::Component(upstream) if members.contains(&upstream)) {
                    // Visited wires determine the native first-arrival heading even when this slot is not a power input.
                    retain = true;
                }
                if guarded && conditional_input {
                    // Horizontal diagonal edge tests inspect their horizontal
                    // neighbor; ascent tests additionally inspect the center-up
                    // site. Retain those captures even when they cannot receive.
                    let geometry_slots: &[usize] = match slot {
                        7 => &[0],
                        8 => &[0, 3],
                        12 => &[1],
                        13 => &[1, 3],
                        17 => &[4],
                        20 => &[4, 3],
                        18 => &[5],
                        21 => &[5, 3],
                        _ => {
                            accepted = false;
                            decline = Some(DeclineReason::Unsupported);
                            &[]
                        }
                    };
                    for &geometry_slot in geometry_slots {
                        neighbors[geometry_slot] = dense_site(source.canonical_neighbors[geometry_slot], &mut sites, &mut site_index);
                    }
                }
                if let SiteSource::Constant(profile) = graph.sites[site.index()].source {
                    if matches!(
                        mchprs_blocks::blocks::Block::from_id(graph.profiles[profile.index()].raw_state),
                        mchprs_blocks::blocks::Block::RedstoneWire { .. }
                    ) {
                        accepted = false;
                        decline = Some(DeclineReason::UnsafeInput);
                    }
                }
                if retain {
                    neighbors[slot] = dense_site(site, &mut sites, &mut site_index);
                }
            }
            for (slot, &site) in source.canonical_neighbors.iter().enumerate() {
                if matches!(graph.sites[site.index()].source, SiteSource::Component(upstream) if members.contains(&upstream))
                    && !guarded
                    && !selection.is_some_and(|selected| selected.known & (1 << slot) != 0)
                {
                    accepted = false;
                    decline = Some(DeclineReason::UnsafeReceiver);
                }
            }
            if !accepted {
                break;
            }
            wires.push(TransportWire {
                node,
                site: root_local,
                neighbors,
                inputs,
                conditional_inputs,
                route,
            });
        }
        if !accepted || wires.len() != corridor.wires.len() {
            match decline.unwrap_or(DeclineReason::Unsupported) {
                DeclineReason::NoRouteOrFacts => output.stats.no_route_or_facts += 1,
                DeclineReason::ConditionalInput => output.stats.conditional_input += 1,
                DeclineReason::NoDustDependency => output.stats.no_dust_dependency += 1,
                DeclineReason::UnsafeReceiver => output.stats.unsafe_receiver += 1,
                DeclineReason::UnsafeInput => output.stats.unsafe_input += 1,
                DeclineReason::Unsupported => output.stats.unsupported += 1,
            }
            continue;
        }
        // Turbo's layer and first-capture tables are shared across source wires.
        // A site that can receive from any row must therefore exist in every
        // row where it is a canonical neighbor, even when that row rejects it.
        retain_shared_site_occurrences(graph, &mut wires, &site_index);
        let local_wires: FxHashMap<_, _> = wires.iter().enumerate().map(|(index, wire)| (wire.node, index as u32)).collect();
        let site_wire = sites
            .iter()
            .map(|site| match graph.sites[site.index()].source {
                SiteSource::Component(node) => local_wires.get(&node).copied().unwrap_or(u32::MAX),
                _ => u32::MAX,
            })
            .collect::<Vec<_>>();
        let mut canonical_repeaters = Vec::new();
        let mut ordered_receivers = false;
        if guarded {
            for site in receiver_sites {
                let receiver_nodes = site_receiver_nodes(graph, site);
                for node in receiver_nodes {
                    match graph.nodes[node.index()].kind {
                        CircuitNodeKind::Electrical(crate::redpiler::components::NodeType::Repeater { .. }) => canonical_repeaters.push(node),
                        CircuitNodeKind::Electrical(
                            crate::redpiler::components::NodeType::Torch | crate::redpiler::components::NodeType::Comparator { .. },
                        ) => ordered_receivers = true,
                        _ => {}
                    }
                }
            }
            canonical_repeaters.sort_unstable_by_key(|node| node.index());
            canonical_repeaters.dedup();
            ordered_receivers |= !canonical_repeaters.is_empty();
        }
        bytes = bytes.saturating_add(
            wires.len() * std::mem::size_of::<TransportWire>()
                + sites.len() * (std::mem::size_of::<SiteId>() + std::mem::size_of::<u32>())
                + canonical_repeaters.len() * std::mem::size_of::<NodeId>(),
        );
        if bytes > crate::redpiler::analysis::MAX_GRAPH_PREPARATION_BYTES / 8 {
            monitor.record_warning("transport discovery declined: transport tables exceed preparation memory bound".into());
            return Ok(InstantTransport {
                guarded,
                ..Default::default()
            });
        }
        output.nets.push(TransportNet {
            wires: wires.into_boxed_slice(),
            sites: sites.into_boxed_slice(),
            site_wire: site_wire.into_boxed_slice(),
            ordered_receivers,
            canonical_repeaters: canonical_repeaters.into_boxed_slice(),
        });
        output.stats.admitted += 1;
    }
    Ok(output)
}

fn retain_shared_site_occurrences(graph: &CompileGraph, wires: &mut [TransportWire], site_index: &FxHashMap<SiteId, u32>) {
    for transport_wire in wires {
        let CircuitNodeKind::Wire(index) = graph.nodes[transport_wire.node.index()].kind else {
            unreachable!("transport rows were certified as wires above")
        };
        for (slot, &site) in graph.wires[index].canonical_neighbors.iter().enumerate() {
            if let Some(&local_site) = site_index.get(&site) {
                transport_wire.neighbors[slot] = local_site;
            }
        }
    }
}

fn stable_block_power(
    graph: &CompileGraph,
    wire: &WirePlan,
    members: &FxHashSet<NodeId>,
    incoming: &FxHashMap<PowerTarget, Vec<&PowerLink>>,
) -> bool {
    if let CircuitNodeKind::Wire(index) = graph.nodes[wire.node.index()].kind
        && let Some(mixed) = graph.plans.mixed_classic.wires.get(index)
    {
        for operand in &graph.plans.mixed_classic.operands[mixed.fixed.start as usize..mixed.fixed.end as usize] {
            if matches!(
                operand.source,
                FixedClassicSource::LiveOutput(node) | FixedClassicSource::CapturedPowered { node, .. }
                    if members.contains(&node)
            ) {
                return false;
            }
        }
    }
    // Turbo block power samples six cardinal neighbors; this slot mapping includes
    // both vertical faces, even though the wire-strength max uses four horizontals.
    for slot in [3, 2, 4, 5, 1, 0] {
        if !conducted_source_independent(
            PowerTarget::Conductor {
                site: wire.canonical_neighbors[slot],
                include_dust: false,
            },
            graph,
            members,
            incoming,
            &mut FxHashSet::default(),
            &mut FxHashSet::default(),
        ) {
            return false;
        }
    }
    true
}

fn conducted_source_independent(
    target: PowerTarget,
    graph: &CompileGraph,
    members: &FxHashSet<NodeId>,
    incoming: &FxHashMap<PowerTarget, Vec<&PowerLink>>,
    visiting: &mut FxHashSet<PowerTarget>,
    complete: &mut FxHashSet<PowerTarget>,
) -> bool {
    if complete.contains(&target) {
        return true;
    }
    if !visiting.insert(target) {
        return false;
    }
    let mut independent = true;
    if let Some(links) = incoming.get(&target) {
        for link in links {
            let source_depends_on_member = match graph.sites[link.from.index()].source {
                SiteSource::Component(node) => members.contains(&node),
                SiteSource::Contact(contact) => graph.contacts[contact.index()]
                    .variants
                    .iter()
                    .any(|variant| matches!(variant.receiver, Some(CallbackReceiver::Component(node)) if members.contains(&node))),
                SiteSource::Constant(_) => false,
            };
            if source_depends_on_member {
                independent = false;
                break;
            }
            if let PowerPort::Conducted { include_dust } = link.from_port {
                independent &= conducted_source_independent(
                    PowerTarget::Conductor {
                        site: link.from,
                        include_dust,
                    },
                    graph,
                    members,
                    incoming,
                    visiting,
                    complete,
                );
            }
        }
    }
    visiting.remove(&target);
    if independent {
        complete.insert(target);
    }
    independent
}

fn dense_site(site: SiteId, sites: &mut Vec<SiteId>, indices: &mut FxHashMap<SiteId, u32>) -> u32 {
    *indices.entry(site).or_insert_with(|| {
        let index = u32::try_from(sites.len()).expect("validated site count fits u32");
        sites.push(site);
        index
    })
}

fn selection_for(graph: &CompileGraph, wire: &WirePlan, index: usize) -> Option<WireCallbackSelection> {
    wire.execution.callback_selection.or_else(|| WireCallbackSelection::infer(graph, index))
}

fn turbo_receiver_mask(
    graph: &CompileGraph,
    wire: &WirePlan,
    route: RouteId,
    selection: Option<WireCallbackSelection>,
    members: &FxHashSet<NodeId>,
    guarded: bool,
) -> (u32, bool) {
    let mut slots = FxHashMap::default();
    for (slot, site) in wire.canonical_neighbors.iter().enumerate() {
        *slots.entry(*site).or_insert(0u32) |= 1 << slot;
    }
    let mut mask = 0;
    let mut safe = true;
    for delivery in graph.routes[route.index()].deliveries.iter() {
        if delivery.receiver.is_none() {
            continue;
        }
        let Some(&target_slots) = slots.get(&delivery.target) else { continue };
        let mut candidates = target_slots;
        while candidates != 0 {
            let slot = candidates.trailing_zeros() as usize;
            candidates &= candidates - 1;
            if selection.is_some_and(|selection| selection.known & (1 << slot) != 0 && selection.admitted & (1 << slot) == 0) {
                continue;
            }
            safe &= delivery.receiver.is_some_and(|receiver| safe_receiver(graph, receiver, members, guarded));
            mask |= 1 << slot;
        }
    }
    (mask, safe)
}

fn site_may_be_wire(graph: &CompileGraph, site: SiteId) -> bool {
    let is_wire = |profile: ProfileId| {
        matches!(
            mchprs_blocks::blocks::Block::from_id(graph.profiles[profile.index()].raw_state),
            mchprs_blocks::blocks::Block::RedstoneWire { .. }
        )
    };
    match graph.sites[site.index()].source {
        SiteSource::Constant(profile) => is_wire(profile),
        SiteSource::Component(node) => matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Wire(_)),
        SiteSource::Contact(contact) => graph.contacts[contact.index()].variants.iter().any(|variant| is_wire(variant.profile)),
    }
}

fn safe_geometry(graph: &CompileGraph, wire: &WirePlan, members: &FxHashSet<NodeId>) -> bool {
    wire.canonical_neighbors.iter().all(|site| match graph.sites[site.index()].source {
        SiteSource::Constant(profile) => !matches!(
            mchprs_blocks::blocks::Block::from_id(graph.profiles[profile.index()].raw_state),
            mchprs_blocks::blocks::Block::RedstoneWire { .. }
        ),
        SiteSource::Component(node) => !matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Wire(_)) || members.contains(&node),
        SiteSource::Contact(contact) => !graph.contacts[contact.index()].variants.iter().any(|variant| {
            matches!(
                mchprs_blocks::blocks::Block::from_id(graph.profiles[variant.profile.index()].raw_state),
                mchprs_blocks::blocks::Block::RedstoneWire { .. }
            )
        }),
    })
}

fn safe_receiver(graph: &CompileGraph, receiver: CallbackReceiver, members: &FxHashSet<NodeId>, guarded: bool) -> bool {
    match receiver {
        CallbackReceiver::PistonBase(_) | CallbackReceiver::PistonHead(_) | CallbackReceiver::Contact(_) => true,
        CallbackReceiver::Component(node) if members.contains(&node) => matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Wire(_)),
        CallbackReceiver::Component(node) => match graph.nodes[node.index()].kind {
            CircuitNodeKind::Piston(_) | CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. } => true,
            CircuitNodeKind::Electrical(
                crate::redpiler::components::NodeType::Lever
                | crate::redpiler::components::NodeType::Button
                | crate::redpiler::components::NodeType::PressurePlate
                | crate::redpiler::components::NodeType::Constant,
            ) => true,
            CircuitNodeKind::Electrical(
                crate::redpiler::components::NodeType::Torch
                | crate::redpiler::components::NodeType::Comparator { .. }
                | crate::redpiler::components::NodeType::Repeater { .. },
            ) => guarded,
            _ => false,
        },
    }
}

fn safe_receiver_site(graph: &CompileGraph, site: SiteId, members: &FxHashSet<NodeId>, guarded: bool) -> bool {
    let site_spec = &graph.sites[site.index()];
    match site_spec.source {
        SiteSource::Contact(contact) => graph.contacts[contact.index()]
            .variants
            .iter()
            .all(|variant| variant.receiver.is_none_or(|receiver| safe_receiver(graph, receiver, members, guarded))),
        SiteSource::Component(node) => safe_receiver(
            graph,
            match graph.nodes[node.index()].kind {
                CircuitNodeKind::Piston(actor) => CallbackReceiver::PistonBase(actor),
                _ => CallbackReceiver::Component(node),
            },
            members,
            guarded,
        ),
        SiteSource::Constant(_) => true,
    }
}

fn site_receiver_nodes(graph: &CompileGraph, site: SiteId) -> Vec<NodeId> {
    match graph.sites[site.index()].source {
        SiteSource::Component(node) => vec![node],
        SiteSource::Contact(contact) => graph.contacts[contact.index()]
            .variants
            .iter()
            .filter_map(|variant| match variant.receiver {
                Some(CallbackReceiver::Component(node)) => Some(node),
                _ => None,
            })
            .collect(),
        SiteSource::Constant(_) => Vec::new(),
    }
}

/// Test-only explanation of transport coverage. Kept beside the private
/// certificate predicates so the ignored large-device census reports the
/// same reasons the compiler actually uses without adding runtime plan data.
#[cfg(test)]
impl CompileGraph {
    pub(crate) fn instant_transport_rejection_report(&self) -> serde_json::Value {
        let corridors =
            match crate::redpiler::analysis::instant::boundaries::discover_transport_corridors(self, &crate::redpiler::TaskMonitor::default()) {
                Ok(corridors) => corridors,
                Err(error) => return serde_json::json!({"error": error}),
            };
        let mut totals = FxHashMap::<String, (usize, usize)>::default();
        let mut subreason_totals = FxHashMap::<String, (usize, usize)>::default();
        let mut receiver_totals = FxHashMap::<String, (usize, usize)>::default();
        let mut witnesses = Vec::new();
        let mut conditional_topology = Vec::new();
        let mut admitted_nodes = FxHashSet::default();
        let guarded = self.plans.instant_transport.guarded;
        let root_nodes: FxHashSet<_> = self
            .sites
            .iter()
            .filter_map(|site| match site.source {
                SiteSource::Component(node) => Some(node),
                _ => None,
            })
            .collect();
        for net in &self.plans.instant_transport.nets {
            admitted_nodes.extend(net.wires.iter().map(|wire| wire.node));
        }
        let incoming: FxHashMap<_, Vec<_>> = self.power_links.iter().fold(FxHashMap::default(), |mut index, link| {
            index.entry(link.to).or_default().push(link);
            index
        });
        let mut excluded_wires = FxHashSet::default();
        for net in &self.plans.classic_nets {
            excluded_wires.extend(net.wires.iter().map(|wire| wire.node));
        }
        for path in &self.plans.dust_paths {
            excluded_wires.extend(path.wires.iter().copied());
        }
        for proxy in &self.plans.instant.proxies {
            excluded_wires.extend(proxy.routes.iter().map(|route| self.routes[route.route.index()].source));
        }
        for wire in &self.wires {
            if wire.execution.instant_reset.is_some() || wire.execution.instant_node.is_some() {
                excluded_wires.insert(wire.node);
            }
        }

        for (corridor_index, corridor) in corridors.iter().enumerate() {
            if corridor.wires.iter().all(|node| admitted_nodes.contains(node)) {
                continue;
            }
            let count = corridor.wires.len();
            let mut reason = "unsupported".to_owned();
            let mut detail = serde_json::Value::Null;
            let members = FxHashSet::from_iter(corridor.wires.iter().copied());
            if count < 2 && !guarded {
                reason = "too_small".into();
            } else if corridor.wires.iter().any(|node| excluded_wires.contains(node)) {
                reason = "owner_overlap".into();
            } else {
                'wire: for &node in corridor.wires.iter() {
                    let CircuitNodeKind::Wire(index) = self.nodes[node.index()].kind else {
                        reason = "unsupported".into();
                        detail = witness_wire(self, node, "non_wire_member", serde_json::Value::Null);
                        break;
                    };
                    let source = &self.wires[index];
                    let Some(route) = source.turbo_route else {
                        reason = "no_route_or_facts".into();
                        detail = witness_wire(self, node, "missing_turbo_route", serde_json::Value::Null);
                        break;
                    };
                    let facts = source.execution.query_facts.unwrap_or_else(|| WireQueryFacts::infer(self, source));
                    let Some(transfer) = facts.transfer else {
                        reason = "no_route_or_facts".into();
                        detail = witness_wire(self, node, "missing_transfer_facts", serde_json::Value::Null);
                        break;
                    };
                    if transfer.turbo.conditional != 0 && !guarded {
                        reason = "conditional_input".into();
                        detail = witness_wire(
                            self,
                            node,
                            "actual_turbo_conditional_input",
                            serde_json::json!({"mask": format!("0x{:06x}", transfer.turbo.conditional)}),
                        );
                        conditional_topology.push(conditional_topology_summary(
                            self,
                            corridor_index,
                            corridor,
                            &members,
                            &incoming,
                            &root_nodes,
                            &excluded_wires,
                        ));
                        break;
                    }
                    if !safe_geometry(self, source, &members) {
                        reason = "conditional_input".into();
                        detail = witness_wire(self, node, "safe_geometry_blanket_rejection", geometry_witness(self, source, &members));
                        break;
                    }
                    if !stable_block_power(self, source, &members, &incoming) {
                        reason = "no_dust_dependency".into();
                        detail = witness_wire(self, node, "member_wire_in_no_dust_dependency", serde_json::Value::Null);
                        break;
                    }
                    if !root_nodes.contains(&node) {
                        reason = "unsupported".into();
                        detail = witness_wire(self, node, "missing_root_site", serde_json::Value::Null);
                        break;
                    }
                    let selection = selection_for(self, source, index);
                    let (receiver_mask, receiver_routes_safe) = turbo_receiver_mask(self, source, route, selection, &members, guarded);
                    let mut wire_accepted = true;
                    if !receiver_routes_safe {
                        wire_accepted = false;
                        reason = "unsafe_receiver".into();
                        detail = unsafe_route_witness(self, source, route, selection, &members);
                    }
                    for (slot, &site) in source.canonical_neighbors.iter().enumerate() {
                        if (transfer.turbo.direct | if guarded { transfer.turbo.conditional } else { 0 }) & (1 << slot) != 0
                            && !matches!(self.sites[site.index()].source, SiteSource::Component(upstream) if members.contains(&upstream))
                            && (guarded || site_may_be_wire(self, site))
                        {
                            wire_accepted = false;
                            reason = "unsafe_input".into();
                            detail = witness_wire(
                                self,
                                node,
                                "external_or_selectable_wire_input",
                                serde_json::json!({"slot": slot, "site": site.index()}),
                            );
                        }
                        if receiver_mask & (1 << slot) != 0 && !safe_receiver_site(self, site, &members, guarded) {
                            wire_accepted = false;
                            reason = "unsafe_receiver".into();
                            detail = unsafe_site_witness(self, node, slot, site, &members);
                        }
                    }
                    for (slot, &site) in source.canonical_neighbors.iter().enumerate() {
                        if matches!(self.sites[site.index()].source, SiteSource::Component(upstream) if members.contains(&upstream))
                            && !guarded
                            && !selection.is_some_and(|selected| selected.known & (1 << slot) != 0)
                        {
                            wire_accepted = false;
                            reason = "unsafe_receiver".into();
                            detail = witness_wire(
                                self,
                                node,
                                "unknown_internal_wire_selection",
                                serde_json::json!({"slot": slot, "site": site.index()}),
                            );
                        }
                    }
                    if !wire_accepted {
                        break 'wire;
                    }
                }
            }
            let entry = totals.entry(reason.clone()).or_default();
            entry.0 += 1;
            entry.1 += count;
            let subreason = detail.get("reason").and_then(serde_json::Value::as_str).unwrap_or(&reason).to_owned();
            let entry = subreason_totals.entry(subreason).or_default();
            entry.0 += 1;
            entry.1 += count;
            let mut kinds_in_corridor = FxHashSet::default();
            let receiver_detail = detail.get("detail").unwrap_or(&detail);
            for rows in [receiver_detail.get("rows"), receiver_detail.get("unsafe_variants")] {
                if let Some(rows) = rows.and_then(serde_json::Value::as_array) {
                    for row in rows {
                        if let Some(kind) = row.get("receiver_kind").and_then(serde_json::Value::as_str) {
                            kinds_in_corridor.insert(kind.to_owned());
                        }
                    }
                }
            }
            for kind in kinds_in_corridor {
                let entry = receiver_totals.entry(kind).or_default();
                entry.0 += 1;
                entry.1 += count;
            }
            if reason != "too_small" || witnesses.len() < 8 {
                witnesses.push(serde_json::json!({
                    "corridor": corridor_index,
                    "wire_count": count,
                    "reason": reason,
                    "witness": detail,
                }));
            }
        }
        let reason_totals = totals
            .into_iter()
            .map(|(reason, (corridors, wires))| (reason, serde_json::json!({"corridors": corridors, "wires": wires})))
            .collect::<std::collections::BTreeMap<_, _>>();
        let subreason_totals = subreason_totals
            .into_iter()
            .map(|(reason, (corridors, wires))| (reason, serde_json::json!({"corridors": corridors, "wires": wires})))
            .collect::<std::collections::BTreeMap<_, _>>();
        let receiver_totals = receiver_totals
            .into_iter()
            .map(|(kind, (corridors, wire_volume))| (kind, serde_json::json!({"corridors": corridors, "wire_volume": wire_volume})))
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut largest_nets = self.plans.instant_transport.nets.iter().enumerate().collect::<Vec<_>>();
        largest_nets.sort_unstable_by_key(|(index, net)| (std::cmp::Reverse(net.wires.len()), *index));
        serde_json::json!({
            "guarded": guarded,
            "candidate_corridors": corridors.len(),
            "candidate_wires": corridors.iter().map(|corridor| corridor.wires.len()).sum::<usize>(),
            "admitted_corridors": self.plans.instant_transport.stats.admitted,
            "admitted_wires": admitted_nodes.len(),
            "rejected_corridors": corridors.len().saturating_sub(self.plans.instant_transport.stats.admitted),
            "compiler_stats": format!("{:?}", self.plans.instant_transport.stats),
            "rejected": reason_totals,
            "rejected_subreasons": subreason_totals,
            "unsafe_receiver_kinds": receiver_totals,
            "conditional_topology": conditional_topology,
            "largest_admitted": largest_nets.into_iter().take(20).map(|(index, net)| serde_json::json!({
                "net": index,
                "wires": net.wires.len(),
                "conditional_rows": net.wires.iter().filter(|wire| wire.conditional_inputs != 0).count(),
                "ordered_receivers": net.ordered_receivers,
                "canonical_repeaters": net.canonical_repeaters.len(),
            })).collect::<Vec<_>>(),
            "witnesses": witnesses,
        })
    }
}

#[cfg(test)]
fn conditional_topology_summary(
    graph: &CompileGraph,
    corridor_index: usize,
    corridor: &WireCorridor,
    members: &FxHashSet<NodeId>,
    incoming: &FxHashMap<PowerTarget, Vec<&PowerLink>>,
    root_nodes: &FxHashSet<NodeId>,
    excluded_wires: &FxHashSet<NodeId>,
) -> serde_json::Value {
    let mut direct_bits = 0usize;
    let mut conditional_bits = 0usize;
    let mut guard_counts = std::collections::BTreeMap::<(usize, Option<usize>, bool), usize>::new();
    let mut source_kinds = FxHashMap::<String, usize>::default();
    let mut blockers = FxHashMap::<String, usize>::default();
    let mut receiver_kinds = std::collections::BTreeMap::<String, usize>::new();
    let mut guard_examples = Vec::new();
    for &node in corridor.wires.iter() {
        let CircuitNodeKind::Wire(index) = graph.nodes[node.index()].kind else {
            continue;
        };
        let wire = &graph.wires[index];
        let facts = wire.execution.query_facts.unwrap_or_else(|| WireQueryFacts::infer(graph, wire));
        let Some(transfer) = facts.transfer else {
            *blockers.entry("missing_transfer_facts".into()).or_default() += 1;
            continue;
        };
        direct_bits += transfer.turbo.direct.count_ones() as usize;
        conditional_bits += transfer.turbo.conditional.count_ones() as usize;
        let mut mask = transfer.turbo.conditional;
        while mask != 0 {
            let slot = mask.trailing_zeros() as usize;
            mask &= mask - 1;
            let (horizontal, up, ascent) = match slot {
                7 => (0, None, false),
                8 => (0, Some(3), true),
                12 => (1, None, false),
                13 => (1, Some(3), true),
                17 => (4, None, false),
                20 => (4, Some(3), true),
                18 => (5, None, false),
                21 => (5, Some(3), true),
                _ => continue,
            };
            let horizontal_site = wire.canonical_neighbors[horizontal];
            let up_site = up.map(|slot| wire.canonical_neighbors[slot]);
            let key = (horizontal_site.index(), up_site.map(SiteId::index), ascent);
            *guard_counts.entry(key).or_default() += 1;
            let source_site = wire.canonical_neighbors[slot];
            let kind = conditional_source_kind(graph, source_site, members);
            *source_kinds.entry(kind.clone()).or_default() += 1;
            if guard_examples.len() < 12
                && !guard_examples.iter().any(|example: &serde_json::Value| {
                    example["horizontal_site"] == horizontal_site.index()
                        && example["up_site"] == up_site.map(SiteId::index).map_or(serde_json::Value::Null, serde_json::Value::from)
                        && example["ascent"] == ascent
                })
            {
                guard_examples.push(serde_json::json!({
                    "horizontal_site": horizontal_site.index(),
                    "horizontal_position": position_json(Some(graph.sites[horizontal_site.index()].position)),
                    "up_site": up_site.map(SiteId::index),
                    "up_position": up_site.map(|site| position_json(Some(graph.sites[site.index()].position))),
                    "ascent": ascent,
                    "predicate": if ascent { "solid(horizontal) && !transparent(horizontal) && !solid(up)" } else { "!solid(horizontal)" },
                    "occurrences": guard_counts[&key],
                    "source_kind": kind,
                    "wire_node": node.index(),
                    "wire_position": position_json(graph.nodes[node.index()].position),
                    "slot": slot,
                }));
            }
        }
        let Some(route) = wire.turbo_route else {
            *blockers.entry("missing_turbo_route".into()).or_default() += 1;
            continue;
        };
        if !stable_block_power(graph, wire, members, incoming) {
            *blockers.entry("no_dust_dependency".into()).or_default() += 1;
            continue;
        }
        if !root_nodes.contains(&node) {
            *blockers.entry("missing_root_site".into()).or_default() += 1;
            continue;
        }
        let selection = selection_for(graph, wire, index);
        let (receiver_mask, routes_safe) = turbo_receiver_mask(graph, wire, route, selection, members, false);
        let mut blocker = None;
        if !routes_safe {
            blocker = Some("unsafe_external_receiver_kind");
        }
        for (slot, &site) in wire.canonical_neighbors.iter().enumerate() {
            if transfer.turbo.direct & (1 << slot) != 0
                && !matches!(graph.sites[site.index()].source, SiteSource::Component(source) if members.contains(&source))
                && site_may_be_wire(graph, site)
            {
                blocker = Some("external_or_selectable_direct_wire_input");
            }
            if receiver_mask & (1 << slot) != 0 && !safe_receiver_site(graph, site, members, false) {
                blocker = Some("unsafe_receiver_site_variant");
                let witness = unsafe_site_witness(graph, node, slot, site, members);
                if let Some(variants) = witness["detail"]["unsafe_variants"].as_array() {
                    for variant in variants {
                        if let Some(kind) = variant["receiver_kind"].as_str() {
                            *receiver_kinds.entry(kind.to_owned()).or_default() += 1;
                        }
                    }
                }
            }
        }
        for (slot, &site) in wire.canonical_neighbors.iter().enumerate() {
            if matches!(graph.sites[site.index()].source, SiteSource::Component(source) if members.contains(&source))
                && !selection.is_some_and(|selection| selection.known & (1 << slot) != 0)
            {
                blocker = Some("unknown_internal_wire_selection");
            }
        }
        if let Some(blocker) = blocker {
            *blockers.entry(blocker.into()).or_default() += 1;
        }
    }
    let guards = guard_counts
        .iter()
        .map(|(&(horizontal, up, ascent), &occurrences)| {
            serde_json::json!({"horizontal_site": horizontal, "up_site": up, "ascent": ascent, "occurrences": occurrences})
        })
        .collect::<Vec<_>>();
    for example in &mut guard_examples {
        let horizontal = example["horizontal_site"].as_u64().unwrap_or_default() as usize;
        let up = example["up_site"].as_u64().map(|site| site as usize);
        let ascent = example["ascent"].as_bool().unwrap_or(false);
        if let Some(count) = guard_counts.get(&(horizontal, up, ascent)) {
            example["occurrences"] = serde_json::Value::from(*count);
        }
    }
    let source_kinds = source_kinds.into_iter().collect::<std::collections::BTreeMap<_, _>>();
    let blockers = blockers.into_iter().collect::<std::collections::BTreeMap<_, _>>();
    serde_json::json!({
        "corridor": corridor_index,
        "wires": corridor.wires.len(),
        "owner_overlap": corridor.wires.iter().any(|node| excluded_wires.contains(node)),
        "turbo_direct_transfer_bits": direct_bits,
        "turbo_conditional_transfer_bits": conditional_bits,
        "distinct_geometry_selectors": guards.len(),
        "geometry_selectors": guards,
        "selector_examples": guard_examples,
        "conditional_source_kinds": source_kinds,
        "unsafe_receiver_occurrences": receiver_kinds,
        // This is the first applicable blocker in the compiler's check order.
        // Early failures skip later checks, so these counts are not an assertion
        // that no additional blockers exist for a given wire.
        "first_blocker_per_wire_after_ignoring_conditional": blockers,
    })
}

#[cfg(test)]
fn position_json(position: Option<BlockPos>) -> serde_json::Value {
    position.map_or(serde_json::Value::Null, |position| {
        serde_json::json!([position.x, position.y, position.z])
    })
}

#[cfg(test)]
fn conditional_source_kind(graph: &CompileGraph, site: SiteId, members: &FxHashSet<NodeId>) -> String {
    match graph.sites[site.index()].source {
        SiteSource::Component(node) => match graph.nodes[node.index()].kind {
            CircuitNodeKind::Wire(_) if members.contains(&node) => "member_fixed_wire".into(),
            CircuitNodeKind::Wire(_) => "external_wire_component".into(),
            _ => "component_not_wire".into(),
        },
        SiteSource::Constant(profile) => {
            if matches!(
                mchprs_blocks::blocks::Block::from_id(graph.profiles[profile.index()].raw_state),
                mchprs_blocks::blocks::Block::RedstoneWire { .. }
            ) {
                "constant_wire_profile".into()
            } else {
                "constant_not_wire".into()
            }
        }
        SiteSource::Contact(contact) => {
            let variants = &graph.contacts[contact.index()].variants;
            let carries_wire = variants.iter().any(|variant| {
                matches!(
                    mchprs_blocks::blocks::Block::from_id(graph.profiles[variant.profile.index()].raw_state),
                    mchprs_blocks::blocks::Block::RedstoneWire { .. }
                )
            });
            let receives_wire = variants.iter().any(|variant| {
                matches!(variant.receiver, Some(CallbackReceiver::Component(node)) if matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Wire(_)))
            });
            match (carries_wire, receives_wire) {
                (true, true) => "contact_carried_and_wire_receiver".into(),
                (true, false) => "contact_carried_wire".into(),
                (false, true) => "contact_wire_receiver".into(),
                (false, false) => "contact_not_wire".into(),
            }
        }
    }
}

#[cfg(test)]
fn witness_wire(graph: &CompileGraph, node: NodeId, reason: &str, detail: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "reason": reason,
        "node": node.index(),
        "position": graph.nodes[node.index()].position.map(|position| [position.x, position.y, position.z]),
        "detail": detail,
    })
}

#[cfg(test)]
fn geometry_witness(graph: &CompileGraph, wire: &WirePlan, members: &FxHashSet<NodeId>) -> serde_json::Value {
    serde_json::Value::Array(
        wire.canonical_neighbors
            .iter()
            .enumerate()
            .filter_map(|(slot, site)| match graph.sites[site.index()].source {
                SiteSource::Component(node) if !members.contains(&node) && matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Wire(_)) => {
                    Some(serde_json::json!({"slot": slot, "site": site.index(), "kind": "external_component_wire", "node": node.index()}))
                }
                SiteSource::Contact(contact) => {
                    let profiles = graph.contacts[contact.index()]
                        .variants
                        .iter()
                        .filter_map(|variant| {
                            matches!(
                                mchprs_blocks::blocks::Block::from_id(graph.profiles[variant.profile.index()].raw_state),
                                mchprs_blocks::blocks::Block::RedstoneWire { .. }
                            )
                            .then_some(serde_json::json!({"profile": variant.profile.index(), "receiver": format!("{:?}", variant.receiver)}))
                        })
                        .collect::<Vec<_>>();
                    (!profiles.is_empty())
                        .then_some(serde_json::json!({"slot": slot, "site": site.index(), "kind": "contact_may_be_dust", "variants": profiles}))
                }
                SiteSource::Constant(profile)
                    if matches!(
                        mchprs_blocks::blocks::Block::from_id(graph.profiles[profile.index()].raw_state),
                        mchprs_blocks::blocks::Block::RedstoneWire { .. }
                    ) =>
                {
                    Some(serde_json::json!({"slot": slot, "site": site.index(), "kind": "constant_dust", "profile": profile.index()}))
                }
                _ => None,
            })
            .collect(),
    )
}

#[cfg(test)]
fn unsafe_route_witness(
    graph: &CompileGraph,
    wire: &WirePlan,
    route: RouteId,
    selection: Option<WireCallbackSelection>,
    members: &FxHashSet<NodeId>,
) -> serde_json::Value {
    let mut rows = Vec::new();
    for (delivery_index, delivery) in graph.routes[route.index()].deliveries.iter().enumerate() {
        let Some(receiver) = delivery.receiver else { continue };
        for (slot, site) in wire.canonical_neighbors.iter().enumerate().filter(|(_, site)| **site == delivery.target) {
            if selection.is_some_and(|selection| selection.known & (1 << slot) != 0 && selection.admitted & (1 << slot) == 0) {
                continue;
            }
            let safe = match receiver {
                CallbackReceiver::PistonBase(_) | CallbackReceiver::PistonHead(_) | CallbackReceiver::Contact(_) => true,
                CallbackReceiver::Component(node) if members.contains(&node) => matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Wire(_)),
                CallbackReceiver::Component(node) => safe_external_component(graph, node),
            };
            if !safe {
                rows.push(serde_json::json!({
                    "slot": slot,
                    "site": site.index(),
                    "delivery": delivery_index,
                    "receiver": format!("{:?}", receiver),
                    "receiver_kind": receiver_kind(graph, receiver),
                    "view": format!("{:?}", delivery.view),
                    "guard": delivery.guard.index(),
                }));
            }
        }
    }
    serde_json::json!({"reason": "unsafe_external_receiver_kind", "rows": rows})
}

#[cfg(test)]
fn unsafe_site_witness(graph: &CompileGraph, node: NodeId, slot: usize, site: SiteId, members: &FxHashSet<NodeId>) -> serde_json::Value {
    let source = graph.sites[site.index()].source;
    let variants = match source {
        SiteSource::Contact(contact) => graph.contacts[contact.index()]
            .variants
            .iter()
            .filter_map(|variant| {
                let receiver = variant.receiver?;
                let safe = match receiver {
                    CallbackReceiver::PistonBase(_) | CallbackReceiver::PistonHead(_) | CallbackReceiver::Contact(_) => true,
                    CallbackReceiver::Component(receiver) if members.contains(&receiver) => matches!(graph.nodes[receiver.index()].kind, CircuitNodeKind::Wire(_)),
                    CallbackReceiver::Component(receiver) => safe_external_component(graph, receiver),
                };
                (!safe).then(|| serde_json::json!({"receiver": format!("{:?}", receiver), "receiver_kind": receiver_kind(graph, receiver), "profile": variant.profile.index()}))
            })
            .collect::<Vec<_>>(),
        SiteSource::Component(receiver) => {
            let callback = match graph.nodes[receiver.index()].kind {
                CircuitNodeKind::Piston(actor) => CallbackReceiver::PistonBase(actor),
                _ => CallbackReceiver::Component(receiver),
            };
            let unsafe_receiver = match callback {
                CallbackReceiver::PistonBase(_) | CallbackReceiver::PistonHead(_) | CallbackReceiver::Contact(_) => false,
                CallbackReceiver::Component(receiver) if members.contains(&receiver) => !matches!(graph.nodes[receiver.index()].kind, CircuitNodeKind::Wire(_)),
                CallbackReceiver::Component(receiver) => !safe_external_component(graph, receiver),
            };
            if unsafe_receiver {
                vec![serde_json::json!({"receiver": format!("{:?}", callback), "receiver_kind": receiver_kind(graph, callback)})]
            } else {
                Vec::new()
            }
        }
        SiteSource::Constant(_) => Vec::new(),
    };
    witness_wire(
        graph,
        node,
        "unsafe_receiver_site_variant",
        serde_json::json!({"slot": slot, "site": site.index(), "source": format!("{:?}", source), "unsafe_variants": variants}),
    )
}

#[cfg(test)]
fn safe_external_component(graph: &CompileGraph, node: NodeId) -> bool {
    matches!(
        graph.nodes[node.index()].kind,
        CircuitNodeKind::Piston(_)
            | CircuitNodeKind::Observer { .. }
            | CircuitNodeKind::BuiltinReset { .. }
            | CircuitNodeKind::Electrical(
                crate::redpiler::components::NodeType::Lever
                    | crate::redpiler::components::NodeType::Button
                    | crate::redpiler::components::NodeType::PressurePlate
                    | crate::redpiler::components::NodeType::Constant
            )
    )
}

#[cfg(test)]
fn receiver_kind(graph: &CompileGraph, receiver: CallbackReceiver) -> String {
    match receiver {
        CallbackReceiver::Component(node) => format!("{:?}", graph.nodes[node.index()].kind),
        CallbackReceiver::PistonBase(_) => "PistonBase".into(),
        CallbackReceiver::PistonHead(_) => "PistonHead".into(),
        CallbackReceiver::Contact(_) => "Contact".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::{TaskMonitor, analysis::AnalysisLimits, passes};

    fn graph() -> CompileGraph {
        let (world, bounds) = passes::simplify_instant_resets::tests::fixture();
        let monitor = TaskMonitor::default();
        let mut prepared = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::default()).unwrap();
        let options = Default::default();
        passes::run_graph_pipeline(
            &mut prepared.graph,
            &options,
            &prepared.entry,
            &passes::default_graph_pipeline(&options),
            &monitor,
        )
        .unwrap();
        prepared.graph
    }

    #[test]
    fn transport_discovery_budget_declines_without_partial_plan() {
        let graph = graph();
        let monitor = TaskMonitor::default();
        let plan = infer_mode_with_discovery(&graph, &monitor, true, |graph, monitor| {
            crate::redpiler::analysis::instant::boundaries::discover_transport_corridors_with_work_limit(graph, monitor, 0)
        })
        .unwrap();
        assert!(plan.nets.is_empty());
        assert!(
            monitor
                .warnings()
                .iter()
                .any(|warning| warning.contains("transport discovery declined") && warning.contains("budget"))
        );
        let error = infer_mode_with_discovery(&graph, &monitor, true, |_, _| Err("invalid budget metadata".into())).unwrap_err();
        assert_eq!(error, "invalid budget metadata");
        monitor.cancel();
        assert!(
            infer_mode_with_discovery(&graph, &monitor, true, |graph, monitor| {
                crate::redpiler::analysis::instant::boundaries::discover_transport_corridors_with_work_limit(graph, monitor, 0)
            })
            .unwrap_err()
            .contains("cancelled")
        );
    }

    #[test]
    fn instant_transport_compilation_honors_cancellation() {
        let graph = graph();
        let monitor = TaskMonitor::default();
        monitor.cancel();
        assert!(infer(&graph, &monitor).unwrap_err().contains("cancelled"));
    }

    #[test]
    fn instant_transport_no_dust_certificate_rejects_an_internal_wire_source() {
        let mut graph = graph();
        let wire = &graph.wires[0];
        let members = FxHashSet::from_iter([wire.node]);
        let root = graph
            .sites
            .iter()
            .enumerate()
            .find_map(|(index, site)| (site.source == SiteSource::Component(wire.node)).then_some(SiteId::new(index as u32)))
            .unwrap();
        let target = PowerTarget::Conductor {
            site: wire.canonical_neighbors[3],
            include_dust: false,
        };
        let link = PowerLink {
            from: root,
            from_port: PowerPort::Strong(BlockFace::East),
            to: target,
            face: BlockFace::East,
            attenuation: 0,
            guard: GuardId::new(0),
        };
        let incoming = FxHashMap::from_iter([(target, vec![&link])]);
        assert!(!stable_block_power(&graph, wire, &members, &incoming));

        // A conducted link may read a selectable contact whose current receiver
        // resolves to a member wire, even when the contact's block profile is not dust.
        let contact_site = graph.sites[wire.canonical_neighbors[4].index()].clone();
        let profile = graph
            .profiles
            .iter()
            .position(|_| true)
            .map(|index| ProfileId::new(index as u32))
            .unwrap();
        let contact = ContactId::new(graph.contacts.len() as u32);
        graph.contacts.push(ContactSpec {
            node: wire.node,
            position: contact_site.position,
            variants: vec![ContactVariant {
                when: GuardId::new(0),
                profile,
                receiver: Some(CallbackReceiver::Component(wire.node)),
            }]
            .into_boxed_slice(),
        });
        graph.sites[wire.canonical_neighbors[4].index()].source = SiteSource::Contact(contact);
        let contact_link = PowerLink {
            from: wire.canonical_neighbors[4],
            from_port: PowerPort::Conducted { include_dust: false },
            ..link
        };
        let incoming = FxHashMap::from_iter([(target, vec![&contact_link])]);
        assert!(!stable_block_power(&graph, wire, &members, &incoming));
    }

    #[test]
    fn instant_transport_plan_change_rejects_a_forged_table() {
        let mut graph = graph();
        graph.plans.instant_transport.nets.push(TransportNet {
            wires: Box::new([]),
            sites: Box::new([]),
            site_wire: Box::new([]),
            ordered_receivers: false,
            canonical_repeaters: Box::new([]),
        });
        assert!(graph.validate_plan_change(PlanChange::InstantTransport, &TaskMonitor::default()).is_err());
    }
}
