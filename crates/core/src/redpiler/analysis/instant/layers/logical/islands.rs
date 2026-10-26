//! Conservative interaction components for independently executable logical devices.
//!
//! This partition intentionally forgets power direction: if either component can
//! change a value, callback, guard, contact or resource observed by the other,
//! they belong to one island. Literal profile leaves do not join components.
use super::*;
use crate::redpiler::program_graph::{CallbackReceiver, Guard, SiteSource};
use mchprs_blocks::blocks::Block;

const MAX_PARTITION_BYTES: usize = crate::redpiler::analysis::MAX_GRAPH_PREPARATION_BYTES;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct LogicalIslandPartition {
    pub(crate) islands: Box<[LogicalIsland]>,
    pub(crate) node_island: Box<[u32]>,
    pub(crate) resource_island: Box<[Option<u32>]>,
    pub(crate) piston_island: Box<[u32]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct LogicalIsland {
    pub(crate) id: u32,
    pub(crate) nodes: Box<[NodeId]>,
    pub(crate) sites: Box<[SiteId]>,
    pub(crate) contacts: Box<[ContactId]>,
    pub(crate) resources: Box<[ResourceId]>,
    pub(crate) pistons: Box<[PistonId]>,
    pub(crate) routes: Box<[RouteId]>,
}

struct Dsu {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl Dsu {
    fn new(len: usize) -> Self {
        Self {
            parent: (0..len).collect(),
            rank: vec![0; len],
        }
    }

    fn root(&mut self, mut item: usize) -> usize {
        let mut root = item;
        while self.parent[root] != root {
            root = self.parent[root];
        }
        while self.parent[item] != item {
            let next = self.parent[item];
            self.parent[item] = root;
            item = next;
        }
        root
    }

    fn join(&mut self, left: usize, right: usize) {
        let mut left = self.root(left);
        let mut right = self.root(right);
        if left == right {
            return;
        }
        if self.rank[left] < self.rank[right] || (self.rank[left] == self.rank[right] && left > right) {
            std::mem::swap(&mut left, &mut right);
        }
        self.parent[right] = left;
        if self.rank[left] == self.rank[right] {
            self.rank[left] = self.rank[left].saturating_add(1);
        }
    }
}

#[derive(Clone, Copy)]
struct EntityOffsets {
    sites: usize,
    contacts: usize,
    resources: usize,
    pistons: usize,
    routes: usize,
    guards: usize,
    end: usize,
}

impl EntityOffsets {
    fn new(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<Self, String> {
        let sites = graph.nodes.len();
        let contacts = sites.checked_add(graph.sites.len()).ok_or("logical island entity count overflow")?;
        let resources = contacts.checked_add(graph.contacts.len()).ok_or("logical island entity count overflow")?;
        let pistons = resources
            .checked_add(graph.resources.len())
            .ok_or("logical island entity count overflow")?;
        let routes = pistons.checked_add(graph.pistons.len()).ok_or("logical island entity count overflow")?;
        let guards = routes.checked_add(graph.routes.len()).ok_or("logical island entity count overflow")?;
        let end = guards.checked_add(graph.guards.len()).ok_or("logical island entity count overflow")?;
        let bytes = end
            .checked_mul(std::mem::size_of::<usize>() * 4 + std::mem::size_of::<u8>() * 2)
            .ok_or("logical island partition size overflow")?;
        let limit = MAX_PARTITION_BYTES.saturating_mul(monitor.budget_multiplier());
        if bytes > limit {
            return Err(format!("logical island partition needs {bytes} bytes; limit is {limit}"));
        }
        Ok(Self {
            sites,
            contacts,
            resources,
            pistons,
            routes,
            guards,
            end,
        })
    }

    fn node(self, id: NodeId) -> usize {
        id.index()
    }
    fn site(self, id: SiteId) -> usize {
        self.sites + id.index()
    }
    fn contact(self, id: ContactId) -> usize {
        self.contacts + id.index()
    }
    fn resource(self, id: ResourceId) -> usize {
        self.resources + id.index()
    }
    fn piston(self, id: PistonId) -> usize {
        self.pistons + id.index()
    }
    fn route(self, id: RouteId) -> usize {
        self.routes + id.index()
    }
    fn guard(self, id: GuardId) -> usize {
        self.guards + id.index()
    }
}

pub(crate) fn partition(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<LogicalIslandPartition, String> {
    let offsets = EntityOffsets::new(graph, monitor)?;
    let mut dsu = Dsu::new(offsets.end);
    let guard_dynamic = dynamic_guard_support(graph, monitor)?;
    let mut work = 0usize;
    let cancelled = |work: &mut usize| -> Result<(), String> {
        *work += 1;
        if *work & 0x3fff == 0 && monitor.cancelled() {
            return Err("cancelled while partitioning logical-device dependencies".into());
        }
        Ok(())
    };

    // Guard DAG edges are indexed once. Each guarded dependency then joins its
    // owner to the guard root in O(1), even when many links share a large guard.
    for (index, guard) in graph.guards.iter().enumerate() {
        let id = GuardId::new(index as u32);
        let entity = offsets.guard(id);
        match guard {
            Guard::Always => {}
            Guard::State(test) => match *test {
                StateTest::PistonExtended(actor) | StateTest::PistonHeadPresent(actor) | StateTest::PistonAvailability { piston: actor, .. } => {
                    dsu.join(entity, offsets.piston(actor));
                }
                StateTest::ResourceAt { resource, contact } => {
                    dsu.join(entity, offsets.resource(resource));
                    dsu.join(offsets.resource(resource), offsets.contact(contact));
                }
                StateTest::ResourceEnabled(resource) => dsu.join(entity, offsets.resource(resource)),
            },
            Guard::Site { site, .. } => join_site_dependency(graph, &mut dsu, offsets, entity, *site),
            Guard::Not(child) if guard_dynamic[child.index()] => dsu.join(entity, offsets.guard(*child)),
            Guard::Not(_) => {}
            Guard::All(children) | Guard::Any(children) => {
                for &child in children.iter() {
                    if guard_dynamic[child.index()] {
                        dsu.join(entity, offsets.guard(child));
                    }
                }
            }
        }
        cancelled(&mut work)?;
    }

    // A dynamic site belongs to the component/contact whose state it samples.
    // Constant profiles remain shared external facts and never join unrelated devices.
    for (index, site) in graph.sites.iter().enumerate() {
        let site_id = SiteId::new(index as u32);
        match site.source {
            SiteSource::Component(node) => dsu.join(offsets.site(site_id), offsets.node(node)),
            SiteSource::Contact(contact) => dsu.join(offsets.site(site_id), offsets.contact(contact)),
            SiteSource::Constant(_) => {}
        }
        cancelled(&mut work)?;
    }

    // Movable contact variants and their guards are runtime state dependencies.
    for (index, contact) in graph.contacts.iter().enumerate() {
        let contact_id = ContactId::new(index as u32);
        dsu.join(offsets.contact(contact_id), offsets.node(contact.node));
        for variant in contact.variants.iter() {
            if let Some(receiver) = variant.receiver {
                join_receiver(&mut dsu, offsets, offsets.contact(contact_id), receiver);
            }
            join_guard(graph, &mut dsu, offsets, &guard_dynamic, offsets.contact(contact_id), variant.when)?;
        }
        cancelled(&mut work)?;
    }

    // Every payload's complete contact set and every possible lane owner share state.
    let mut resources_by_contact = vec![Vec::<ResourceId>::new(); graph.contacts.len()];
    for (index, resource) in graph.resources.iter().enumerate() {
        let resource_id = ResourceId::new(index as u32);
        for &contact in resource.contacts.iter() {
            dsu.join(offsets.resource(resource_id), offsets.contact(contact));
            resources_by_contact[contact.index()].push(resource_id);
        }
        cancelled(&mut work)?;
    }
    for (index, piston) in graph.pistons.iter().enumerate() {
        let actor = PistonId::new(index as u32);
        let entity = offsets.piston(actor);
        dsu.join(entity, offsets.node(piston.node));
        for contact in [piston.base, piston.near, piston.far] {
            dsu.join(entity, offsets.contact(contact));
        }
        if let PistonLane::SingleResource { far } = piston.lane {
            for &resource in &resources_by_contact[far.index()] {
                dsu.join(entity, offsets.resource(resource));
            }
        }
        for route in [
            Some(piston.routes.base_changed),
            Some(piston.routes.near_changed),
            piston.routes.far_changed,
            piston.routes.far_shape_changed,
            Some(piston.routes.base_restored),
            Some(piston.routes.head_restored),
            piston.routes.resource_restored_near,
            piston.routes.resource_restored_far,
        ]
        .into_iter()
        .flatten()
        {
            dsu.join(entity, offsets.route(route));
        }
        cancelled(&mut work)?;
    }

    let live_conductors: FxHashSet<_> = graph
        .power_links
        .iter()
        .filter_map(|link| match link.to {
            PowerTarget::Conductor { site, .. } if !matches!(graph.sites[link.from.index()].source, SiteSource::Constant(_)) => Some(site),
            _ => None,
        })
        .collect();

    // Power edges include direct component reads and conducted sites. A live
    // conduit is connected through its Conductor target; literal source sites
    // do not couple otherwise independent readers.
    for link in &graph.power_links {
        let source = offsets.site(link.from);
        match graph.sites.get(link.from.index()).map(|site| site.source) {
            Some(SiteSource::Component(node)) => dsu.join(source, offsets.node(node)),
            Some(SiteSource::Contact(contact)) => dsu.join(source, offsets.contact(contact)),
            _ => {}
        }
        match link.to {
            PowerTarget::Input { node, .. } => {
                let source_is_dynamic = !matches!(graph.sites[link.from.index()].source, SiteSource::Constant(_));
                let source_is_conducted = matches!(link.from_port, PowerPort::Conducted { .. }) || live_conductors.contains(&link.from);
                if source_is_dynamic || source_is_conducted {
                    dsu.join(source, offsets.node(node));
                }
                join_guard(graph, &mut dsu, offsets, &guard_dynamic, offsets.node(node), link.guard)?;
            }
            PowerTarget::Conductor { site, .. } => {
                let live_conduit = matches!(graph.sites[link.from.index()].source, SiteSource::Component(_) | SiteSource::Contact(_))
                    || live_conductors.contains(&link.from)
                    || live_conductors.contains(&site);
                if live_conduit {
                    dsu.join(source, offsets.site(site));
                }
                join_guard(graph, &mut dsu, offsets, &guard_dynamic, offsets.site(site), link.guard)?;
            }
        }
        cancelled(&mut work)?;
    }

    // Callback sources and semantic receivers may exchange mutable state or
    // scheduled work. A notification of a literal inert site with no receiver
    // is deliberately not an interaction edge.
    for (index, route) in graph.routes.iter().enumerate() {
        let route_id = RouteId::new(index as u32);
        let source = offsets.node(route.source);
        dsu.join(source, offsets.route(route_id));
        for delivery in route.deliveries.iter() {
            if let Some(receiver) = delivery.receiver {
                join_receiver(&mut dsu, offsets, offsets.route(route_id), receiver);
            }
            if let Some(site) = graph.sites.get(delivery.target.index()) {
                match site.source {
                    SiteSource::Component(_) | SiteSource::Contact(_) => dsu.join(offsets.route(route_id), offsets.site(delivery.target)),
                    SiteSource::Constant(_) => {}
                }
            }
            join_guard(graph, &mut dsu, offsets, &guard_dynamic, offsets.route(route_id), delivery.guard)?;
        }
        cancelled(&mut work)?;
    }

    // Callback links are redundant with routes for execution but preserve
    // source/receiver relationships for graph variants with empty routes.
    for link in &graph.links {
        dsu.join(offsets.node(link.from), offsets.node(link.to));
        let guard = match link.kind {
            LinkKind::Update { guard, .. } | LinkKind::Shape { guard, .. } => guard,
        };
        join_guard(graph, &mut dsu, offsets, &guard_dynamic, offsets.node(link.from), guard)?;
        cancelled(&mut work)?;
    }

    // Geometry-driven wire reads and callback capture can cross a direct power
    // edge, so wire plans and relations join their live site owners as well.
    for wire in &graph.wires {
        let wire_entity = offsets.node(wire.node);
        for &site_id in &wire.canonical_neighbors {
            match graph.sites.get(site_id.index()).map(|site| site.source) {
                Some(SiteSource::Component(_) | SiteSource::Contact(_)) => dsu.join(wire_entity, offsets.site(site_id)),
                _ => {}
            }
        }
        for route in [wire.shape_route, wire.turbo_route].into_iter().flatten() {
            dsu.join(wire_entity, offsets.route(route));
        }
        cancelled(&mut work)?;
    }
    for relation in &graph.wire_relations {
        if matches!(
            graph.sites.get(relation.to.index()).map(|site| site.source),
            Some(SiteSource::Component(_) | SiteSource::Contact(_))
        ) || live_conductors.contains(&relation.to)
        {
            dsu.join(offsets.node(relation.from), offsets.site(relation.to));
        }
        join_guard(graph, &mut dsu, offsets, &guard_dynamic, offsets.node(relation.from), relation.guard)?;
        cancelled(&mut work)?;
    }

    // Observers, comparator overrides and note-block fixed reads are semantic
    // dependencies even when no power link directly names the observed site.
    let site_by_position: FxHashMap<_, _> = graph
        .sites
        .iter()
        .enumerate()
        .map(|(index, site)| (site.position, SiteId::new(index as u32)))
        .collect();
    let node_by_position: FxHashMap<_, _> = graph
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| node.position.map(|position| (position, NodeId::new(index as u32))))
        .collect();
    for (index, node) in graph.nodes.iter().enumerate() {
        let node_id = NodeId::new(index as u32);
        match node.kind {
            CircuitNodeKind::Observer { watched, .. } | CircuitNodeKind::BuiltinReset { watched, .. } => {
                dsu.join(offsets.node(node_id), offsets.site(watched));
            }
            CircuitNodeKind::Piston(actor) => dsu.join(offsets.node(node_id), offsets.piston(actor)),
            CircuitNodeKind::Contact(contact) => dsu.join(offsets.node(node_id), offsets.contact(contact)),
            CircuitNodeKind::Electrical(NodeType::NoteBlock { .. }) => {
                if let Some(position) = node.position {
                    for face in [BlockFace::Bottom, BlockFace::Top] {
                        if let Some(&site) = site_by_position.get(&position.offset(face)) {
                            if matches!(graph.sites[site.index()].source, SiteSource::Component(_) | SiteSource::Contact(_)) {
                                dsu.join(offsets.node(node_id), offsets.site(site));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        if let CircuitNodeKind::Electrical(NodeType::CommandBlock { chain, .. }) = node.kind {
            if let (Some(position), Some(raw_state)) = (node.position, node.raw_state) {
                let block = Block::from_id(raw_state);
                let facing = crate::redpiler::program_graph::command_facing(block);
                let neighbor_position = if chain {
                    position.offset(facing.opposite())
                } else {
                    position.offset(facing)
                };
                if let Some(&neighbor_id) = node_by_position.get(&neighbor_position) {
                    let neighbor = &graph.nodes[neighbor_id.index()];
                    let include = if chain {
                        matches!(neighbor.kind, CircuitNodeKind::Electrical(NodeType::CommandBlock { .. }))
                    } else {
                        matches!(neighbor.kind, CircuitNodeKind::Electrical(NodeType::CommandBlock { chain: true, .. }))
                    };
                    if include {
                        dsu.join(offsets.node(node_id), offsets.node(neighbor_id));
                    }
                }
            }
        }
        cancelled(&mut work)?;
    }
    for comparator in &graph.comparator_inputs {
        let node = offsets.node(comparator.node);
        join_site_dependency(graph, &mut dsu, offsets, node, comparator.rear);
        if let Some(override_) = comparator.direct_override {
            if let ComparatorOverride::LiveComponentOutput(source) = override_ {
                dsu.join(node, offsets.node(source));
            }
        }
        if let Some((site, override_, guard)) = comparator.far_override {
            join_site_dependency(graph, &mut dsu, offsets, node, site);
            if let ComparatorOverride::LiveComponentOutput(source) = override_ {
                dsu.join(node, offsets.node(source));
            }
            join_guard(graph, &mut dsu, offsets, &guard_dynamic, node, guard)?;
        }
        cancelled(&mut work)?;
    }

    build_partition(graph, offsets, dsu, monitor)
}

fn join_site_dependency(graph: &CompileGraph, dsu: &mut Dsu, offsets: EntityOffsets, owner: usize, site: SiteId) {
    if matches!(
        graph.sites.get(site.index()).map(|site| site.source),
        Some(SiteSource::Component(_) | SiteSource::Contact(_))
    ) {
        dsu.join(owner, offsets.site(site));
    }
}

fn join_receiver(dsu: &mut Dsu, offsets: EntityOffsets, owner: usize, receiver: CallbackReceiver) {
    match receiver {
        CallbackReceiver::Component(node) => dsu.join(owner, offsets.node(node)),
        CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor) => dsu.join(owner, offsets.piston(actor)),
        CallbackReceiver::Contact(contact) => dsu.join(owner, offsets.contact(contact)),
    }
}

fn dynamic_guard_support(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<Vec<bool>, String> {
    let mut dynamic = vec![false; graph.guards.len()];
    let mut visited = vec![false; graph.guards.len()];
    let mut work = 0usize;
    for start in 0..graph.guards.len() {
        if visited[start] {
            continue;
        }
        let mut stack = vec![(GuardId::new(start as u32), false)];
        while let Some((id, expanded)) = stack.pop() {
            if id.index() >= graph.guards.len() {
                return Err("logical island guard reference is out of range".into());
            }
            if expanded {
                dynamic[id.index()] = match &graph.guards[id.index()] {
                    Guard::Always => false,
                    Guard::State(_) => true,
                    Guard::Site { site, .. } => matches!(
                        graph.sites.get(site.index()).map(|site| site.source),
                        Some(SiteSource::Component(_) | SiteSource::Contact(_))
                    ),
                    Guard::Not(child) => dynamic[child.index()],
                    Guard::All(children) | Guard::Any(children) => children.iter().any(|child| dynamic[child.index()]),
                };
                work += 1;
                if work & 0x3fff == 0 && monitor.cancelled() {
                    return Err("cancelled while indexing logical guard dependencies".into());
                }
                continue;
            }
            if visited[id.index()] {
                continue;
            }
            visited[id.index()] = true;
            stack.push((id, true));
            let children = match &graph.guards[id.index()] {
                Guard::Not(child) => std::slice::from_ref(child),
                Guard::All(children) | Guard::Any(children) => children,
                Guard::Always | Guard::State(_) | Guard::Site { .. } => &[],
            };
            for &child in children.iter().rev() {
                if child.index() >= graph.guards.len() {
                    return Err("logical island guard reference is out of range".into());
                }
                if !visited[child.index()] {
                    stack.push((child, false));
                }
            }
        }
    }
    Ok(dynamic)
}

fn join_guard(_graph: &CompileGraph, dsu: &mut Dsu, offsets: EntityOffsets, dynamic: &[bool], owner: usize, root: GuardId) -> Result<(), String> {
    if root.index() >= offsets.end.saturating_sub(offsets.guards) {
        return Err("logical island guard reference is out of range".into());
    }
    if dynamic.get(root.index()).copied().unwrap_or(false) {
        dsu.join(owner, offsets.guard(root));
    }
    Ok(())
}

fn build_partition(graph: &CompileGraph, offsets: EntityOffsets, mut dsu: Dsu, monitor: &TaskMonitor) -> Result<LogicalIslandPartition, String> {
    let mut roots = FxHashMap::<usize, NodeId>::default();
    for index in 0..graph.nodes.len() {
        if index & 0x3fff == 0 && monitor.cancelled() {
            return Err("cancelled while indexing logical-device islands".into());
        }
        let root = dsu.root(index);
        let node = NodeId::new(index as u32);
        roots.entry(root).and_modify(|first| *first = (*first).min(node)).or_insert(node);
    }
    let mut ordered_roots = roots.into_iter().collect::<Vec<_>>();
    ordered_roots.sort_unstable_by_key(|(_, first_node)| *first_node);
    let root_ids: FxHashMap<_, _> = ordered_roots
        .iter()
        .enumerate()
        .map(|(id, (root, _))| u32::try_from(id).map(|id| (*root, id)).map_err(|_| "too many logical-device islands"))
        .collect::<Result<_, _>>()?;
    struct IslandBuilder {
        id: u32,
        nodes: Vec<NodeId>,
        sites: Vec<SiteId>,
        contacts: Vec<ContactId>,
        resources: Vec<ResourceId>,
        pistons: Vec<PistonId>,
        routes: Vec<RouteId>,
    }
    let mut islands: Vec<_> = ordered_roots
        .iter()
        .enumerate()
        .map(|(id, _)| IslandBuilder {
            id: id as u32,
            nodes: Vec::new(),
            sites: Vec::new(),
            contacts: Vec::new(),
            resources: Vec::new(),
            pistons: Vec::new(),
            routes: Vec::new(),
        })
        .collect();
    let mut node_island = Vec::with_capacity(graph.nodes.len());
    for index in 0..graph.nodes.len() {
        let id = root_ids[&dsu.root(index)];
        node_island.push(id);
        islands[id as usize].nodes.push(NodeId::new(index as u32));
    }
    for (index, _) in graph.sites.iter().enumerate() {
        if let Some(&id) = root_ids.get(&dsu.root(offsets.sites + index)) {
            islands[id as usize].sites.push(SiteId::new(index as u32));
        }
    }
    for (index, _) in graph.contacts.iter().enumerate() {
        if let Some(&id) = root_ids.get(&dsu.root(offsets.contacts + index)) {
            islands[id as usize].contacts.push(ContactId::new(index as u32));
        }
    }
    let mut resource_island = vec![None; graph.resources.len()];
    for (index, _) in graph.resources.iter().enumerate() {
        if let Some(&id) = root_ids.get(&dsu.root(offsets.resources + index)) {
            resource_island[index] = Some(id);
            islands[id as usize].resources.push(ResourceId::new(index as u32));
        }
    }
    let mut piston_island = Vec::with_capacity(graph.pistons.len());
    for (index, _) in graph.pistons.iter().enumerate() {
        let id = root_ids[&dsu.root(offsets.pistons + index)];
        piston_island.push(id);
        islands[id as usize].pistons.push(PistonId::new(index as u32));
    }
    for (index, _) in graph.routes.iter().enumerate() {
        let id = root_ids[&dsu.root(offsets.routes + index)];
        islands[id as usize].routes.push(RouteId::new(index as u32));
    }
    let islands = islands
        .into_iter()
        .map(|island| LogicalIsland {
            id: island.id,
            nodes: island.nodes.into_boxed_slice(),
            sites: island.sites.into_boxed_slice(),
            contacts: island.contacts.into_boxed_slice(),
            resources: island.resources.into_boxed_slice(),
            pistons: island.pistons.into_boxed_slice(),
            routes: island.routes.into_boxed_slice(),
        })
        .collect::<Vec<_>>();
    Ok(LogicalIslandPartition {
        islands: islands.into_boxed_slice(),
        node_island: node_island.into_boxed_slice(),
        resource_island: resource_island.into_boxed_slice(),
        piston_island: piston_island.into_boxed_slice(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::components::{NodeState, NodeType};
    use crate::redpiler::program_graph::{ComponentState, ContactSpec, ResourceSpec, RouteDelivery};
    use mchprs_blocks::blocks::Block;
    use mchprs_blocks::{BlockFace, BlockPos};

    fn component(position: i32) -> CircuitNodeDraft {
        CircuitNodeDraft {
            position: Some(BlockPos::new(position, 0, 0)),
            raw_state: None,
            kind: CircuitNodeKind::Electrical(NodeType::Lever),
            state: ComponentState::Electrical(NodeState::default()),
            output_route: None,
        }
    }

    fn graph(nodes: Vec<CircuitNodeDraft>, sites: Vec<SiteSpec>) -> CompileGraph {
        CompileGraph {
            nodes,
            links: Vec::new(),
            power_links: Vec::new(),
            comparator_inputs: Vec::new(),
            profiles: Vec::new(),
            guards: Vec::new(),
            contacts: Vec::new(),
            routes: Vec::new(),
            wires: Vec::new(),
            wire_relations: Vec::new(),
            sites,
            pistons: Vec::new(),
            resources: Vec::new(),
            initial_pistons: Vec::new(),
            initial_resources: Vec::new(),
            plans: Default::default(),
        }
    }

    fn constant_site(x: i32) -> SiteSpec {
        SiteSpec {
            position: BlockPos::new(x, 0, 0),
            source: SiteSource::Constant(ProfileId::new(0)),
            source_strength: 0,
        }
    }

    fn same_island(graph: &CompileGraph, left: usize, right: usize) -> bool {
        let monitor = TaskMonitor::default();
        monitor.set_budget_multiplier(1);
        let partition = partition(graph, &monitor).unwrap();
        partition.node_island[left] == partition.node_island[right]
    }

    fn power_link(from: SiteId, to: NodeId, guard: GuardId, from_port: PowerPort) -> PowerLink {
        PowerLink {
            from,
            from_port,
            to: PowerTarget::Input {
                node: to,
                port: InputPort::Main,
            },
            face: BlockFace::East,
            attenuation: 0,
            guard,
        }
    }

    #[test]
    fn shared_static_guard_and_constant_wire_relation_do_not_merge_islands() {
        let mut graph = graph(vec![component(0), component(1)], vec![constant_site(2)]);
        graph.guards.push(Guard::Site {
            site: SiteId::new(0),
            view: crate::redpiler::program_graph::OperandView::Live,
            test: crate::redpiler::program_graph::SiteTest::Solid,
        });
        graph.wire_relations.extend([0, 1].map(|node| WireRelation {
            from: NodeId::new(node),
            to: SiteId::new(0),
            kind: WireRelationKind::Step,
            direction: BlockFace::East,
            guard: GuardId::new(0),
        }));
        assert!(!same_island(&graph, 0, 1));
    }

    #[test]
    fn shared_dynamic_guard_merges_its_users() {
        let mut graph = graph(vec![component(0), component(1)], vec![constant_site(2)]);
        graph.resources.push(ResourceSpec {
            material: ProfileId::new(0),
            contacts: Box::new([]),
        });
        graph.guards.push(Guard::State(StateTest::ResourceEnabled(ResourceId::new(0))));
        graph.power_links = vec![
            power_link(SiteId::new(0), NodeId::new(0), GuardId::new(0), PowerPort::Output),
            power_link(SiteId::new(0), NodeId::new(1), GuardId::new(0), PowerPort::Output),
        ];
        assert!(same_island(&graph, 0, 1));
    }

    #[test]
    fn conducted_power_connects_source_and_downstream_reader() {
        let mut graph = graph(
            vec![component(0), component(1)],
            vec![
                SiteSpec {
                    position: BlockPos::new(2, 0, 0),
                    source: SiteSource::Component(NodeId::new(0)),
                    source_strength: 0,
                },
                constant_site(3),
            ],
        );
        graph.guards.push(Guard::Always);
        graph.power_links = vec![
            PowerLink {
                from: SiteId::new(0),
                from_port: PowerPort::Output,
                to: PowerTarget::Conductor {
                    site: SiteId::new(1),
                    include_dust: false,
                },
                face: BlockFace::East,
                attenuation: 0,
                guard: GuardId::new(0),
            },
            power_link(
                SiteId::new(1),
                NodeId::new(1),
                GuardId::new(0),
                PowerPort::Conducted { include_dust: false },
            ),
        ];
        assert!(same_island(&graph, 0, 1));
    }

    #[test]
    fn shared_resource_contacts_join_both_state_owners() {
        let mut graph = graph(
            vec![
                component(0),
                component(1),
                CircuitNodeDraft {
                    position: Some(BlockPos::new(2, 0, 0)),
                    raw_state: None,
                    kind: CircuitNodeKind::Contact(ContactId::new(0)),
                    state: ComponentState::Specialized,
                    output_route: None,
                },
                CircuitNodeDraft {
                    position: Some(BlockPos::new(3, 0, 0)),
                    raw_state: None,
                    kind: CircuitNodeKind::Contact(ContactId::new(1)),
                    state: ComponentState::Specialized,
                    output_route: None,
                },
            ],
            vec![
                SiteSpec {
                    position: BlockPos::new(4, 0, 0),
                    source: SiteSource::Contact(ContactId::new(0)),
                    source_strength: 0,
                },
                SiteSpec {
                    position: BlockPos::new(5, 0, 0),
                    source: SiteSource::Contact(ContactId::new(1)),
                    source_strength: 0,
                },
            ],
        );
        graph.contacts = vec![
            ContactSpec {
                node: NodeId::new(2),
                position: BlockPos::new(2, 0, 0),
                variants: Box::new([]),
            },
            ContactSpec {
                node: NodeId::new(3),
                position: BlockPos::new(3, 0, 0),
                variants: Box::new([]),
            },
        ];
        graph.resources.push(ResourceSpec {
            material: ProfileId::new(0),
            contacts: vec![ContactId::new(0), ContactId::new(1)].into_boxed_slice(),
        });
        // Bind each component to one resource contact through callback routes.
        graph.guards.push(Guard::Always);
        graph.routes = vec![
            Route {
                source: NodeId::new(0),
                deliveries: vec![RouteDelivery {
                    target: SiteId::new(0),
                    kind: DeliveryKind::Update { direction: None },
                    guard: GuardId::new(0),
                    view: crate::redpiler::program_graph::OperandView::Live,
                    sequence: 0,
                    receiver: Some(CallbackReceiver::Contact(ContactId::new(0))),
                }]
                .into_boxed_slice(),
            },
            Route {
                source: NodeId::new(1),
                deliveries: vec![RouteDelivery {
                    target: SiteId::new(1),
                    kind: DeliveryKind::Update { direction: None },
                    guard: GuardId::new(0),
                    view: crate::redpiler::program_graph::OperandView::Live,
                    sequence: 0,
                    receiver: Some(CallbackReceiver::Contact(ContactId::new(1))),
                }]
                .into_boxed_slice(),
            },
        ];
        assert!(same_island(&graph, 0, 1));
    }

    #[test]
    fn semantic_callback_and_observer_dependencies_join_their_owners() {
        let mut callback_graph = graph(vec![component(0), component(1)], vec![constant_site(2)]);
        callback_graph.guards.push(Guard::Always);
        callback_graph.routes.push(Route {
            source: NodeId::new(0),
            deliveries: vec![RouteDelivery {
                target: SiteId::new(0),
                kind: DeliveryKind::Update { direction: None },
                guard: GuardId::new(0),
                view: crate::redpiler::program_graph::OperandView::Live,
                sequence: 0,
                receiver: Some(CallbackReceiver::Component(NodeId::new(1))),
            }]
            .into_boxed_slice(),
        });
        assert!(same_island(&callback_graph, 0, 1));

        let observer_graph = graph(
            vec![
                component(0),
                CircuitNodeDraft {
                    position: Some(BlockPos::new(1, 0, 0)),
                    raw_state: None,
                    kind: CircuitNodeKind::Observer {
                        watched: SiteId::new(0),
                        facing: BlockFace::East,
                    },
                    state: ComponentState::Specialized,
                    output_route: None,
                },
            ],
            vec![SiteSpec {
                position: BlockPos::new(2, 0, 0),
                source: SiteSource::Component(NodeId::new(0)),
                source_strength: 0,
            }],
        );
        assert!(same_island(&observer_graph, 0, 1));
    }

    #[test]
    fn command_chain_neighbors_are_in_the_same_island() {
        let find_state = |name: &str, facing: &str| {
            (0..30_000)
                .find(|&raw| {
                    let block = Block::from_id(raw);
                    block.get_name() == name && block.property("facing") == Some(facing)
                })
                .unwrap_or_else(|| panic!("missing block state {name} facing {facing}"))
        };
        let normal = find_state("command_block", "east");
        let chain = find_state("chain_command_block", "east");
        let graph = graph(
            vec![
                CircuitNodeDraft {
                    position: Some(BlockPos::new(0, 0, 0)),
                    raw_state: Some(normal),
                    kind: CircuitNodeKind::Electrical(NodeType::CommandBlock {
                        repeating: false,
                        chain: false,
                        automatic: false,
                        initial_tick: false,
                    }),
                    state: ComponentState::Electrical(NodeState::default()),
                    output_route: None,
                },
                CircuitNodeDraft {
                    position: Some(BlockPos::new(1, 0, 0)),
                    raw_state: Some(chain),
                    kind: CircuitNodeKind::Electrical(NodeType::CommandBlock {
                        repeating: false,
                        chain: true,
                        automatic: false,
                        initial_tick: false,
                    }),
                    state: ComponentState::Electrical(NodeState::default()),
                    output_route: None,
                },
            ],
            Vec::new(),
        );
        assert!(same_island(&graph, 0, 1));
    }
}
