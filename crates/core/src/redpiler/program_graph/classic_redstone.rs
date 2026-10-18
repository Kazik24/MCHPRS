//! Fixed electrical witnesses and compact ordered committed-signal programs.
use super::*;
use crate::redpiler::power_rules::{emits_strong, emits_weak};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const CLASSIC_MAX_WIRES: usize = 32;
pub(crate) const CLASSIC_PROOF_WORK: usize = 2_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClassicWire {
    pub(crate) node: NodeId,
    pub(crate) attenuation: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClassicInput {
    pub(crate) receiver: NodeId,
    pub(crate) attenuation: u8,
    /// The same ingress may also contribute directly before the net route commits.
    pub(crate) source_attenuation: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClassicOp {
    pub(crate) receiver: NodeId,
    pub(crate) nonzero: bool,
    /// Pair index is `(old << 4) | new`; masks select operations, not whole transcripts.
    pub(crate) when: [u64; 4],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClassicEntry {
    pub(crate) route: RouteId,
    /// The actual delivery ordinal disambiguates duplicate sequence numbers.
    pub(crate) ordinal: u32,
    pub(crate) wire: NodeId,
    pub(crate) commit: bool,
    pub(crate) ops: Box<[ClassicOp]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClassicNetPlan {
    pub(crate) guarded_wires: Box<[NodeId]>,
    /// Strength is the existing component output, committed at each admitted occurrence.
    pub(crate) source: NodeId,
    pub(crate) initial_signal: u8,
    pub(crate) wires: Box<[ClassicWire]>,
    pub(crate) inputs: Box<[ClassicInput]>,
    pub(crate) entries: Box<[ClassicEntry]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClassicNet {
    pub(crate) guarded_wires: Range<usize>,
    pub(crate) source: NodeId,
    pub(crate) wires: Range<usize>,
    pub(crate) inputs: Range<usize>,
    pub(crate) entries: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClassicEntryOp {
    pub(crate) net: u32,
    pub(crate) commit: bool,
    pub(crate) ops: Range<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClassicRouteEntry {
    pub(crate) ordinal: u32,
    pub(crate) entry: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ClassicProgram {
    pub(crate) guarded_wires: Box<[NodeId]>,
    pub(crate) nets: Box<[ClassicNet]>,
    pub(crate) wires: Box<[ClassicWire]>,
    pub(crate) inputs: Box<[ClassicInput]>,
    pub(crate) entries: Box<[ClassicEntryOp]>,
    pub(crate) ops: Box<[ClassicOp]>,
    pub(crate) route_ranges: Box<[Range<usize>]>,
    pub(crate) route_entries: Box<[ClassicRouteEntry]>,
    pub(crate) wire_by_node: Box<[Option<(u32, u8)>]>,
    pub(crate) input_by_node: Box<[Option<(u32, u8, u8)>]>,
}

/// A pure static expression: maximum of attenuated variable leaves and a constant.
#[derive(Clone, Debug, Default)]
struct Expression {
    leaves: BTreeMap<NodeId, u8>,
    constant: u8,
}

impl Expression {
    fn include(&mut self, other: Self, attenuation: u8) {
        self.constant = self.constant.max(other.constant.saturating_sub(attenuation));
        for (source, distance) in other.leaves {
            let distance = distance.saturating_add(attenuation);
            if distance < 15 {
                self.leaves
                    .entry(source)
                    .and_modify(|old| *old = (*old).min(distance))
                    .or_insert(distance);
            }
        }
    }
}

/// Resolves the complete fixed electrical query rather than the saved active contacts.
/// Guard-blind reverse indexes only narrow candidates; inference retains the original checks
/// and sorted target/route order. This avoids scanning the whole CPU for each local net.
pub(crate) struct ClassicAnalysis<'a> {
    graph: &'a CompileGraph,
    inputs: BTreeMap<PowerTarget, Vec<&'a PowerLink>>,
    /// All input ports structurally consulting each component, including guarded conducted paths.
    readers: Vec<Vec<PowerTarget>>,
    /// Route IDs touching each node as source, explicit receiver, or fallback target, in route order.
    touching_routes: Vec<Vec<usize>>,
    /// Watched and already-owned wires cannot become private classic cells.
    watched: BTreeSet<NodeId>,
    dust_owned: BTreeSet<NodeId>,
    instant_owned: BTreeSet<NodeId>,
}

impl<'a> ClassicAnalysis<'a> {
    pub(crate) fn new(graph: &'a CompileGraph) -> Self {
        let mut inputs = BTreeMap::<_, Vec<_>>::new();
        for link in &graph.power_links {
            inputs.entry(link.to).or_default().push(link);
        }
        let mut analysis = Self {
            graph,
            inputs,
            readers: vec![Vec::new(); graph.nodes.len()],
            touching_routes: vec![Vec::new(); graph.nodes.len()],
            watched: BTreeSet::new(),
            dust_owned: graph.plans.dust_paths.iter().flat_map(|path| path.wires.iter().copied()).collect(),
            instant_owned: graph
                .plans
                .instant
                .regions
                .iter()
                .flat_map(|region| region.wires.iter().copied())
                .collect(),
        };
        for &target in analysis.inputs.keys().filter(|target| matches!(target, PowerTarget::Input { .. })) {
            let mut dependencies = BTreeSet::new();
            analysis.dependencies(target, 0, &mut dependencies);
            for node in dependencies {
                analysis.readers[node.index()].push(target);
            }
        }
        for (index, route) in graph.routes.iter().enumerate() {
            let mut nodes = Vec::with_capacity(route.deliveries.len() * 2 + 1);
            nodes.push(route.source);
            for delivery in &route.deliveries {
                if let Some(CallbackReceiver::Component(node)) = delivery.receiver {
                    nodes.push(node);
                }
                if let SiteSource::Component(node) = graph.sites[delivery.target.index()].source {
                    nodes.push(node);
                }
            }
            nodes.sort_unstable();
            nodes.dedup();
            for node in nodes {
                analysis.touching_routes[node.index()].push(index);
            }
        }
        for node in &graph.nodes {
            if let CircuitNodeKind::Observer { watched, .. } | CircuitNodeKind::BuiltinReset { watched, .. } = node.kind
                && let SiteSource::Component(node) = graph.sites[watched.index()].source
            {
                analysis.watched.insert(node);
            }
        }
        analysis
    }

    // Match depends exactly: guards, emission and attenuation do not prune structural readers.
    fn dependencies(&self, target: PowerTarget, depth: usize, nodes: &mut BTreeSet<NodeId>) {
        if depth > 8 {
            return;
        }
        for link in self.inputs.get(&target).into_iter().flatten() {
            if let SiteSource::Component(node) = self.graph.sites[link.from.index()].source {
                nodes.insert(node);
            }
            if let PowerPort::Conducted { include_dust } = link.from_port {
                self.dependencies(
                    PowerTarget::Conductor {
                        site: link.from,
                        include_dust,
                    },
                    depth + 1,
                    nodes,
                );
            }
        }
    }

    fn block(&self, site: SiteId) -> Result<Block, &'static str> {
        match self.graph.sites[site.index()].source {
            SiteSource::Constant(profile) => Ok(Block::from_id(self.graph.profiles[profile.index()].raw_state)),
            SiteSource::Component(node) => self.graph.nodes[node.index()]
                .raw_state
                .map(Block::from_id)
                .ok_or("component lacks fixed block"),
            SiteSource::Contact(_) => Err("contact-dependent geometry or power"),
        }
    }

    fn guard(&self, id: GuardId, depth: usize, captured: Option<SiteId>) -> Result<bool, &'static str> {
        if depth > 64 {
            return Err("guard proof budget");
        }
        Ok(match &self.graph.guards[id.index()] {
            Guard::Always => true,
            Guard::State(_) => return Err("state-dependent boundary guard"),
            Guard::Site { site, view, test } => {
                if *view == OperandView::Captured && captured != Some(*site) {
                    return Ok(false);
                }
                let block = self.block(*site)?;
                match test {
                    SiteTest::Solid => block.is_solid(),
                    SiteTest::Transparent => block.is_transparent(),
                    SiteTest::IsWire => matches!(block, Block::RedstoneWire { .. }),
                    SiteTest::Profile(profile) => match self.graph.sites[site.index()].source {
                        SiteSource::Constant(actual) => actual == *profile,
                        SiteSource::Component(_) => false,
                        SiteSource::Contact(_) => return Err("contact profile guard"),
                    },
                }
            }
            Guard::Not(child) => !self.guard(*child, depth + 1, captured)?,
            Guard::All(children) => {
                let mut result = true;
                for child in children {
                    result &= self.guard(*child, depth + 1, captured)?;
                }
                result
            }
            Guard::Any(children) => {
                let mut result = false;
                for child in children {
                    result |= self.guard(*child, depth + 1, captured)?;
                }
                result
            }
        })
    }

    fn query(&self, target: PowerTarget, depth: usize) -> Result<Expression, &'static str> {
        if depth > 8 {
            return Err("conducted query proof budget");
        }
        let mut result = Expression::default();
        for link in self.inputs.get(&target).into_iter().flatten() {
            if self.guard(link.guard, 0, None)? {
                result.include(self.site(link.from, link.from_port, depth + 1)?, link.attenuation);
            }
        }
        Ok(result)
    }

    fn depends(&self, target: PowerTarget, owned: &BTreeSet<NodeId>, depth: usize) -> bool {
        if depth > 8 {
            return false;
        }
        self.inputs.get(&target).into_iter().flatten().any(|link| {
            matches!(self.graph.sites[link.from.index()].source,SiteSource::Component(node) if owned.contains(&node))
                || matches!(link.from_port,PowerPort::Conducted { include_dust } if self.depends(PowerTarget::Conductor { site:link.from,include_dust },owned,depth+1))
        })
    }

    fn site(&self, site: SiteId, port: PowerPort, depth: usize) -> Result<Expression, &'static str> {
        let block = self.block(site)?;
        match port {
            PowerPort::Conducted { include_dust } => return self.query(PowerTarget::Conductor { site, include_dust }, depth),
            PowerPort::Weak(face) if !emits_weak(block, face) => return Ok(Expression::default()),
            PowerPort::Strong(face) if !emits_strong(block, face) => return Ok(Expression::default()),
            _ => {}
        }
        Ok(match self.graph.sites[site.index()].source {
            SiteSource::Component(node) => Expression {
                leaves: BTreeMap::from([(node, 0)]),
                constant: 0,
            },
            SiteSource::Constant(_) => Expression {
                leaves: BTreeMap::new(),
                constant: self.graph.sites[site.index()].source_strength,
            },
            SiteSource::Contact(_) => return Err("contact-dependent electrical leaf"),
        })
    }

    /// A guarded callback wire is geometrically fixed and electrically disjoint
    /// from both private writes and this episode's changing source. The runtime
    /// checks its live seed at entry; seed/Turbo equality makes all held visits inert.
    fn check_guarded_wire(&self, node: NodeId, source: NodeId, owned: &BTreeSet<NodeId>) -> Result<(), &'static str> {
        let graph = self.graph;
        let CircuitNodeKind::Wire(index) = graph.nodes[node.index()].kind else {
            return Err("guarded boundary is not wire");
        };
        let plan = &graph.wires[index];
        let ComponentState::Wire(wire) = graph.nodes[node.index()].state else {
            return Err("guarded wire state");
        };
        let mut blocks = [Block::Air; 24];
        for (slot, site) in plan.canonical_neighbors.iter().enumerate() {
            blocks[slot] = self.block(*site)?;
        }
        let raw = crate::redstone::wire::get_raw_sides_from_neighbors(wire, &blocks);
        if crate::redstone::wire::regulate_sides(wire, raw) != wire
            || BlockFace::values()
                .into_iter()
                .any(|face| crate::redstone::wire::on_neighbor_changed_from(wire, raw, face) != wire)
        {
            return Err("guarded wire shape is not inert");
        }
        let facts = WireQueryFacts::infer(graph, plan);
        let transfer = facts.transfer.ok_or("guarded wire transfer missing")?;
        if facts.known != (1 << 24) - 1 || transfer.seed != transfer.turbo || transfer.seed.conditional != 0 {
            return Err("guarded wire seed/Turbo geometry differs");
        }
        let expression = self.query(PowerTarget::Input { node, port: InputPort::Main }, 0)?;
        if expression.leaves.keys().any(|leaf| *leaf == source || owned.contains(leaf)) {
            return Err("callback boundary depends on private ingress");
        }
        if self.dust_owned.contains(&node) || self.instant_owned.contains(&node) {
            return Err("guarded boundary has another owner");
        }
        Ok(())
    }

    /// Re-infer all static ownership and transfer facts for a whole proposed region.
    #[cfg(test)]
    pub(crate) fn direct_wire_sources(&self, node: NodeId) -> Result<Vec<NodeId>, &'static str> {
        Ok(self
            .query(PowerTarget::Input { node, port: InputPort::Main }, 0)?
            .leaves
            .into_keys()
            .collect())
    }

    /// Re-infer all static ownership and transfer facts for a whole proposed region.
    pub(crate) fn infer(&self, members: &[NodeId]) -> Result<ClassicNetPlan, &'static str> {
        if members.is_empty() || members.len() > CLASSIC_MAX_WIRES {
            return Err("region wire bound");
        }
        let graph = self.graph;
        let owned: BTreeSet<_> = members.iter().copied().collect();
        if owned.len() != members.len() {
            return Err("duplicate private wire");
        }
        let mut expressions = Vec::new();
        let mut guarded_wires = BTreeSet::new();
        let mut sources = BTreeSet::new();
        for &node in members {
            let Some(CircuitNodeDraft {
                kind: CircuitNodeKind::Wire(index),
                state: ComponentState::Wire(wire),
                ..
            }) = graph.nodes.get(node.index())
            else {
                return Err("private owner is not a wire");
            };
            if graph.wires[*index].execution.instant_reset.is_some() {
                return Err("wire already has instant reset owner");
            }
            let neighbors = graph.wires[*index].canonical_neighbors.map(|site| self.block(site));
            let mut blocks = [Block::Air; 24];
            for (slot, block) in neighbors.into_iter().enumerate() {
                let block = block?;
                if let Block::RedstoneWire { .. } = block {
                    match graph.sites[graph.wires[*index].canonical_neighbors[slot].index()].source {
                        SiteSource::Component(other) if owned.contains(&other) => {}
                        SiteSource::Component(other) if matches!(graph.nodes[other.index()].kind, CircuitNodeKind::Wire(_)) => {
                            guarded_wires.insert(other);
                        }
                        _ => return Err("callback-only or external private wire"),
                    }
                }
                blocks[slot] = block;
            }
            let raw = crate::redstone::wire::get_raw_sides_from_neighbors(*wire, &blocks);
            if crate::redstone::wire::regulate_sides(*wire, raw) != *wire {
                return Err("stale saved wire geometry");
            }
            if BlockFace::values()
                .into_iter()
                .any(|face| crate::redstone::wire::on_neighbor_changed_from(*wire, raw, face) != *wire)
            {
                return Err("shape callback is not inert");
            }
            let expression = self.query(PowerTarget::Input { node, port: InputPort::Main }, 0)?;
            if expression.constant != 0 {
                return Err("fixed nonzero ingress needs broader proof");
            }
            sources.extend(expression.leaves.keys().filter(|source| !owned.contains(source)).copied());
            expressions.push((node, expression));
        }
        if sources.len() != 1 {
            return Err("not exactly one variable ingress");
        }
        let source = *sources.first().unwrap();
        for &other in &guarded_wires {
            self.check_guarded_wire(other, source, &owned)?;
        }

        if !matches!(
            graph.nodes[source.index()].kind,
            CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::PressurePlate | NodeType::Torch | NodeType::Repeater { .. })
        ) {
            return Err("unsupported ingress component");
        }
        let mut distances = BTreeMap::<NodeId, u8>::from([(source, 0)]);
        for _ in 0..15 {
            for (node, expression) in &expressions {
                for (&from, &weight) in &expression.leaves {
                    if let Some(distance) = distances.get(&from).copied() {
                        let distance = distance.saturating_add(weight).min(15);
                        distances.entry(*node).and_modify(|old| *old = (*old).min(distance)).or_insert(distance);
                    }
                }
            }
        }
        let ComponentState::Electrical(source_state) = &graph.nodes[source.index()].state else {
            return Err("source state");
        };
        let initial_signal = source_state.output_strength;
        let mut wires = Vec::new();
        for &node in members {
            let attenuation = distances.get(&node).copied().unwrap_or(15);
            let ComponentState::Wire(wire) = graph.nodes[node.index()].state else {
                unreachable!()
            };
            if wire.power != initial_signal.saturating_sub(attenuation) {
                return Err("stale saved wire strength");
            }
            wires.push(ClassicWire { node, attenuation });
        }
        let mut receivers = BTreeSet::new();
        // Every electrical reader is inventoried, including conducted reads and all ports.
        let reader_targets = members
            .iter()
            .flat_map(|node| self.readers[node.index()].iter().copied())
            .collect::<BTreeSet<_>>();
        for target in &reader_targets {
            let PowerTarget::Input { node, port } = *target else { continue };
            if owned.contains(&node) {
                continue;
            }
            if !self.depends(*target, &owned, 0) {
                continue;
            }
            let expression = self.query(*target, 0)?;
            if expression.leaves.keys().any(|leaf| owned.contains(leaf)) {
                if port != InputPort::Main {
                    return Err("private wire read through side or quasi port");
                }
                receivers.insert(node);
            }
        }
        let mut entries = Vec::new();
        let route_ids = members
            .iter()
            .flat_map(|node| self.touching_routes[node.index()].iter().copied())
            .collect::<BTreeSet<_>>();
        for route_index in route_ids {
            let route = &graph.routes[route_index];
            let private_route = owned.contains(&route.source);
            let turbo_route = match graph.nodes[route.source.index()].kind {
                CircuitNodeKind::Wire(index) => graph.wires[index].turbo_route == Some(RouteId::new(route_index as u32)),
                _ => false,
            };
            for (ordinal, delivery) in route.deliveries.iter().enumerate() {
                let enters_owned = matches!(delivery.receiver,Some(CallbackReceiver::Component(node)) if owned.contains(&node))
                    || matches!(graph.sites[delivery.target.index()].source,SiteSource::Component(node) if owned.contains(&node));
                let active = match self.guard(delivery.guard, 0, turbo_route.then_some(delivery.target)) {
                    Ok(active) => active,
                    Err(reason) if private_route || enters_owned => return Err(reason),
                    Err(_) => continue,
                };
                if !active {
                    continue;
                }
                let receiver = delivery
                    .receiver
                    .or_else(|| match (graph.sites[delivery.target.index()].source, delivery.kind) {
                        (SiteSource::Component(node), DeliveryKind::Update { .. }) => Some(CallbackReceiver::Component(node)),
                        _ => None,
                    });
                if let Some(CallbackReceiver::Component(node)) = receiver
                    && owned.contains(&node)
                    && !private_route
                {
                    if guarded_wires.contains(&route.source) {
                        continue;
                    }
                    if delivery.view != OperandView::Live {
                        return Err("captured external wire entry");
                    }
                    entries.push(ClassicEntry {
                        route: RouteId::new(route_index as u32),
                        ordinal: ordinal as u32,
                        wire: node,
                        commit: matches!(delivery.kind, DeliveryKind::Update { .. }),
                        ops: Box::new([]),
                    });
                }
                if private_route {
                    match receiver {
                        None => {}
                        Some(CallbackReceiver::Component(node)) if owned.contains(&node) || guarded_wires.contains(&node) => {}
                        Some(CallbackReceiver::Component(node))
                            if matches!(
                                graph.nodes[node.index()].kind,
                                CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::PressurePlate)
                            ) => {}
                        Some(CallbackReceiver::Component(node))
                            if matches!(
                                graph.nodes[node.index()].kind,
                                CircuitNodeKind::Electrical(NodeType::Torch | NodeType::Repeater { .. })
                            ) =>
                        {
                            receivers.insert(node);
                        }
                        _ => return Err("effectful synchronous boundary callback"),
                    }
                }
            }
        }
        if !entries.iter().any(|entry| entry.commit) {
            return Err("no source-route wire entry");
        }
        let source_route = graph.nodes[source.index()].output_route.ok_or("ingress lacks output route")?;
        let first = entries
            .iter()
            .filter(|entry| entry.route == source_route && entry.commit)
            .map(|entry| entry.ordinal)
            .min()
            .ok_or("ingress route never enters net")?;
        // Only inert/delayed handlers may precede the first seed, closing the interval
        // where the source output is newer than this net's committed signal.
        for delivery in &graph.routes[source_route.index()].deliveries[..first as usize] {
            if delivery.view == OperandView::Captured || !self.guard(delivery.guard, 0, None)? {
                continue;
            }
            let receiver = delivery
                .receiver
                .or_else(|| match (graph.sites[delivery.target.index()].source, delivery.kind) {
                    (SiteSource::Component(node), DeliveryKind::Update { .. }) => Some(CallbackReceiver::Component(node)),
                    _ => None,
                });
            match receiver {
                None => {}
                Some(CallbackReceiver::Component(node))
                    if matches!(delivery.kind, DeliveryKind::Shape { .. }) && !matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Wire(_)) => {
                }
                Some(CallbackReceiver::Component(node)) if matches!(delivery.kind, DeliveryKind::Shape { .. }) && owned.contains(&node) => {}
                Some(CallbackReceiver::Component(node))
                    if matches!(
                        graph.nodes[node.index()].kind,
                        CircuitNodeKind::Electrical(
                            NodeType::Lever | NodeType::Button | NodeType::PressurePlate | NodeType::Torch | NodeType::Repeater { .. }
                        )
                    ) => {}
                Some(CallbackReceiver::Component(node)) if guarded_wires.contains(&node) => {}
                _ => return Err("ingress route prefix may synchronously emit or reenter"),
            }
        }
        for entry in &mut entries {
            if !entry.commit {
                continue;
            }
            if entry.route == source_route {
                entry.commit = entry.ordinal == first;
            } else {
                let owner = graph.routes[entry.route.index()].source;
                if graph.nodes[owner.index()].output_route != Some(entry.route)
                    || !matches!(
                        graph.nodes[owner.index()].kind,
                        CircuitNodeKind::Electrical(
                            NodeType::Lever | NodeType::Button | NodeType::PressurePlate | NodeType::Torch | NodeType::Repeater { .. }
                        )
                    )
                {
                    return Err("external update origin is not a closed ordinary output event");
                }
                entry.commit = false;
            }
        }
        if matches!(graph.nodes[source.index()].kind, CircuitNodeKind::Electrical(NodeType::Repeater { .. })) {
            let side = self.query(
                PowerTarget::Input {
                    node: source,
                    port: InputPort::Side,
                },
                0,
            )?;
            if side.constant != 0 || !side.leaves.is_empty() || source_state.repeater_locked {
                return Err("ingress repeater may change output silently while locking");
            }
        }
        if source_state.output_strength != u8::from(source_state.powered) * 15 {
            return Err("ingress saved output does not match powered state");
        }
        let mut inputs = Vec::new();
        for receiver in receivers {
            if !matches!(
                graph.nodes[receiver.index()].kind,
                CircuitNodeKind::Electrical(NodeType::Torch | NodeType::Repeater { .. })
            ) {
                return Err("receiver outside delayed first family");
            }
            if let ComponentState::Electrical(state) = &graph.nodes[receiver.index()].state
                && state.repeater_locked
            {
                return Err("saved repeater is locked");
            }
            let side = self.query(
                PowerTarget::Input {
                    node: receiver,
                    port: InputPort::Side,
                },
                0,
            )?;
            if side.constant != 0 || !side.leaves.is_empty() {
                return Err("repeater locking is not always zero");
            }
            let expression = self.query(
                PowerTarget::Input {
                    node: receiver,
                    port: InputPort::Main,
                },
                0,
            )?;
            if expression.constant != 0 || expression.leaves.keys().any(|leaf| !owned.contains(leaf) && *leaf != source) {
                return Err("receiver has independent main input");
            }
            let attenuation = expression
                .leaves
                .iter()
                .filter(|(node, _)| owned.contains(node))
                .map(|(node, weight)| distances.get(node).copied().unwrap_or(15).saturating_add(*weight).min(15))
                .min()
                .unwrap_or(15);
            let source_attenuation = expression.leaves.get(&source).copied().unwrap_or(15);
            inputs.push(ClassicInput {
                receiver,
                attenuation,
                source_attenuation,
            });
        }
        if owned.iter().any(|node| self.watched.contains(node)) {
            return Err("observer watches private wire");
        }
        if owned.iter().any(|node| self.dust_owned.contains(node)) {
            return Err("dust-path ownership overlap");
        }
        if owned.iter().any(|node| self.instant_owned.contains(node)) {
            return Err("instant region ownership overlap");
        }
        Ok(ClassicNetPlan {
            guarded_wires: guarded_wires.into_iter().collect(),
            source,
            initial_signal,
            wires: wires.into_boxed_slice(),
            inputs: inputs.into_boxed_slice(),
            entries: entries.into_boxed_slice(),
        })
    }
}

