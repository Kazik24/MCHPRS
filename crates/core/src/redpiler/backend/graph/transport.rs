//! Dense closed-wire transport: original captures, writes, heading and FIFO sample cuts.
//! Boundary handlers preserve emissions during a walk; delayed effects retain their order.
use super::*;

const ABSENT: u32 = u32::MAX;
const MAX_TRANSCRIPT_OPS: usize = 65_536;

#[derive(Default)]
struct TransportRecording {
    ops: Vec<SignalOp>,
    selections: u32,
    queued_selections: [VecDeque<u32>; 3],
    work: usize,
}

#[derive(Default)]
struct TransportSite {
    /// First-discovery snapshot; wire writes update only their captured root, as in Turbo.
    observed: Option<ObservedSite>,
    /// Member-wire power captured at discovery; full properties are needed only by dynamic admission.
    wire_power: Option<u8>,
    /// Only wire dispatch marks a site visited, determining the original arrival heading.
    visited: bool,
    /// Inherited horizontal bias is relevant only to a subsequently visited wire.
    bias: (i8, i8),
    /// Last enqueued layer, including suppressed deliveries.
    layer: u32,
    /// Heading is fixed on this wire's first neighborhood discovery.
    heading: Option<u8>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum RegionState {
    #[default]
    Unknown,
    Exact,
    Inert,
}

#[derive(Default)]
pub(super) struct ValueRegionCache {
    valid: Vec<bool>,
    revisions: Vec<u64>,
    wires: Vec<RedstoneWire>,
    groups: Vec<u8>,
    arcs: Vec<bool>,
}

impl ValueRegionCache {
    pub(super) fn invalidate(&mut self, region: u32) {
        if let Some(valid) = self.valid.get_mut(region as usize) {
            *valid = false;
            self.revisions[region as usize] = self.revisions[region as usize].wrapping_add(1);
        }
    }

    fn prepare(&mut self, values: &InstantValueRegionNet) {
        if self.wires.len() == values.wire_region.len() {
            return;
        }
        self.valid.resize(values.regions.len(), false);
        self.revisions.resize(values.regions.len(), 0);
        self.wires.resize(values.wire_region.len(), RedstoneWire::default());
        self.groups.resize(values.drive_groups.len(), 0);
        self.arcs.resize(values.conditional_arcs.len(), false);
    }
}

#[derive(Default)]
pub(super) struct TransportScratch {
    learning: Option<super::transitions::TransitionRecording>,
    value_plan: Option<std::sync::Arc<InstantValueRegionNet>>,
    net_index: usize,
    regions: Vec<RegionState>,
    touched_regions: Vec<u32>,
    region_wires: Vec<RedstoneWire>,
    query_values: Vec<u8>,
    sampled_queries: Vec<u32>,
    selector_values: Vec<bool>,
    selector_ready: Vec<bool>,
    sampled_selectors: Vec<u32>,
    solved_values: Vec<u8>,
    region_solves: u64,
    cache_hits: u64,
    inert_visits: u64,
    compact_captures: u64,
    /// Allocated only by the cold transition compiler; runtime instantiations erase recording branches.
    recording: Option<Box<TransportRecording>>,
    /// Enqueue-time decisions for a specialized straight-line transition.
    selected: Vec<bool>,
    /// Local site state is allocated once and reset only for discovered sites.
    sites: Vec<TransportSite>,
    /// Discovered local IDs identify the state that needs clearing after an episode.
    touched: Vec<u32>,
    /// Stable no-dust drives, sampled lazily; MAX means not yet sampled this episode.
    drives: Vec<u8>,
    /// Sampled wire IDs identify the cached drives to clear.
    sampled: Vec<u32>,
    /// Exact three rotating Turbo queues; each item retains its source route.
    queues: [VecDeque<QueuedWireSite>; 3],
    /// Current native propagation layer, separate from game ticks.
    layer: u32,
}

impl TransportScratch {
    fn clear(&mut self) {
        self.learning = None;
        for region in self.touched_regions.drain(..) {
            self.regions[region as usize] = RegionState::Unknown;
        }
        self.value_plan = None;
        self.region_solves = 0;
        self.cache_hits = 0;
        self.inert_visits = 0;
        self.compact_captures = 0;
        for query in self.sampled_queries.drain(..) {
            self.query_values[query as usize] = u8::MAX;
        }
        for selector in self.sampled_selectors.drain(..) {
            self.selector_ready[selector as usize] = false;
        }
        for slot in self.touched.drain(..) {
            self.sites[slot as usize] = TransportSite::default();
        }
        for wire in self.sampled.drain(..) {
            self.drives[wire as usize] = u8::MAX;
        }
        for queue in &mut self.queues {
            queue.clear();
        }
        self.layer = 0;
        self.selected.clear();
    }
}

impl GraphBackend {
    pub(super) fn transport_entry_admitted(&self, net: u32) -> bool {
        self.canonical_transport_repeaters
            || self.program.instant_transport_net(net).canonical_repeaters.iter().all(|&node| {
                let state = self.node_state(node);
                state.output_strength == 15 * u8::from(state.powered)
            })
    }

