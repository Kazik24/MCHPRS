//! UNSAFE research only: fixed weighted dust, final writes, deduplicated boundary samples.
use super::*;
use crate::redpiler::power_rules::{emits_strong, emits_weak};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

#[cfg(test)]
#[path = "unchecked_chungus_tick512.rs"]
mod chungus_tick512_diagnostic;

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
    diagnostic_focus_node: AtomicU64,
    diagnostic_focus_comparator: AtomicU64,
    diagnostic_focus_tick: AtomicU64,
    diagnostic_focus_reported: AtomicBool,
    preserved: Vec<bool>,
    preservation: serde_json::Value,
}

impl UncheckedProgram {
    pub(super) fn attach_all(backend: &mut GraphBackend) -> Arc<Self> {
        assert!(backend.program.contacts().is_empty() && backend.program.pistons().is_empty() && backend.program.resources().is_empty());
        assert!(
            backend
                .program
                .wires()
                .iter()
                .all(|wire| wire.execution.query_facts.is_some_and(|facts| facts.known == (1 << 24) - 1))
        );
        let program = Arc::new(Self::compile_mode(backend, false));
        backend.test_recorder.unchecked_classic = Some(program.clone());
        program
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
                    .diagnostic_wire_neighbors(node)
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
            diagnostic_focus_node: AtomicU64::new(u64::MAX),
            diagnostic_focus_comparator: AtomicU64::new(u64::MAX),
            diagnostic_focus_tick: AtomicU64::new(u64::MAX),
            diagnostic_focus_reported: AtomicBool::new(false),
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
        let net_index = self.by_node[root.index()];
        let net = &self.nets[net_index];
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
                    backend.seed_power_general(&plan).block_power
                }
            })
            .collect::<Vec<_>>();
        self.trace_focus_if_mismatched(backend, root, captured, sampled_old, net_index, net, &order, &sources);
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
            backend.dispatch_turbo_nonwire(site, &observed, 1, None)?;
        }
        Ok(())
    }

    /// Test-only diagnostic for a bounded external witness. It records the live source values
    /// at the exact weighted write that first changes the focused wire.
    fn trace_focus_if_mismatched(
        &self,
        backend: &GraphBackend,
        root: NodeId,
        captured: Option<&ObservedSite>,
        sampled_old: u8,
        net_index: usize,
        net: &Net,
        order: &[usize],
        sources: &[u8],
    ) {
        let tick = self.diagnostic_focus_tick.load(Ordering::Relaxed);
        if tick != backend.future.clock.logical_tick {
            return;
        }
        let focus = self.diagnostic_focus_node.load(Ordering::Relaxed);
        let comparator = self.diagnostic_focus_comparator.load(Ordering::Relaxed);
        if focus == u64::MAX || focus as usize >= self.by_node.len() || self.by_node[focus as usize] != net_index {
            return;
        }
        let focus = NodeId::new(focus as u32);
        let Some(local) = net.wires.iter().position(|&node| node == focus) else {
            return;
        };
        if !order.contains(&local) {
            return;
        }
        let target_terms = &net.terms[local];
        let weighted = target_terms
            .iter()
            .map(|&(source, attenuation)| sources[source].saturating_sub(attenuation))
            .max()
            .unwrap_or(0)
            .max(net.constants[local]);
        let old = backend.wire_state(focus).power;
        if old == weighted || self.diagnostic_focus_reported.swap(true, Ordering::Relaxed) {
            return;
        }
        let source_rows = target_terms
            .iter()
            .map(|&(source, attenuation)| {
                let source_id = net.sources[source];
                let node = match source_id {
                    Source::Node(node) | Source::LiveBlockPower(node) => node,
                };
                json!({
                    "source":format!("{source_id:?}"),
                    "source_position":backend.program.nodes()[node.index()].position,
                    "source_kind":format!("{:?}", backend.program.nodes()[node.index()].kind),
                    "live_strength":sources[source],
                    "attenuation":attenuation,
                    "contribution":sources[source].saturating_sub(attenuation),
                })
            })
            .collect::<Vec<_>>();
        let comparator_state = (comparator != u64::MAX && (comparator as usize) < backend.state.components.len())
            .then(|| format!("{:?}", backend.state.components[comparator as usize]));
        let comparator_pending = (comparator != u64::MAX && (comparator as usize) < backend.program.nodes().len())
            .then(|| backend.is_pending(NodeId::new(comparator as u32)));
        println!(
            "UNCHECKED FOCUS WRITE {}",
            json!({
                "tick":tick,
                "root":root.index(),
                "root_position":backend.program.nodes()[root.index()].position,
                "sampled_old":sampled_old,
                "root_power_after_seed":backend.wire_state(root).power,
                "captured":format!("{captured:?}"),
                "target":focus.index(),
                "target_position":backend.program.nodes()[focus.index()].position,
                "target_old":old,
                "target_weighted":weighted,
                "source_terms":source_rows,
                "comparator":comparator,
                "comparator_state":comparator_state,
                "comparator_pending":comparator_pending,
                "scheduled_ticks":backend.scheduled_ticks(),
            })
        );
    }

    pub(super) fn set_diagnostic_focus(&self, node: NodeId, comparator: NodeId, tick: u64) {
        self.diagnostic_focus_node.store(node.index() as u64, Ordering::Relaxed);
        self.diagnostic_focus_comparator.store(comparator.index() as u64, Ordering::Relaxed);
        self.diagnostic_focus_tick.store(tick, Ordering::Relaxed);
        self.diagnostic_focus_reported.store(false, Ordering::Relaxed);
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
        let seed = backend.seed_power_general(&root_plan).power;
        let mut wires = vec![root];
        let mut indices = FxHashMap::from_iter([(root, 0usize)]);
        let mut incoming = Vec::new();
        let mut cursor = 0;
        while cursor < wires.len() {
            let mut from = Vec::new();
            for node in backend.diagnostic_wire_neighbors(wires[cursor]) {
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
                backend.seed_power_general(&plan).block_power
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
                let block = backend.seed_power_general(&plan).block_power;
                differences.push(json!({"node":node.index(),"position":backend.program.nodes()[node.index()].position,"kind":format!("{:?}",backend.program.nodes()[node.index()].kind),"old":backend.wire_state(node).power,"weighted":weighted,"live_fixed_point":live,"live_block_power":block,"live_incoming":backend.diagnostic_wire_neighbors(node).iter().map(|node|json!({"node":node.index(),"position":backend.program.nodes()[node.index()].position,"power":backend.wire_state(*node).power})).collect::<Vec<_>>(),"frozen_incoming":net.incoming[index].iter().map(|&index|json!({"node":net.wires[index].index(),"position":backend.program.nodes()[net.wires[index].index()].position})).collect::<Vec<_>>(),"terms":net.terms[index].iter().map(|&(source,weight)|json!({"source":format!("{:?}",net.sources[source]),"source_node":match net.sources[source] {Source::Node(node)|Source::LiveBlockPower(node)=>json!({"position":backend.program.nodes()[node.index()].position,"kind":format!("{:?}",backend.program.nodes()[node.index()].kind)})},"value":sources[source],"attenuation":weight})).collect::<Vec<_>>(),"neighbors":plan.canonical_neighbors.iter().map(|&site|json!({"site":site.index(),"source":format!("{:?}",backend.program.sites()[site.index()].source),"resolved":format!("{:?}",backend.resolve_site(site,None))})).collect::<Vec<_>>()}));
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

    pub(super) fn counts(&self) -> serde_json::Value {
        json!({"net_invocations":self.calls.load(Ordering::Relaxed),"source_samples":self.source_samples.load(Ordering::Relaxed),"weighted_terms":self.weighted_terms.load(Ordering::Relaxed),"final_wire_writes":self.final_writes.load(Ordering::Relaxed),"boundary_callbacks":self.boundary_calls.load(Ordering::Relaxed),"changed_wires_outside_directed_root_closure":self.unreachable_changed.load(Ordering::Relaxed)})
    }
}

impl UncheckedProgram {
    pub(super) fn coverage(&self) -> serde_json::Value {
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
    small_lines_receiver(piston, None)
}

fn small_lines_receiver(piston: bool, receiver: Option<Block>) -> GraphBackend {
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
    if let Some(block) = receiver {
        world.set_block(BlockPos::new(6, 20, 2), block);
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
    backend.test_recorder.unchecked_classic = Some(program.clone());
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

// Research cache: replay one complete native Turbo episode, never a final-state collapse.
#[derive(Clone, Debug, PartialEq, Eq)]
enum TapeGuard {
    Wire(RedstoneWire),
    Source { powered: bool, locked: bool, strength: u8 },
}

impl TapeGuard {
    fn current(backend: &GraphBackend, node: NodeId) -> Option<Self> {
        match &backend.state.components[node.index()] {
            ComponentState::Wire(wire) => Some(Self::Wire(*wire)),
            ComponentState::Electrical(state) if tape_emitter(backend, node) => Some(Self::Source {
                powered: state.powered,
                locked: state.repeater_locked,
                strength: state.output_strength,
            }),
            _ => None,
        }
    }
}

fn tape_emitter(backend: &GraphBackend, node: NodeId) -> bool {
    matches!(
        backend.program.nodes()[node.index()].kind,
        CircuitNodeKind::Observer { .. }
            | CircuitNodeKind::BuiltinReset { .. }
            | CircuitNodeKind::Electrical(
                NodeType::Torch
                    | NodeType::Repeater { .. }
                    | NodeType::Comparator { .. }
                    | NodeType::Observer { .. }
                    | NodeType::Lever
                    | NodeType::Button
                    | NodeType::PressurePlate
                    | NodeType::Constant
            )
    )
}

fn tape_handler(backend: &GraphBackend, receiver: CallbackReceiver) -> bool {
    match receiver {
        CallbackReceiver::PistonBase(_) | CallbackReceiver::PistonHead(_) => true,
        CallbackReceiver::Contact(_) => false,
        CallbackReceiver::Component(node) => matches!(
            backend.program.nodes()[node.index()].kind,
            CircuitNodeKind::Wire(_)
                | CircuitNodeKind::BuiltinReset { .. }
                | CircuitNodeKind::Electrical(
                    NodeType::Torch
                        | NodeType::Repeater { .. }
                        | NodeType::Comparator { .. }
                        | NodeType::Lamp
                        | NodeType::Hopper
                        | NodeType::Lever
                        | NodeType::Button
                        | NodeType::PressurePlate
                        | NodeType::Constant
                )
        ),
    }
}

// None means the condition can vary during simulation; captured tests need their actual target.
fn tape_static_guard(backend: &GraphBackend, guard: GuardId, captured: Option<(SiteId, &ObservedSite)>) -> Option<bool> {
    match &backend.program.guards()[guard.index()] {
        Guard::Always => Some(true),
        Guard::State(_) => None,
        Guard::Site { site, view, .. } => {
            if *view == OperandView::Captured && !captured.is_some_and(|(target, _)| target == *site) {
                return Some(false);
            }
            if matches!(backend.program.sites()[site.index()].source, SiteSource::Contact(_)) {
                return None;
            }
            Some(match captured {
                Some((site, observed)) => backend.guard_matches_for_site(guard, site, observed),
                None => backend.guard_matches(guard, None),
            })
        }
        Guard::Not(child) => tape_static_guard(backend, *child, captured).map(|value| !value),
        Guard::All(children) => {
            let values = children
                .iter()
                .map(|&child| tape_static_guard(backend, child, captured))
                .collect::<Vec<_>>();
            if values.contains(&Some(false)) {
                Some(false)
            } else if values.contains(&None) {
                None
            } else {
                Some(true)
            }
        }
        Guard::Any(children) => {
            let values = children
                .iter()
                .map(|&child| tape_static_guard(backend, child, captured))
                .collect::<Vec<_>>();
            if values.contains(&Some(true)) {
                Some(true)
            } else if values.contains(&None) {
                None
            } else {
                Some(false)
            }
        }
    }
}

fn tape_dynamic_query(backend: &GraphBackend, target: PowerTarget, depth: usize) -> bool {
    assert!(depth < 16);
    backend.program.power_links_to(target).iter().any(|link| {
        match tape_static_guard(backend, link.guard, None) {
            Some(false) => false,
            None => true,
            Some(true) => matches!(backend.program.sites()[link.from.index()].source, SiteSource::Contact(_))
                || matches!(link.from_port, PowerPort::Conducted { include_dust } if tape_dynamic_query(backend, PowerTarget::Conductor { site: link.from, include_dust }, depth+1)),
        }
    })
}

#[derive(Clone)]
enum TapeOp {
    Capture(SiteId),
    Write(NodeId, RedstoneWire),
    Call { site: SiteId, capture: usize, layer: u32 },
}
struct EpisodeTape {
    guards: Box<[(NodeId, TapeGuard)]>,
    ops: Box<[TapeOp]>,
    private_callbacks: u64,
}
#[derive(Default)]
struct TapeRecording {
    guards: BTreeMap<NodeId, TapeGuard>,
    captures: BTreeMap<SiteId, usize>,
    ops: Vec<TapeOp>,
    wires: BTreeSet<NodeId>,
    private_callbacks: u64,
    abort: Option<&'static str>,
}
#[derive(Default)]
struct TapeCache {
    recording: Option<TapeRecording>,
    replaying: bool,
    variants: BTreeMap<NodeId, Vec<Arc<EpisodeTape>>>,
    hits: u64,
    misses: u64,
    learning_sealed: bool,
    fallbacks: BTreeMap<&'static str, u64>,
    build_seconds: f64,
    retained_bytes: usize,
    capture_ops: u64,
    write_ops: u64,
    call_ops: u64,
    private_callbacks_elided: u64,
    represented: BTreeSet<NodeId>,
    fallback_wires: BTreeSet<NodeId>,
}

pub(super) struct TapeProgram {
    core: Vec<bool>,
    cache: std::sync::Mutex<TapeCache>,
    recording_active: std::sync::atomic::AtomicBool,
}
impl TapeProgram {
    pub(super) fn attach(backend: &mut GraphBackend) -> Arc<Self> {
        let mut core = vec![false; backend.program.nodes().len()];
        for plan in backend.program.wires() {
            let mut why = BTreeSet::new();
            if plan
                .canonical_neighbors
                .iter()
                .any(|site| matches!(backend.program.sites()[site.index()].source, SiteSource::Contact(_)))
            {
                why.insert("direct_contact_neighborhood");
            }
            if tape_dynamic_query(
                backend,
                PowerTarget::Input {
                    node: plan.node,
                    port: InputPort::Main,
                },
                0,
            ) {
                why.insert("direct_dynamic_power_query");
            }
            if plan.execution.instant_reset.is_some() {
                why.insert("instant_execution");
            }
            if let Some(route) = plan.turbo_route {
                for delivery in &backend.program.routes()[route.index()].deliveries {
                    let observed = backend.observe_site(delivery.target).unwrap();
                    let captured = (delivery.view == OperandView::Captured).then_some((delivery.target, &observed));
                    match tape_static_guard(backend, delivery.guard, captured) {
                        Some(false) => {
                            continue;
                        }
                        None => {
                            why.insert("dynamic_delivery_selection");
                        }
                        _ => (),
                    }
                    if let Some(receiver) = delivery.receiver
                        && !tape_handler(backend, receiver)
                    {
                        why.insert("synchronous_or_effectful_handler");
                    }
                }
            }
            core[plan.node.index()] = !why.is_empty();
        }
        let program = Arc::new(Self {
            core,
            cache: Default::default(),
            recording_active: false.into(),
        });
        backend.test_recorder.wire_tapes = Some(program.clone());
        program
    }
    pub(super) fn counts(&self) -> serde_json::Value {
        let cache = self.cache.lock().unwrap();
        json!({"hits":cache.hits,"misses":cache.misses,"build_seconds":cache.build_seconds,
            "variants":cache.variants.values().map(Vec::len).sum::<usize>(),"retained_bytes":cache.retained_bytes,"learning_sealed":cache.learning_sealed,
            "capture_ops":cache.capture_ops,"write_ops":cache.write_ops,"call_ops":cache.call_ops,
            "private_wire_callbacks_elided":cache.private_callbacks_elided,"wires_in_accepted_tapes":cache.represented.len(),
            "distinct_fallback_prefix_wires":cache.fallback_wires.len(),"fallbacks":cache.fallbacks})
    }
    pub(super) fn read(&self, backend: &GraphBackend, node: NodeId) {
        if !self.recording_active.load(Ordering::Relaxed) {
            return;
        }
        let mut cache = self.cache.lock().unwrap();
        if let Some(recording) = &mut cache.recording
            && recording.abort.is_none()
        {
            if let Some(value) = TapeGuard::current(backend, node) {
                recording.guards.entry(node).or_insert(value);
            }
        }
    }
    pub(super) fn source_site(&self, backend: &GraphBackend, site: SiteId) {
        if !self.recording_active.load(Ordering::Relaxed) {
            return;
        }
        match backend.program.sites()[site.index()].source {
            SiteSource::Component(node) => self.read(backend, node),
            SiteSource::Contact(_) => self.abort("dynamic_source_contact"),
            _ => (),
        }
    }
    fn abort(&self, reason: &'static str) {
        if !self.recording_active.swap(false, Ordering::Relaxed) {
            return;
        }
        if let Some(recording) = &mut self.cache.lock().unwrap().recording {
            recording.abort.get_or_insert(reason);
        }
    }
    pub(super) fn visit(&self, node: NodeId) {
        if !self.recording_active.load(Ordering::Relaxed) {
            return;
        }
        let mut cache = self.cache.lock().unwrap();
        if let Some(recording) = &mut cache.recording {
            recording.wires.insert(node);
            recording.private_callbacks += 1;
            if self.core[node.index()] {
                recording.abort.get_or_insert("core_crossing");
                self.recording_active.store(false, Ordering::Relaxed);
            }
        }
    }
    pub(super) fn write(&self, backend: &GraphBackend, node: NodeId, wire: RedstoneWire) {
        if !self.recording_active.load(Ordering::Relaxed) {
            return;
        }
        self.read(backend, node);
        let mut cache = self.cache.lock().unwrap();
        if let Some(recording) = &mut cache.recording
            && recording.abort.is_none()
        {
            if backend.state.components[node.index()] != ComponentState::Wire(wire) {
                recording.ops.push(TapeOp::Write(node, wire));
            }
            if recording.ops.len() > 65536 {
                recording.abort = Some("episode_op_limit");
                self.recording_active.store(false, Ordering::Relaxed);
            }
        }
    }
    pub(super) fn capture(&self, backend: &GraphBackend, site: SiteId, observed: &ObservedSite) {
        if !self.recording_active.load(Ordering::Relaxed) {
            return;
        }
        self.source_site(backend, site);
        let mut cache = self.cache.lock().unwrap();
        if let Some(recording) = &mut cache.recording
            && recording.abort.is_none()
        {
            if observed.receiver.is_some_and(|receiver| !tape_handler(backend, receiver)) {
                recording.abort = Some("effectful_capture");
                self.recording_active.store(false, Ordering::Relaxed);
                return;
            }
            let slot = recording.captures.len();
            if recording.captures.insert(site, slot).is_some() {
                recording.abort = Some("duplicate_capture");
                self.recording_active.store(false, Ordering::Relaxed);
                return;
            }
            if recording.ops.len() >= 65536 {
                recording.abort = Some("episode_op_limit");
                self.recording_active.store(false, Ordering::Relaxed);
            } else {
                recording.ops.push(TapeOp::Capture(site));
            }
        }
    }
    pub(super) fn call(&self, _backend: &GraphBackend, site: SiteId, _observed: &ObservedSite, layer: u32) {
        if !self.recording_active.load(Ordering::Relaxed) {
            return;
        }
        let mut cache = self.cache.lock().unwrap();
        if let Some(recording) = &mut cache.recording
            && recording.abort.is_none()
        {
            if let Some(&capture) = recording.captures.get(&site) {
                if recording.ops.len() >= 65536 {
                    recording.abort = Some("episode_op_limit");
                    self.recording_active.store(false, Ordering::Relaxed);
                } else {
                    recording.ops.push(TapeOp::Call { site, capture, layer });
                }
            } else {
                recording.abort = Some("uncaptured_call");
                self.recording_active.store(false, Ordering::Relaxed);
            }
        }
    }
    pub(super) fn run(&self, backend: &mut GraphBackend, node: NodeId) -> Result<(), GraphExecutionError> {
        let tape = {
            let mut cache = self.cache.lock().unwrap();
            assert!(!cache.replaying, "synchronous reentry into admitted wire tape");
            if let Some(recording) = &mut cache.recording {
                recording.abort = Some("synchronous_reentry");
                self.recording_active.store(false, Ordering::Relaxed);
                drop(cache);
                return backend.run_wire_tape_reference(node);
            }
            let found = cache.variants.get(&node).and_then(|variants| {
                variants
                    .iter()
                    .find(|tape| {
                        tape.guards
                            .iter()
                            .all(|(node, guard)| TapeGuard::current(backend, *node).as_ref() == Some(guard))
                    })
                    .cloned()
            });
            if found.is_some() {
                cache.hits += 1;
                cache.replaying = true;
            } else {
                cache.misses += 1;
                if self.core[node.index()] || cache.learning_sealed || cache.variants.get(&node).is_some_and(|variants| variants.len() >= 8) {
                    let reason = if self.core[node.index()] {
                        "root_core"
                    } else if cache.learning_sealed {
                        "sealed_byte_budget"
                    } else {
                        "root_variant_limit"
                    };
                    *cache.fallbacks.entry(reason).or_default() += 1;
                    cache.fallback_wires.insert(node);
                    drop(cache);
                    return backend.run_wire_tape_reference(node);
                }
                let mut recording = TapeRecording::default();
                recording.wires.insert(node);
                cache.recording = Some(recording);
                self.recording_active.store(true, Ordering::Relaxed);
            }
            found
        };
        if let Some(tape) = tape {
            let mut captures = Vec::new();
            let result = (|| {
                for op in &tape.ops {
                    match *op {
                        TapeOp::Capture(site) => captures.push(backend.observe_site(site).expect("fixed tape capture")),
                        TapeOp::Write(node, wire) => backend.set_wire(node, wire),
                        TapeOp::Call { site, capture, layer } => backend.dispatch_turbo_nonwire(site, &captures[capture], layer, None)?,
                    }
                }
                Ok(())
            })();
            let mut cache = self.cache.lock().unwrap();
            cache.replaying = false;
            cache.private_callbacks_elided += tape.private_callbacks;
            for op in &tape.ops {
                match op {
                    TapeOp::Capture(_) => cache.capture_ops += 1,
                    TapeOp::Write(..) => cache.write_ops += 1,
                    TapeOp::Call { .. } => cache.call_ops += 1,
                }
            }
            return result;
        }
        self.read(backend, node);
        if self.core[node.index()] {
            self.abort("root_core");
        }
        let started = Instant::now();
        let result = backend.run_wire_tape_reference(node);
        self.recording_active.store(false, Ordering::Relaxed);
        let mut cache = self.cache.lock().unwrap();
        cache.build_seconds += started.elapsed().as_secs_f64();
        let mut recording = cache.recording.take().unwrap();
        for (node, guard) in &recording.guards {
            if matches!(guard, TapeGuard::Source { .. }) && TapeGuard::current(backend, *node).as_ref() != Some(guard) {
                recording.abort.get_or_insert("source_mutation");
            }
        }
        if let Some(reason) = recording.abort {
            *cache.fallbacks.entry(reason).or_default() += 1;
            cache.fallback_wires.extend(recording.wires);
            return result;
        }
        // Only boundary callbacks consume captured snapshots. Keep their original discovery points.
        let used = recording
            .ops
            .iter()
            .filter_map(|op| match op {
                TapeOp::Call { capture, .. } => Some(*capture),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let mut remap = BTreeMap::new();
        let mut ordinal = 0;
        recording.ops = recording
            .ops
            .into_iter()
            .filter_map(|op| match op {
                TapeOp::Capture(site) => {
                    let old = ordinal;
                    ordinal += 1;
                    if used.contains(&old) {
                        remap.insert(old, remap.len());
                        Some(TapeOp::Capture(site))
                    } else {
                        None
                    }
                }
                TapeOp::Call { site, capture, layer } => Some(TapeOp::Call {
                    site,
                    capture: remap[&capture],
                    layer,
                }),
                other => Some(other),
            })
            .collect();
        let bytes = recording.ops.len() * std::mem::size_of::<TapeOp>()
            + recording.guards.len() * std::mem::size_of::<(NodeId, TapeGuard)>()
            + std::mem::size_of::<EpisodeTape>()
            + 2 * std::mem::size_of::<usize>();
        if recording.abort.is_none() && cache.retained_bytes + bytes > 33554432 {
            recording.abort = Some("cache_byte_limit");
            cache.learning_sealed = true;
        }
        if cache.variants.get(&node).is_some_and(|variants| variants.len() >= 8) {
            recording.abort.get_or_insert("root_variant_limit");
        }
        if let Some(reason) = recording.abort {
            *cache.fallbacks.entry(reason).or_default() += 1;
            cache.fallback_wires.extend(recording.wires);
        } else if result.is_ok() {
            cache.represented.extend(recording.wires.iter().copied());
            cache.retained_bytes += bytes;
            cache.variants.entry(node).or_default().push(Arc::new(EpisodeTape {
                guards: recording.guards.into_iter().collect::<Vec<_>>().into_boxed_slice(),
                ops: recording.ops.into_boxed_slice(),
                private_callbacks: recording.private_callbacks,
            }));
        }
        result
    }
}

#[test]
fn unchecked_tape_rebinds_receiver_capture_and_uses_live_pending() {
    let mut template = small_lines_receiver(false, Some(Block::RedstoneLamp { lit: true }));
    let root = template.positions[&BlockPos::new(3, 20, 2)];
    let lamp = template.positions[&BlockPos::new(6, 20, 2)];
    for x in 3..=5 {
        let node = template.positions[&BlockPos::new(x, 20, 2)];
        let mut wire = template.wire_state(node);
        wire.power = if x == 3 { 14 } else { 15 };
        template.set_wire(node, wire);
    }
    let state = template.state.clone();
    let program = template.program.clone();
    let tapes = TapeProgram::attach(&mut template);
    for (powered, pending) in [(true, false), (false, false), (true, true)] {
        let mut native = GraphBackend::new_unactivated(program.clone(), state.clone(), 0).unwrap();
        let mut replay = GraphBackend::new_unactivated(program.clone(), state.clone(), 0).unwrap();
        for backend in [&mut native, &mut replay] {
            let ComponentState::Electrical(lamp_state) = &mut backend.state.components[lamp.index()] else {
                unreachable!()
            };
            lamp_state.powered = powered;
            lamp_state.repeater_locked = !powered;
            if pending {
                backend.schedule(lamp, 2, TickPriority::Normal);
            }
        }
        replay.test_recorder.wire_tapes = Some(tapes.clone());
        native.run_wire_tape_reference(root).unwrap();
        tapes.run(&mut replay, root).unwrap();
        assert_eq!(replay.state, native.state, "receiver powered={powered}, pending={pending}");
        assert_eq!(format!("{:?}", replay.future.pending), format!("{:?}", native.future.pending));
        assert_eq!(replay.scheduled_ticks(), native.scheduled_ticks());
    }
    assert_eq!(tapes.counts()["misses"], 1, "one initial native episode");
    assert_eq!(tapes.counts()["hits"], 2, "receiver flags and pending are live, not cache guards");
    assert!(tapes.counts()["call_ops"].as_u64().unwrap() > 0);
}

#[test]
fn unchecked_tape_source_properties_guard_replay() {
    let mut template = small_lines(false);
    let root = template.positions[&BlockPos::new(3, 20, 2)];
    let source = template.positions[&BlockPos::new(2, 20, 2)];
    let mut wire = template.wire_state(root);
    wire.power = 1;
    template.set_wire(root, wire);
    let state = template.state.clone();
    let program = template.program.clone();
    let tapes = TapeProgram::attach(&mut template);
    for strength in [0, 0, 1] {
        let mut backend = GraphBackend::new_unactivated(program.clone(), state.clone(), 0).unwrap();
        let ComponentState::Electrical(source_state) = &mut backend.state.components[source.index()] else {
            unreachable!()
        };
        source_state.output_strength = strength;
        backend.test_recorder.wire_tapes = Some(tapes.clone());
        tapes.run(&mut backend, root).unwrap();
    }
    assert_eq!(tapes.counts()["hits"], 1);
    assert_eq!(
        tapes.counts()["misses"],
        2,
        "even a property not changing this episode must invalidate its recorded source guard"
    );
}

#[test]
fn unchecked_tape_unchanged_seed_does_not_publish_undelivered_source() {
    let mut backend = small_lines(false);
    let tapes = TapeProgram::attach(&mut backend);
    let source = backend.positions[&BlockPos::new(2, 20, 2)];
    let ComponentState::Electrical(state) = &mut backend.state.components[source.index()] else {
        unreachable!()
    };
    state.powered = true;
    state.output_strength = 15;
    let far = backend.positions[&BlockPos::new(5, 20, 2)];
    backend.evaluate_seed(far, None).unwrap();
    assert_eq!(tapes.counts()["misses"], 0);
    assert_eq!(tapes.counts()["hits"], 0);
    assert!(backend.program.wires().iter().all(|plan| backend.wire_state(plan.node).power == 0));
}

impl GraphBackend {
    fn diagnostic_wire_neighbors(&self, node: NodeId) -> Vec<NodeId> {
        let plan = self.wire_plan(node);
        let view = |index| {
            self.resolve_site_view(plan.canonical_neighbors[index], None)
                .expect("validated wire neighborhood")
        };
        let up_solid = view(3).solid;
        let mut indices = vec![0, 1, 2, 3, 4, 5];
        // Match seed_power_general's ascent and descent rules, not Turbo's notification radius.
        for (side, above, below) in [(4, 20, 17), (5, 21, 18), (1, 13, 12), (0, 8, 7)] {
            let neighbor = view(side);
            if !up_solid && !neighbor.transparent {
                indices.push(above);
            }
            if !neighbor.solid {
                indices.push(below);
            }
        }
        indices
            .into_iter()
            .filter_map(|index| match view(index).block {
                Block::RedstoneWire { .. } => match self.program.sites()[plan.canonical_neighbors[index].index()].source {
                    SiteSource::Component(node) => Some(node),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }
}
