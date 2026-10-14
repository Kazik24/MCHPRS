//! Local power queries for a private two- or three-wire horizontal path.
//!
//! Why: seed and Turbo recalculation repeatedly sample all six neighbors and then
//! inspect up to eight extra wire sites. What: attach an immutable path certificate
//! and calculate only its declared electrical ingress plus adjacent path-wire
//! strengths. When: one direct electrical input, fixed non-contact geometry, a
//! straight path, no other nearby wire/electrical source/consumer, no observer or
//! comparator reading path state, and the saved strengths already equal the
//! one-level-per-wire profile. What remains general: every unproved candidate and
//! every callback, captured neighborhood, write, route, scheduler entry, and state
//! slot. Why correct: under those restrictions the omitted sites are non-wires
//! without electrical output; the replacement reads the same live source
//! strength and captured adjacent-wire strengths used by the general query. The
//! Graph Turbo walk still controls intermediate writes and ordered callbacks.
//! Depends on: required parser plans and opt-01, which runs first; this pass runs
//! last by default and later enabled passes invalidate its certificates. Cost: one
//! bounded scan of wire components and their compiled neighborhoods per compile;
//! each accepted seed/Turbo query replaces six site-power samples with one. This
//! pass runs with the other O1 transformations.

use super::{GraphPass, PassContext};
use crate::redpiler::PreparationTableStatistics;
use crate::redpiler::program_graph::{
    CallbackReceiver, CircuitNodeKind, ComparatorOverride, CompileGraph, ComponentState, DeliveryKind, DustPathPlan, EntryState, Guard, InputPort,
    PowerPort, PowerTarget, ProfileElectrical, SiteId, SiteSource, WireRelationKind,
};
use mchprs_blocks::BlockFace;
use mchprs_blocks::blocks::Block;
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::{BTreeMap, VecDeque};

/// Recognizes short wire paths whose power can be queried from a certificate.
pub(crate) struct ContractDustPaths;

impl GraphPass for ContractDustPaths {
    fn name(&self) -> &'static str {
        "contract-static-dust-paths"
    }

    fn validate_input(&self, graph: &CompileGraph) -> Result<(), String> {
        graph.validate().map_err(|error| format!("dust path input graph is invalid: {error}"))
    }

    fn run(&self, graph: &mut CompileGraph, context: &PassContext<'_>) -> Result<(), String> {
        let mut report = CandidateReport::default();
        let candidates = discover_wire_components(graph);
        report.candidates = candidates.len();
        let already_assigned: FxHashSet<_> = graph.dust_paths.iter().flat_map(|path| path.wires.iter().copied()).collect();
        let mut accepted = Vec::new();

        for (index, component) in candidates.into_iter().enumerate() {
            if context.monitor.cancelled() {
                return Ok(());
            }
            let component_len = component.len();
            if component_len > 3 {
                *report.long_candidates.entry(component_len).or_default() += 1;
                #[cfg(feature = "graph-profile")]
                context.monitor.record_dust_candidate_wires(
                    component_len,
                    &component.iter().filter_map(|node| graph.nodes[node.index()].position).collect::<Vec<_>>(),
                );
            }
            if component.iter().any(|node| already_assigned.contains(node)) {
                report.decline(DeclineReason::AlreadyAssigned);
                if component_len > 3 {
                    report.decline_long(component_len, DeclineReason::AlreadyAssigned);
                }
                continue;
            }
            match certify_candidate(graph, &context.entry, component) {
                Ok(plan) if plan.wires.len() > 3 => {
                    // Keep the current runtime bound while measuring the wider proof's coverage.
                    *report.long_otherwise_eligible.entry(plan.wires.len()).or_default() += 1;
                    report.decline(DeclineReason::TooLong);
                    report.decline_long(component_len, DeclineReason::TooLong);
                }
                Ok(plan) => accepted.push(plan),
                Err(reason) => {
                    report.decline(reason);
                    if component_len > 3 {
                        report.decline_long(component_len, reason);
                    }
                }
            }
            if index % 64 == 0 && context.monitor.cancelled() {
                return Ok(());
            }
        }
        if context.monitor.cancelled() {
            return Ok(());
        }

        report.accepted = accepted.len();
        report.covered_wires = accepted.iter().map(|path| path.wires.len()).sum();
        report.record(context.monitor);
        graph.dust_paths.extend(accepted);
        Ok(())
    }

    fn preserves_dust_paths(&self) -> bool {
        true
    }

    fn preserves_turbo_callback_indices(&self) -> bool {
        true
    }
}

#[derive(Default)]
struct CandidateReport {
    candidates: usize,
    accepted: usize,
    covered_wires: usize,
    long_candidates: BTreeMap<usize, usize>,
    long_otherwise_eligible: BTreeMap<usize, usize>,
    long_declines: BTreeMap<(usize, DeclineReason), usize>,
    declines: FxHashMap<DeclineReason, usize>,
}

impl CandidateReport {
    fn decline(&mut self, reason: DeclineReason) {
        *self.declines.entry(reason).or_default() += 1;
    }

    fn decline_long(&mut self, length: usize, reason: DeclineReason) {
        *self.long_declines.entry((length, reason)).or_default() += 1;
    }