impl ClassicProgram {
    pub(crate) fn estimate_plans(plans: &[ClassicNetPlan], nodes: usize, routes: usize) -> usize {
        if plans.is_empty() {
            return 0;
        }
        let wires = plans.iter().map(|plan| plan.wires.len()).sum::<usize>();
        let inputs = plans.iter().map(|plan| plan.inputs.len()).sum::<usize>();
        let entries = plans.iter().map(|plan| plan.entries.len()).sum::<usize>();
        let ops = plans
            .iter()
            .flat_map(|plan| plan.entries.iter())
            .map(|entry| entry.ops.len())
            .sum::<usize>();
        use std::mem::size_of;
        plans.len() * size_of::<ClassicNet>()
            + plans.iter().map(|plan| plan.guarded_wires.len()).sum::<usize>() * size_of::<NodeId>()
            + wires * size_of::<ClassicWire>()
            + inputs * size_of::<ClassicInput>()
            + entries * (size_of::<ClassicEntryOp>() + size_of::<ClassicRouteEntry>())
            + ops * size_of::<ClassicOp>()
            + routes * size_of::<Range<usize>>()
            + nodes * (size_of::<Option<(u32, u8)>>() + size_of::<Option<(u32, u8, u8)>>())
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        std::mem::size_of_val(self.guarded_wires.as_ref())
            + std::mem::size_of_val(self.nets.as_ref())
            + std::mem::size_of_val(self.wires.as_ref())
            + std::mem::size_of_val(self.inputs.as_ref())
            + std::mem::size_of_val(self.entries.as_ref())
            + std::mem::size_of_val(self.ops.as_ref())
            + std::mem::size_of_val(self.route_ranges.as_ref())
            + std::mem::size_of_val(self.route_entries.as_ref())
            + std::mem::size_of_val(self.wire_by_node.as_ref())
            + std::mem::size_of_val(self.input_by_node.as_ref())
    }
    pub(crate) fn lower(plans: &[ClassicNetPlan], nodes: usize, routes: usize) -> Self {
        if plans.is_empty() {
            return Self::default();
        }
        let mut nets = Vec::new();
        let mut guarded_wires = Vec::new();
        let mut wires = Vec::new();
        let mut inputs = Vec::new();
        let mut entries = Vec::new();
        let mut ops = Vec::new();
        let mut wire_by_node = vec![None; nodes];
        let mut input_by_node = vec![None; nodes];
        let mut route_buckets = vec![Vec::new(); routes];
        for (net, plan) in plans.iter().enumerate() {
            let guard_start = guarded_wires.len();
            guarded_wires.extend_from_slice(&plan.guarded_wires);
            let wire_start = wires.len();
            let input_start = inputs.len();
            let entry_start = entries.len();
            for wire in &plan.wires {
                wire_by_node[wire.node.index()] = Some((net as u32, wire.attenuation));
                wires.push(*wire);
            }
            for input in &plan.inputs {
                input_by_node[input.receiver.index()] = Some((net as u32, input.attenuation, input.source_attenuation));
                inputs.push(*input);
            }
            for entry in &plan.entries {
                route_buckets[entry.route.index()].push(ClassicRouteEntry {
                    ordinal: entry.ordinal,
                    entry: entries.len() as u32,
                });
                let start = ops.len();
                ops.extend_from_slice(&entry.ops);
                entries.push(ClassicEntryOp {
                    net: net as u32,
                    commit: entry.commit,
                    ops: start..ops.len(),
                });
            }
            nets.push(ClassicNet {
                guarded_wires: guard_start..guarded_wires.len(),
                source: plan.source,
                wires: wire_start..wires.len(),
                inputs: input_start..inputs.len(),
                entries: entry_start..entries.len(),
            });
        }
        let mut route_ranges = Vec::new();
        let mut route_entries = Vec::new();
        for mut bucket in route_buckets {
            bucket.sort_by_key(|entry| entry.ordinal);
            let start = route_entries.len();
            route_entries.extend(bucket);
            route_ranges.push(start..route_entries.len());
        }
        Self {
            guarded_wires: guarded_wires.into_boxed_slice(),
            nets: nets.into_boxed_slice(),
            wires: wires.into_boxed_slice(),
            inputs: inputs.into_boxed_slice(),
            entries: entries.into_boxed_slice(),
            ops: ops.into_boxed_slice(),
            route_ranges: route_ranges.into_boxed_slice(),
            route_entries: route_entries.into_boxed_slice(),
            wire_by_node: wire_by_node.into_boxed_slice(),
            input_by_node: input_by_node.into_boxed_slice(),
        }
    }
}

