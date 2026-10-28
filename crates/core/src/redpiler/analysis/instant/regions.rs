//! Deterministic connected regions over cell/corridor owners and original port witnesses.
use crate::redpiler::TaskMonitor;
use crate::redpiler::analysis::instant::boundaries::{OwnerMap, bind_owners};
use crate::redpiler::program_graph::*;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;

mod fan;

/// Rescan with merged region owners so internal cell/corridor witnesses are not kernel outputs.
pub(crate) fn perimeters(
    graph: &CompileGraph,
    partition: &RegionPartition,
    monitor: &TaskMonitor,
) -> Result<Vec<FxHashSet<(BoundaryDirection, BoundaryPortKind)>>, String> {
    let mut owners = partition.owners.clone();
    for table in [
        &mut owners.actors,
        &mut owners.resources,
        &mut owners.contacts,
        &mut owners.nodes,
        &mut owners.sites,
    ] {
        for owner in table {
            if let Some(local) = owner {
                *local = partition.owner_region[*local] as usize;
            }
        }
    }
    let scan = super::boundaries::scan_boundaries(graph, &owners, monitor)?;
    let mut perimeters = vec![FxHashSet::default(); partition.regions.len()];
    for port in scan.ports {
        perimeters[port.owner].insert((port.direction, port.kind));
    }
    Ok(perimeters)
}

