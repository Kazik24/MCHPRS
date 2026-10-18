//! UNSAFE research only: fixed weighted dust, final writes, deduplicated boundary samples.
use super::*;
use crate::redpiler::power_rules::{emits_strong, emits_weak};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[path = "../../../../../benches/support/cpus.rs"]
#[allow(dead_code)]
mod cpus;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Source {
    Node(NodeId),
    LiveBlockPower(NodeId),
}

#[derive(Clone, Default)]
struct Expression {
    constant: u8,
    sources: BTreeMap<Source, u8>,
}

impl Expression {
    fn include(&mut self, other: &Self, distance: u8) {
        self.constant = self.constant.max(other.constant.saturating_sub(distance));
        for (&source, &weight) in &other.sources {
            let weight = weight.saturating_add(distance);
            if weight < 15 {
                self.sources.entry(source).and_modify(|old| *old = (*old).min(weight)).or_insert(weight);
            }
        }
    }
}

fn dynamic_guard(backend: &GraphBackend, guard: GuardId) -> bool {
    match &backend.program.guards()[guard.index()] {
        Guard::Always => false,
        Guard::State(_) => true,
        Guard::Site { site, .. } => matches!(backend.program.sites()[site.index()].source, SiteSource::Contact(_)),
        Guard::Not(child) => dynamic_guard(backend, *child),
        Guard::All(children) | Guard::Any(children) => children.iter().any(|&child| dynamic_guard(backend, child)),
    }
}

fn dynamic_query(backend: &GraphBackend, target: PowerTarget, depth: usize) -> bool {
    assert!(depth < 16);
    backend.program.power_links_to(target).iter().any(|link| {
        matches!(backend.program.sites()[link.from.index()].source, SiteSource::Contact(_))
        || dynamic_guard(backend, link.guard)
        || matches!(link.from_port, PowerPort::Conducted { include_dust } if dynamic_query(backend, PowerTarget::Conductor { site:link.from, include_dust }, depth+1))
    })
}

fn external(backend: &GraphBackend, target: PowerTarget, depth: usize) -> Expression {
    assert!(depth < 16, "unchecked expression recursion limit");
    let mut expression = Expression::default();
    for link in backend.program.power_links_to(target) {
        // shortcut: freeze geometry/contact/guard selection at compilation; this is deliberately not admission.
        if !backend.guard_matches(link.guard, None) {
            continue;
        }
        let value = if let PowerPort::Conducted { include_dust } = link.from_port {
            external(
                backend,
                PowerTarget::Conductor {
                    site: link.from,
                    include_dust,
                },
                depth + 1,
            )
        } else {
            let Some(site) = backend.resolve_site_view(link.from, None) else {
                continue;
            };
            let emits = match link.from_port {
                PowerPort::Weak(face) => emits_weak(site.block, face),
                PowerPort::Strong(face) => emits_strong(site.block, face),
                _ => true,
            };
            if !emits || matches!(site.block, Block::RedstoneWire { .. }) {
                continue;
            }
            match site.receiver {
                Some(CallbackReceiver::Component(node)) => Expression {
                    constant: 0,
                    sources: BTreeMap::from([(Source::Node(node), 0)]),
                },
                _ => Expression {
                    constant: if matches!(site.block, Block::RedstoneBlock) {
                        15
                    } else {
                        backend.program.sites()[link.from.index()].source_strength
                    },
                    sources: BTreeMap::new(),
                },
            }
        };
        expression.include(&value, link.attenuation);
    }
    expression
}

struct Net {
    wires: Vec<NodeId>,
    sources: Vec<Source>,
    terms: Vec<Vec<(usize, u8)>>,
    constants: Vec<u8>,
    incoming: Vec<Vec<usize>>,
}

fn receivers(backend: &GraphBackend, site: SiteId) -> Vec<CallbackReceiver> {
    match backend.program.sites()[site.index()].source {
        SiteSource::Component(node) => vec![CallbackReceiver::Component(node)],
        SiteSource::Contact(contact) => backend.program.contacts()[contact.index()]
            .variants
            .iter()
            .filter_map(|variant| variant.receiver)
            .collect(),
        SiteSource::Constant(_) => Vec::new(),
    }
}

fn ordinary(backend: &GraphBackend, receiver: CallbackReceiver) -> bool {
    let CallbackReceiver::Component(node) = receiver else { return false };
    match backend.program.nodes()[node.index()].kind {
        CircuitNodeKind::Wire(_) => true,
        CircuitNodeKind::Electrical(NodeType::Torch | NodeType::Lever | NodeType::Button | NodeType::PressurePlate) => true,
        CircuitNodeKind::Electrical(NodeType::Repeater { .. }) => {
            let target = PowerTarget::Input { node, port: InputPort::Side };
            let expression = external(backend, target, 0);
            !backend.node_state(node).repeater_locked
                && !dynamic_query(backend, target, 0)
                && expression.constant == 0
                && expression.sources.is_empty()
                && {
                    let mut leaves = BTreeSet::new();
                    query_leaves(backend, target, 0, &mut leaves, &mut BTreeSet::new());
                    !leaves
                        .iter()
                        .any(|node| matches!(backend.program.nodes()[node.index()].kind, CircuitNodeKind::Wire(_)))
                }
        }
        _ => false,
    }
}

