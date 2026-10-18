//! Shared owner-relative scans for legacy regions and the read-only boundary catalog.
use super::cells;
use crate::redpiler::TaskMonitor;
use crate::redpiler::analysis::MAX_GRAPH_PREPARATION_BYTES;
use crate::redpiler::program_graph::*;

use rustc_hash::{FxHashMap, FxHashSet};

mod corridors;
mod scan;
pub(crate) use scan::{scan_boundaries, scan_legacy_boundaries};

const MAX_BOUNDARY_BYTES: usize = MAX_GRAPH_PREPARATION_BYTES / 4;
const MAX_BOUNDARY_WORK: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnerMap {
    /// These original piston IDs map to the one state owner assigned by the caller.
    pub(crate) actors: Vec<Option<usize>>,
    /// These original resource IDs map to their exclusive payload owner.
    pub(crate) resources: Vec<Option<usize>>,
    /// These possible occupied positions map to their actor/resource owner.
    pub(crate) contacts: Vec<Option<usize>>,
    /// These component, actor, reset, and corridor nodes map to one local owner.
    pub(crate) nodes: Vec<Option<usize>>,
    /// These live and captured query sites map to their source owner.
    pub(crate) sites: Vec<Option<usize>>,
}

pub(crate) struct BoundaryScan {
    /// These rows preserve each owner-relative read or effect using the original graph reference.
    pub(crate) ports: Vec<ScannedPort>,
    /// These routes originate in an owned node, even when no delivery crosses an owner boundary.
    pub(crate) owned_routes: Vec<(usize, RouteId)>,
}

#[derive(Clone, Copy)]
pub(crate) struct ScannedPort {
    /// This is an internal owner index resolved by the caller into a region, cell, or corridor.
    pub(crate) owner: usize,
    /// This separates owner reads from externally visible effects.
    pub(crate) direction: BoundaryDirection,
    /// This references an original graph row or typed source read.
    pub(crate) kind: BoundaryPortKind,
}

struct Budget<'a> {
    /// This cancellation source is shared with compilation preparation.
    monitor: &'a TaskMonitor,
    /// This counts scanned rows and guard-graph visits.
    work: usize,
    /// This approximates retained catalog and temporary index payload bytes.
    bytes: usize,
    /// This caps catalog scan work, while `None` keeps legacy scans unbounded.
    max_work: Option<usize>,
    /// This caps retained catalog payload, while `None` keeps legacy scans unbounded.
    max_bytes: Option<usize>,
}

impl Budget<'_> {
    fn bounded(monitor: &TaskMonitor) -> Budget<'_> {
        Budget {
            monitor,
            work: 0,
            bytes: 0,
            max_work: Some(MAX_BOUNDARY_WORK),
            max_bytes: Some(MAX_BOUNDARY_BYTES),
        }
    }

    fn unbounded(monitor: &TaskMonitor) -> Budget<'_> {
        Budget {
            monitor,
            work: 0,
            bytes: 0,
            max_work: None,
            max_bytes: None,
        }
    }

    fn visit(&mut self) -> Result<(), String> {
        self.check_cancelled()?;
        if let Some(max_work) = self.max_work {
            self.work = self.work.checked_add(1).ok_or("instant-boundary work budget overflow")?;
            if self.work > max_work {
                return Err("instant-boundary work budget exceeded".into());
            }
        }
        Ok(())
    }

    fn retain<T>(&mut self, _: &T) -> Result<(), String> {
        self.retain_bytes(std::mem::size_of::<T>())
    }

    fn retain_bytes(&mut self, bytes: usize) -> Result<(), String> {
        self.check_cancelled()?;
        if let Some(max_bytes) = self.max_bytes {
            self.bytes = self.bytes.checked_add(bytes).ok_or("instant-boundary byte budget overflow")?;
            if self.bytes > max_bytes {
                return Err("instant-boundary retained-table budget exceeded".into());
            }
        }
        Ok(())
    }

    fn check_cancelled(&self) -> Result<(), String> {
        if self.monitor.cancelled() {
            Err("cancelled while identifying instant boundaries".into())
        } else {
            Ok(())
        }
    }
}

