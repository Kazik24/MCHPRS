//! Compose ownership groups through compiled power, notifications, and wire reads.
//! Inputs stay stable from an activating falling edge until reset; internal causal order remains.
use super::*;
use rustc_hash::FxHashMap;

struct Connections<'a> {
    /// This borrows the validated graph; composition never reads or changes the World.
    graph: &'a CompileGraph,
    /// These disjoint-set parents cover ownership groups, component ports, and conductors.
    parents: Vec<usize>,
    /// This maps eligible actor/contact/reset nodes to their original ownership group.
    node_groups: Vec<Option<usize>>,
    /// This maps eligible contacts to their original ownership group.
    contact_groups: Vec<Option<usize>>,
    /// This is the first component-port vertex after ownership-group vertices.
    nodes_start: usize,
    /// This is the first conductor vertex after component-port vertices.
    sites_start: usize,
}

impl Connections<'_> {
    fn internal_node(&self, node: NodeId) -> Option<usize> {
        self.node_groups[node.index()]
            .or_else(|| matches!(self.graph.nodes[node.index()].kind, CircuitNodeKind::Wire(_)).then_some(self.nodes_start + node.index()))
    }

    fn source_node(&self, node: NodeId) -> Option<usize> {
        // An outside component is a boundary port; its fanout must not join owners.
        self.internal_node(node)
    }

    fn site(&self, site: SiteId) -> Option<usize> {
        match self.graph.sites[site.index()].source {
            SiteSource::Component(node) => self.internal_node(node),
            SiteSource::Contact(contact) => self.contact_groups[contact.index()],
            SiteSource::Constant(_) => None,
        }
    }

    fn power_source(&self, link: &PowerLink) -> Option<usize> {
        if matches!(link.from_port, PowerPort::Conducted { .. }) {
            return Some(self.sites_start + link.from.index());
        }
        match self.graph.sites[link.from.index()].source {
            SiteSource::Component(node) => self.source_node(node),
            _ => self.site(link.from),
        }
    }

    fn receiver(&self, delivery: &RouteDelivery) -> Option<usize> {
        match delivery.receiver {
            Some(CallbackReceiver::Component(node)) => self.internal_node(node),
            Some(CallbackReceiver::Contact(contact)) => self.contact_groups[contact.index()],
            Some(CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor)) => {
                self.node_groups[self.graph.pistons[actor.index()].node.index()]
            }
            None => self.site(delivery.target),
        }
    }

    fn connect(&mut self, source: Option<usize>, target: Option<usize>) {
        if let (Some(source), Some(target)) = (source, target) {
            join(&mut self.parents, source, target);
        }
    }

    fn connect_dependencies(&mut self, monitor: &TaskMonitor) -> Result<(), String> {
        for link in &self.graph.power_links {
            check_cancelled(monitor)?;
            let target = match link.to {
                PowerTarget::Input { node, .. } => self.internal_node(node),
                PowerTarget::Conductor { site, .. } => Some(self.sites_start + site.index()),
            };
            self.connect(self.power_source(link), target);
        }
        for route in &self.graph.routes {
            check_cancelled(monitor)?;
            let source = self.source_node(route.source);
            for delivery in &route.deliveries {
                self.connect(source, self.receiver(delivery));
            }
        }
        for wire in &self.graph.wires {
            check_cancelled(monitor)?;
            for &site in &wire.canonical_neighbors {
                self.connect(self.internal_node(wire.node), self.site(site));
            }
        }
        Ok(())
    }
}