    pub(super) fn run_instant_transport(&mut self, net: u32, root: u32, drive: u8) -> Result<(), GraphExecutionError> {
        let net_index = net as usize;
        let values = self.program.instant_value_region_net(net).cloned();
        let net = self.program.instant_transport_net(net).clone();
        let visits_before = self.counts.instant_transport_wire_visits;
        self.counts.instant_transport_invocations += 1;
        let learning = match self.transition_entry(net_index as u32, root, &net) {
            super::transitions::TransitionEntry::Replay(program) => return self.run_transition(&net, &program),
            super::transitions::TransitionEntry::Learn(recording) => Some(recording),
            super::transitions::TransitionEntry::Exact => None,
        };
        let values = if learning.is_some() { None } else { values };
        let mut scratch = self.transport_scratch.pop().unwrap_or_default();
        scratch.learning = learning;
        scratch.net_index = net_index;
        if let Some(values) = &values {
            scratch
                .regions
                .resize(scratch.regions.len().max(values.regions.len()), RegionState::Unknown);
            scratch
                .region_wires
                .resize(scratch.region_wires.len().max(net.wires.len()), RedstoneWire::default());
            let queries = self.program.instant_drive_program().queries.len();
            scratch.query_values.resize(scratch.query_values.len().max(queries), u8::MAX);
            scratch
                .selector_values
                .resize(scratch.selector_values.len().max(values.selectors.len()), false);
            scratch
                .selector_ready
                .resize(scratch.selector_ready.len().max(values.selectors.len()), false);
            if self.value_region_cache.len() <= net_index {
                self.value_region_cache.resize_with(net_index + 1, ValueRegionCache::default);
            }
            self.value_region_cache[net_index].prepare(values);
        }
        scratch.value_plan = values;
        scratch
            .sites
            .resize_with(scratch.sites.len().max(net.sites.len()), TransportSite::default);
        scratch.drives.resize(scratch.drives.len().max(net.wires.len()), u8::MAX);
        scratch.drives[root as usize] = drive;
        scratch.sampled.push(root);
        let result = self.execute_transport::<false>(&net, root, &mut scratch, None);
        if let Some(recording) = scratch.learning.take()
            && result.is_ok()
        {
            self.publish_transition(
                net_index as u32,
                root,
                &net,
                recording,
                &scratch.sampled,
                &scratch.drives,
                self.counts.instant_transport_wire_visits - visits_before,
            );
        }
        self.counts.instant_value_region_solves += scratch.region_solves;
        self.counts.instant_value_region_cache_hits += scratch.cache_hits;
        self.counts.instant_value_region_inert_visits += scratch.inert_visits;
        self.counts.instant_transport_compact_captures += scratch.compact_captures;
        #[cfg(test)]
        if self.test_recorder.value_region_census_enabled {
            let visits = self.counts.instant_transport_wire_visits - visits_before;
            if visits > 0 && visits == scratch.inert_visits {
                let census = &mut self.test_recorder.value_region_census;
                census.all_inert_invocations += 1;
                census.all_inert_visits += visits;
            }
        }
        #[cfg(test)]
        if self.test_recorder.callback_census_enabled {
            let census = &mut self.test_recorder.callback_census;
            let index = net_index as usize;
            let new_len = census.transport_by_net.len().max(index + 1);
            census.transport_by_net.resize(new_len, TransportNetCensus::default());
            census.transport_by_net[index].invocations += 1;
            census.transport_by_net[index].wire_visits += self.counts.instant_transport_wire_visits - visits_before;
        }
        // Errors cannot leave captures or deliveries in reusable scratch.
        scratch.clear();
        self.transport_scratch.push(scratch);
        result
    }

