//! Dense local bindings for exact queue-only Turbo wire corridors.
//!
//! Admission keeps a corridor only when every wire has a Turbo route and its
//! partially evaluated query has no geometry-conditional wire-power inputs.
//! Every direct dynamic wire input must be another member of the same corridor;
//! selectable contacts that may contain wire, external wire inputs, constant
//! dust, and wires owned by another execution plan decline the whole corridor.
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
//! wire's independent state and allowing the backend to run the original three
//! rotating FIFO queues without physical neighborhood discovery. The backend
//! retains each first-discovery observation, bias, heading, and layer; this
//! compiler does not collapse the corridor to a settled Boolean equation.
use super::*;
use rustc_hash::{FxHashMap, FxHashSet};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct InstantTransport {
    pub(crate) nets: Vec<TransportNet>,
    pub(crate) stats: TransportCompileStats,
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
    pub(crate) route: RouteId,
}

pub(crate) fn infer(graph: &CompileGraph, monitor: &crate::redpiler::TaskMonitor) -> Result<InstantTransport, String> {
    if monitor.cancelled() {
        return Err("cancelled while compiling instant transport".into());
    }
    let Some(catalog) = &graph.plans.instant.boundaries else {
        return Ok(InstantTransport::default());
    };
    let mut output = InstantTransport::default();
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
        if wire.execution.instant_reset.is_some() || wire.execution.instant_node.is_some() || wire.execution.abstract_instant_net {
            excluded_wires.insert(wire.node);
        }
    }
    for corridor in catalog.corridors.iter() {
        output.stats.corridors += 1;
        if monitor.cancelled() {
            return Err("cancelled while compiling instant transport".into());
        }
        if corridor.wires.len() < 2 {
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
            if transfer.turbo.conditional != 0 || !safe_geometry(graph, source, &members) {
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
            let (receiver_mask, receivers_safe) = turbo_receiver_mask(graph, source, route, selection, &members);
            if !receivers_safe {
                accepted = false;
                decline = Some(DeclineReason::UnsafeReceiver);
            }
            let mut neighbors = [u32::MAX; 24];
            let mut inputs = 0u32;
            for (slot, &site) in source.canonical_neighbors.iter().enumerate() {
                let mut retain = false;
                if transfer.turbo.direct & (1 << slot) != 0 {
                    match graph.sites[site.index()].source {
                        SiteSource::Component(upstream) if members.contains(&upstream) => {
                            inputs |= 1 << slot;
                            retain = true;
                        }
                        _ if site_may_be_wire(graph, site) => {
                            accepted = false;
                            decline = Some(DeclineReason::UnsafeInput);
                        }
                        _ => {}
                    }
                }
                let receiver = receiver_mask & (1 << slot) != 0;
                if receiver && !safe_receiver_site(graph, site, &members) {
                    accepted = false;
                    decline = Some(DeclineReason::UnsafeReceiver);
                }
                if receiver {
                    retain = true;
                }
                if matches!(graph.sites[site.index()].source, SiteSource::Component(upstream) if members.contains(&upstream)) {
                    // Visited wires determine the native first-arrival heading even when this slot is not a power input.
                    retain = true;
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
        bytes = bytes.saturating_add(
            wires.len() * std::mem::size_of::<TransportWire>() + sites.len() * (std::mem::size_of::<SiteId>() + std::mem::size_of::<u32>()),
        );
        if bytes > crate::redpiler::analysis::MAX_GRAPH_PREPARATION_BYTES / 8 {
            return Err("instant transport tables exceed preparation memory bound".into());
        }
        output.nets.push(TransportNet {
            wires: wires.into_boxed_slice(),
            sites: sites.into_boxed_slice(),
            site_wire: site_wire.into_boxed_slice(),
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
) -> (u32, bool) {
    let mut slots = FxHashMap::default();
    for (slot, site) in wire.canonical_neighbors.iter().enumerate() {
        *slots.entry(*site).or_insert(0u32) |= 1 << slot;
    }
    let mut mask = 0;
    let mut safe = true;
    let route_is_safe = |receiver: CallbackReceiver| match receiver {
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
            _ => false,
        },
    };
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
            safe &= delivery.receiver.is_some_and(route_is_safe);
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

fn safe_receiver_site(graph: &CompileGraph, site: SiteId, members: &FxHashSet<NodeId>) -> bool {
    let safe = |receiver: CallbackReceiver| match receiver {
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
            _ => false,
        },
    };
    let site_spec = &graph.sites[site.index()];
    match site_spec.source {
        SiteSource::Contact(contact) => graph.contacts[contact.index()]
            .variants
            .iter()
            .all(|variant| variant.receiver.is_none_or(safe)),
        SiteSource::Component(node) => safe(match graph.nodes[node.index()].kind {
            CircuitNodeKind::Piston(actor) => CallbackReceiver::PistonBase(actor),
            _ => CallbackReceiver::Component(node),
        }),
        SiteSource::Constant(_) => true,
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
        });
        assert!(graph.validate_plan_change(PlanChange::InstantTransport, &TaskMonitor::default()).is_err());
    }
}
