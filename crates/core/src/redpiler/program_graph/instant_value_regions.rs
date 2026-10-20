//! Bounded max-plus value programs for electrical components inside transport nets.
//!
//! Notification connectivity and wire-strength connectivity are different
//! graphs. This product partitions each already-certified transport net by its
//! possible direct and conditional power edges, then stores unconditional
//! shortest-distance rows from typed drive groups and conditional targets.
//! The runtime may use the result to prove interval invariants; this module does
//! not dispatch callbacks, mutate wire state, or replace ordered transport.
//!
//! Distances are bytes capped at fifteen. Equal rows are interned, and all
//! region-owned tables are contiguous slices addressed by ranges. A failed
//! component remains unmapped (`u32::MAX`) while independent components may be
//! published. No block query or source-site lookup is rebuilt here: each drive
//! group refers to the existing `InstantDriveProgram` query ID.

use super::*;
use crate::redpiler::TaskMonitor;
use std::collections::{BTreeMap, VecDeque};
use std::ops::Range;

const MAX_REGION_WORK: usize = 16_000_000;
const MAX_REGION_BYTES: usize = crate::redpiler::analysis::MAX_GRAPH_PREPARATION_BYTES / 8;
const NO_INDEX: u32 = u32::MAX;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct InstantValueRegions {
    /// Rows align with `InstantTransport::nets`.
    pub(crate) nets: Vec<Option<InstantValueRegionNet>>,
    pub(crate) stats: InstantValueRegionStats,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct InstantValueRegionStats {
    pub(crate) nets: usize,
    pub(crate) regions: usize,
    pub(crate) admitted: usize,
    pub(crate) declined: usize,
    pub(crate) wires: usize,
    pub(crate) compact_wire_captures: usize,
    pub(crate) compact_capture_slots: usize,
    pub(crate) drive_groups: usize,
    pub(crate) conditional_arcs: usize,
    pub(crate) distance_rows: usize,
    pub(crate) work: usize,
    pub(crate) bytes: usize,
    pub(crate) declines: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct InstantValueRegionNet {
    /// Static member wires whose incoming transport slots all have known
    /// callback admission. Runtime may retain only their captured power.
    /// Aligned with the parent `TransportNet::wires` table.
    pub(crate) compact_wire_captures: Box<[bool]>,
    /// `u32::MAX` for wires in a declined component; otherwise indexes `regions`.
    pub(crate) wire_region: Box<[u32]>,
    /// Region-local wire index, or `u32::MAX` when `wire_region` is absent.
    pub(crate) wire_local: Box<[u32]>,
    pub(crate) regions: Box<[ValueRegion]>,
    /// Maps region-local IDs back to transport-net-local IDs.
    pub(crate) region_wires: Box<[u32]>,
    pub(crate) drive_groups: Box<[ValueDriveGroup]>,
    pub(crate) conditional_arcs: Box<[ValueConditionalArc]>,
    /// Geometry selectors use native typed site IDs from the transport net.
    pub(crate) selectors: Box<[ValueGeometrySelector]>,
    pub(crate) distance_rows: Box<[ValueDistanceRow]>,
    pub(crate) distances: Box<[u8]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ValueRegion {
    /// Deterministic weak-component number before any components are declined.
    pub(crate) component: u32,
    pub(crate) wires: Range<u32>,
    pub(crate) drive_groups: Range<u32>,
    pub(crate) conditional_arcs: Range<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ValueDriveGroup {
    /// Existing typed source expression, evaluated once by the caller.
    pub(crate) query: u32,
    /// Unconditional multi-source distance row from every injection site.
    pub(crate) row: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ValueConditionalArc {
    /// Region-local source wire; the arc loses one level before its target.
    pub(crate) source: u32,
    /// Unconditional distance row beginning at the target wire.
    pub(crate) row: u32,
    pub(crate) selector: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ValueGeometrySelector {
    pub(crate) horizontal_site: SiteId,
    /// Present for a step-up input; its solid face participates in selection.
    pub(crate) ascent_up_site: Option<SiteId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ValueDistanceRow {
    pub(crate) start: u32,
    pub(crate) len: u32,
}

#[derive(Default)]
struct Dsu {
    parents: Vec<usize>,
    ranks: Vec<u8>,
}

impl Dsu {
    fn new(len: usize) -> Self {
        Self {
            parents: (0..len).collect(),
            ranks: vec![0; len],
        }
    }

    fn root(&mut self, mut node: usize) -> usize {
        let mut root = node;
        while self.parents[root] != root {
            root = self.parents[root];
        }
        while self.parents[node] != node {
            let next = self.parents[node];
            self.parents[node] = root;
            node = next;
        }
        root
    }

    fn union(&mut self, left: usize, right: usize) {
        let mut left = self.root(left);
        let mut right = self.root(right);
        if left == right {
            return;
        }
        if self.ranks[left] < self.ranks[right] || (self.ranks[left] == self.ranks[right] && left > right) {
            std::mem::swap(&mut left, &mut right);
        }
        self.parents[right] = left;
        if self.ranks[left] == self.ranks[right] {
            self.ranks[left] = self.ranks[left].saturating_add(1);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompileFailure {
    Unsupported(&'static str),
    WorkBudget,
    MemoryBudget,
    Cancelled,
}

impl CompileFailure {
    fn reason(self) -> &'static str {
        match self {
            Self::Unsupported(reason) => reason,
            Self::WorkBudget => "value region work budget",
            Self::MemoryBudget => "value region memory budget",
            Self::Cancelled => "cancelled",
        }
    }
}

struct RegionBuild {
    groups: Vec<ValueDriveGroup>,
    arcs: Vec<ValueConditionalArc>,
    selectors: Vec<ValueGeometrySelector>,
    rows: Vec<Vec<u8>>,
    distances: Vec<u8>,
}

#[derive(Default)]
struct BuildingNet {
    compact_wire_captures: Vec<bool>,
    wire_region: Vec<u32>,
    wire_local: Vec<u32>,
    regions: Vec<ValueRegion>,
    region_wires: Vec<u32>,
    drive_groups: Vec<ValueDriveGroup>,
    conditional_arcs: Vec<ValueConditionalArc>,
    selectors: Vec<ValueGeometrySelector>,
    distance_rows: Vec<ValueDistanceRow>,
    distances: Vec<u8>,
}

impl BuildingNet {
    fn finish(self) -> InstantValueRegionNet {
        InstantValueRegionNet {
            compact_wire_captures: self.compact_wire_captures.into_boxed_slice(),
            wire_region: self.wire_region.into_boxed_slice(),
            wire_local: self.wire_local.into_boxed_slice(),
            regions: self.regions.into_boxed_slice(),
            region_wires: self.region_wires.into_boxed_slice(),
            drive_groups: self.drive_groups.into_boxed_slice(),
            conditional_arcs: self.conditional_arcs.into_boxed_slice(),
            selectors: self.selectors.into_boxed_slice(),
            distance_rows: self.distance_rows.into_boxed_slice(),
            distances: self.distances.into_boxed_slice(),
        }
    }
}

/// Compile every electrical component independently. A declined component has
/// no mapping in `wire_region` and remains on exact transport execution.
pub(crate) fn infer_instant_value_regions(
    graph: &CompileGraph,
    transport: &InstantTransport,
    drives: &InstantDriveProgram,
    monitor: &TaskMonitor,
) -> Result<InstantValueRegions, String> {
    if monitor.cancelled() {
        return Err("cancelled compiling instant value regions".into());
    }
    let work_limit = MAX_REGION_WORK.saturating_mul(monitor.budget_multiplier());
    let byte_limit = MAX_REGION_BYTES.saturating_mul(monitor.budget_multiplier());
    let mut result = InstantValueRegions {
        nets: Vec::with_capacity(transport.nets.len()),
        ..Default::default()
    };
    let compact_scan_work = graph.wires.len().saturating_mul(24);
    let compact_scan_peak_bytes = graph.sites.len().saturating_mul(std::mem::size_of::<bool>() * 4);
    let compact_capture_sites = if compact_scan_work <= work_limit && compact_scan_peak_bytes <= byte_limit {
        Some(compact_capture_sites(graph, transport, monitor)?)
    } else {
        None
    };
    let compact_site_bytes = compact_capture_sites
        .as_ref()
        .map_or(0, |_| graph.sites.len().saturating_mul(std::mem::size_of::<bool>()));
    let mut total_work = if compact_capture_sites.is_some() { compact_scan_work } else { 0 };
    result.stats.work = total_work;
    let mut total_bytes = 0usize;

    for (net_index, net) in transport.nets.iter().enumerate() {
        if net_index % 16 == 0 && monitor.cancelled() {
            return Err("cancelled compiling instant value regions".into());
        }
        result.stats.nets += 1;
        let components = components(net, monitor)?;
        result.stats.regions += components.len();
        let compact_wire_captures: Vec<bool> = net
            .wires
            .iter()
            .map(|wire| {
                compact_capture_sites
                    .as_ref()
                    .is_some_and(|sites| sites[net.sites[wire.site as usize].index()])
            })
            .collect();
        let compact_capture_count = compact_wire_captures.iter().filter(|&&eligible| eligible).count();
        let wire_map_bytes = net
            .wires
            .len()
            .saturating_mul(std::mem::size_of::<u32>() * 2 + std::mem::size_of::<bool>());
        if total_bytes.saturating_add(compact_site_bytes).saturating_add(wire_map_bytes) > byte_limit {
            result.stats.declined += components.len();
            *result.stats.declines.entry(CompileFailure::MemoryBudget.reason().into()).or_default() += components.len();
            result.nets.push(None);
            continue;
        }
        total_bytes = total_bytes.saturating_add(wire_map_bytes);
        result.stats.bytes = total_bytes;
        let mut output = empty_net(net.wires.len(), compact_wire_captures);
        let mut row_ids: BTreeMap<Vec<u8>, u32> = BTreeMap::new();
        let mut selector_ids: BTreeMap<ValueGeometrySelector, u32> = BTreeMap::new();

        for (component_index, wires) in components.iter().enumerate() {
            if monitor.cancelled() {
                return Err("cancelled compiling instant value regions".into());
            }
            if total_work >= work_limit {
                *result.stats.declines.entry(CompileFailure::WorkBudget.reason().into()).or_default() += 1;
                continue;
            }
            let region_start = output.region_wires.len() as u32;
            output.region_wires.extend(wires.iter().map(|&wire| wire as u32));
            let component_marker = NO_INDEX - 1 - component_index as u32;
            for (local, &wire) in wires.iter().enumerate() {
                output.wire_local[wire] = local as u32;
                output.wire_region[wire] = component_marker;
            }
            let built = match compile_region(
                graph,
                net,
                drives,
                wires,
                monitor,
                &mut total_work,
                work_limit,
                byte_limit.saturating_sub(total_bytes),
                &output.wire_local,
                &output.wire_region,
                component_marker,
            ) {
                Ok(region) => region,
                Err(CompileFailure::Cancelled) => return Err("cancelled compiling instant value regions".into()),
                Err(failure) => {
                    result.stats.work = total_work;
                    *result.stats.declines.entry(failure.reason().into()).or_default() += 1;
                    output.region_wires.truncate(region_start as usize);
                    for &wire in wires {
                        output.wire_local[wire] = NO_INDEX;
                        output.wire_region[wire] = NO_INDEX;
                    }
                    continue;
                }
            };
            let incremental_bytes = region_storage_bytes(wires.len(), &built);
            if total_bytes.saturating_add(compact_site_bytes).saturating_add(incremental_bytes) > byte_limit {
                *result.stats.declines.entry(CompileFailure::MemoryBudget.reason().into()).or_default() += 1;
                output.region_wires.truncate(region_start as usize);
                for &wire in wires {
                    output.wire_local[wire] = NO_INDEX;
                    output.wire_region[wire] = NO_INDEX;
                }
                continue;
            }
            total_bytes = total_bytes.saturating_add(incremental_bytes);
            result.stats.work = total_work;
            result.stats.bytes = total_bytes;

            let region_id = output.regions.len() as u32;
            let group_start = output.drive_groups.len() as u32;
            let arc_start = output.conditional_arcs.len() as u32;
            let mut row_remap = Vec::with_capacity(built.rows.len());
            for row in built.rows {
                let row_id = if let Some(&id) = row_ids.get(&row) {
                    id
                } else {
                    let id = output.distance_rows.len() as u32;
                    let start = output.distances.len() as u32;
                    output.distances.extend_from_slice(&row);
                    output.distance_rows.push(ValueDistanceRow {
                        start,
                        len: row.len() as u32,
                    });
                    row_ids.insert(row, id);
                    id
                };
                row_remap.push(row_id);
            }
            let mut selector_remap = Vec::with_capacity(built.selectors.len());
            for &selector in &built.selectors {
                if !selector_ids.contains_key(&selector) {
                    let id = output.selectors.len() as u32;
                    output.selectors.push(selector);
                    selector_ids.insert(selector, id);
                }
                selector_remap.push(selector_ids[&selector]);
            }
            output.drive_groups.extend(built.groups.into_iter().map(|group| ValueDriveGroup {
                query: group.query,
                row: row_remap[group.row as usize],
            }));
            output.conditional_arcs.extend(built.arcs.into_iter().map(|arc| ValueConditionalArc {
                source: arc.source,
                row: row_remap[arc.row as usize],
                selector: selector_remap[arc.selector as usize],
            }));
            // Region descriptors own contiguous slices of every region-indexed table.
            let wires_end = output.region_wires.len() as u32;
            let groups_end = output.drive_groups.len() as u32;
            let arcs_end = output.conditional_arcs.len() as u32;
            output.regions.push(ValueRegion {
                component: component_index as u32,
                wires: region_start..wires_end,
                drive_groups: group_start..groups_end,
                conditional_arcs: arc_start..arcs_end,
            });
            for (local, &wire) in wires.iter().enumerate() {
                output.wire_region[wire] = region_id;
                output.wire_local[wire] = local as u32;
            }
            result.stats.admitted += 1;
            result.stats.wires += wires.len();
            result.stats.drive_groups += groups_end.saturating_sub(group_start) as usize;
            result.stats.conditional_arcs += arcs_end.saturating_sub(arc_start) as usize;
        }
        result.stats.declined += components.len().saturating_sub(output.regions.len());
        result.stats.compact_wire_captures += compact_capture_count;
        result.stats.distance_rows += output.distance_rows.len();
        if output.regions.is_empty() && compact_capture_count == 0 {
            total_bytes = total_bytes.saturating_sub(wire_map_bytes);
            result.stats.bytes = total_bytes;
            result.nets.push(None);
        } else {
            result.stats.compact_capture_slots += net.wires.len();
            result.nets.push(Some(output.finish()));
        }
    }
    Ok(result)
}

fn empty_net(wire_count: usize, compact_wire_captures: Vec<bool>) -> BuildingNet {
    BuildingNet {
        compact_wire_captures,
        wire_region: vec![NO_INDEX; wire_count],
        wire_local: vec![NO_INDEX; wire_count],
        ..Default::default()
    }
}

/// Mark static component-wire sites only when every incoming canonical source
/// slot already has an authoritative, known callback-selection bit. Unknown
/// slots require the complete captured observation at runtime.
fn compact_capture_sites(graph: &CompileGraph, transport: &InstantTransport, monitor: &TaskMonitor) -> Result<Vec<bool>, String> {
    let mut is_transport_wire_site = vec![false; graph.sites.len()];
    for net in &transport.nets {
        for wire in net.wires.iter() {
            let site = net.sites[wire.site as usize];
            is_transport_wire_site[site.index()] = true;
        }
    }
    let mut has_incoming = vec![false; graph.sites.len()];
    let mut all_incoming_known = vec![true; graph.sites.len()];
    for (source_index, source) in graph.wires.iter().enumerate() {
        if source_index % 256 == 0 && monitor.cancelled() {
            return Err("cancelled compiling compact wire captures".into());
        }
        let selection = source.execution.callback_selection;
        for (slot, &site) in source.canonical_neighbors.iter().enumerate() {
            if !is_transport_wire_site[site.index()] {
                continue;
            }
            has_incoming[site.index()] = true;
            let known = selection.is_some_and(|selection| selection.known & (1 << slot) != 0);
            all_incoming_known[site.index()] &= known;
        }
    }
    Ok(is_transport_wire_site
        .into_iter()
        .enumerate()
        .map(|(site, is_member)| is_member && (!has_incoming[site] || all_incoming_known[site]))
        .collect())
}

fn components(net: &TransportNet, monitor: &TaskMonitor) -> Result<Vec<Vec<usize>>, String> {
    let mut dsu = Dsu::new(net.wires.len());
    for (target, wire) in net.wires.iter().enumerate() {
        if target % 256 == 0 && monitor.cancelled() {
            return Err("cancelled partitioning instant value regions".into());
        }
        let mut mask = wire.inputs | wire.conditional_inputs;
        while mask != 0 {
            let slot = mask.trailing_zeros() as usize;
            mask &= mask - 1;
            let site = wire.neighbors[slot];
            if site == NO_INDEX || site as usize >= net.site_wire.len() {
                return Err("transport edge has no retained site".into());
            }
            let source = net.site_wire[site as usize];
            if source == NO_INDEX || source as usize >= net.wires.len() {
                return Err("transport edge source is not a member wire".into());
            }
            dsu.union(target, source as usize);
        }
    }
    let mut components: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for wire in 0..net.wires.len() {
        let root = dsu.root(wire);
        components.entry(root).or_default().push(wire);
    }
    let mut output = components.into_values().collect::<Vec<_>>();
    output.sort_by_key(|component| component[0]);
    Ok(output)
}

fn compile_region(
    graph: &CompileGraph,
    net: &TransportNet,
    drives: &InstantDriveProgram,
    wires: &[usize],
    monitor: &TaskMonitor,
    work: &mut usize,
    work_budget: usize,
    memory_budget: usize,
    wire_local: &[u32],
    wire_region: &[u32],
    component_marker: u32,
) -> Result<RegionBuild, CompileFailure> {
    let direct_edge_count = wires.iter().map(|&wire| net.wires[wire].inputs.count_ones() as usize).sum::<usize>();
    let conditional_edge_count = wires
        .iter()
        .map(|&wire| net.wires[wire].conditional_inputs.count_ones() as usize)
        .sum::<usize>();
    let mut drive_term_count = 0usize;
    for &wire in wires {
        let CircuitNodeKind::Wire(wire_index) = graph.nodes[net.wires[wire].node.index()].kind else {
            return Err(CompileFailure::Unsupported("transport member is not a wire"));
        };
        let query_id = drives
            .by_wire
            .get(wire_index)
            .copied()
            .flatten()
            .ok_or(CompileFailure::Unsupported("wire drive expression unavailable"))?;
        let query = drives
            .queries
            .get(query_id as usize)
            .ok_or(CompileFailure::Unsupported("wire drive expression index invalid"))?;
        let term_count = query
            .terms
            .end
            .checked_sub(query.terms.start)
            .ok_or(CompileFailure::Unsupported("wire drive expression range invalid"))? as usize;
        drive_term_count = drive_term_count.saturating_add(term_count);
    }
    // Bound the non-row peak scratch: adjacency vectors/edges, drive grouping,
    // and conditional transition records. Distance-row storage has its own
    // tighter per-intern check below.
    let scratch_bytes = wires
        .len()
        .saturating_mul(std::mem::size_of::<Vec<u32>>() + std::mem::size_of::<(u8, Vec<DriveTerm>)>() + 32)
        .saturating_add(direct_edge_count.saturating_mul(std::mem::size_of::<u32>()))
        .saturating_add(drive_term_count.saturating_mul(std::mem::size_of::<DriveTerm>()))
        .saturating_add(wires.len().saturating_mul(std::mem::size_of::<u32>()))
        .saturating_add(conditional_edge_count.saturating_mul(std::mem::size_of::<(u32, u32, ValueGeometrySelector)>() + 48));
    if scratch_bytes > memory_budget {
        return Err(CompileFailure::MemoryBudget);
    }
    let row_memory_budget = memory_budget - scratch_bytes;
    let mut outgoing = vec![Vec::<u32>::new(); wires.len()];
    let mut conditional_edges = Vec::<(u32, u32, ValueGeometrySelector)>::new();
    let mut drive_keys: BTreeMap<(u8, Vec<DriveTerm>), (u32, Vec<u32>)> = BTreeMap::new();

    for (target_local, &target_wire) in wires.iter().enumerate() {
        if target_local % 128 == 0 && monitor.cancelled() {
            return Err(CompileFailure::Cancelled);
        }
        let plan = &net.wires[target_wire];
        let CircuitNodeKind::Wire(wire_index) = graph.nodes[plan.node.index()].kind else {
            return Err(CompileFailure::Unsupported("transport member is not a wire"));
        };
        let query_id = drives
            .by_wire
            .get(wire_index)
            .copied()
            .flatten()
            .ok_or(CompileFailure::Unsupported("wire drive expression unavailable"))?;
        let query = drives
            .queries
            .get(query_id as usize)
            .ok_or(CompileFailure::Unsupported("wire drive expression index invalid"))?;
        let terms = drives
            .terms
            .get(query.terms.start as usize..query.terms.end as usize)
            .ok_or(CompileFailure::Unsupported("wire drive expression range invalid"))?
            .to_vec();
        let mut key_terms = terms;
        key_terms.sort_unstable();
        if query.constant != 0 || !key_terms.is_empty() {
            let key = (query.constant, key_terms);
            let entry = drive_keys.entry(key).or_insert_with(|| (query_id, Vec::new()));
            entry.0 = entry.0.min(query_id);
            entry.1.push(target_local as u32);
        }

        let mut direct = plan.inputs;
        while direct != 0 {
            let slot = direct.trailing_zeros() as usize;
            direct &= direct - 1;
            let source_wire = member_neighbor(net, plan, slot)?;
            if wire_region[source_wire] != component_marker {
                return Err(CompileFailure::Unsupported("direct source crossed value region"));
            }
            let source_local = wire_local[source_wire];
            if source_local == NO_INDEX || source_local as usize >= wires.len() {
                return Err(CompileFailure::Unsupported("direct source has invalid region-local index"));
            }
            outgoing[source_local as usize].push(target_local as u32);
        }
        let mut conditional = plan.conditional_inputs;
        while conditional != 0 {
            let slot = conditional.trailing_zeros() as usize;
            conditional &= conditional - 1;
            let source_wire = member_neighbor(net, plan, slot)?;
            if wire_region[source_wire] != component_marker {
                return Err(CompileFailure::Unsupported("conditional source crossed value region"));
            }
            let source_local = wire_local[source_wire];
            if source_local == NO_INDEX || source_local as usize >= wires.len() {
                return Err(CompileFailure::Unsupported("conditional source has invalid region-local index"));
            }
            let selector = geometry_selector(net, plan, slot)?;
            conditional_edges.push((source_local, target_local as u32, selector));
        }
    }
    for edges in &mut outgoing {
        edges.sort_unstable();
        edges.dedup();
    }

    let mut build = RegionBuild {
        groups: Vec::new(),
        arcs: Vec::new(),
        selectors: Vec::new(),
        rows: Vec::new(),
        distances: Vec::new(),
    };
    let mut row_intern = BTreeMap::<Vec<u8>, u32>::new();
    let mut selector_intern = BTreeMap::<ValueGeometrySelector, u32>::new();

    for (_, (query, sources)) in drive_keys {
        let row = compile_distance_row(&outgoing, &sources, work, work_budget, monitor)?;
        let row_id = intern_row(row, &mut build, &mut row_intern, row_memory_budget)?;
        build.groups.push(ValueDriveGroup { query, row: row_id });
    }
    for (source, target, selector) in conditional_edges {
        let sources = [target];
        let row = compile_distance_row(&outgoing, &sources, work, work_budget, monitor)?;
        let row_id = intern_row(row, &mut build, &mut row_intern, row_memory_budget)?;
        let selector_id = if let Some(&id) = selector_intern.get(&selector) {
            id
        } else {
            let id = build.selectors.len() as u32;
            build.selectors.push(selector);
            selector_intern.insert(selector, id);
            id
        };
        build.arcs.push(ValueConditionalArc {
            source,
            row: row_id,
            selector: selector_id,
        });
    }
    Ok(build)
}

fn member_neighbor(net: &TransportNet, wire: &TransportWire, slot: usize) -> Result<usize, CompileFailure> {
    let site = *wire.neighbors.get(slot).ok_or(CompileFailure::Unsupported("invalid wire slot"))?;
    if site == NO_INDEX || site as usize >= net.site_wire.len() {
        return Err(CompileFailure::Unsupported("wire input site unavailable"));
    }
    let source = net.site_wire[site as usize];
    if source == NO_INDEX || source as usize >= net.wires.len() {
        return Err(CompileFailure::Unsupported("wire input is not a member"));
    }
    Ok(source as usize)
}

fn geometry_selector(net: &TransportNet, wire: &TransportWire, slot: usize) -> Result<ValueGeometrySelector, CompileFailure> {
    let (horizontal_slot, ascent) = match slot {
        7 => (0, false),
        8 => (0, true),
        12 => (1, false),
        13 => (1, true),
        17 => (4, false),
        20 => (4, true),
        18 => (5, false),
        21 => (5, true),
        _ => return Err(CompileFailure::Unsupported("conditional input is not diagonal")),
    };
    let horizontal = wire.neighbors[horizontal_slot];
    if horizontal == NO_INDEX || horizontal as usize >= net.sites.len() {
        return Err(CompileFailure::Unsupported("conditional horizontal site unavailable"));
    }
    let up = if ascent {
        let up = wire.neighbors[3];
        if up == NO_INDEX || up as usize >= net.sites.len() {
            return Err(CompileFailure::Unsupported("conditional ascent site unavailable"));
        }
        Some(net.sites[up as usize])
    } else {
        None
    };
    Ok(ValueGeometrySelector {
        horizontal_site: net.sites[horizontal as usize],
        ascent_up_site: up,
    })
}

fn compile_distance_row(
    outgoing: &[Vec<u32>],
    sources: &[u32],
    work: &mut usize,
    budget: usize,
    monitor: &TaskMonitor,
) -> Result<Vec<u8>, CompileFailure> {
    if monitor.cancelled() {
        return Err(CompileFailure::Cancelled);
    }
    let mut distances = vec![15u8; outgoing.len()];
    let mut queue = VecDeque::new();
    for &source in sources {
        let slot = distances
            .get_mut(source as usize)
            .ok_or(CompileFailure::Unsupported("distance source out of region"))?;
        if *slot != 0 {
            *slot = 0;
            queue.push_back(source as usize);
        }
    }
    while let Some(source) = queue.pop_front() {
        *work = work.saturating_add(1);
        if *work > budget {
            return Err(CompileFailure::WorkBudget);
        }
        if *work % 256 == 0 && monitor.cancelled() {
            return Err(CompileFailure::Cancelled);
        }
        let next_distance = distances[source] + 1;
        if next_distance >= 15 {
            continue;
        }
        for &target in &outgoing[source] {
            *work = work.saturating_add(1);
            if *work > budget {
                return Err(CompileFailure::WorkBudget);
            }
            let value = &mut distances[target as usize];
            if next_distance < *value {
                *value = next_distance;
                queue.push_back(target as usize);
            }
        }
    }
    Ok(distances)
}

fn intern_row(row: Vec<u8>, build: &mut RegionBuild, intern: &mut BTreeMap<Vec<u8>, u32>, memory_budget: usize) -> Result<u32, CompileFailure> {
    if let Some(&id) = intern.get(&row) {
        return Ok(id);
    }
    let estimated = build
        .distances
        .len()
        .saturating_add(row.len())
        .saturating_mul(3)
        .saturating_add((build.rows.len() + 1) * std::mem::size_of::<ValueDistanceRow>());
    if estimated > memory_budget {
        return Err(CompileFailure::MemoryBudget);
    }
    let id = build.rows.len() as u32;
    build.distances.extend_from_slice(&row);
    build.rows.push(row.clone());
    intern.insert(row, id);
    Ok(id)
}

fn region_storage_bytes(wires: usize, build: &RegionBuild) -> usize {
    wires * std::mem::size_of::<u32>()
        + std::mem::size_of::<ValueRegion>()
        + build.groups.len() * std::mem::size_of::<ValueDriveGroup>()
        + build.arcs.len() * std::mem::size_of::<ValueConditionalArc>()
        + build.selectors.len() * std::mem::size_of::<ValueGeometrySelector>()
        + build.rows.len() * std::mem::size_of::<ValueDistanceRow>()
        + build.distances.len()
        // Each row is held once in `rows`, once in the flat distance table,
        // and once as a key in the interning map during construction. The
        // latter is dropped after compilation, but charging all copies keeps
        // the temporary compiler peak within the same bound.
        + build.rows.iter().map(Vec::capacity).sum::<usize>() * 2
}

impl InstantValueRegionNet {
    /// Solve one admitted region without allocating. `query_values` are the
    /// already-evaluated typed drive queries; selectors are sampled coherently
    /// from this transport walk's captured geometry.
    pub(crate) fn solve_region(&self, region_id: u32, query_values: &[u8], selectors: &[bool], output: &mut [u8]) -> bool {
        let Some(region) = self.regions.get(region_id as usize) else {
            return false;
        };
        let wire_count = (region.wires.end - region.wires.start) as usize;
        if output.len() < wire_count {
            return false;
        }
        let Some(groups) = self
            .drive_groups
            .get(region.drive_groups.start as usize..region.drive_groups.end as usize)
        else {
            return false;
        };
        let Some(arcs) = self
            .conditional_arcs
            .get(region.conditional_arcs.start as usize..region.conditional_arcs.end as usize)
        else {
            return false;
        };
        let values = &mut output[..wire_count];
        values.fill(0);
        for group in groups {
            let Some(&drive) = query_values.get(group.query as usize) else {
                return false;
            };
            if drive > 15 {
                return false;
            }
            let Some(row) = self.distance_rows.get(group.row as usize) else {
                return false;
            };
            let distances = &self.distances[row.start as usize..(row.start + row.len) as usize];
            if distances.len() != wire_count {
                return false;
            }
            for (value, &distance) in values.iter_mut().zip(distances) {
                *value = (*value).max(drive.saturating_sub(distance));
            }
        }
        for _ in 0..15 {
            let mut changed = false;
            for arc in arcs {
                let Some(&enabled) = selectors.get(arc.selector as usize) else {
                    return false;
                };
                if !enabled {
                    continue;
                }
                let Some(row) = self.distance_rows.get(arc.row as usize) else {
                    return false;
                };
                if arc.source as usize >= wire_count {
                    return false;
                }
                let strength = values[arc.source as usize].saturating_sub(1);
                let distances = &self.distances[row.start as usize..(row.start + row.len) as usize];
                if distances.len() != wire_count {
                    return false;
                }
                for (value, &distance) in values.iter_mut().zip(distances) {
                    let next = strength.saturating_sub(distance);
                    if next > *value {
                        *value = next;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy)]
    struct TestArc {
        source: u32,
        target: u32,
        selector: usize,
    }

    fn test_region(count: usize, direct: &[(u32, u32)], drives: &[(u32, u32)], conditional: &[TestArc]) -> InstantValueRegionNet {
        let mut outgoing = vec![Vec::new(); count];
        for &(source, target) in direct {
            outgoing[source as usize].push(target);
        }
        let mut build = RegionBuild {
            groups: Vec::new(),
            arcs: Vec::new(),
            selectors: (0..conditional.iter().map(|arc| arc.selector).max().map_or(0, |max| max + 1))
                .map(|id| ValueGeometrySelector {
                    horizontal_site: SiteId::new(id as u32),
                    ascent_up_site: None,
                })
                .collect(),
            rows: Vec::new(),
            distances: Vec::new(),
        };
        let mut intern = BTreeMap::new();
        let mut work = 0;
        for &(query, wire) in drives {
            let row = compile_distance_row(&outgoing, &[wire], &mut work, usize::MAX, &TaskMonitor::default()).unwrap();
            let row_id = intern_row(row, &mut build, &mut intern, usize::MAX).unwrap();
            build.groups.push(ValueDriveGroup { query, row: row_id });
        }
        for arc in conditional {
            let row = compile_distance_row(&outgoing, &[arc.target], &mut work, usize::MAX, &TaskMonitor::default()).unwrap();
            let row_id = intern_row(row, &mut build, &mut intern, usize::MAX).unwrap();
            build.arcs.push(ValueConditionalArc {
                source: arc.source,
                row: row_id,
                selector: arc.selector as u32,
            });
        }
        InstantValueRegionNet {
            regions: vec![ValueRegion {
                component: 0,
                wires: 0..count as u32,
                drive_groups: 0..build.groups.len() as u32,
                conditional_arcs: 0..build.arcs.len() as u32,
            }]
            .into_boxed_slice(),
            drive_groups: build.groups.into_boxed_slice(),
            conditional_arcs: build.arcs.into_boxed_slice(),
            selectors: build.selectors.into_boxed_slice(),
            distance_rows: build
                .rows
                .iter()
                .scan(0u32, |start, row| {
                    let result = ValueDistanceRow {
                        start: *start,
                        len: row.len() as u32,
                    };
                    *start += row.len() as u32;
                    Some(result)
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            distances: build.distances.into_boxed_slice(),
            ..Default::default()
        }
    }

    fn relax(count: usize, direct: &[(u32, u32)], drives: &[u8], drive_wires: &[u32], conditional: &[TestArc], selectors: &[bool]) -> Vec<u8> {
        let mut values = vec![0; count];
        for (&drive, &wire) in drives.iter().zip(drive_wires) {
            values[wire as usize] = values[wire as usize].max(drive);
        }
        for _ in 0..15 {
            let mut changed = false;
            for &(source, target) in direct {
                let value = values[source as usize].saturating_sub(1);
                if value > values[target as usize] {
                    values[target as usize] = value;
                    changed = true;
                }
            }
            for arc in conditional {
                if !selectors[arc.selector] {
                    continue;
                }
                let value = values[arc.source as usize].saturating_sub(1);
                if value > values[arc.target as usize] {
                    values[arc.target as usize] = value;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        values
    }

    #[test]
    fn weighted_distance_rows_match_independent_direct_relaxation() {
        let direct = [(0, 1), (1, 2), (2, 0), (1, 3), (4, 3), (5, 6)];
        let drives = [(0, 0), (1, 4), (2, 5)];
        let conditional = [
            TestArc {
                source: 3,
                target: 4,
                selector: 0,
            },
            TestArc {
                source: 6,
                target: 2,
                selector: 1,
            },
            TestArc {
                source: 4,
                target: 5,
                selector: 0,
            },
        ];
        let region = test_region(8, &direct, &drives, &conditional);
        for mask in 0..4 {
            for low in 0..16u8 {
                let query_values = [15, low, low / 2];
                let selectors = [mask & 1 != 0, mask & 2 != 0];
                let mut actual = [0u8; 8];
                assert!(region.solve_region(0, &query_values, &selectors, &mut actual));
                let expected = relax(8, &direct, &query_values, &[0, 4, 5], &conditional, &selectors);
                assert_eq!(actual.as_slice(), expected.as_slice(), "selectors={selectors:?} drives={query_values:?}");
            }
        }
    }

    #[test]
    fn generated_directed_cyclic_graphs_match_bounded_relaxation() {
        let mut seed = 0x9e37_79b9u32;
        for case in 0..48 {
            let count = 2 + case % 7;
            let mut next = || {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                seed
            };
            let mut direct = Vec::new();
            for source in 0..count as u32 {
                for target in 0..count as u32 {
                    if next() % 7 == 0 {
                        direct.push((source, target));
                    }
                }
            }
            let mut drive_wires = Vec::new();
            let mut query_values = Vec::new();
            for _ in 0..3u32 {
                drive_wires.push(next() as usize as u32 % count as u32);
                query_values.push((next() % 16) as u8);
            }
            let mut conditional = Vec::new();
            for index in 0..(count + 1) {
                conditional.push(TestArc {
                    source: next() % count as u32,
                    target: next() % count as u32,
                    selector: index % 2,
                });
            }
            let selectors = [next() & 1 != 0, next() & 1 != 0];
            let drive_bindings = drive_wires
                .iter()
                .enumerate()
                .map(|(query, &wire)| (query as u32, wire))
                .collect::<Vec<_>>();
            let region = test_region(count, &direct, &drive_bindings, &conditional);
            let mut actual = vec![0u8; count];
            assert!(region.solve_region(0, &query_values, &selectors, &mut actual));
            let expected = relax(count, &direct, &query_values, &drive_wires, &conditional, &selectors);
            assert_eq!(actual, expected, "generated case {case}");
        }
    }

    #[test]
    fn distance_search_stops_at_fifteen_and_obeys_work_budget_and_cancellation() {
        let outgoing = (0..20)
            .map(|index| (index + 1 < 20).then_some((index + 1) as u32).into_iter().collect())
            .collect::<Vec<Vec<u32>>>();
        let mut work = 0;
        let row = compile_distance_row(&outgoing, &[0], &mut work, usize::MAX, &TaskMonitor::default()).unwrap();
        assert_eq!(row[0], 0);
        assert_eq!(row[14], 14);
        assert_eq!(row[15], 15);
        let mut work = 0;
        assert_eq!(
            compile_distance_row(&outgoing, &[0], &mut work, 2, &TaskMonitor::default()),
            Err(CompileFailure::WorkBudget)
        );
        let monitor = TaskMonitor::default();
        monitor.cancel();
        let mut work = 0;
        assert_eq!(
            compile_distance_row(&outgoing, &[0], &mut work, usize::MAX, &monitor),
            Err(CompileFailure::Cancelled)
        );
    }
}