const MAX_REGION_WORK: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegionComponent {
    /// These admitted local cell contracts belong to this connected instant region.
    pub(crate) cells: Box<[u32]>,
    /// These connected wire corridors join cell dependencies but never own piston state.
    pub(crate) corridors: Box<[u32]>,
    /// These original catalog row indices preserve every internal and perimeter witness.
    pub(crate) ports: Box<[u32]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegionPartition {
    /// These components are ordered by the smallest original cell/corridor owner ID.
    pub(crate) regions: Box<[RegionComponent]>,
    /// These owner indices map to the stable region ID assigned by the partition.
    pub(crate) owner_region: Box<[u32]>,
    /// This is the same source binding used by boundary discovery, extended with corridor owners.
    pub(crate) owners: OwnerMap,
}

/// Bind source graph entities to catalog owners and connect only paired owned dependencies.
pub(crate) fn partition(graph: &CompileGraph, catalog: &BoundaryCatalog, monitor: &TaskMonitor) -> Result<RegionPartition, String> {
    let owner_count = catalog.cells.len() + catalog.corridors.len();
    partition_with_cuts(graph, catalog, monitor, &vec![true; owner_count], 0, |_| true)
}

/// A cut changes composition ownership, never the retained read/effect evidence.
pub(crate) fn partition_with_cuts(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    monitor: &TaskMonitor,
    joinable: &[bool],
    retained_bytes: usize,
    couples: impl Fn(BoundaryPortKind) -> bool,
) -> Result<RegionPartition, String> {
    partition_with_connector(graph, catalog, monitor, joinable, retained_bytes, |sets, budget| {
        connect_selected_ports(sets, catalog.cells.len(), &catalog.ports, budget, joinable, couples)
    })
}

/// Partition only the owner pairs reached by the event worklist. Reads and
/// potential edges not present in the witness stay on the boundary.
#[cfg(test)]
pub(crate) fn partition_with_event_edges(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    monitor: &TaskMonitor,
    joinable: &[bool],
    retained_bytes: usize,
    edges: &[EventEdge],
) -> Result<RegionPartition, String> {
    partition_with_connector(graph, catalog, monitor, joinable, retained_bytes, |sets, budget| {
        for edge in edges {
            budget.visit()?;
            if !joinable.get(edge.source).copied().unwrap_or(false) || !joinable.get(edge.target).copied().unwrap_or(false) {
                return Err("instant event edge references an unselected owner".into());
            }
            sets.join(edge.source, edge.target);
        }
        Ok(())
    })
}

fn partition_with_connector(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    monitor: &TaskMonitor,
    joinable: &[bool],
    retained_bytes: usize,
    connect: impl FnOnce(&mut DisjointSets, &mut WorkBudget<'_>) -> Result<(), String>,
) -> Result<RegionPartition, String> {
    let mut budget = WorkBudget::new(monitor);
    budget.retain_bytes(retained_bytes)?;
    let owners = build_owner_map(graph, catalog, &mut budget)?;
    let owner_count = catalog
        .cells
        .len()
        .checked_add(catalog.corridors.len())
        .ok_or("instant-region owner count overflow")?;
    if joinable.len() != owner_count {
        return Err("instant-region cut table has stale owner dimensions".into());
    }
    let mut sets = DisjointSets::new(owner_count, &mut budget)?;
    connect(&mut sets, &mut budget)?;

    budget.retain_bytes(
        owner_count
            .checked_mul(std::mem::size_of::<usize>())
            .ok_or("instant-region owner map size overflow")?,
    )?;
    let mut groups = FxHashMap::<usize, RegionBuilder>::default();
    let mut owner_roots = Vec::with_capacity(owner_count);
    for owner in 0..owner_count {
        budget.visit()?;
        let root = sets.root(owner);
        owner_roots.push(root);
        let group = groups.entry(root).or_default();
        if owner < catalog.cells.len() {
            append_index(&mut group.cells, owner, &mut budget)?;
        } else {
            append_index(&mut group.corridors, owner - catalog.cells.len(), &mut budget)?;
        }
    }

    let mut region_roots: Vec<_> = groups.keys().copied().collect();
    // Union always keeps the minimum original owner as the component root.
    region_roots.sort_unstable();
    budget.retain_bytes(
        owner_count
            .checked_mul(std::mem::size_of::<u32>())
            .ok_or("instant-region ID map size overflow")?,
    )?;
    let mut root_to_region = vec![0u32; owner_count];
    for (region, &root) in region_roots.iter().enumerate() {
        budget.visit()?;
        let region = u32::try_from(region).map_err(|_| "too many instant regions")?;
        root_to_region[root] = region;
    }
    budget.retain_bytes(
        owner_count
            .checked_mul(std::mem::size_of::<u32>())
            .ok_or("instant-region owner map size overflow")?,
    )?;
    let owner_region: Vec<_> = owner_roots.iter().map(|&root| root_to_region[root]).collect();

    let mut regions: Vec<_> = region_roots.iter().map(|root| groups.remove(root).expect("known owner root")).collect();
    for (ordinal, port) in catalog.ports.iter().enumerate() {
        budget.visit()?;
        let owner = owner_index(port.owner, catalog.cells.len())?;
        let region = owner_region[owner] as usize;
        append_index(&mut regions[region].ports, ordinal, &mut budget)?;
    }
    budget.check_cancelled()?;

    validate_owner_map(&owners, graph, catalog, &mut budget)?;
    Ok(RegionPartition {
        regions: regions.into_iter().map(RegionBuilder::finish).collect::<Vec<_>>().into_boxed_slice(),
        owner_region: owner_region.into_boxed_slice(),
        owners,
    })
}

/// Pair typed source witnesses once, retaining outside fanout as separate cell components.
#[cfg(test)]
fn connect_ports(sets: &mut DisjointSets, cell_count: usize, ports: &[BoundaryPort], budget: &mut WorkBudget<'_>) -> Result<(), String> {
    let joinable = vec![true; sets.parents.len()];
    connect_selected_ports(sets, cell_count, ports, budget, &joinable, |_| true)
}

fn connect_selected_ports(
    sets: &mut DisjointSets,
    cell_count: usize,
    ports: &[BoundaryPort],
    budget: &mut WorkBudget<'_>,
    joinable: &[bool],
    couples: impl Fn(BoundaryPortKind) -> bool,
) -> Result<(), String> {
    // Only opposite-side representatives are needed for union. Retaining all
    // fan rows here needlessly duplicates a large portion of the catalog.
    let mut representatives = FxHashMap::<BoundaryPortKind, (Option<usize>, Option<usize>)>::default();
    for port in ports {
        budget.visit()?;
        let owner = owner_index(port.owner, cell_count)?;
        if owner >= sets.parents.len() {
            return Err("instant boundary port references a missing owner".into());
        }
        if !joinable[owner] || !couples(port.kind) {
            continue;
        }
        if !representatives.contains_key(&port.kind) {
            budget.retain_bytes(2 * std::mem::size_of::<(BoundaryPortKind, (Option<usize>, Option<usize>))>())?;
        }
        let endpoints = representatives.entry(port.kind).or_default();
        match port.direction {
            BoundaryDirection::Input => {
                endpoints.0.get_or_insert(owner);
            }
            BoundaryDirection::Output => {
                endpoints.1.get_or_insert(owner);
            }
        }
    }
    for port in ports {
        budget.visit()?;
        let owner = owner_index(port.owner, cell_count)?;
        if !joinable[owner] || !couples(port.kind) {
            continue;
        }
        let endpoints = representatives[&port.kind];
        let opposite = match port.direction {
            BoundaryDirection::Input => endpoints.1,
            BoundaryDirection::Output => endpoints.0,
        };
        if let Some(opposite) = opposite {
            sets.join(owner, opposite);
        }
    }
    Ok(())
}

mod greedy;
pub(crate) use greedy::greedy_event_owners;
#[cfg(test)]
pub(crate) use greedy::{EventEdge, greedy_event_pairs};
#[cfg(test)]
use greedy::{EventIndex, EventPhase};

#[cfg(test)]
mod tests;

#[derive(Default)]
struct RegionBuilder {
    /// These cell indices are accumulated once before becoming the immutable region table.
    cells: Vec<u32>,
    /// These corridor indices are accumulated once before becoming the immutable region table.
    corridors: Vec<u32>,
    /// These original boundary rows are accumulated once before becoming the immutable region table.
    ports: Vec<u32>,
}

impl RegionBuilder {
    fn finish(self) -> RegionComponent {
        RegionComponent {
            cells: self.cells.into_boxed_slice(),
            corridors: self.corridors.into_boxed_slice(),
            ports: self.ports.into_boxed_slice(),
        }
    }
}

/// Extend the shared Step03 source binding instead of rebuilding graph entity ownership here.
fn build_owner_map(graph: &CompileGraph, catalog: &BoundaryCatalog, budget: &mut WorkBudget<'_>) -> Result<OwnerMap, String> {
    let cell_count = catalog.cells.len();
    let index_slots = graph
        .pistons
        .len()
        .checked_add(graph.resources.len())
        .and_then(|count| count.checked_add(graph.contacts.len()))
        .and_then(|count| count.checked_add(graph.nodes.len()))
        .and_then(|count| count.checked_add(graph.sites.len()))
        .ok_or("instant-region ownership table size overflow")?;
    budget.retain_bytes(
        index_slots
            .checked_mul(std::mem::size_of::<Option<usize>>())
            .ok_or("instant-region ownership byte size overflow")?,
    )?;
    let mut actors = vec![None; graph.pistons.len()];
    let mut resources = vec![None; graph.resources.len()];
    let mut contacts = vec![None; graph.contacts.len()];
    let reset_count = catalog.cells.iter().map(|cell| cell.uses.len()).sum::<usize>();
    budget.retain_bytes(
        reset_count
            .checked_mul(std::mem::size_of::<(NodeId, usize)>())
            .ok_or("instant-region reset index size overflow")?,
    )?;
    let mut resets = Vec::with_capacity(reset_count);
    for (cell, contract) in catalog.cells.iter().enumerate() {
        budget.visit()?;
        for use_ in &contract.uses {
            budget.visit()?;
            assign(&mut actors, use_.actor.index(), cell)?;
            if let Some(reset) = use_.reset.observer() {
                resets.push((reset, cell));
            }
            let actor = *graph.pistons.get(use_.actor.index()).ok_or("cell actor is outside the source graph")?;
            for contact in [
                actor.base,
                actor.near,
                match actor.lane {
                    PistonLane::SingleResource { far } => far,
                    PistonLane::Empty => actor.near,
                },
            ] {
                assign(&mut contacts, contact.index(), cell)?;
            }
        }
        for resource_pin in &contract.resources {
            budget.visit()?;
            assign(&mut resources, resource_pin.resource.index(), cell)?;
            let resource = graph
                .resources
                .get(resource_pin.resource.index())
                .ok_or("cell resource is outside the source graph")?;
            for &contact in &resource.contacts {
                assign(&mut contacts, contact.index(), cell)?;
            }
        }
    }
    let mut result = bind_owners(graph, actors, resources, contacts, &resets);
    for (corridor, spec) in catalog.corridors.iter().enumerate() {
        budget.visit()?;
        let owner = cell_count.checked_add(corridor).ok_or("instant-region owner index overflow")?;
        for &wire in &spec.wires {
            assign(&mut result.nodes, wire.index(), owner)?;
        }
        for &site in &spec.conductive_sites {
            assign(&mut result.sites, site.index(), owner)?;
        }
    }
    for (index, site) in graph.sites.iter().enumerate() {
        budget.visit()?;
        result.sites[index] = match site.source {
            SiteSource::Component(node) => result.nodes[node.index()],
            SiteSource::Contact(contact) => result.contacts[contact.index()],
            SiteSource::Constant(_) => result.sites[index],
        };
    }
    Ok(result)
}

fn validate_owner_map(owners: &OwnerMap, graph: &CompileGraph, catalog: &BoundaryCatalog, budget: &mut WorkBudget<'_>) -> Result<(), String> {
    if owners.actors.len() != graph.pistons.len()
        || owners.resources.len() != graph.resources.len()
        || owners.contacts.len() != graph.contacts.len()
        || owners.nodes.len() != graph.nodes.len()
        || owners.sites.len() != graph.sites.len()
    {
        return Err("instant-region source index has stale graph dimensions".into());
    }
    for port in &catalog.ports {
        budget.visit()?;
        let owner = owner_index(port.owner, catalog.cells.len())?;
        if owner >= catalog.cells.len() + catalog.corridors.len() {
            return Err("instant boundary port references a missing owner".into());
        }
    }
    Ok(())
}

fn assign(slots: &mut [Option<usize>], index: usize, owner: usize) -> Result<(), String> {
    let slot = slots.get_mut(index).ok_or("instant catalog source ID is outside the graph")?;
    if slot.is_some_and(|previous| previous != owner) {
        return Err("instant catalog gives one graph entity multiple owners".into());
    }
    *slot = Some(owner);
    Ok(())
}

fn owner_index(owner: BoundaryOwner, cell_count: usize) -> Result<usize, String> {
    match owner {
        BoundaryOwner::Cell(cell) if (cell as usize) < cell_count => Ok(cell as usize),
        BoundaryOwner::Corridor(corridor) => cell_count
            .checked_add(corridor as usize)
            .ok_or_else(|| "instant corridor owner index overflow".into()),
        BoundaryOwner::Cell(_) => Err("instant boundary port references a missing cell".into()),
    }
}

fn append_index(table: &mut Vec<u32>, index: usize, budget: &mut WorkBudget<'_>) -> Result<(), String> {
    let id = u32::try_from(index).map_err(|_| "instant region table has too many entries")?;
    budget.retain_bytes(std::mem::size_of::<u32>())?;
    table.push(id);
    Ok(())
}

struct WorkBudget<'a> {
    /// This shared task monitor cancels long catalog and graph traversals promptly.
    monitor: &'a TaskMonitor,
    /// This counts owner-index, port and disjoint-set visits.
    work: usize,
    /// This counts retained table payload before allocation.
    bytes: usize,
    max_work: usize,
    max_bytes: usize,
}

impl<'a> WorkBudget<'a> {
    fn new(monitor: &'a TaskMonitor) -> Self {
        let multiplier = monitor.budget_multiplier();
        Self {
            monitor,
            work: 0,
            bytes: 0,
            max_work: MAX_REGION_WORK.saturating_mul(multiplier),
            max_bytes: (crate::redpiler::analysis::MAX_GRAPH_PREPARATION_BYTES / 4).saturating_mul(multiplier),
        }
    }

    fn visit(&mut self) -> Result<(), String> {
        self.check_cancelled()?;
        self.work = self.work.checked_add(1).ok_or("instant-region work counter overflow")?;
        if self.work > self.max_work {
            return Err("instant-region work budget exceeded".into());
        }
        Ok(())
    }

    fn retain_bytes(&mut self, bytes: usize) -> Result<(), String> {
        self.check_cancelled()?;
        self.bytes = self.bytes.checked_add(bytes).ok_or("instant-region retained byte counter overflow")?;
        if self.bytes > self.max_bytes {
            return Err("instant-region retained-row budget exceeded".into());
        }
        Ok(())
    }

    fn check_cancelled(&self) -> Result<(), String> {
        if self.monitor.cancelled() {
            Err("cancelled while composing instant regions".into())
        } else {
            Ok(())
        }
    }
}

struct DisjointSets {
    /// These parent pointers choose a deterministic representative for every owner component.
    parents: Vec<usize>,
}

impl DisjointSets {
    fn new(count: usize, budget: &mut WorkBudget<'_>) -> Result<Self, String> {
        let bytes = count
            .checked_mul(std::mem::size_of::<usize>())
            .ok_or("instant-region owner index size overflow")?;
        budget.retain_bytes(bytes)?;
        let mut parents = Vec::with_capacity(count);
        for owner in 0..count {
            budget.visit()?;
            parents.push(owner);
        }
        Ok(Self { parents })
    }

    fn root(&mut self, mut owner: usize) -> usize {
        while self.parents[owner] != owner {
            self.parents[owner] = self.parents[self.parents[owner]];
            owner = self.parents[owner];
        }
        owner
    }

    fn join(&mut self, first: usize, second: usize) {
        let first = self.root(first);
        let second = self.root(second);
        self.parents[first.max(second)] = first.min(second);
    }
}