/// Rebuild static facts and the compact certificate independently of pass output.
pub(crate) fn validate_classic(graph: &CompileGraph, monitor: &crate::redpiler::TaskMonitor) -> Result<(), GraphBuildError> {
    if graph.plans.classic_nets.is_empty() {
        return Ok(());
    }
    let invalid = |net| GraphBuildError::InvalidClassicNet {
        net,
        reason: "ownership, entry or ordered boundary program",
    };
    let analysis = ClassicAnalysis::new(graph);
    let mut owned = BTreeSet::new();
    let mut receivers = BTreeSet::new();
    for (index, plan) in graph.plans.classic_nets.iter().enumerate() {
        let members = plan.wires.iter().map(|wire| wire.node).collect::<Vec<_>>();
        if members.iter().any(|node| !owned.insert(*node)) || plan.inputs.iter().any(|input| !receivers.insert(input.receiver)) {
            return Err(invalid(index));
        }
        if plan.entries.iter().any(|entry| {
            graph
                .plans
                .instant
                .proxies
                .iter()
                .any(|proxy| proxy.routes.iter().any(|route| route.route == entry.route))
        }) {
            return Err(invalid(index));
        }
        let inferred = analysis
            .infer(&members)
            .map_err(|reason| GraphBuildError::InvalidClassicNet { net: index, reason })?;
        let mut supplied = plan.clone();
        for entry in &mut supplied.entries {
            entry.ops = Box::new([]);
        }
        if supplied != inferred {
            return Err(invalid(index));
        }
    }
    let mut reference = crate::redpiler::backend::graph::classic::ClassicReference::new(graph).map_err(|_| invalid(0))?;
    for (index, plan) in graph.plans.classic_nets.iter().enumerate() {
        reference.check(plan, monitor).map_err(|_| invalid(index))?;
    }
    Ok(())
}