/// Bind original components and sites from caller-provided exclusive owners.
pub(crate) fn bind_owners(
    graph: &CompileGraph,
    actors: Vec<Option<usize>>,
    resources: Vec<Option<usize>>,
    contacts: Vec<Option<usize>>,
    resets: &[(NodeId, usize)],
) -> OwnerMap {
    let mut nodes = vec![None; graph.nodes.len()];
    for (index, actor) in graph.pistons.iter().enumerate() {
        nodes[actor.node.index()] = actors[index];
    }
    for &(reset, owner) in resets {
        nodes[reset.index()] = Some(owner);
    }
    for (index, contact) in graph.contacts.iter().enumerate() {
        nodes[contact.node.index()] = contacts[index];
    }
    let sites = graph
        .sites
        .iter()
        .map(|site| match site.source {
            SiteSource::Component(node) => nodes[node.index()],
            SiteSource::Contact(contact) => contacts[contact.index()],
            SiteSource::Constant(_) => None,
        })
        .collect();
    OwnerMap {
        actors,
        resources,
        contacts,
        nodes,
        sites,
    }
}

/// Derive admitted cell owners, non-classic wire corridors, and all typed crossings once.
pub(crate) fn infer_boundaries(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<BoundaryCatalog, String> {
    let discovery = cells::discover_cells(graph, monitor)?;

    let mut declines = discovery.declined.clone();
    let mut declined_actors: FxHashSet<_> = declines.iter().flat_map(|decline| decline.actors.iter().copied()).collect();
    for (index, owner) in discovery.actor_owners.iter().enumerate() {
        if owner.is_none() {
            let actor = PistonId::new(index as u32);
            if declined_actors.insert(actor) {
                declines.push(CellDecline {
                    actors: vec![actor],
                    reason: CellDeclineReason::UnrecognizedActor(actor),
                });
            }
        }
    }

    let cell_count = discovery.cells.len();
    let actors = discovery.actor_owners.iter().map(|owner| owner.map(|id| id as usize)).collect::<Vec<_>>();
    let resources = discovery.resource_owners.iter().map(|owner| owner.map(|id| id as usize)).collect();
    let contacts = discovery.contact_owners.iter().map(|owner| owner.map(|id| id as usize)).collect();
    let mut resets = Vec::new();
    for (cell, contract) in discovery.cells.iter().enumerate() {
        for use_ in contract.uses.iter() {
            resets.push((use_.reset, cell));
        }
    }
    let mut owners = bind_owners(graph, actors, resources, contacts, &resets);
    let corridors = corridors::discover_corridors(graph, &owners, monitor)?;
    let mut corridor_wire_owners = FxHashMap::default();
    for (corridor_index, corridor) in corridors.iter().enumerate() {
        let owner = cell_count + corridor_index;
        for &wire in &corridor.wires {
            owners.nodes[wire.index()] = Some(owner);
            corridor_wire_owners.insert(wire, owner);
        }
        for &site in &corridor.conductive_sites {
            owners.sites[site.index()] = Some(owner);
        }
    }
    for (site_index, site) in graph.sites.iter().enumerate() {
        if let SiteSource::Component(node) = site.source
            && let Some(&owner) = corridor_wire_owners.get(&node)
        {
            owners.sites[site_index] = Some(owner);
        }
    }

    let scan = scan_boundaries(graph, &owners, monitor)?;
    let ports = scan
        .ports
        .into_iter()
        .map(|port| {
            let owner = if port.owner < cell_count {
                BoundaryOwner::Cell(port.owner as u32)
            } else {
                BoundaryOwner::Corridor((port.owner - cell_count) as u32)
            };
            BoundaryPort {
                owner,
                direction: port.direction,
                kind: port.kind,
            }
        })
        .collect::<Vec<_>>();
    if monitor.cancelled() {
        return Err("cancelled while identifying instant boundaries".into());
    }
    Ok(BoundaryCatalog {
        cells: discovery.cells.into_boxed_slice(),
        declines: declines.into_boxed_slice(),
        corridors: corridors.into_boxed_slice(),
        ports: ports.into_boxed_slice(),
    })
}