    fn capture_transport_site<const RECORD: bool>(&mut self, net: &TransportNet, local: u32, scratch: &mut TransportScratch) {
        if local == ABSENT || scratch.sites[local as usize].observed.is_some() || scratch.sites[local as usize].wire_power.is_some() {
            return;
        }
        let site = net.sites[local as usize];
        let wire = net.site_wire[local as usize];
        if let Some(recording) = scratch.learning.as_mut() {
            if wire != ABSENT {
                recording.wire(wire, self.wire_state(net.wires[wire as usize].node).power);
            } else {
                recording.push(SignalOp::Capture(local));
            }
        }
        let inert = !RECORD && wire != ABSENT && self.transport_region_inert(net, wire, scratch);
        if !RECORD
            && wire != ABSENT
            && scratch
                .value_plan
                .as_ref()
                .is_some_and(|values| values.compact_wire_captures.get(wire as usize) == Some(&true))
        {
            scratch.sites[local as usize].wire_power = Some(if inert {
                self.value_region_cache[scratch.net_index].wires[wire as usize].power
            } else {
                self.wire_state(net.wires[wire as usize].node).power
            });
            scratch.compact_captures += 1;
            scratch.touched.push(local);
            return;
        }
        scratch.sites[local as usize].observed = Some(if inert {
            ObservedSite {
                profile: None,
                receiver: Some(CallbackReceiver::Component(net.wires[wire as usize].node)),
                properties: ObservedProperties::Wire(self.value_region_cache[scratch.net_index].wires[wire as usize]),
            }
        } else {
            self.observe_wire_site(site).expect("validated transport site")
        });
        if let ObservedProperties::Wire(wire) = scratch.sites[local as usize].observed.as_ref().unwrap().properties {
            scratch.sites[local as usize].wire_power = Some(wire.power);
        }
        scratch.touched.push(local);
        if RECORD {
            scratch.recording.as_mut().unwrap().ops.push(SignalOp::Capture(local));
        }
    }

    /// Certify before the first member capture; other electrical regions cannot change these wires.
    fn transport_region_inert(&mut self, net: &TransportNet, wire: u32, scratch: &mut TransportScratch) -> bool {
        let Some(values) = scratch.value_plan.as_ref() else {
            return false;
        };
        let region_id = values.wire_region[wire as usize];
        if region_id == ABSENT {
            return false;
        }
        match scratch.regions[region_id as usize] {
            RegionState::Inert => return true,
            RegionState::Exact => return false,
            RegionState::Unknown => {}
        }
        scratch.touched_regions.push(region_id);
        scratch.regions[region_id as usize] = RegionState::Exact;
        let region = &values.regions[region_id as usize];
        for group in &values.drive_groups[region.drive_groups.start as usize..region.drive_groups.end as usize] {
            if scratch.query_values[group.query as usize] == u8::MAX {
                scratch.query_values[group.query as usize] = self.instant_drive_query_power(group.query);
                scratch.sampled_queries.push(group.query);
            }
        }
        for arc in &values.conditional_arcs[region.conditional_arcs.start as usize..region.conditional_arcs.end as usize] {
            if scratch.selector_ready[arc.selector as usize] {
                continue;
            }
            let selector = &values.selectors[arc.selector as usize];
            let horizontal = self.live_site(selector.horizontal_site);
            scratch.selector_values[arc.selector as usize] = if let Some(up) = selector.ascent_up_site {
                horizontal.solid && !horizontal.transparent && !self.live_site(up).solid
            } else {
                !horizontal.solid
            };
            scratch.selector_ready[arc.selector as usize] = true;
            scratch.sampled_selectors.push(arc.selector);
        }
        let cache = &self.value_region_cache[scratch.net_index];
        let groups = region.drive_groups.start as usize..region.drive_groups.end as usize;
        let arcs = region.conditional_arcs.start as usize..region.conditional_arcs.end as usize;
        if cache.valid[region_id as usize]
            && groups
                .clone()
                .all(|index| cache.groups[index] == scratch.query_values[values.drive_groups[index].query as usize])
            && arcs
                .clone()
                .all(|index| cache.arcs[index] == scratch.selector_values[values.conditional_arcs[index].selector as usize])
        {
            scratch.cache_hits += 1;
            scratch.regions[region_id as usize] = RegionState::Inert;
            return true;
        }
        let revision = cache.revisions[region_id as usize];
        let wires = &values.region_wires[region.wires.start as usize..region.wires.end as usize];
        scratch.solved_values.resize(wires.len(), 0);
        scratch.region_solves += 1;
        if !values.solve_region(region_id, &scratch.query_values, &scratch.selector_values, &mut scratch.solved_values) {
            return false;
        }
        for (&wire, &power) in wires.iter().zip(&scratch.solved_values) {
            let state = self.wire_state(net.wires[wire as usize].node);
            if state.power != power {
                return false;
            }
            scratch.region_wires[wire as usize] = state;
        }
        let cache = &mut self.value_region_cache[scratch.net_index];
        if cache.revisions[region_id as usize] != revision {
            return false;
        }
        for &wire in wires {
            cache.wires[wire as usize] = scratch.region_wires[wire as usize];
        }
        for index in groups {
            cache.groups[index] = scratch.query_values[values.drive_groups[index].query as usize];
        }
        for index in arcs {
            cache.arcs[index] = scratch.selector_values[values.conditional_arcs[index].selector as usize];
        }
        cache.valid[region_id as usize] = true;
        // A fixed point cannot move while the transport certificate holds emissions and geometry stable.
        scratch.regions[region_id as usize] = RegionState::Inert;
        true
    }