    fn record(&self, monitor: &crate::redpiler::TaskMonitor) {
        let mut tables = vec![
            report_table("candidate groups", self.candidates),
            report_table("accepted paths", self.accepted),
            report_table("covered wires", self.covered_wires),
            report_table("long paths passing all other checks", self.long_otherwise_eligible.values().sum()),
            report_table(
                "wires in those long paths",
                self.long_otherwise_eligible.iter().map(|(length, count)| length * count).sum(),
            ),
        ];
        for (&length, &count) in &self.long_candidates {
            tables.push(report_table(format!("long candidate length {length}"), count));
        }
        for (&length, &count) in &self.long_otherwise_eligible {
            tables.push(report_table(format!("otherwise eligible length {length}"), count));
        }
        for (&(length, reason), &count) in &self.long_declines {
            tables.push(report_table(format!("length {length}: {}", reason.table()), count));
        }
        for reason in DeclineReason::ALL {
            tables.push(report_table(reason.table(), self.declines.get(&reason).copied().unwrap_or(0)));
        }
        monitor.record_preparation_tables(tables);
    }
}

fn report_table(table: impl Into<String>, entries: usize) -> PreparationTableStatistics {
    PreparationTableStatistics {
        stage: "dust contraction",
        table: table.into(),
        entries,
        capacity: entries,
        estimated_bytes: 0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum DeclineReason {
    AlreadyAssigned,
    TooLong,
    BranchOrBend,
    NearbyWire,
    DynamicContact,
    ExtraComponent,
    NoSingleInput,
    OtherPowerInput,
    ConductedInput,
    ExternalPowerReader,
    ExternalCallback,
    ObserverBoundary,
    ComparatorBoundary,
    StaleWireState,
    UnstableSides,
    UnsupportedPendingWork,
}

impl DeclineReason {
    const ALL: [Self; 16] = [
        Self::AlreadyAssigned,
        Self::TooLong,
        Self::BranchOrBend,
        Self::NearbyWire,
        Self::DynamicContact,
        Self::ExtraComponent,
        Self::NoSingleInput,
        Self::OtherPowerInput,
        Self::ConductedInput,
        Self::ExternalPowerReader,
        Self::ExternalCallback,
        Self::ObserverBoundary,
        Self::ComparatorBoundary,
        Self::StaleWireState,
        Self::UnstableSides,
        Self::UnsupportedPendingWork,
    ];

    fn table(self) -> &'static str {
        match self {
            Self::AlreadyAssigned => "declined: already assigned",
            Self::TooLong => "declined: otherwise eligible path exceeds three wires",
            Self::BranchOrBend => "declined: branch, bend, or cycle",
            Self::NearbyWire => "declined: nearby external wire",
            Self::DynamicContact => "declined: contact-dependent site",
            Self::ExtraComponent => "declined: extra electrical component",
            Self::NoSingleInput => "declined: input is not one direct electrical component",
            Self::OtherPowerInput => "declined: additional power input",
            Self::ConductedInput => "declined: conducted power input",
            Self::ExternalPowerReader => "declined: external electrical reader",
            Self::ExternalCallback => "declined: external callback boundary",
            Self::ObserverBoundary => "declined: observer watches path",
            Self::ComparatorBoundary => "declined: comparator reads path",
            Self::StaleWireState => "declined: saved wire strength is stale",
            Self::UnstableSides => "declined: saved wire sides are stale",
            Self::UnsupportedPendingWork => "declined: pending work is not a matching wire tick",
        }
    }
}

fn discover_wire_components(graph: &CompileGraph) -> Vec<Vec<crate::redpiler::program_graph::NodeId>> {
    let mut wire_at = FxHashMap::default();
    for (index, node) in graph.nodes.iter().enumerate() {
        if matches!(node.kind, CircuitNodeKind::Wire(_)) {
            if let Some(position) = node.position {
                wire_at.insert(position, crate::redpiler::program_graph::NodeId::new(index as u32));
            }
        }
    }
    let mut starts: Vec<_> = wire_at.iter().map(|(position, node)| (*position, *node)).collect();
    starts.sort_by_key(|(position, _)| (position.y, position.z, position.x));
    let mut visited = FxHashSet::default();
    let mut components = Vec::new();
    for (_, start) in starts {
        if !visited.insert(start) {
            continue;
        }
        let mut queue = VecDeque::from([start]);
        let mut component = Vec::new();
        while let Some(node) = queue.pop_front() {
            component.push(node);
            let position = graph.nodes[node.index()].position.unwrap();
            for face in [BlockFace::North, BlockFace::South, BlockFace::East, BlockFace::West] {
                if let Some(next) = wire_at.get(&position.offset(face)).copied()
                    && visited.insert(next)
                {
                    queue.push_back(next);
                }
            }
        }
        if component.len() > 1 {
            component.sort_by_key(|node| {
                let position = graph.nodes[node.index()].position.unwrap();
                (position.y, position.z, position.x)
            });
            components.push(component);
        }
    }
    components
}

fn certify_candidate(
    graph: &CompileGraph,
    entry: &EntryState,
    component: Vec<crate::redpiler::program_graph::NodeId>,
) -> Result<DustPathPlan, DeclineReason> {
    use crate::redpiler::program_graph::NodeId;

    let members: FxHashSet<_> = component.iter().copied().collect();
    let positions: FxHashMap<_, _> = component
        .iter()
        .map(|&node| (graph.nodes[node.index()].position.unwrap(), node))
        .collect();
    let mut adjacency: FxHashMap<NodeId, Vec<NodeId>> = FxHashMap::default();
    for &node in &component {
        let position = graph.nodes[node.index()].position.unwrap();
        let neighbors: Vec<_> = [BlockFace::North, BlockFace::South, BlockFace::East, BlockFace::West]
            .into_iter()
            .filter_map(|face| positions.get(&position.offset(face)).copied())
            .collect();
        adjacency.insert(node, neighbors);
    }
    let endpoints: Vec<_> = component.iter().copied().filter(|node| adjacency[node].len() == 1).collect();
    if endpoints.len() != 2 || adjacency.values().any(|neighbors| neighbors.len() > 2) {
        return Err(DeclineReason::BranchOrBend);
    }

    let mut ordered = Vec::with_capacity(component.len());
    let mut previous = None;
    let mut current = endpoints[0];
    while ordered.len() < component.len() {
        ordered.push(current);
        let next = adjacency[&current].iter().copied().find(|node| Some(*node) != previous);
        let Some(next) = next else { break };
        previous = Some(current);
        current = next;
    }
    if ordered.len() != component.len() {
        return Err(DeclineReason::BranchOrBend);
    }
    let first_position = graph.nodes[ordered[0].index()].position.unwrap();
    let second_position = graph.nodes[ordered[1].index()].position.unwrap();
    let dx = second_position.x - first_position.x;
    let dz = second_position.z - first_position.z;
    let axis = if first_position.y == second_position.y && dx.abs() == 1 && dz == 0 {
        0
    } else if first_position.y == second_position.y && dz.abs() == 1 && dx == 0 {
        1
    } else {
        return Err(DeclineReason::BranchOrBend);
    };
    for pair in ordered.windows(2) {
        let a = graph.nodes[pair[0].index()].position.unwrap();
        let b = graph.nodes[pair[1].index()].position.unwrap();
        if (axis == 0 && (a.y != b.y || (b.x - a.x).abs() != 1 || a.z != b.z)) || (axis == 1 && (a.y != b.y || (b.z - a.z).abs() != 1 || a.x != b.x))
        {
            return Err(DeclineReason::BranchOrBend);
        }
    }

    let mut neighborhood = FxHashSet::<SiteId>::default();
    for &node in &component {
        let CircuitNodeKind::Wire(index) = graph.nodes[node.index()].kind else {
            unreachable!();
        };
        let position = graph.nodes[node.index()].position.unwrap();
        let wire = &graph.wires[index];
        for &site in &wire.canonical_neighbors {
            neighborhood.insert(site);
            let site_position = graph.sites[site.index()].position;
            let direct_distance = (position.x - site_position.x).abs() + (position.y - site_position.y).abs() + (position.z - site_position.z).abs();
            match graph.sites[site.index()].source {
                SiteSource::Contact(_) => return Err(DeclineReason::DynamicContact),
                SiteSource::Component(other) if !members.contains(&other) => {
                    if matches!(graph.nodes[other.index()].kind, CircuitNodeKind::Wire(_)) {
                        return Err(DeclineReason::NearbyWire);
                    }
                }
                SiteSource::Constant(profile) => {
                    let profile = graph.profiles[profile.index()];
                    let block = Block::from_id(profile.raw_state);
                    if matches!(block, Block::RedstoneWire { .. }) {
                        return Err(DeclineReason::NearbyWire);
                    }
                    if direct_distance == 1 && (profile.electrical != ProfileElectrical::None || graph.sites[site.index()].source_strength > 0) {
                        return Err(DeclineReason::ExtraComponent);
                    }
                }
                _ => {}
            }
        }
    }

    let mut direct_component_sites = FxHashMap::default();
    for &wire_node in &component {
        let wire_position = graph.nodes[wire_node.index()].position.unwrap();
        let CircuitNodeKind::Wire(wire_index) = graph.nodes[wire_node.index()].kind else {
            unreachable!();
        };
        for &site in &graph.wires[wire_index].canonical_neighbors {
            let site_position = graph.sites[site.index()].position;
            let distance =
                (wire_position.x - site_position.x).abs() + (wire_position.y - site_position.y).abs() + (wire_position.z - site_position.z).abs();
            if distance != 1 {
                continue;
            }
            if let SiteSource::Component(node) = graph.sites[site.index()].source
                && !members.contains(&node)
            {
                direct_component_sites.insert(node, site);
            }
        }
    }
    if direct_component_sites.len() != 1 {
        return Err(DeclineReason::NoSingleInput);
    }
    let (&input_node, &input_site) = direct_component_sites.iter().next().unwrap();
    if !matches!(graph.nodes[input_node.index()].kind, CircuitNodeKind::Electrical(_)) || graph.nodes[input_node.index()].raw_state.is_none() {
        return Err(DeclineReason::NoSingleInput);
    }

    let input_position = graph.nodes[input_node.index()].position.ok_or(DeclineReason::NoSingleInput)?;
    let mut input_candidates = Vec::new();
    for &endpoint in &endpoints {
        let position = graph.nodes[endpoint.index()].position.unwrap();
        for face in BlockFace::values() {
            if position.offset(face) == input_position {
                input_candidates.push((endpoint, face));
            }
        }
    }
    if input_candidates.len() != 1 {
        return Err(DeclineReason::NoSingleInput);
    }
    let (root, input_face) = input_candidates[0];
    if ordered[0] != root {
        ordered.reverse();
    }
    certify_power_inputs(graph, &members, &neighborhood, root, input_node, input_site, input_face)?;
    certify_external_readers(graph, &members, &neighborhood)?;
    certify_callbacks(graph, &members, root, input_node)?;
    certify_wire_geometry(graph, &ordered)?;
    certify_entry_state(graph, entry, &ordered, input_node)?;

    Ok(DustPathPlan {
        input_node,
        input_site,
        input_face,
        wires: ordered.into_boxed_slice(),
    })
}

fn certify_power_inputs(
    graph: &CompileGraph,
    members: &FxHashSet<crate::redpiler::program_graph::NodeId>,
    neighborhood: &FxHashSet<SiteId>,
    root: crate::redpiler::program_graph::NodeId,
    input_node: crate::redpiler::program_graph::NodeId,
    input_site: SiteId,
    input_face: BlockFace,
) -> Result<(), DeclineReason> {
    let mut direct_inputs = 0;
    for link in &graph.power_links {
        match link.to {
            PowerTarget::Input { node, port } if members.contains(&node) => {
                if port != InputPort::Main {
                    return Err(DeclineReason::OtherPowerInput);
                }
                match graph.sites[link.from.index()].source {
                    SiteSource::Component(source) if members.contains(&source) => {
                        let a = graph.nodes[source.index()].position.unwrap();
                        let b = graph.nodes[node.index()].position.unwrap();
                        if !matches!(link.from_port, PowerPort::Output)
                            || link.attenuation != 1
                            || graph.guards.get(link.guard.index()) != Some(&Guard::Always)
                            || b.offset(link.face) != a
                            || ![BlockFace::North, BlockFace::South, BlockFace::East, BlockFace::West]
                                .into_iter()
                                .any(|face| a.offset(face) == b)
                        {
                            return Err(DeclineReason::OtherPowerInput);
                        }
                    }
                    SiteSource::Constant(_)
                        if matches!(link.from_port, PowerPort::Conducted { include_dust: false })
                            && !graph.power_links.iter().any(|candidate| {
                                candidate.to
                                    == (PowerTarget::Conductor {
                                        site: link.from,
                                        include_dust: false,
                                    })
                            }) =>
                    {
                        // The compiled support-block conductor has no incoming power links.
                    }
                    SiteSource::Component(source)
                        if source == input_node
                            && node == root
                            && link.from == input_site
                            && matches!(link.from_port, PowerPort::Weak(face) if face == input_face)
                            && link.face == input_face
                            && link.attenuation == 0
                            && graph.guards.get(link.guard.index()) == Some(&Guard::Always) =>
                    {
                        direct_inputs += 1;
                    }
                    _ => {
                        return Err(DeclineReason::OtherPowerInput);
                    }
                }
            }
            PowerTarget::Input { node, .. } => {
                if graph.sites[link.from.index()].source.is_path_node(members) {
                    let _ = node;
                    return Err(DeclineReason::ExternalPowerReader);
                }
            }
            PowerTarget::Conductor { site, .. } if neighborhood.contains(&site) && !graph.sites[link.from.index()].source.is_path_node(members) => {
                return Err(DeclineReason::ConductedInput);
            }
            PowerTarget::Conductor { .. } if graph.sites[link.from.index()].source.is_path_node(members) => {
                return Err(DeclineReason::ExternalPowerReader);
            }
            _ => {}
        }
    }
    if direct_inputs != 1 {
        return Err(DeclineReason::NoSingleInput);
    }
    Ok(())
}

fn certify_external_readers(
    graph: &CompileGraph,
    members: &FxHashSet<crate::redpiler::program_graph::NodeId>,
    neighborhood: &FxHashSet<SiteId>,
) -> Result<(), DeclineReason> {
    for node in &graph.nodes {
        match node.kind {
            CircuitNodeKind::Observer { watched, .. }
                if neighborhood.contains(&watched)
                    && matches!(graph.sites[watched.index()].source, SiteSource::Component(wire) if members.contains(&wire)) =>
            {
                return Err(DeclineReason::ObserverBoundary);
            }
            _ => {}
        }
    }
    for plan in &graph.comparator_inputs {
        let reads_path = |site: SiteId| matches!(graph.sites[site.index()].source, SiteSource::Component(node) if members.contains(&node));
        if reads_path(plan.rear)
            || plan.far_override.is_some_and(|(site, _, _)| reads_path(site))
            || [plan.direct_override, plan.far_override.map(|(_, value, _)| value)]
                .into_iter()
                .flatten()
                .any(|value| matches!(value, ComparatorOverride::LiveComponentOutput(node) if members.contains(&node)))
        {
            return Err(DeclineReason::ComparatorBoundary);
        }
    }
    Ok(())
}

fn certify_callbacks(
    graph: &CompileGraph,
    members: &FxHashSet<crate::redpiler::program_graph::NodeId>,
    root: crate::redpiler::program_graph::NodeId,
    input_node: crate::redpiler::program_graph::NodeId,
) -> Result<(), DeclineReason> {
    let mut input_updates_root = 0;
    for route in &graph.routes {
        let source_is_path = members.contains(&route.source);
        for delivery in route.deliveries.iter() {
            let source = graph.sites[delivery.target.index()].source;
            let receiver = delivery.receiver.or_else(|| match (source, delivery.kind) {
                (SiteSource::Component(node), DeliveryKind::Update { .. }) => Some(CallbackReceiver::Component(node)),
                _ => None,
            });
            if route.source == input_node
                && receiver == Some(CallbackReceiver::Component(root))
                && matches!(delivery.kind, DeliveryKind::Update { .. })
            {
                input_updates_root += 1;
            }
            match receiver {
                Some(CallbackReceiver::Component(node)) if members.contains(&node) => {
                    if !source_is_path && route.source != input_node || matches!(delivery.kind, DeliveryKind::Shape { .. }) {
                        return Err(DeclineReason::ExternalCallback);
                    }
                }
                Some(CallbackReceiver::Component(node)) if source_is_path => {
                    // The sole input may receive a no-power callback; its route stays general.
                    if node != input_node {
                        return Err(DeclineReason::ExternalCallback);
                    }
                }
                Some(CallbackReceiver::PistonBase(_) | CallbackReceiver::PistonHead(_) | CallbackReceiver::Contact(_)) if source_is_path => {
                    return Err(DeclineReason::DynamicContact);
                }
                _ => {}
            }
        }
    }
    if input_updates_root == 0 {
        return Err(DeclineReason::ExternalCallback);
    }
    Ok(())
}

fn certify_wire_geometry(graph: &CompileGraph, ordered: &[crate::redpiler::program_graph::NodeId]) -> Result<(), DeclineReason> {
    let members: FxHashSet<_> = ordered.iter().copied().collect();
    let mut profile_blocks = Vec::with_capacity(graph.profiles.len());
    for profile in &graph.profiles {
        profile_blocks.push(Block::from_id(profile.raw_state));
    }
    for relation in &graph.wire_relations {
        if !members.contains(&relation.from) {
            continue;
        }
        if matches!(relation.kind, WireRelationKind::Ascend | WireRelationKind::Descend)
            && matches!(graph.sites[relation.to.index()].source, SiteSource::Component(node) if members.contains(&node))
        {
            return Err(DeclineReason::NearbyWire);
        }
        if matches!(graph.sites[relation.to.index()].source, SiteSource::Component(node) if matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Wire(_)) && !members.contains(&node))
        {
            return Err(DeclineReason::NearbyWire);
        }
    }
    for &node in ordered {
        let CircuitNodeKind::Wire(index) = graph.nodes[node.index()].kind else {
            unreachable!();
        };
        let wire_plan = &graph.wires[index];
        let ComponentState::Wire(wire) = graph.nodes[node.index()].state else {
            unreachable!();
        };
        let mut neighbors = [Block::Air; 24];
        for (index, &site) in wire_plan.canonical_neighbors.iter().enumerate() {
            neighbors[index] = match graph.sites[site.index()].source {
                SiteSource::Constant(profile) => profile_blocks[profile.index()],
                SiteSource::Component(other) if members.contains(&other) => match graph.nodes[other.index()].state {
                    ComponentState::Wire(wire) => Block::RedstoneWire { wire },
                    _ => return Err(DeclineReason::BranchOrBend),
                },
                SiteSource::Component(other) => graph.nodes[other.index()]
                    .raw_state
                    .map(Block::from_id)
                    .ok_or(DeclineReason::ExtraComponent)?,
                SiteSource::Contact(_) => return Err(DeclineReason::DynamicContact),
            };
        }
        let raw = crate::redstone::wire::get_raw_sides_from_neighbors(wire, &neighbors);
        if crate::redstone::wire::regulate_sides(wire, raw) != wire {
            return Err(DeclineReason::UnstableSides);
        }
    }
    Ok(())
}