fn query_leaves(backend: &GraphBackend, target: PowerTarget, depth: usize, leaves: &mut BTreeSet<NodeId>, reasons: &mut BTreeSet<&'static str>) {
    assert!(depth < 16);
    for link in backend.program.power_links_to(target) {
        if dynamic_guard(backend, link.guard) {
            reasons.insert("contact/resource guard");
        }
        if matches!(backend.program.sites()[link.from.index()].source, SiteSource::Contact(_)) {
            reasons.insert("selectable contact dependency");
        }
        for receiver in receivers(backend, link.from) {
            if let CallbackReceiver::Component(node) = receiver {
                leaves.insert(node);
            } else {
                reasons.insert("piston/contact dependency");
            }
        }
        if let PowerPort::Conducted { include_dust } = link.from_port {
            query_leaves(
                backend,
                PowerTarget::Conductor {
                    site: link.from,
                    include_dust,
                },
                depth + 1,
                leaves,
                reasons,
            );
        }
    }
}

fn preservation(backend: &GraphBackend) -> (Vec<bool>, serde_json::Value) {
    let nodes = backend.program.nodes().len();
    let mut adjacency = vec![Vec::new(); nodes];
    let mut reasons = vec![BTreeSet::new(); nodes];
    let mut callbacks = Vec::new();
    for plan in backend.program.wires() {
        let node = plan.node;
        for slot in [0, 1, 2, 3, 4, 5, 7, 8, 12, 13, 17, 18, 20, 21] {
            for receiver in receivers(backend, plan.canonical_neighbors[slot]) {
                if let CallbackReceiver::Component(other) = receiver
                    && matches!(backend.program.nodes()[other.index()].kind, CircuitNodeKind::Wire(_))
                {
                    adjacency[node.index()].push(other);
                    adjacency[other.index()].push(node);
                }
            }
        }
        let mut leaves = BTreeSet::new();
        query_leaves(
            backend,
            PowerTarget::Input { node, port: InputPort::Main },
            0,
            &mut leaves,
            &mut reasons[node.index()],
        );
        for other in leaves {
            if matches!(backend.program.nodes()[other.index()].kind, CircuitNodeKind::Wire(_)) {
                adjacency[node.index()].push(other);
                adjacency[other.index()].push(node);
            } else if !ordinary(backend, CallbackReceiver::Component(other)) {
                reasons[node.index()].insert("effectful electrical dependency");
            }
        }
        for &site in &plan.canonical_neighbors {
            if matches!(backend.program.sites()[site.index()].source, SiteSource::Contact(_)) {
                reasons[node.index()].insert("selectable contact neighborhood");
            }
            for receiver in receivers(backend, site) {
                if !ordinary(backend, receiver) {
                    reasons[node.index()].insert("piston/effectful neighborhood");
                }
            }
        }
        let routes = [backend.program.nodes()[node.index()].output_route, plan.shape_route, plan.turbo_route]
            .into_iter()
            .flatten()
            .collect::<BTreeSet<_>>();
        for route in routes {
            for delivery in &backend.program.routes()[route.index()].deliveries {
                if dynamic_guard(backend, delivery.guard) {
                    reasons[node.index()].insert("contact/resource callback guard");
                }
                if matches!(backend.program.sites()[delivery.target.index()].source, SiteSource::Contact(_)) {
                    reasons[node.index()].insert("selectable contact callback");
                }
                for receiver in delivery.receiver.into_iter().chain(receivers(backend, delivery.target)) {
                    if let CallbackReceiver::Component(other) = receiver
                        && matches!(backend.program.nodes()[other.index()].kind, CircuitNodeKind::Wire(_))
                    {
                        callbacks.push((node, other));
                    } else if !ordinary(backend, receiver) {
                        reasons[node.index()].insert("effectful callback receiver");
                    }
                }
            }
        }
    }
    for node in backend.program.nodes() {
        if let CircuitNodeKind::Observer { watched, .. } | CircuitNodeKind::BuiltinReset { watched, .. } = node.kind {
            for receiver in receivers(backend, watched) {
                if let CallbackReceiver::Component(wire) = receiver
                    && matches!(backend.program.nodes()[wire.index()].kind, CircuitNodeKind::Wire(_))
                {
                    reasons[wire.index()].insert("observer/reset watch");
                }
            }
        }
    }
    let targets = backend
        .program
        .power_links()
        .iter()
        .filter_map(|link| match link.to {
            PowerTarget::Input { node, port } if !matches!(backend.program.nodes()[node.index()].kind, CircuitNodeKind::Wire(_)) => {
                Some((node, port))
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    for (node, port) in targets {
        if port == InputPort::Main && ordinary(backend, CallbackReceiver::Component(node)) {
            continue;
        }
        let mut leaves = BTreeSet::new();
        query_leaves(backend, PowerTarget::Input { node, port }, 0, &mut leaves, &mut BTreeSet::new());
        for wire in leaves {
            if matches!(backend.program.nodes()[wire.index()].kind, CircuitNodeKind::Wire(_)) {
                reasons[wire.index()].insert(if port == InputPort::Main {
                    "effectful power reader"
                } else {
                    "side/quasi power reader"
                });
            }
        }
    }
    let mut labels = vec![usize::MAX; nodes];
    let mut groups = Vec::new();
    let mut region_reasons = Vec::new();
    for wire in backend.program.wires() {
        if labels[wire.node.index()] != usize::MAX {
            continue;
        }
        let mut group = vec![wire.node];
        labels[wire.node.index()] = groups.len();
        let mut cursor = 0;
        let mut why = BTreeSet::new();
        while cursor < group.len() {
            let node = group[cursor];
            why.extend(reasons[node.index()].iter().copied());
            for &other in &adjacency[node.index()] {
                if labels[other.index()] == usize::MAX {
                    labels[other.index()] = groups.len();
                    group.push(other);
                }
            }
            cursor += 1;
        }
        groups.push(group);
        region_reasons.push(why);
    }
    let mut reverse = vec![BTreeSet::new(); groups.len()];
    for (from, to) in callbacks {
        reverse[labels[to.index()]].insert(labels[from.index()]);
    }
    let mut queue = region_reasons
        .iter()
        .enumerate()
        .filter_map(|(index, reasons)| (!reasons.is_empty()).then_some(index))
        .collect::<VecDeque<_>>();
    while let Some(region) = queue.pop_front() {
        for &from in &reverse[region] {
            if region_reasons[from].is_empty() {
                region_reasons[from].insert("callback into preserved region");
                queue.push_back(from);
            }
        }
    }
    let mut mask = vec![false; nodes];
    let mut summary = BTreeMap::<&str, serde_json::Value>::new();
    let mut counts = BTreeMap::<&str, (usize, usize)>::new();
    for (group, why) in groups.iter().zip(&region_reasons) {
        if why.is_empty() {
            continue;
        }
        for node in group {
            mask[node.index()] = true;
        }
        for &reason in why {
            let count = counts.entry(reason).or_default();
            count.0 += 1;
            count.1 += group.len();
        }
    }
    for (reason, (regions, wires)) in counts {
        summary.insert(reason, json!({"regions":regions,"wires":wires}));
    }
    let metadata = json!({"potential_regions":groups.len(),"preserved_regions":region_reasons.iter().filter(|why|!why.is_empty()).count(),"preserved_wires":mask.iter().filter(|&&preserved|preserved).count(),"reasons_overlapping":summary});
    (mask, metadata)
}

pub(super) struct UncheckedProgram {
    nets: Vec<Net>,
    by_node: Vec<usize>,
    calls: AtomicU64,
    source_samples: AtomicU64,
    weighted_terms: AtomicU64,
    final_writes: AtomicU64,
    boundary_calls: AtomicU64,
    unreachable_changed: AtomicU64,
    diagnostic: bool,
    diagnostic_from: u64,
    diagnostic_events: AtomicU64,
    preserved: Vec<bool>,
    preservation: serde_json::Value,
}

impl UncheckedProgram {
    fn compile(backend: &GraphBackend) -> Self {
        Self::compile_mode(backend, std::env::var_os("MCHPRS_UNCHECKED_ANPU_HYBRID").is_some())
    }

    fn compile_mode(backend: &GraphBackend, hybrid: bool) -> Self {
        assert!(backend.program.classic().nets.is_empty());
        let wires = backend.program.wires().iter().map(|wire| wire.node).collect::<Vec<_>>();
        let mut indices = vec![usize::MAX; backend.program.nodes().len()];
        for (index, node) in wires.iter().enumerate() {
            indices[node.index()] = index;
        }
        let incoming = wires
            .iter()
            .map(|&node| {
                backend
                    .fixed_point_wire_neighbors(node)
                    .into_iter()
                    .map(|node| indices[node.index()])
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let mut adjacent = incoming.clone();
        for (to, froms) in incoming.iter().enumerate() {
            for &from in froms {
                adjacent[from].push(to);
            }
        }
        let mut labels = vec![usize::MAX; wires.len()];
        let mut groups = Vec::new();
        for start in 0..wires.len() {
            if labels[start] != usize::MAX {
                continue;
            }
            labels[start] = groups.len();
            let mut members = vec![start];
            let mut cursor = 0;
            while cursor < members.len() {
                for &neighbor in &adjacent[members[cursor]] {
                    if labels[neighbor] == usize::MAX {
                        labels[neighbor] = groups.len();
                        members.push(neighbor);
                    }
                }
                cursor += 1;
            }
            members.sort_unstable();
            groups.push(members);
        }
        let external = wires
            .iter()
            .map(|&node| {
                let target = PowerTarget::Input { node, port: InputPort::Main };
                if dynamic_query(backend, target, 0) {
                    Expression {
                        constant: 0,
                        sources: BTreeMap::from([(Source::LiveBlockPower(node), 0)]),
                    }
                } else {
                    external(backend, target, 0)
                }
            })
            .collect::<Vec<_>>();
        let mut nets = Vec::new();
        let mut by_node = vec![usize::MAX; backend.program.nodes().len()];
        for members in groups {
            let mut expressions = Vec::new();
            for &wire in &members {
                let mut expression = Expression::default();
                let mut reached = FxHashSet::default();
                let mut queue = VecDeque::from([(wire, 0u8)]);
                reached.insert(wire);
                while let Some((at, distance)) = queue.pop_front() {
                    expression.include(&external[at], distance);
                    if distance < 14 {
                        for &from in &incoming[at] {
                            if reached.insert(from) {
                                queue.push_back((from, distance + 1));
                            }
                        }
                    }
                }
                expressions.push(expression);
            }
            let sources = expressions
                .iter()
                .flat_map(|expression| expression.sources.keys().copied())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let source_index = sources
                .iter()
                .enumerate()
                .map(|(index, &node)| (node, index))
                .collect::<FxHashMap<_, _>>();
            let local = members
                .iter()
                .enumerate()
                .map(|(local, &global)| (global, local))
                .collect::<FxHashMap<_, _>>();
            let local_incoming = members
                .iter()
                .map(|&global| incoming[global].iter().map(|from| local[from]).collect())
                .collect();
            let nodes = members.iter().map(|&index| wires[index]).collect::<Vec<_>>();
            for node in &nodes {
                by_node[node.index()] = nets.len();
            }
            nets.push(Net {
                wires: nodes,
                sources,
                terms: expressions
                    .iter()
                    .map(|expression| {
                        expression
                            .sources
                            .iter()
                            .map(|(node, &distance)| (source_index[node], distance))
                            .collect()
                    })
                    .collect(),
                constants: expressions.iter().map(|expression| expression.constant).collect(),
                incoming: local_incoming,
            });
        }
        let (preserved, preservation) = if hybrid {
            preservation(backend)
        } else {
            (vec![false; backend.program.nodes().len()], json!({"enabled":false}))
        };
        for net in &nets {
            assert!(
                net.wires.iter().all(|wire| preserved[wire.index()] == preserved[net.wires[0].index()]),
                "preservation must own the entire fixed net"
            );
        }
        Self {
            nets,
            by_node,
            calls: AtomicU64::new(0),
            source_samples: AtomicU64::new(0),
            weighted_terms: AtomicU64::new(0),
            final_writes: AtomicU64::new(0),
            boundary_calls: AtomicU64::new(0),
            unreachable_changed: AtomicU64::new(0),
            diagnostic: std::env::var_os("MCHPRS_UNCHECKED_ANPU_DIAGNOSTIC").is_some(),
            diagnostic_from: std::env::var("MCHPRS_UNCHECKED_ANPU_DIAGNOSTIC")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(0),
            diagnostic_events: AtomicU64::new(0),
            preserved,
            preservation,
        }
    }

    pub(super) fn handles(&self, node: NodeId) -> bool {
        !self.preserved[node.index()]
    }

    pub(super) fn run(
        &self,
        backend: &mut GraphBackend,
        root: NodeId,
        captured: Option<&ObservedSite>,
        sampled_old: u8,
    ) -> Result<(), GraphExecutionError> {
        let net = &self.nets[self.by_node[root.index()]];
        let root_index = net.wires.iter().position(|&node| node == root).unwrap();
        let mut order = vec![root_index];
        let mut reached = vec![false; net.wires.len()];
        reached[root_index] = true;
        let mut cursor = 0;
        while cursor < order.len() {
            for &neighbor in &net.incoming[order[cursor]] {
                if !reached[neighbor] {
                    reached[neighbor] = true;
                    order.push(neighbor);
                }
            }
            cursor += 1;
        }
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.source_samples.fetch_add(net.sources.len() as u64, Ordering::Relaxed);
        let sources = net
            .sources
            .iter()
            .map(|&source| match source {
                Source::Node(node) => backend.node_state(node).output_strength,
                Source::LiveBlockPower(node) => {
                    let plan = backend.wire_plan(node).clone();
                    backend.seed_power_general(&plan).1
                }
            })
            .collect::<Vec<_>>();
        let mut changed = vec![false; net.wires.len()];
        if self.diagnostic && backend.future.clock.logical_tick >= self.diagnostic_from && self.diagnostic_events.load(Ordering::Relaxed) < 16 {
            self.diagnose(backend, root, captured, sampled_old, net, &order, &sources);
        }
        self.weighted_terms
            .fetch_add(order.iter().map(|&index| net.terms[index].len()).sum::<usize>() as u64, Ordering::Relaxed);
        for &index in &order {
            let node = net.wires[index];
            let power = net.terms[index]
                .iter()
                .map(|&(source, attenuation)| sources[source].saturating_sub(attenuation))
                .max()
                .unwrap_or(0)
                .max(net.constants[index]);
            let mut wire = backend.wire_state(node);
            if wire.power != power {
                changed[index] = true;
                wire.power = power;
                backend.set_wire(node, wire);
                self.final_writes.fetch_add(1, Ordering::Relaxed);
            }
        }
        debug_assert!(changed.iter().zip(&reached).all(|(changed, reached)| !*changed || *reached));
        let network = order.iter().map(|&index| net.wires[index]).collect::<FxHashSet<_>>();
        let mut seen = FxHashSet::default();
        let mut boundary = Vec::new();
        // shortcut: retain final samples only, using the dynamic solver's root-first BFS selection order.
        for wire in order {
            if !changed[wire] && net.wires[wire] != root {
                continue;
            }
            let plan = backend.wire_plan(net.wires[wire]);
            let Some(route) = plan.turbo_route else { continue };
            let CircuitNodeKind::Wire(wire_index) = backend.program.nodes()[plan.node.index()].kind else {
                unreachable!()
            };
            for (slot, site) in plan.canonical_neighbors.iter().copied().enumerate() {
                let Some(observed) = backend.resolve_site(site, None).map(|site| site.observed()) else {
                    continue;
                };
                if matches!(observed.receiver, Some(CallbackReceiver::Component(node)) if network.contains(&node)) {
                    continue;
                }
                if backend.wire_route_delivery(wire_index, route, slot, site, &observed).is_some() && seen.insert(site) {
                    boundary.push((site, observed));
                }
            }
        }
        for (site, observed) in boundary {
            self.boundary_calls.fetch_add(1, Ordering::Relaxed);
            backend.dispatch_turbo_nonwire(site, &observed, 1)?;
        }
        Ok(())
    }

    #[cold]
    fn diagnose(
        &self,
        backend: &mut GraphBackend,
        root: NodeId,
        captured: Option<&ObservedSite>,
        sampled_old: u8,
        net: &Net,
        order: &[usize],
        sources: &[u8],
    ) {
        let root_plan = backend.wire_plan(root).clone();
        let seed = backend.seed_power_general(&root_plan).0;
        let mut wires = vec![root];
        let mut indices = FxHashMap::from_iter([(root, 0usize)]);
        let mut incoming = Vec::new();
        let mut cursor = 0;
        while cursor < wires.len() {
            let mut from = Vec::new();
            for node in backend.fixed_point_wire_neighbors(wires[cursor]) {
                let index = *indices.entry(node).or_insert_with(|| {
                    let index = wires.len();
                    wires.push(node);
                    index
                });
                from.push(index);
            }
            incoming.push(from);
            cursor += 1;
        }
        let blocks = wires
            .iter()
            .map(|&node| {
                let plan = backend.wire_plan(node).clone();
                backend.seed_power_general(&plan).1
            })
            .collect::<Vec<_>>();
        let mut powers = vec![0u8; wires.len()];
        let mut next = powers.clone();
        for _ in 0..15 {
            for (index, from) in incoming.iter().enumerate() {
                next[index] = blocks[index].max(from.iter().map(|&from| powers[from].saturating_sub(1)).max().unwrap_or(0));
            }
            std::mem::swap(&mut powers, &mut next);
        }
        if seed == sampled_old {
            for (index, &node) in wires.iter().enumerate() {
                powers[index] = backend.wire_state(node).power;
            }
        }
        let mut differences = Vec::new();
        for &index in order {
            let node = net.wires[index];
            let weighted = net.terms[index]
                .iter()
                .map(|&(source, weight)| sources[source].saturating_sub(weight))
                .max()
                .unwrap_or(0)
                .max(net.constants[index]);
            let live = indices
                .get(&node)
                .map(|&index| powers[index])
                .unwrap_or_else(|| backend.wire_state(node).power);
            if weighted != live {
                let plan = backend.wire_plan(node).clone();
                let block = backend.seed_power_general(&plan).1;
                differences.push(json!({"node":node.index(),"position":backend.program.nodes()[node.index()].position,"kind":format!("{:?}",backend.program.nodes()[node.index()].kind),"old":backend.wire_state(node).power,"weighted":weighted,"live_fixed_point":live,"live_block_power":block,"live_incoming":backend.fixed_point_wire_neighbors(node).iter().map(|node|json!({"node":node.index(),"position":backend.program.nodes()[node.index()].position,"power":backend.wire_state(*node).power})).collect::<Vec<_>>(),"frozen_incoming":net.incoming[index].iter().map(|&index|json!({"node":net.wires[index].index(),"position":backend.program.nodes()[net.wires[index].index()].position})).collect::<Vec<_>>(),"terms":net.terms[index].iter().map(|&(source,weight)|json!({"source":format!("{:?}",net.sources[source]),"source_node":match net.sources[source] {Source::Node(node)|Source::LiveBlockPower(node)=>json!({"position":backend.program.nodes()[node.index()].position,"kind":format!("{:?}",backend.program.nodes()[node.index()].kind)})},"value":sources[source],"attenuation":weight})).collect::<Vec<_>>(),"neighbors":plan.canonical_neighbors.iter().map(|&site|json!({"site":site.index(),"source":format!("{:?}",backend.program.sites()[site.index()].source),"resolved":format!("{:?}",backend.resolve_site(site,None))})).collect::<Vec<_>>()}));
            }
        }
        for (&node, &index) in &indices {
            if !order.iter().any(|&index| net.wires[index] == node) && powers[index] != backend.wire_state(node).power {
                differences.push(json!({"live_only_node":node.index(),"position":backend.program.nodes()[node.index()].position,"old":backend.wire_state(node).power,"live_fixed_point":powers[index]}));
            }
        }
        if differences.is_empty() {
            return;
        }
        let event = self.diagnostic_events.fetch_add(1, Ordering::Relaxed);
        if event >= 16 {
            return;
        }
        let row = json!({"event":event,"tick":backend.future.clock.logical_tick,"phase":format!("{:?}",backend.future.clock.phase),"root":root.index(),"root_position":backend.program.nodes()[root.index()].position,"root_live_old":backend.wire_state(root).power,"root_sampled_old":sampled_old,"root_seed":seed,"captured":format!("{captured:?}"),"differences":differences,"pistons":backend.state.pistons.iter().enumerate().map(|(index,state)|json!({"node":backend.program.pistons()[index].node.index(),"position":backend.program.nodes()[backend.program.pistons()[index].node.index()].position,"state":format!("{state:?}")})).collect::<Vec<_>>()});
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/unchecked-classic-local-diagnostic.jsonl");
        use std::io::Write;
        writeln!(std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap(), "{row}").unwrap();
        println!(
            "LOCAL DIAGNOSTIC event{event} tick{} root{:?}: {} differences",
            backend.future.clock.logical_tick,
            backend.program.nodes()[root.index()].position,
            row["differences"].as_array().unwrap().len()
        );
    }

    fn counts(&self) -> serde_json::Value {
        json!({"net_invocations":self.calls.load(Ordering::Relaxed),"source_samples":self.source_samples.load(Ordering::Relaxed),"weighted_terms":self.weighted_terms.load(Ordering::Relaxed),"final_wire_writes":self.final_writes.load(Ordering::Relaxed),"boundary_callbacks":self.boundary_calls.load(Ordering::Relaxed),"changed_wires_outside_directed_root_closure":self.unreachable_changed.load(Ordering::Relaxed)})
    }

    fn coverage(&self) -> serde_json::Value {
        json!({"compiled_nets":self.nets.iter().filter(|net|self.handles(net.wires[0])).count(),"compiled_wires":self.nets.iter().filter(|net|self.handles(net.wires[0])).map(|net|net.wires.len()).sum::<usize>(),"preservation":self.preservation,"nets":self.nets.len(),"wires":self.nets.iter().map(|net|net.wires.len()).sum::<usize>(),"zero_source_nets":self.nets.iter().filter(|net|net.sources.is_empty()).count(),"single_source_nets":self.nets.iter().filter(|net|net.sources.len()==1).count(),"multi_source_nets":self.nets.iter().filter(|net|net.sources.len()>1).count(),"largest_net":self.nets.iter().map(|net|net.wires.len()).max(),"source_terms":self.nets.iter().flat_map(|net|&net.terms).map(Vec::len).sum::<usize>(),"live_contact_block_samples":self.nets.iter().flat_map(|net|&net.sources).filter(|source| matches!(source,Source::LiveBlockPower(_))).count(),"declines":{},"topology":"initial resolved dust geometry frozen; external node strengths, contact block-power queries and boundary selection live","boundary":"v4 ordinary seed entry, root-closure writes only; root-first directed BFS, live callback selection, deduplicated snapshots before dispatch; no transient/order proof"})
    }
}

#[test]
fn unchecked_weighted_expression_combines_two_sources_and_attenuates_constants() {
    let mut expression = Expression {
        constant: 0,
        sources: BTreeMap::from([(Source::Node(NodeId::new(1)), 0)]),
    };
    expression.include(
        &Expression {
            constant: 8,
            sources: BTreeMap::from([(Source::Node(NodeId::new(2)), 0)]),
        },
        3,
    );
    assert_eq!(expression.sources.len(), 2);
    assert_eq!(expression.sources[&Source::Node(NodeId::new(2))], 3);
    assert_eq!(expression.constant, 5);
}

fn small_lines(piston: bool) -> GraphBackend {
    use crate::plot::PlotWorld;
    use crate::world::storage::Chunk;
    use mchprs_blocks::blocks::{Lever, LeverFace, RedstonePiston};
    use mchprs_blocks::{BlockDirection, BlockFacing};
    let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
    for z in [2, 10] {
        for x in 2..=5 {
            world.set_block(BlockPos::new(x, 19, z), Block::Stone {});
        }
        world.set_block(
            BlockPos::new(2, 20, z),
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
            },
        );
        for x in 3..=5 {
            world.set_block(BlockPos::new(x, 20, z), Block::RedstoneWire { wire: Default::default() });
        }
    }
    if piston {
        world.set_block(
            BlockPos::new(3, 21, 3),
            Block::Piston {
                piston: RedstonePiston {
                    facing: BlockFacing::North,
                    ..Default::default()
                },
            },
        );
    }
    for z in [2, 10] {
        for x in 3..=5 {
            let position = BlockPos::new(x, 20, z);
            let wire = crate::redstone::wire::get_state_for_placement(&world, position);
            world.set_block(position, Block::RedstoneWire { wire });
        }
    }
    GraphBackend::compile_with_options(
        &world,
        (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15)),
        vec![],
        &crate::redpiler::CompilerOptions {
            optimization_level: Some(0),
            ..Default::default()
        },
        Arc::new(TaskMonitor::default()),
    )
    .unwrap()
}

#[test]
fn unchecked_entry_does_not_publish_undelivered_source_on_unchanged_seed() {
    let mut backend = small_lines(false);
    let program = Arc::new(UncheckedProgram::compile_mode(&backend, false));
    backend.unchecked_classic = Some(program.clone());
    let source = backend.positions[&BlockPos::new(2, 20, 2)];
    let ComponentState::Electrical(state) = &mut backend.state.components[source.index()] else {
        unreachable!()
    };
    state.powered = true;
    state.output_strength = 15;
    let far = backend.positions[&BlockPos::new(5, 20, 2)];
    backend.evaluate_seed(far, None).unwrap();
    assert_eq!(
        program.calls.load(Ordering::Relaxed),
        0,
        "an unchanged seed cannot enter weighted settlement"
    );
    assert!(backend.program.wires().iter().all(|wire| backend.wire_state(wire.node).power == 0));
    assert!(backend.scheduled_ticks().is_empty());
}

#[test]
fn unchecked_hybrid_preserves_whole_contact_region_but_keeps_remote_fixed_line() {
    let backend = small_lines(true);
    let program = UncheckedProgram::compile_mode(&backend, true);
    for x in 3..=5 {
        assert!(!program.handles(backend.positions[&BlockPos::new(x, 20, 2)]));
        assert!(program.handles(backend.positions[&BlockPos::new(x, 20, 10)]));
    }
    assert_eq!(program.preservation["preserved_wires"], 3);
}

fn counts(backend: &GraphBackend) -> serde_json::Value {
    let counts = backend.execution_counts();
    json!({"ticks":counts.ticks,"callbacks":counts.callbacks,"wire_walks":counts.wire_walks,"scheduled_ticks":counts.scheduled_ticks,"piston_samples":counts.piston_samples,"piston_actions":counts.piston_actions})
}

#[test]
#[ignore = "UNSAFE full-ANPU weighted-collapse performance/output experiment; never production admission"]
fn unchecked_classic_anpu_benchmark() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cpu = cpus::CPUS[1];
    let world = cpus::load_cpu(cpu);
    let clip = crate::plot::worldedit::load_schematic(std::fs::File::open(root.join("test_data").join(cpu.schematic)).unwrap()).unwrap();
    let bounds = (
        cpu.origin,
        cpu.origin + BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1),
    );
    let options = crate::redpiler::CompilerOptions {
        optimization_level: Some(2),
        budget_multiplier: 8,
        ..Default::default()
    };
    let template = GraphBackend::compile_with_options(
        &world,
        bounds,
        world.scheduler().iter_entries().collect(),
        &options,
        Arc::new(TaskMonitor::default()),
    )
    .unwrap();
    let program = template.program.clone();
    if std::env::var_os("MCHPRS_UNCHECKED_ANPU_DIAGNOSTIC").is_some() {
        let source = &program.nodes()[13961];
        println!("ENTRY WITNESS source13961 position{:?} kind{:?}", source.position, source.kind);
    }
    let state = template.state.clone();
    let compile_started = Instant::now();
    let unchecked = Arc::new(UncheckedProgram::compile(&template));
    let compiled_seconds = compile_started.elapsed().as_secs_f64();
    let coverage = unchecked.coverage();
    assert_eq!(coverage["wires"], program.wires().len());
    assert_eq!(program.wires().len(), 34_781, "pinned ANPU wire inventory");
    drop(template);
    drop(world);
    let frames: Vec<cpus::ScreenFrame> =
        serde_json::from_slice(&std::fs::read(root.join("test_data/cpu-references/anpu_screen.json")).unwrap()).unwrap();
    let ticks = std::env::var("MCHPRS_UNCHECKED_ANPU_TICKS")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(400);
    let samples = std::env::var("MCHPRS_UNCHECKED_ANPU_SAMPLES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(3);
    assert!(ticks > 0 && samples > 0);
    let mut reports = Vec::new();
    println!("UNSAFE ANPU coverage {coverage}; compiled {compiled_seconds:.6}s; {ticks} ticks x {samples} samples");
    for sample in 0..samples {
        let mut backends = (0..3)
            .map(|_| GraphBackend::new(program.clone(), state.clone(), 0).unwrap())
            .collect::<Vec<_>>();
        let experiment = Arc::new(UncheckedProgram::compile(&backends[2]));
        backends[2].unchecked_classic = Some(experiment.clone());
        let mut worlds = (0..3).map(|_| cpus::load_cpu(cpu)).collect::<Vec<_>>();
        let mut execution = [Duration::ZERO; 3];
        let mut projection = [Duration::ZERO; 3];
        let mut activation = [Duration::ZERO; 3];
        let mut first_state = [const { None }; 3];
        let mut first_future = [None; 3];
        let mut first_screen = [const { None }; 3];
        let mut screen_mismatch_ticks = [0u32; 3];
        let mut state_mismatch_ticks = [0u32; 3];
        let mut future_mismatch_ticks = [0u32; 3];
        let mut scheduler_mismatch_ticks = [0u32; 3];
        let mut scheduler_multiset_mismatch_ticks = [0u32; 3];
        let mut pending_mismatch_ticks = [0u32; 3];
        let mut first_scheduler = [None; 3];
        let mut first_scheduler_witness = [const { None }; 3];
        let mut first_scheduler_multiset = [None; 3];
        let mut first_pending = [None; 3];
        let mut other_future_mismatch_ticks = [0u32; 3];
        let mut first_other_future = [None; 3];
        let mut changed_frames = [0u32; 3];
        let mut previous = [String::new(), String::new(), String::new()];
        let mut hashes = [0xcbf29ce484222325u64; 3];
        for index in 0..3 {
            let started = Instant::now();
            fixed_point::with_mode(index == 1, || backends[index].use_block(cpu.origin + cpu.start)).unwrap();
            activation[index] = started.elapsed();
        }
        for tick in 0..=ticks {
            if tick > 0 {
                let order = if sample % 2 == 0 { [0, 1, 2] } else { [2, 1, 0] };
                for index in order {
                    let started = Instant::now();
                    fixed_point::with_mode(index == 1, || backends[index].tick()).unwrap();
                    execution[index] += started.elapsed();
                }
            }
            let expected = &frames.iter().rev().find(|frame| frame.tick <= tick).unwrap().pixels;
            for index in 0..3 {
                let started = Instant::now();
                backends[index].flush(&mut worlds[index], false);
                projection[index] += started.elapsed();
                let screen = cpus::screen(&worlds[index]);
                if screen != *expected {
                    screen_mismatch_ticks[index] += 1;
                    if first_screen[index].is_none() {
                        let pixel = screen.bytes().zip(expected.bytes()).position(|(a, b)| a != b).unwrap();
                        first_screen[index] = Some(
                            json!({"tick":tick,"pixel":pixel,"position":[148,97-pixel/32,46+pixel%32],"actual":screen.as_bytes()[pixel] as char,"expected":expected.as_bytes()[pixel] as char}),
                        );
                    }
                }
                if previous[index] != screen {
                    changed_frames[index] += 1;
                }
                for byte in screen.bytes() {
                    hashes[index] = (hashes[index] ^ u64::from(byte)).wrapping_mul(0x100000001b3);
                }
                previous[index] = screen;
                if index != 0 {
                    if backends[index].state != backends[0].state {
                        state_mismatch_ticks[index] += 1;
                        if first_state[index].is_none() {
                            let component = backends[index]
                                .state
                                .components
                                .iter()
                                .zip(&backends[0].state.components)
                                .position(|(a, b)| a != b);
                            first_state[index] =
                                Some(json!({"tick":tick,"component":component,"position":component.and_then(|node|program.nodes()[node].position)}));
                        }
                    }
                    let mut actual_scheduler = backends[index].scheduled_ticks();
                    let mut expected_scheduler = backends[0].scheduled_ticks();
                    let scheduler_differs = actual_scheduler != expected_scheduler;
                    let pending_differs = backends[index].future.pending != backends[0].future.pending;
                    let other_future = |backend: &GraphBackend| {
                        format!(
                            "{:?}|{:?}|{:?}",
                            backend.future.clock, backend.future.piston_requests, backend.future.deferred_pistons
                        )
                    };
                    let other_future_differs = other_future(&backends[index]) != other_future(&backends[0]);
                    if other_future_differs {
                        other_future_mismatch_ticks[index] += 1;
                        first_other_future[index].get_or_insert(tick);
                    }
                    if scheduler_differs {
                        scheduler_mismatch_ticks[index] += 1;
                        if first_scheduler[index].is_none() {
                            let ordinal = actual_scheduler
                                .iter()
                                .zip(&expected_scheduler)
                                .position(|(a, b)| a != b)
                                .unwrap_or(actual_scheduler.len().min(expected_scheduler.len()));
                            let window = |entries: &[TickEntry]| {
                                entries[ordinal.saturating_sub(1)..(ordinal + 3).min(entries.len())]
                                    .iter()
                                    .map(|entry| format!("{entry:?}"))
                                    .collect::<Vec<_>>()
                            };
                            first_scheduler_witness[index] = Some(
                                json!({"tick":tick,"ordinal":ordinal,"normal":window(&expected_scheduler),"experimental":window(&actual_scheduler)}),
                            );
                        }
                        first_scheduler[index].get_or_insert(tick);
                        actual_scheduler.sort_by_cached_key(|entry| format!("{entry:?}"));
                        expected_scheduler.sort_by_cached_key(|entry| format!("{entry:?}"));
                        if actual_scheduler != expected_scheduler {
                            scheduler_multiset_mismatch_ticks[index] += 1;
                            first_scheduler_multiset[index].get_or_insert(tick);
                        }
                    }
                    if pending_differs {
                        pending_mismatch_ticks[index] += 1;
                        first_pending[index].get_or_insert(tick);
                    }
                    if scheduler_differs || pending_differs || other_future_differs {
                        future_mismatch_ticks[index] += 1;
                        if first_future[index].is_none() {
                            first_future[index] = Some(tick);
                        }
                    }
                }
            }
        }
        let names = ["normal_graph_o2", "dynamic_fixed_point_unsafe", "compiled_weighted_unsafe"];
        assert_eq!(screen_mismatch_ticks[0], 0, "normal Graph must match the frozen screen at every tick");
        assert!(backends[0].counts.callbacks > 0 && backends[0].counts.wire_walks > 0);
        assert!(experiment.calls.load(Ordering::Relaxed) > 0, "unchecked hook must actually execute");
        if std::env::var_os("MCHPRS_UNCHECKED_ANPU_HYBRID").is_some() {
            assert!(backends[2].counts.wire_walks > 0, "hybrid must execute preserved general wire work");
        }
        let modes = (0..3).map(|index| json!({"mode":names[index],"activation_seconds":activation[index].as_secs_f64(),"execution_seconds":execution[index].as_secs_f64(),"projection_seconds":projection[index].as_secs_f64(),"counts":counts(&backends[index]),"screen_mismatch_ticks":screen_mismatch_ticks[index],"first_screen_mismatch":first_screen[index],"state_mismatch_ticks":state_mismatch_ticks[index],"first_state_mismatch":first_state[index],"future_mismatch_ticks":future_mismatch_ticks[index],"first_future_mismatch":first_future[index],"changed_frames_including_initial":changed_frames[index],"screen_trace_fnv1a64":format!("{:016x}",hashes[index])})).collect::<Vec<_>>();
        let mut modes = modes;
        for (index, mode) in modes.iter_mut().enumerate() {
            mode["scheduler_mismatch_ticks"] = json!(scheduler_mismatch_ticks[index]);
            mode["first_scheduler_mismatch"] = json!(first_scheduler[index]);
            mode["first_scheduler_witness"] = json!(first_scheduler_witness[index]);
            mode["scheduler_multiset_mismatch_ticks"] = json!(scheduler_multiset_mismatch_ticks[index]);
            mode["first_scheduler_multiset_mismatch"] = json!(first_scheduler_multiset[index]);
            mode["pending_mismatch_ticks"] = json!(pending_mismatch_ticks[index]);
            mode["first_pending_mismatch"] = json!(first_pending[index]);
            mode["piston_deferred_clock_mismatch_ticks"] = json!(other_future_mismatch_ticks[index]);
            mode["first_piston_deferred_clock_mismatch"] = json!(first_other_future[index]);
        }
        let row = json!({"sample":sample,"modes":modes,"compiled_work":experiment.counts()});
        println!("UNSAFE ANPU sample {row}");
        reports.push(row);
    }
    let report = json!({"experiment":"UNSAFE v4 seed-admitted weighted hybrid with complete potential-region preservation, not certified classic mask runtime","timing_protocol":"tick-interleaved modes, alternating first/last per sample; state/future/screen checks and projection outside execution timers; cache-perturbed exploratory timing","profile":if cfg!(debug_assertions){"debug"}else{"release"},"schematic_sha256":cpu.sha256,"ticks":ticks,"samples":samples,"coverage":coverage,"compile_seconds":compiled_seconds,"reports":reports});
    let destination = root.join(format!("target/unchecked-classic-anpu-{ticks}.json"));
    std::fs::write(&destination, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!("UNSAFE ANPU result {}", destination.display());
}
