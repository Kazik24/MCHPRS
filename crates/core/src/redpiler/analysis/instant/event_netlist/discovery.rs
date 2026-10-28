//! Grow from actual callback occurrences, leaving native reads at the cut.
use super::ports::{PortRole, port_role};
use crate::redpiler::TaskMonitor;
use crate::redpiler::analysis::MAX_GRAPH_PREPARATION_BYTES;
use crate::redpiler::analysis::instant::regions::{self, EventEdge, RegionPartition};
use crate::redpiler::program_graph::{BoundaryCatalog, BoundaryOwner, CompileGraph};

const MAX_PORT_SCAN: usize = 32 * 1024 * 1024;

/// A typed source witness; `row` indexes the original catalog without copying it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CandidatePort {
    pub(crate) region: u32,
    pub(crate) row: u32,
    pub(crate) role: PortRole,
}

/// This is a discovery result, not proof that any region can execute.
pub(crate) struct EventCandidates {
    /// Indexed as cells followed by corridors; false owners remain native.
    pub(crate) selected: Vec<bool>,
    /// Owner-qualified source dependencies reached by discovery, not execution proof.
    pub(crate) edges: Vec<EventEdge>,
    /// Existing source owner mapping and original catalog port indices.
    pub(crate) partition: RegionPartition,
    /// Original rows for selected owners, in catalog order, including external reads.
    pub(crate) ports: Vec<CandidatePort>,
}

/// `eligible` is a capability cut, not evidence that ownership has transferred.
/// Shared resource owners remain atomic in the source catalog and owner mapping.
pub(crate) fn discover(graph: &CompileGraph, catalog: &BoundaryCatalog, eligible: &[bool], monitor: &TaskMonitor) -> Result<EventCandidates, String> {
    if catalog.ports.len() > MAX_PORT_SCAN.saturating_mul(monitor.budget_multiplier()) {
        return Err("event port scan budget exceeded".into());
    }
    let (selected, edges) = regions::greedy_event_pairs(graph, catalog, eligible, 0, monitor)?;
    let base = selected
        .capacity()
        .checked_mul(std::mem::size_of::<bool>())
        .and_then(|bytes| {
            edges
                .capacity()
                .checked_mul(std::mem::size_of::<EventEdge>())
                .and_then(|events| bytes.checked_add(events))
        })
        .ok_or("event candidate retained size overflow")?;
    let mut port_count = 0usize;
    for port in &catalog.ports {
        if monitor.cancelled() {
            return Err("cancelled while counting event ports".into());
        }
        if *selected
            .get(owner_index(port.owner, catalog.cells.len())?)
            .ok_or("event port owner is missing")?
        {
            port_count = port_count.checked_add(1).ok_or("event port count overflow")?;
        }
    }
    let retained_bytes = port_count
        .checked_mul(std::mem::size_of::<CandidatePort>())
        .and_then(|bytes| base.checked_add(bytes))
        .ok_or("event port size overflow")?;
    if retained_bytes > (MAX_GRAPH_PREPARATION_BYTES / 4).saturating_mul(monitor.budget_multiplier()) {
        return Err("event port budget exceeded".into());
    }
    let partition = regions::partition_with_event_edges(graph, catalog, monitor, &selected, retained_bytes, &edges)?;
    let mut ports = Vec::with_capacity(port_count);
    for (row, &port) in catalog.ports.iter().enumerate() {
        if monitor.cancelled() {
            return Err("cancelled while classifying event ports".into());
        }
        let owner = owner_index(port.owner, catalog.cells.len())?;
        if *selected.get(owner).ok_or("event port owner is missing")? {
            ports.push(CandidatePort {
                region: *partition.owner_region.get(owner).ok_or("event region is missing")?,
                row: u32::try_from(row).map_err(|_| "too many event ports")?,
                role: port_role(port),
            });
        }
    }
    Ok(EventCandidates {
        selected,
        edges,
        partition,
        ports,
    })
}

fn owner_index(owner: BoundaryOwner, cells: usize) -> Result<usize, String> {
    match owner {
        BoundaryOwner::Cell(index) => Ok(index as usize),
        BoundaryOwner::Corridor(index) => cells.checked_add(index as usize).ok_or_else(|| "event owner overflow".into()),
    }
}