    fn identify_transport_neighbors<const RECORD: bool>(&mut self, net: &TransportNet, wire: u32, scratch: &mut TransportScratch) -> usize {
        let plan = &net.wires[wire as usize];
        self.capture_transport_site::<RECORD>(net, plan.site, scratch);
        if let Some(heading) = scratch.sites[plan.site as usize].heading {
            return heading as usize;
        }
        for &site in &plan.neighbors {
            self.capture_transport_site::<RECORD>(net, site, scratch);
        }
        let visited = |slot: usize| {
            let local = plan.neighbors[slot];
            local != ABSENT && scratch.sites[local as usize].visited
        };
        let mut x = i8::from(visited(0) || visited(7) || visited(8)) - i8::from(visited(1) || visited(12) || visited(13));
        let mut z = i8::from(visited(4) || visited(17) || visited(20)) - i8::from(visited(5) || visited(18) || visited(21));
        let (old_x, old_z) = scratch.sites[plan.site as usize].bias;
        if x == 0 && z == 0 {
            x = old_x;
            z = old_z;
        } else if x != 0 && z != 0 {
            if old_x != 0 {
                z = 0;
            }
            if old_z != 0 {
                x = 0;
            }
        }
        let heading = crate::redstone::wire::compute_heading(x.into(), z.into());
        for &local in &plan.neighbors {
            if local != ABSENT {
                scratch.sites[local as usize].bias = (x, z);
            }
        }
        scratch.sites[plan.site as usize].heading = Some(heading as u8);
        heading
    }

    fn propagate_transport<const RECORD: bool>(&mut self, net: &TransportNet, wire: u32, scratch: &mut TransportScratch) {
        let plan = &net.wires[wire as usize];
        let heading = self.identify_transport_neighbors::<RECORD>(net, wire, scratch);
        let CircuitNodeKind::Wire(index) = self.program.nodes()[plan.node.index()].kind else {
            unreachable!()
        };
        let order = &crate::redstone::wire::TURBO_ORDER[heading];
        for (slots, delay) in [(order.as_slice(), 1usize), (&order[..4], 2)] {
            for &slot in slots {
                let local = plan.neighbors[slot];
                if local == ABSENT {
                    continue;
                }
                let state = &mut scratch.sites[local as usize];
                let layer = scratch.layer + delay as u32;
                if layer <= state.layer {
                    continue;
                }
                // Layer marking precedes admission, including a rejected callback.
                state.layer = layer;
                let site = net.sites[local as usize];
                let mut selection = ABSENT;
                let mut admitted = if RECORD && net.site_wire[local as usize] == ABSENT {
                    // External admission cannot affect interior control. Compile a conditional
                    // delivery, retaining the guard's original enqueue cut and all layer marks.
                    let recorder = scratch.recording.as_mut().unwrap();
                    selection = recorder.selections;
                    recorder.selections += 1;
                    recorder.ops.push(SignalOp::Select {
                        wire,
                        slot: slot as u8,
                        selection,
                    });
                    true
                } else {
                    self.wire_route_admitted(index, plan.route, slot, site, state.observed.as_ref())
                };
                if !RECORD
                    && net.site_wire[local as usize] == ABSENT
                    && let Some(recording) = scratch.learning.as_mut()
                {
                    (selection, admitted) = recording.select(wire, slot as u8, admitted);
                }
                if admitted {
                    scratch.queues[delay].push_back(QueuedWireSite {
                        slot: local,
                        route: plan.route,
                    });
                    if RECORD {
                        scratch.recording.as_mut().unwrap().queued_selections[delay].push_back(selection);
                    }
                    if let Some(recording) = scratch.learning.as_mut() {
                        recording.queues[delay].push_back(selection);
                    }
                }
            }
        }
    }