fn certify_entry_state(
    graph: &CompileGraph,
    entry: &EntryState,
    ordered: &[crate::redpiler::program_graph::NodeId],
    input_node: crate::redpiler::program_graph::NodeId,
) -> Result<(), DeclineReason> {
    let ComponentState::Electrical(input_state) = &graph.nodes[input_node.index()].state else {
        return Err(DeclineReason::NoSingleInput);
    };
    for (distance, &node) in ordered.iter().enumerate() {
        let ComponentState::Wire(wire) = graph.nodes[node.index()].state else {
            unreachable!();
        };
        if wire.power != input_state.output_strength.saturating_sub(distance as u8) {
            return Err(DeclineReason::StaleWireState);
        }
    }
    let wire_positions: FxHashMap<_, _> = ordered.iter().map(|node| (graph.nodes[node.index()].position.unwrap(), *node)).collect();
    for tick in &entry.pending_ticks {
        if let Some(&node) = wire_positions.get(&tick.tick.pos) {
            let expected = graph.nodes[node.index()].raw_state.map(|state| Block::from_id(state).registry_id());
            if tick.current_registry_type != expected || tick.tick.block_type.is_some_and(|block_type| Some(block_type) != expected) {
                return Err(DeclineReason::UnsupportedPendingWork);
            }
        }
    }
    Ok(())
}

