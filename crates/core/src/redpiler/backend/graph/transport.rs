//! Dense closed-wire transport: original captures, writes, heading and FIFO sample cuts.
//! Queue-only boundary handlers cannot change the live no-dust inputs during a walk.
use super::*;

const ABSENT: u32 = u32::MAX;

#[derive(Default)]
struct TransportSite {
    /// First-discovery snapshot; wire writes update only their captured root, as in Turbo.
    observed: Option<ObservedSite>,
    /// Only wire dispatch marks a site visited, determining the original arrival heading.
    visited: bool,
    /// Inherited horizontal bias is relevant only to a subsequently visited wire.
    bias: (i8, i8),
    /// Last enqueued layer, including suppressed deliveries.
    layer: u32,
    /// Heading is fixed on this wire's first neighborhood discovery.
    heading: Option<u8>,
}

#[derive(Default)]
pub(super) struct TransportScratch {
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
    }
}

impl GraphBackend {
    pub(super) fn run_instant_transport(&mut self, net: u32, root: u32, drive: u8) -> Result<(), GraphExecutionError> {
        let net = self.program.instant_transport_net(net).clone();
        let mut scratch = self.transport_scratch.pop().unwrap_or_default();
        scratch
            .sites
            .resize_with(scratch.sites.len().max(net.sites.len()), TransportSite::default);
        scratch.drives.resize(scratch.drives.len().max(net.wires.len()), u8::MAX);
        scratch.drives[root as usize] = drive;
        scratch.sampled.push(root);
        self.counts.instant_transport_invocations += 1;
        let result = self.execute_transport(&net, root, &mut scratch);
        // Errors cannot leave captures or deliveries in reusable scratch.
        scratch.clear();
        self.transport_scratch.push(scratch);
        result
    }

    fn capture_transport_site(&self, net: &TransportNet, local: u32, scratch: &mut TransportScratch) {
        if local == ABSENT || scratch.sites[local as usize].observed.is_some() {
            return;
        }
        let site = net.sites[local as usize];
        scratch.sites[local as usize].observed = Some(self.observe_wire_site(site).expect("validated transport site"));
        scratch.touched.push(local);
    }

    fn identify_transport_neighbors(&self, net: &TransportNet, wire: u32, scratch: &mut TransportScratch) -> usize {
        let plan = &net.wires[wire as usize];
        self.capture_transport_site(net, plan.site, scratch);
        if let Some(heading) = scratch.sites[plan.site as usize].heading {
            return heading as usize;
        }
        for &site in &plan.neighbors {
            self.capture_transport_site(net, site, scratch);
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

    fn propagate_transport(&self, net: &TransportNet, wire: u32, scratch: &mut TransportScratch) {
        let plan = &net.wires[wire as usize];
        let heading = self.identify_transport_neighbors(net, wire, scratch);
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
                if self.wire_route_admitted(index, plan.route, slot, site, state.observed.as_ref().unwrap()) {
                    scratch.queues[delay].push_back(QueuedWireSite {
                        slot: local,
                        route: plan.route,
                    });
                }
            }
        }
    }

    fn execute_transport(&mut self, net: &TransportNet, root: u32, scratch: &mut TransportScratch) -> Result<(), GraphExecutionError> {
        let root_site = net.wires[root as usize].site;
        self.capture_transport_site(net, root_site, scratch);
        scratch.sites[root_site as usize].visited = true;
        self.propagate_transport(net, root, scratch);
        scratch.queues.rotate_left(1);
        scratch.layer = 1;
        while !scratch.queues[0].is_empty() || !scratch.queues[1].is_empty() {
            let count = scratch.queues[0].len();
            for _ in 0..count {
                let queued = scratch.queues[0].pop_front().unwrap();
                let site = net.sites[queued.slot as usize];
                let wire = net.site_wire[queued.slot as usize];
                if wire != ABSENT {
                    self.counts.instant_transport_wire_visits += 1;
                    let node = net.wires[wire as usize].node;
                    self.record_turbo_delivery(site, CallbackReceiver::Component(node), scratch.layer);
                    self.evaluate_transport_wire(net, wire, scratch);
                } else {
                    let observed = scratch.sites[queued.slot as usize].observed.as_ref().unwrap();
                    self.dispatch_turbo_nonwire(site, observed, scratch.layer, Some(queued.route))?;
                }
            }
            scratch.queues.rotate_left(1);
            scratch.queues[2].clear();
            scratch.layer += 1;
        }
        Ok(())
    }

    fn evaluate_transport_wire(&mut self, net: &TransportNet, wire: u32, scratch: &mut TransportScratch) {
        let plan = &net.wires[wire as usize];
        self.capture_transport_site(net, plan.site, scratch);
        scratch.sites[plan.site as usize].visited = true;
        self.identify_transport_neighbors(net, wire, scratch);
        let ObservedProperties::Wire(mut state) = scratch.sites[plan.site as usize].observed.as_ref().unwrap().properties else {
            unreachable!()
        };
        let mut drive = scratch.drives[wire as usize];
        if drive == u8::MAX {
            drive = self.transport_block_power(plan.node);
            scratch.drives[wire as usize] = drive;
            scratch.sampled.push(wire);
        }
        let mut maximum = 0;
        if drive < 15 {
            let mut inputs = plan.inputs;
            while inputs != 0 {
                let slot = inputs.trailing_zeros() as usize;
                inputs &= inputs - 1;
                let local = plan.neighbors[slot];
                let ObservedProperties::Wire(neighbor) = scratch.sites[local as usize].observed.as_ref().unwrap().properties else {
                    unreachable!()
                };
                maximum = maximum.max(neighbor.power);
            }
        }
        let power = drive.max(maximum.saturating_sub(1));
        if power == state.power {
            return;
        }
        state.power = power;
        scratch.sites[plan.site as usize].observed.as_mut().unwrap().properties = ObservedProperties::Wire(state);
        self.set_wire(plan.node, state);
        self.propagate_transport(net, wire, scratch);
    }
}