    fn execute_transport<const RECORD: bool>(
        &mut self,
        net: &TransportNet,
        root: u32,
        scratch: &mut TransportScratch,
        monitor: Option<&TaskMonitor>,
    ) -> Result<(), GraphExecutionError> {
        let root_site = net.wires[root as usize].site;
        self.capture_transport_site::<RECORD>(net, root_site, scratch);
        scratch.sites[root_site as usize].visited = true;
        self.propagate_transport::<RECORD>(net, root, scratch);
        scratch.queues.rotate_left(1);
        if let Some(recording) = scratch.learning.as_mut() {
            recording.queues.rotate_left(1);
        }
        if RECORD {
            scratch.recording.as_mut().unwrap().queued_selections.rotate_left(1);
        }
        scratch.layer = 1;
        while !scratch.queues[0].is_empty() || !scratch.queues[1].is_empty() {
            let count = scratch.queues[0].len();
            for _ in 0..count {
                let queued = scratch.queues[0].pop_front().unwrap();
                let selection = if RECORD {
                    let recorder = scratch.recording.as_mut().unwrap();
                    recorder.work += 1;
                    if recorder.work > MAX_TRANSCRIPT_OPS || recorder.ops.len() > MAX_TRANSCRIPT_OPS || monitor.is_some_and(TaskMonitor::cancelled) {
                        return Err(GraphExecutionError::UnsupportedComponent {
                            position: None,
                            kind: "signal transition proof budget or cancellation",
                        });
                    }
                    recorder.queued_selections[0].pop_front().unwrap()
                } else if let Some(recording) = scratch.learning.as_mut() {
                    recording.queues[0].pop_front().unwrap()
                } else {
                    ABSENT
                };
                let site = net.sites[queued.slot as usize];
                let wire = net.site_wire[queued.slot as usize];
                if wire != ABSENT {
                    self.counts.instant_transport_wire_visits += 1;
                    let node = net.wires[wire as usize].node;
                    self.record_turbo_delivery(site, CallbackReceiver::Component(node), scratch.layer);
                    self.evaluate_transport_wire::<RECORD>(net, wire, scratch);
                } else {
                    if RECORD {
                        // No acceptance outcome or pending membership is frozen into the program.
                        scratch.recording.as_mut().unwrap().ops.push(SignalOp::Call {
                            site: queued.slot,
                            selection,
                            layer: scratch.layer,
                            route: queued.route,
                        });
                    } else {
                        let admitted = if let Some(recording) = scratch.learning.as_mut() {
                            recording.push(SignalOp::Call {
                                site: queued.slot,
                                selection,
                                layer: scratch.layer,
                                route: queued.route,
                            });
                            recording.admitted(selection)
                        } else {
                            true
                        };
                        if admitted {
                            let observed = scratch.sites[queued.slot as usize].observed.as_ref().unwrap();
                            self.dispatch_turbo_nonwire(site, observed, scratch.layer, Some(queued.route))?;
                        }
                    }
                }
            }
            scratch.queues.rotate_left(1);
            scratch.queues[2].clear();
            if let Some(recording) = scratch.learning.as_mut() {
                recording.queues.rotate_left(1);
                recording.queues[2].clear();
            }
            if RECORD {
                let recorder = scratch.recording.as_mut().unwrap();
                recorder.queued_selections.rotate_left(1);
                recorder.queued_selections[2].clear();
            }
            scratch.layer += 1;
        }
        Ok(())
    }