trait SiteSourceExt {
    fn is_path_node(self, members: &FxHashSet<crate::redpiler::program_graph::NodeId>) -> bool;
}

impl SiteSourceExt for SiteSource {
    fn is_path_node(self, members: &FxHashSet<crate::redpiler::program_graph::NodeId>) -> bool {
        matches!(self, SiteSource::Component(node) if members.contains(&node))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plot::PlotWorld;
    use crate::redpiler::analysis::AnalysisLimits;
    use crate::redpiler::passes::rich;
    use crate::redpiler::{BackendKind, Compiler, CompilerOptions, TaskMonitor};
    use crate::world::World;
    use crate::world::storage::Chunk;
    use mchprs_blocks::block_entities::BlockEntity;
    use mchprs_blocks::blocks::{
        ComparatorMode, Lever, LeverFace, RedstoneComparator, RedstoneObserver, RedstonePiston, RedstoneWire, RedstoneWireSide,
    };
    use mchprs_blocks::{BlockDirection, BlockFace, BlockFacing, BlockPos};

    const BOUNDS: (BlockPos, BlockPos) = (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15));
    const INPUT: BlockPos = BlockPos::new(5, 20, 5);
    const FIRST_WIRE: BlockPos = BlockPos::new(6, 20, 5);

    fn path_world() -> PlotWorld {
        let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
        for x in 3..=9 {
            world.set_block(BlockPos::new(x, 19, 5), Block::Stone {});
        }
        world.set_block(
            INPUT,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, true),
            },
        );
        for distance in 0..3 {
            world.set_block(
                FIRST_WIRE + BlockPos::new(distance, 0, 0),
                Block::RedstoneWire {
                    wire: RedstoneWire {
                        power: 15 - distance as u8,
                        ..Default::default()
                    },
                },
            );
        }
        for distance in 0..3 {
            let position = FIRST_WIRE + BlockPos::new(distance, 0, 0);
            let Block::RedstoneWire { wire } = world.get_block(position) else {
                unreachable!();
            };
            world.set_block(
                position,
                Block::RedstoneWire {
                    wire: crate::redstone::wire::get_regulated_sides(wire, &world, position),
                },
            );
        }
        world
    }

    fn analog_input_path_world() -> PlotWorld {
        let mut world = path_world();
        world.set_block(
            INPUT,
            Block::RedstoneComparator {
                comparator: RedstoneComparator::new(BlockDirection::West, ComparatorMode::Compare, false),
            },
        );
        world.set_block_entity(INPUT, BlockEntity::Comparator { output_strength: 15 });
        world
    }

    fn run_pass(world: &PlotWorld, ticks: Vec<mchprs_world::TickEntry>) -> rich::PreparedGraph {
        let monitor = TaskMonitor::default();
        let mut prepared = rich::prepare(world, BOUNDS, ticks, &monitor, AnalysisLimits::default()).expect("dust fixture should parse");
        let options = CompilerOptions::default();
        ContractDustPaths
            .run(
                &mut prepared.graph,
                &PassContext {
                    monitor: &monitor,
                    options: &options,
                    entry: &prepared.entry,
                },
            )
            .unwrap();
        prepared.graph.validate().unwrap();
        prepared
    }

    fn compile_path(world: &PlotWorld, optimization_level: Option<usize>) -> Compiler {
        let mut compiler = Compiler::default();
        compiler
            .compile(
                world,
                BOUNDS,
                CompilerOptions {
                    optimization_level,
                    ..Default::default()
                },
                world.scheduler().iter_entries().collect(),
                Default::default(),
            )
            .expect("the private dust path should compile");
        assert_eq!(compiler.backend_kind(), Some(BackendKind::Graph));
        compiler
    }

    fn set_interpreted_lever(world: &mut PlotWorld, powered: bool) {
        let Block::Lever { mut lever } = world.get_block(INPUT) else {
            unreachable!();
        };
        if lever.powered == powered {
            return;
        }
        lever.powered = powered;
        world.set_block(INPUT, Block::Lever { lever });
        crate::redstone::update_surrounding_blocks(world, INPUT);
        crate::redstone::update_surrounding_blocks(world, INPUT.offset(BlockFace::Bottom));
    }

    fn assert_path_worlds_match(expected: &PlotWorld, actual: &PlotWorld) {
        for position in [
            INPUT,
            FIRST_WIRE,
            FIRST_WIRE + BlockPos::new(1, 0, 0),
            FIRST_WIRE + BlockPos::new(2, 0, 0),
        ] {
            assert_eq!(actual.get_block(position), expected.get_block(position), "{position:?}");
        }
    }

    #[test]
    fn certifies_private_lever_fed_path_and_is_deterministic() {
        let world = path_world();
        let mut first = run_pass(&world, vec![]);
        let second = run_pass(&world, vec![]);
        assert_eq!(first.graph.dust_paths.len(), 1);
        assert_eq!(first.graph.dust_paths, second.graph.dust_paths);
        assert_eq!(first.graph.dust_paths[0].wires.len(), 3);
        assert_eq!(
            first.graph.dust_paths[0].input_node,
            first
                .graph
                .nodes
                .iter()
                .position(|node| node.position == Some(INPUT))
                .map(|index| crate::redpiler::program_graph::NodeId::new(index as u32))
                .unwrap()
        );
        let expected = first.graph.dust_paths.clone();
        let monitor = TaskMonitor::default();
        let entry = first.entry.clone();
        ContractDustPaths
            .run(
                &mut first.graph,
                &PassContext {
                    monitor: &monitor,
                    options: &CompilerOptions::default(),
                    entry: &entry,
                },
            )
            .unwrap();
        assert_eq!(first.graph.dust_paths, expected);
    }

    #[test]
    fn certifies_analog_comparator_ingress_without_feedback() {
        let prepared = run_pass(&analog_input_path_world(), vec![]);
        assert_eq!(prepared.graph.dust_paths.len(), 1);
    }

    #[test]
    fn default_pipeline_export_contains_the_certified_path() {
        let world = path_world();
        let options = CompilerOptions {
            optimization_level: Some(1),
            ..Default::default()
        };
        let export =
            crate::redpiler::analysis::export_json::prepare(&world, BOUNDS, &[], &options, &TaskMonitor::default(), AnalysisLimits::default())
                .unwrap();
        let document: serde_json::Value = serde_json::from_slice(&export.to_bytes().unwrap()).unwrap();
        assert!(
            document["capabilities"]
                .as_array()
                .unwrap()
                .iter()
                .any(|capability| capability == "dust_path_certificates")
        );
        let paths = document["graph"]["dust_paths"].as_array().unwrap();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0]["wires"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn turbo_indices_and_dust_metadata_compose_in_either_order_after_opt01() {
        use crate::redpiler::passes::{
            PassEntry, PassPipeline, index_turbo_callbacks::IndexTurboCallbacks, run_graph_pipeline, simplify_guards::GuardAndUpdateReduction,
        };

        let compile_order = |index_first| {
            let monitor = TaskMonitor::default();
            let options = CompilerOptions {
                optimization_level: Some(1),
                ..Default::default()
            };
            let mut prepared = rich::prepare(&path_world(), BOUNDS, vec![], &monitor, AnalysisLimits::default()).unwrap();
            let mut entries = vec![PassEntry {
                pass: Box::new(GuardAndUpdateReduction),
                enabled: true,
            }];
            if index_first {
                entries.push(PassEntry {
                    pass: Box::new(IndexTurboCallbacks),
                    enabled: true,
                });
                entries.push(PassEntry {
                    pass: Box::new(ContractDustPaths),
                    enabled: true,
                });
            } else {
                entries.push(PassEntry {
                    pass: Box::new(ContractDustPaths),
                    enabled: true,
                });
                entries.push(PassEntry {
                    pass: Box::new(IndexTurboCallbacks),
                    enabled: true,
                });
            }
            run_graph_pipeline(&mut prepared.graph, &options, &prepared.entry, &PassPipeline::new(entries), &monitor).unwrap();
            prepared.graph
        };

        let index_first = compile_order(true);
        let dust_first = compile_order(false);
        for graph in [&index_first, &dust_first] {
            assert_eq!(graph.dust_paths.len(), 1);
            assert_eq!(graph.turbo_callback_indices.len(), graph.wires.len());
            assert!(graph.turbo_callback_indices.iter().any(Option::is_some));
            graph.validate().unwrap();
        }
        assert_eq!(index_first.dust_paths, dust_first.dust_paths);
        assert_eq!(index_first.turbo_callback_indices, dust_first.turbo_callback_indices);
    }

    #[test]
    fn declines_branch_second_source_and_stale_saved_strength() {
        let mut branch = path_world();
        branch.set_block(
            BlockPos::new(7, 20, 6),
            Block::RedstoneWire {
                wire: RedstoneWire::default(),
            },
        );
        branch.set_block(BlockPos::new(7, 19, 6), Block::Stone {});
        let branch = run_pass(&branch, vec![]);
        assert!(branch.graph.dust_paths.is_empty());

        let mut second_source = path_world();
        second_source.set_block(BlockPos::new(7, 19, 6), Block::Stone {});
        second_source.set_block(
            BlockPos::new(7, 20, 6),
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
            },
        );
        let second_source = run_pass(&second_source, vec![]);
        assert!(second_source.graph.dust_paths.is_empty());

        let mut stale = path_world();
        stale.set_block(
            FIRST_WIRE + BlockPos::new(1, 0, 0),
            Block::RedstoneWire {
                wire: RedstoneWire {
                    power: 4,
                    ..Default::default()
                },
            },
        );
        let stale = run_pass(&stale, vec![]);
        assert!(stale.graph.dust_paths.is_empty());

        let mut stale_sides = path_world();
        stale_sides.set_block(
            FIRST_WIRE + BlockPos::new(1, 0, 0),
            Block::RedstoneWire {
                wire: RedstoneWire {
                    north: RedstoneWireSide::Side,
                    ..Default::default()
                },
            },
        );
        let stale_sides = run_pass(&stale_sides, vec![]);
        assert!(stale_sides.graph.dust_paths.is_empty());

        let mut conducted = path_world();
        conducted.set_block(BlockPos::new(5, 19, 5), Block::RedstoneBlock);
        let conducted = run_pass(&conducted, vec![]);
        assert!(conducted.graph.dust_paths.is_empty());

        let mut watched = path_world();
        watched.set_block(
            BlockPos::new(7, 20, 6),
            Block::Observer {
                observer: RedstoneObserver {
                    facing: BlockFacing::North,
                    powered: false,
                },
            },
        );
        watched.set_block(BlockPos::new(7, 19, 6), Block::Stone {});
        let watched = run_pass(&watched, vec![]);
        assert!(watched.graph.dust_paths.is_empty());

        let mut contact = path_world();
        contact.set_block(
            BlockPos::new(7, 20, 6),
            Block::Piston {
                piston: RedstonePiston {
                    facing: BlockFacing::South,
                    sticky: false,
                    extended: false,
                },
            },
        );
        let contact = run_pass(&contact, vec![]);
        assert!(contact.graph.dust_paths.is_empty());
    }

    #[test]
    fn imported_wire_tick_deadline_is_preserved_and_type_mismatch_declines() {
        let world = path_world();
        let tick = mchprs_world::TickEntry {
            ticks_left: 3,
            tick_priority: mchprs_world::TickPriority::Normal,
            pos: FIRST_WIRE + BlockPos::new(1, 0, 0),
            block_type: Some(
                Block::RedstoneWire {
                    wire: RedstoneWire::default(),
                }
                .registry_id(),
            ),
        };
        let prepared = run_pass(&world, vec![tick.clone()]);
        assert_eq!(prepared.graph.dust_paths.len(), 1);
        assert_eq!(prepared.entry.pending_ticks.len(), 1);
        assert_eq!(prepared.entry.pending_ticks[0].tick.ticks_left, 3);
        assert_eq!(prepared.entry.pending_ticks[0].tick.tick_priority, mchprs_world::TickPriority::Normal);

        let mut mismatched = tick;
        mismatched.block_type = Some(Block::Stone {}.registry_id());
        let prepared = run_pass(&world, vec![mismatched]);
        assert!(prepared.graph.dust_paths.is_empty());
        assert_eq!(prepared.entry.pending_ticks.len(), 1);
        assert_eq!(prepared.entry.pending_ticks[0].tick.ticks_left, 3);
    }

    #[test]
    fn specialized_queries_match_general_graph_and_interpreter_across_input_edges_and_reset() {
        let mut interpreted = path_world();
        let mut general_world = path_world();
        let mut disabled_world = path_world();
        let mut specialized_world = path_world();
        let mut general = compile_path(&general_world, Some(0));
        let mut disabled = compile_path(&disabled_world, Some(0));
        let mut specialized = compile_path(&specialized_world, None);
        let transformations = &specialized.stats().unwrap().graph.transformations;
        assert_eq!(
            transformations.iter().map(|pass| pass.name).collect::<Vec<_>>(),
            ["guard-and-update-reduction", "index-turbo-callbacks", "contract-static-dust-paths"]
        );
        let disabled_dust_pass = disabled
            .stats()
            .unwrap()
            .graph
            .transformations
            .iter()
            .find(|pass| pass.name == "contract-static-dust-paths")
            .expect("the O1 dust pass is visible in pipeline statistics");
        assert!(!disabled_dust_pass.enabled);
        let disabled_index_pass = disabled
            .stats()
            .unwrap()
            .graph
            .transformations
            .iter()
            .find(|pass| pass.name == "index-turbo-callbacks")
            .expect("the O1 index entry is visible in pipeline statistics");
        assert!(!disabled_index_pass.enabled);

        general.reset(&mut general_world, BOUNDS);
        disabled.reset(&mut disabled_world, BOUNDS);
        specialized.reset(&mut specialized_world, BOUNDS);
        assert_path_worlds_match(&interpreted, &general_world);
        assert_path_worlds_match(&interpreted, &disabled_world);
        assert_path_worlds_match(&interpreted, &specialized_world);
        general = compile_path(&general_world, Some(0));
        disabled = compile_path(&disabled_world, Some(0));
        specialized = compile_path(&specialized_world, None);

        let mut previous_power = true;
        let mut history = vec![false, true, false, false, true];
        let mut seed = 0x6d63_7a91_0f22_4bc5u64;
        for _ in 0..8 {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            history.push(seed >> 63 != 0);
        }
        for powered in history {
            set_interpreted_lever(&mut interpreted, powered);
            if powered != previous_power {
                general.on_use_block(INPUT);
                disabled.on_use_block(INPUT);
                specialized.on_use_block(INPUT);
            }
            previous_power = powered;
            for _ in 0..=1 {
                interpreted.tick_interpreted();
                general.tick_with_world(&mut general_world).unwrap();
                disabled.tick_with_world(&mut disabled_world).unwrap();
                specialized.tick_with_world(&mut specialized_world).unwrap();
                general.flush(&mut general_world);
                disabled.flush(&mut disabled_world);
                specialized.flush(&mut specialized_world);
                assert_path_worlds_match(&interpreted, &general_world);
                assert_path_worlds_match(&interpreted, &disabled_world);
                assert_path_worlds_match(&interpreted, &specialized_world);
            }
        }
        assert!(specialized.graph_execution_counts().unwrap().dust_path_power_samples_elided > 0);
        assert_eq!(general.graph_execution_counts().unwrap().dust_path_power_samples_elided, 0);
        assert_eq!(disabled.graph_execution_counts().unwrap().dust_path_power_samples_elided, 0);

        general.reset(&mut general_world, BOUNDS);
        disabled.reset(&mut disabled_world, BOUNDS);
        specialized.reset(&mut specialized_world, BOUNDS);
        assert_path_worlds_match(&interpreted, &general_world);
        assert_path_worlds_match(&interpreted, &disabled_world);
        assert_path_worlds_match(&interpreted, &specialized_world);
        for _ in 0..4 {
            interpreted.tick_interpreted();
            general_world.tick_interpreted();
            disabled_world.tick_interpreted();
            specialized_world.tick_interpreted();
            assert_path_worlds_match(&interpreted, &general_world);
            assert_path_worlds_match(&interpreted, &disabled_world);
            assert_path_worlds_match(&interpreted, &specialized_world);
        }

        let mut resumed = compile_path(&specialized_world, None);
        let was_powered = matches!(specialized_world.get_block(INPUT), Block::Lever { lever } if lever.powered);
        set_interpreted_lever(&mut interpreted, !was_powered);
        resumed.on_use_block(INPUT);
        interpreted.tick_interpreted();
        resumed.tick_with_world(&mut specialized_world).unwrap();
        resumed.flush(&mut specialized_world);
        assert_path_worlds_match(&interpreted, &specialized_world);
    }

    #[cfg(feature = "graph-profile")]
    #[test]
    fn long_candidate_profile_counts_general_neighbor_power_samples() {
        let mut world = path_world();
        for distance in 0..4 {
            world.set_block(
                FIRST_WIRE + BlockPos::new(distance, 0, 0),
                Block::RedstoneWire {
                    wire: RedstoneWire {
                        power: 15 - distance as u8,
                        ..Default::default()
                    },
                },
            );
        }
        for distance in 0..4 {
            let position = FIRST_WIRE + BlockPos::new(distance, 0, 0);
            let Block::RedstoneWire { wire } = world.get_block(position) else {
                unreachable!();
            };
            world.set_block(
                position,
                Block::RedstoneWire {
                    wire: crate::redstone::wire::get_regulated_sides(wire, &world, position),
                },
            );
        }
        let mut compiler = Compiler::default();
        compiler
            .compile(
                &world,
                BOUNDS,
                CompilerOptions {
                    optimization_level: Some(1),
                    ..Default::default()
                },
                vec![],
                Default::default(),
            )
            .unwrap();
        let tables = &compiler.stats().unwrap().graph.preparation_tables;
        assert!(
            tables
                .iter()
                .any(|table| { table.stage == "dust contraction" && table.table == "otherwise eligible length 4" && table.entries == 1 }),
            "{tables:?}"
        );

        set_interpreted_lever(&mut world, false);
        compiler.on_use_block(INPUT);
        compiler.tick_with_world(&mut world).unwrap();
        let samples = compiler.dust_candidate_power_samples().unwrap();
        assert!(samples.iter().any(|(length, count)| *length == 4 && *count > 0));
        assert_eq!(compiler.graph_execution_counts().unwrap().dust_path_power_samples_elided, 0);
    }
}