/// Build a composed boundary only from groups whose actors all have admitted reset mechanisms.
pub(crate) fn infer_stable_regions(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<Vec<InstantRegion>, String> {
    let ownership = infer_regions(graph, monitor)?;
    let mechanisms = discover_mechanisms(graph, monitor)?;
    let mut recognized = vec![false; graph.pistons.len()];
    for mechanism in &mechanisms {
        recognized[mechanism.piston.index()] = true;
    }
    let eligible: Vec<_> = ownership
        .iter()
        .map(|region| {
            region.actors.iter().all(|actor| recognized[actor.index()])
                && region
                    .resets
                    .iter()
                    .all(|reset| matches!(graph.nodes[reset.index()].kind, CircuitNodeKind::BuiltinReset { .. }))
        })
        .collect();
    if !eligible.iter().any(|&eligible| eligible) {
        check_cancelled(monitor)?;
        return Ok(ownership);
    }
    let nodes_start = ownership.len();
    let sites_start = nodes_start + graph.nodes.len();
    let mut connections = Connections {
        graph,
        parents: (0..sites_start + graph.sites.len()).collect(),
        node_groups: vec![None; graph.nodes.len()],
        contact_groups: vec![None; graph.contacts.len()],
        nodes_start,
        sites_start,
    };
    for (index, region) in ownership.iter().enumerate() {
        if !eligible[index] {
            continue;
        }
        for &actor in &region.actors {
            connections.node_groups[graph.pistons[actor.index()].node.index()] = Some(index);
        }
        for &contact in &region.contacts {
            connections.contact_groups[contact.index()] = Some(index);
            connections.node_groups[graph.contacts[contact.index()].node.index()] = Some(index);
        }
        for &reset in &region.resets {
            connections.node_groups[reset.index()] = Some(index);
        }
    }
    connections.connect_dependencies(monitor)?;
    assemble_regions(graph, &ownership, &eligible, &mechanisms, &mut connections, monitor)
}

fn assemble_regions(
    graph: &CompileGraph,
    ownership: &[InstantRegion],
    eligible: &[bool],
    mechanisms: &[InstantMechanism],
    connections: &mut Connections<'_>,
    monitor: &TaskMonitor,
) -> Result<Vec<InstantRegion>, String> {
    let mut groups = FxHashMap::default();
    let mut regions: Vec<InstantRegion> = Vec::new();
    let mut actors = vec![None; graph.pistons.len()];
    let mut contacts = vec![None; graph.contacts.len()];
    for (index, original) in ownership.iter().enumerate() {
        let group = root(&mut connections.parents, index);
        let owner = *groups.entry(group).or_insert_with(|| {
            let owner = regions.len();
            regions.push(InstantRegion {
                stable_inputs: eligible[index],
                ..Default::default()
            });
            owner
        });
        let region = &mut regions[owner];
        region.actors.extend_from_slice(&original.actors);
        region.resources.extend_from_slice(&original.resources);
        for &actor in &original.actors {
            actors[actor.index()] = Some(owner);
        }
        for &contact in &original.contacts {
            contacts[contact.index()] = Some(owner);
        }
    }
    for region in &mut regions {
        region.actors.sort_unstable();
        region.resources.sort_unstable();
    }
    let (nodes, sites) = bind_owned_sites(graph, mechanisms, &actors, &contacts, &mut regions);
    let mut owners = Ownership {
        actors,
        contacts,
        nodes,
        sites,
    };
    for wire in &graph.wires {
        let group = root(&mut connections.parents, connections.nodes_start + wire.node.index());
        if let Some(&owner) = groups.get(&group) {
            if regions[owner].stable_inputs {
                owners.nodes[wire.node.index()] = Some(owner);
                regions[owner].wires.push(wire.node);
            }
        }
    }
    for (index, site) in graph.sites.iter().enumerate() {
        if let SiteSource::Component(node) = site.source {
            owners.sites[index] = owners.nodes[node.index()];
        }
    }
    for link in &graph.power_links {
        if let PowerTarget::Conductor { site, .. } = link.to {
            let group = root(&mut connections.parents, connections.sites_start + site.index());
            if let Some(&owner) = groups.get(&group) {
                if regions[owner].stable_inputs && matches!(graph.sites[site.index()].source, SiteSource::Constant(_)) {
                    owners.sites[site.index()] = Some(owner);
                }
            }
        }
    }
    for region in &mut regions {
        region.readable_sites.clear();
    }
    for (index, &owner) in owners.sites.iter().enumerate() {
        if let Some(owner) = owner {
            regions[owner].readable_sites.push(SiteId::new(index as u32));
        }
    }
    collect_power_boundary(graph, &owners, &mut regions, monitor)?;
    collect_callback_boundary(graph, &owners, &mut regions, monitor)?;
    collect_geometry_readers(graph, &owners, &mut regions, monitor)?;
    for region in &mut regions {
        region.resets.sort_unstable();
    }
    check_cancelled(monitor)?;
    Ok(regions)
}