    fn evaluate_transport_wire<const RECORD: bool>(&mut self, net: &TransportNet, wire: u32, scratch: &mut TransportScratch) {
        let plan = &net.wires[wire as usize];
        self.capture_transport_site::<RECORD>(net, plan.site, scratch);
        scratch.sites[plan.site as usize].visited = true;
        self.identify_transport_neighbors::<RECORD>(net, wire, scratch);
        #[cfg(test)]
        if !RECORD
            && self.test_recorder.value_region_census_enabled
            && let Some(values) = &scratch.value_plan
        {
            let region = values.wire_region[wire as usize];
            if region != ABSENT {
                self.record_value_region_visit(scratch.net_index, region, scratch.regions[region as usize] == RegionState::Inert);
            }
        }
        if !RECORD && let Some(values) = &scratch.value_plan {
            let region = values.wire_region[wire as usize];
            if region != ABSENT && scratch.regions[region as usize] == RegionState::Inert {
                scratch.inert_visits += 1;
                return;
            }
        }
        let held_power = scratch.sites[plan.site as usize].wire_power.expect("captured member wire");
        if let Some(recording) = scratch.learning.as_mut() {
            recording.read_wire(wire);
        }
        let mut drive = scratch.drives[wire as usize];
        if drive == u8::MAX {
            drive = self.transport_block_power(plan.node);
            scratch.drives[wire as usize] = drive;
            scratch.sampled.push(wire);
        }
        let mut maximum = 0;
        if drive < 15 {
            let mut inputs = WireInputs {
                direct: plan.inputs,
                conditional: plan.conditional_inputs,
            }
            .selected(true, |slot| {
                let local = plan.neighbors[slot] as usize;
                if net.site_wire[local] != ABSENT {
                    let wire = Block::RedstoneWire {
                        wire: RedstoneWire::default(),
                    };
                    return (wire.is_solid(), wire.is_transparent());
                }
                let observed = scratch.sites[local].observed.as_ref().unwrap();
                let view = self
                    .resolve_site_view(net.sites[local], Some(observed))
                    .expect("validated transport geometry");
                (view.solid, view.transparent)
            });
            if plan.conditional_inputs != 0
                && let Some(recording) = scratch.learning.as_mut()
            {
                recording.geometry(wire, inputs);
            }
            while inputs != 0 {
                let slot = inputs.trailing_zeros() as usize;
                inputs &= inputs - 1;
                let local = plan.neighbors[slot];
                if let Some(recording) = scratch.learning.as_mut() {
                    recording.read_wire(net.site_wire[local as usize]);
                }
                maximum = maximum.max(scratch.sites[local as usize].wire_power.expect("captured member input"));
            }
        }
        let power = drive.max(maximum.saturating_sub(1));
        if power == held_power {
            return;
        }
        // Side properties cannot change inside an admitted update-only walk.
        let mut state = self.wire_state(plan.node);
        #[cfg(test)]
        let changed_wire = state.power != power;
        state.power = power;
        scratch.sites[plan.site as usize].wire_power = Some(power);
        if let Some(observed) = scratch.sites[plan.site as usize].observed.as_mut() {
            observed.properties = ObservedProperties::Wire(state);
        }
        self.set_wire(plan.node, state);
        if let Some(recording) = scratch.learning.as_mut() {
            recording.push(SignalOp::SetPower { wire, power });
        }
        #[cfg(test)]
        if changed_wire
            && !RECORD
            && self.test_recorder.value_region_census_enabled
            && let Some(values) = &scratch.value_plan
        {
            let region = values.wire_region[wire as usize];
            if region != ABSENT {
                self.record_value_region_write(scratch.net_index, region);
            }
        }
        if RECORD {
            scratch.recording.as_mut().unwrap().ops.push(SignalOp::SetPower { wire, power });
        }
        self.propagate_transport::<RECORD>(net, wire, scratch);
    }
}

/// One plan-free reference is reused by the compiler; hypothetical boundary calls never execute.
pub(crate) struct TransportReference {
    backend: GraphBackend,
}

impl TransportReference {
    pub(crate) fn new(graph: &CompileGraph) -> Result<Self, String> {
        let mut graph = graph.clone();
        graph.clear_execution_plans();
        let (program, state) = graph.lower().map_err(|error| error.to_string())?;
        Ok(Self {
            backend: GraphBackend::new(program, state, 0).map_err(|error| error.to_string())?,
        })
    }

    /// Control depends only on entry strengths and stable drives. Boundary guards remain operations.
    pub(crate) fn compile(
        &mut self,
        net: &TransportNet,
        root: u32,
        old: &[u8],
        drives: &[u8],
        monitor: &TaskMonitor,
    ) -> Result<SignalTranscript, String> {
        if old.len() != net.wires.len()
            || net.wires.iter().any(|wire| wire.conditional_inputs != 0)
            || drives.len() != net.wires.len()
            || root as usize >= net.wires.len()
            || old.iter().chain(drives).any(|&power| power > 15)
        {
            return Err("invalid signal transition entry".into());
        }
        if monitor.cancelled() {
            return Err("cancelled during signal transition compilation".into());
        }
        for (wire, &power) in net.wires.iter().zip(old) {
            let mut value = self.backend.wire_state(wire.node);
            value.power = power;
            self.backend.set_wire(wire.node, value);
        }
        let mut scratch = TransportScratch {
            sites: (0..net.sites.len()).map(|_| TransportSite::default()).collect(),
            drives: drives.to_vec(),
            recording: Some(Box::default()),
            ..Default::default()
        };
        let plan = &net.wires[root as usize];
        let mut maximum = 0u8;
        let mut inputs = plan.inputs;
        while inputs != 0 {
            let slot = inputs.trailing_zeros() as usize;
            inputs &= inputs - 1;
            let neighbor = net.site_wire[plan.neighbors[slot] as usize];
            maximum = maximum.max(old[neighbor as usize]);
        }
        let power = drives[root as usize].max(maximum.saturating_sub(1));
        if power != old[root as usize] {
            let mut value = self.backend.wire_state(plan.node);
            value.power = power;
            self.backend.set_wire(plan.node, value);
            scratch.recording.as_mut().unwrap().ops.push(SignalOp::SetPower { wire: root, power });
            self.backend
                .execute_transport::<true>(net, root, &mut scratch, Some(monitor))
                .map_err(|error| error.to_string())?;
        }
        let final_powers = net
            .wires
            .iter()
            .map(|wire| self.backend.wire_state(wire.node).power)
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let recorder = scratch.recording.take().unwrap();
        let mut needed = vec![false; net.sites.len()];
        for op in &recorder.ops {
            match *op {
                SignalOp::Select { wire, slot, .. } => needed[net.wires[wire as usize].neighbors[slot as usize] as usize] = true,
                SignalOp::Call { site, .. } => needed[site as usize] = true,
                _ => {}
            }
        }
        let ops = recorder
            .ops
            .into_iter()
            .filter(|op| !matches!(*op, SignalOp::Capture(site) if !needed[site as usize]))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Ok(SignalTranscript {
            ops,
            selections: recorder.selections,
            final_powers,
        })
    }
}

impl GraphBackend {
    pub(super) fn run_signal_transcript(&mut self, net: &TransportNet, transcript: &SignalTranscript) -> Result<(), GraphExecutionError> {
        self.run_signal_operations(net, &transcript.ops, transcript.selections)
    }

    pub(super) fn run_signal_operations(&mut self, net: &TransportNet, ops: &[SignalOp], selections: u32) -> Result<(), GraphExecutionError> {
        let mut scratch = self.transport_scratch.pop().unwrap_or_default();
        scratch
            .sites
            .resize_with(scratch.sites.len().max(net.sites.len()), TransportSite::default);
        scratch.selected.resize(selections as usize, false);
        let result = (|| {
            for op in ops {
                match *op {
                    SignalOp::Capture(site) => self.capture_transport_site::<false>(net, site, &mut scratch),
                    SignalOp::Select { wire, slot, selection } => {
                        let plan = &net.wires[wire as usize];
                        let local = plan.neighbors[slot as usize];
                        let site = net.sites[local as usize];
                        let CircuitNodeKind::Wire(index) = self.program.nodes()[plan.node.index()].kind else {
                            unreachable!()
                        };
                        scratch.selected[selection as usize] =
                            self.wire_route_admitted(index, plan.route, slot as usize, site, scratch.sites[local as usize].observed.as_ref());
                    }
                    SignalOp::SetPower { wire, power } => {
                        let node = net.wires[wire as usize].node;
                        let mut value = self.wire_state(node);
                        value.power = power;
                        self.set_wire(node, value);
                    }
                    SignalOp::Call {
                        site,
                        selection,
                        layer,
                        route,
                    } => {
                        if selection == u32::MAX || scratch.selected[selection as usize] {
                            self.dispatch_turbo_nonwire(
                                net.sites[site as usize],
                                scratch.sites[site as usize].observed.as_ref().unwrap(),
                                layer,
                                Some(route),
                            )?;
                        }
                    }
                }
            }
            Ok(())
        })();
        scratch.clear();
        self.transport_scratch.push(scratch);
        result
    }
}
