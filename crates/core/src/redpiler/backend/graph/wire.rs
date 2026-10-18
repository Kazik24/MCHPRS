#[cfg(test)]
use super::graph_test_trace;
use super::*;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct WireWalkNeighbors {
    heading: usize,
    slots: [usize; 24],
}

#[inline]
fn first_turbo_delivery(
    deliveries: &[RouteDelivery],
    target: SiteId,
    observed: &ObservedSite,
    mut guard_matches: impl FnMut(GuardId, SiteId, &ObservedSite) -> bool,
) -> Option<RouteDelivery> {
    for delivery in deliveries.iter().copied() {
        if delivery.target == target
            && delivery.receiver.is_some()
            && delivery.view == OperandView::Captured
            && guard_matches(delivery.guard, target, observed)
        {
            return Some(delivery);
        }
    }
    None
}

#[cfg(test)]
#[path = "tests/turbo_callback_selection.rs"]
mod turbo_callback_selection_tests;

impl GraphBackend {
    pub(super) fn wire_plan(&self, node: NodeId) -> &WirePlan {
        let CircuitNodeKind::Wire(index) = &self.program.nodes()[node.index()].kind else {
            unreachable!("wire handler received a non-wire node");
        };
        &self.program.wires()[*index]
    }

    pub(super) fn wire_state(&self, node: NodeId) -> RedstoneWire {
        #[cfg(test)]
        if let Some(tapes) = &self.wire_tapes {
            tapes.read(self, node);
        }
        match &self.state.components[node.index()] {
            ComponentState::Wire(wire) => {
                let mut wire = *wire;
                if let Some((net, attenuation)) = self.program.classic().wire_by_node.get(node.index()).copied().flatten()
                    && self.state.classic_active[net as usize] {
                    wire.power = self.state.classic_signals[net as usize].saturating_sub(attenuation);
                }
                wire
            }
            _ => unreachable!("wire node has non-wire state"),
        }
    }

    pub(super) fn set_wire(&mut self, node: NodeId, wire: RedstoneWire) {
        if let Some((net, _)) = self.program.classic().wire_by_node.get(node.index()).copied().flatten()
            && self.state.classic_active[net as usize] && self.wire_state(node) != wire {
            self.deopt_classic_net(net as usize);
        }
        #[cfg(test)]
        if let Some(tapes) = &self.wire_tapes {
            tapes.write(self, node, wire);
        }
        let ComponentState::Wire(current) = &mut self.state.components[node.index()] else {
            unreachable!("wire node has non-wire state");
        };
        if *current == wire {
            return;
        }
        #[cfg(test)]
        let old = *current;
        *current = wire;
        self.dirty[node.index()] = true;
        #[cfg(test)]
        graph_test_trace!(self, format!("wire state {node:?} {old:?} -> {wire:?}"));
    }

    pub(super) fn update_wire_shape(
        &mut self,
        node: NodeId,
        direction: BlockFace,
        captured: Option<&ObservedSite>,
    ) -> Result<(), GraphExecutionError> {
        if self.fixed_classic.is_some() {
            return self.fixed_classic_shape(node, direction, captured);
        }
        let wire = captured
            .and_then(|site| match site.properties {
                ObservedProperties::Wire(wire) => Some(wire),
                _ => None,
            })
            .unwrap_or_else(|| self.wire_state(node));
        let plan = self.wire_plan(node).clone();
        let raw = self.raw_wire_sides(&plan, wire);
        let updated = crate::redstone::wire::on_neighbor_changed_from(wire, raw, direction);
        if updated != wire {
            self.set_wire(node, updated);
            if let Some(route) = plan.shape_route {
                self.deliver_route(route, None)?;
            }
        }
        Ok(())
    }

    pub(super) fn evaluate_seed(&mut self, node: NodeId, captured: Option<&ObservedSite>) -> Result<(), GraphExecutionError> {
        if self.fixed_classic.is_some() {
            return self.fixed_classic_seed(node, captured);
        }
        let mut wire = captured
            .and_then(|site| match site.properties {
                ObservedProperties::Wire(wire) => Some(wire),
                _ => None,
            })
            .unwrap_or_else(|| self.wire_state(node));
        let plan = self.wire_plan(node).clone();
        #[cfg(test)]
        let (power, block_power, wire_power) = self.seed_power(&plan);
        #[cfg(not(test))]
        let (power, _, _) = self.seed_power(&plan);
        #[cfg(test)]
        graph_test_trace!(
            self,
            format!(
                "wire seed {node:?} operands=live-sites/live-strong block={block_power} wire={wire_power} power {} -> {power}",
                wire.power,
            )
        );
        if wire.power == power {
            return Ok(());
        }
        #[cfg(test)]
        let unchecked_sampled_old = wire.power;
        wire.power = power;
        self.set_wire(node, wire);
        #[cfg(test)]
        if let Some(tapes) = self.wire_tapes.clone() {
            return tapes.run(self, node);
        }
        #[cfg(test)]
        if let Some(program) = self.unchecked_classic.clone() {
            if program.handles(node) {
                return program.run(self, node, captured, unchecked_sampled_old);
            }
            self.counts.wire_walks += 1;
            return super::fixed_point::with_mode(false, || self.evaluate_turbo_wire(node));
        }
        self.counts.wire_walks += 1;
        self.evaluate_turbo_wire(node)
    }

    pub(super) fn seed_power(&mut self, plan: &WirePlan) -> (u8, u8, u8) {
        if let Some((input_site, input_face, is_input, adjacent)) = {
            self.program.dust_path_for_wire(plan.node).map(|path| {
                let index = path
                    .wires
                    .iter()
                    .position(|&node| node == plan.node)
                    .expect("validated dust path contains its wire");
                let adjacent = [
                    index.checked_sub(1).map(|index| path.wires[index]),
                    (index + 1 < path.wires.len()).then(|| path.wires[index + 1]),
                ];
                (path.input_site, path.input_face, index == 0, adjacent)
            })
        } {
            let block_power = if is_input { self.power_no_dust(input_site, input_face, None) } else { 0 };
            let wire_power = adjacent.into_iter().flatten().map(|node| self.wire_state(node).power).max().unwrap_or(0);
            self.counts.dust_path_power_samples_elided += 6 - u64::from(is_input);
            return (block_power.max(wire_power.saturating_sub(1)), block_power, wire_power);
        }
        if let Some(transfer) = plan.execution.query_facts.and_then(|facts| facts.transfer) {
            let mut inputs = transfer.seed.selected(false, |slot| self.live_neighbor_geometry(plan, slot));
            let mut wire_power = 0;
            while inputs != 0 {
                let slot = inputs.trailing_zeros() as usize;
                inputs &= inputs - 1;
                wire_power = wire_power.max(self.live_neighbor_wire_strength(plan, slot));
            }
            let block_power = BlockFace::values()
                .into_iter()
                .zip([3, 2, 4, 5, 1, 0])
                .map(|(face, slot)| self.neighbor_power_no_dust(plan, slot, face, None))
                .max()
                .unwrap_or(0);
            return (block_power.max(wire_power.saturating_sub(1)), block_power, wire_power);
        }
        self.seed_power_general(plan)
    }

    pub(super) fn seed_power_general(&mut self, plan: &WirePlan) -> (u8, u8, u8) {
        let (up_solid, _) = self.live_neighbor_geometry(plan, 3);
        let mut block_power = 0;
        let mut wire_power = 0;
        for face in BlockFace::values() {
            let (index, above, below) = match face {
                BlockFace::Top => (3, None, None),
                BlockFace::Bottom => (2, None, None),
                BlockFace::North => (4, Some(20), Some(17)),
                BlockFace::South => (5, Some(21), Some(18)),
                BlockFace::East => (1, Some(13), Some(12)),
                BlockFace::West => (0, Some(8), Some(7)),
            };
            let (solid, transparent, neighbor_wire_power) = self.live_neighbor_properties(plan, index);
            block_power = block_power.max(self.neighbor_power_no_dust(plan, index, face, None));
            wire_power = wire_power.max(neighbor_wire_power);
            if let (Some(above), Some(below)) = (above, below) {
                if !up_solid && !transparent {
                    wire_power = wire_power.max(self.live_neighbor_wire_strength(plan, above));
                }
                if !solid {
                    wire_power = wire_power.max(self.live_neighbor_wire_strength(plan, below));
                }
            }
        }
        (block_power.max(wire_power.saturating_sub(1)), block_power, wire_power)
    }

    fn live_neighbor_geometry(&self, plan: &WirePlan, index: usize) -> (bool, bool) {
        if let Some(geometry) = plan.execution.query_facts.and_then(|facts| facts.geometry_at(index)) {
            return geometry;
        }
        let site = self.live_site(plan.canonical_neighbors[index]);
        (site.solid, site.transparent)
    }

    fn live_neighbor_properties(&self, plan: &WirePlan, index: usize) -> (bool, bool, u8) {
        if let Some((solid, transparent)) = plan.execution.query_facts.and_then(|facts| facts.geometry_at(index)) {
            return (solid, transparent, self.live_neighbor_wire_strength(plan, index));
        }
        // Resolve an uncertified neighbor once for both geometry and wire strength.
        let site = self.live_site(plan.canonical_neighbors[index]);
        let power = match site.block {
            Block::RedstoneWire { wire } => wire.power,
            _ => 0,
        };
        (site.solid, site.transparent, power)
    }

    fn live_neighbor_wire_strength(&self, plan: &WirePlan, index: usize) -> u8 {
        if let Some(facts) = plan.execution.query_facts
            && facts.known & (1 << index) != 0
        {
            if facts.wire & (1 << index) == 0 {
                return 0;
            }
            let source = self.program.sites()[plan.canonical_neighbors[index].index()].source;
            if let SiteSource::Component(node) = source
                && matches!(self.program.nodes()[node.index()].kind, CircuitNodeKind::Wire(_))
            {
                return self.wire_state(node).power;
            }
        }
        match self.site_block(plan.canonical_neighbors[index]) {
            Block::RedstoneWire { wire } => wire.power,
            _ => 0,
        }
    }

    fn neighbor_power_no_dust(&self, plan: &WirePlan, index: usize, face: BlockFace, captured: Option<&ObservedSite>) -> u8 {
        let site = plan.canonical_neighbors[index];
        if let Some(facts) = plan.execution.query_facts {
            let bit = 1 << index;
            if facts.zero_no_dust & bit != 0 {
                return 0;
            }
            if facts.solid & bit != 0 {
                // Geometry is fixed, but conducted contributors must still be read live.
                return self.conducted_power(site, false);
            }
        }
        self.power_no_dust(site, face, captured)
    }

    pub(super) fn raw_wire_sides(&self, plan: &WirePlan, wire: RedstoneWire) -> RedstoneWire {
        if let Some(raw) = plan.execution.query_facts.and_then(|facts| facts.raw_wire(wire.power)) {
            return raw;
        }
        let neighbors = plan.canonical_neighbors.map(|site| self.site_block(site));
        crate::redstone::wire::get_raw_sides_from_neighbors(wire, &neighbors)
    }

    fn capture_walk_site(&mut self, walk: &mut WireWalk, site: SiteId) -> usize {
        if let Some(slot) = walk.slot(site) {
            return slot;
        }
        let observed = if walk.is_compiled() {
            self.observe_site(site).expect("compiled hop site was validated during preparation")
        } else {
            self.resolve_site(site, None)
                .expect("wire traversal site was validated during preparation")
                .observed()
        };
        #[cfg(test)]
        graph_test_trace!(
            self,
            format!(
                "wire discover {:?} receiver={:?} properties={:?}",
                self.program.sites()[site.index()].position,
                observed.receiver,
                observed.properties
            )
        );
        #[cfg(test)]
        if let Some(tapes) = &self.wire_tapes {
            tapes.capture(self, site, &observed);
        }
        walk.insert_site(
            site,
            WalkSite {
                site,
                observed,
                visited: false,
                x_bias: 0,
                z_bias: 0,
                layer: 0,
                heading: None,
                neighbor_slots: None,
            },
        )
    }

    pub(super) fn identify_wire_neighbors(&mut self, node: NodeId, walk: &mut WireWalk) -> WireWalkNeighbors {
        let plan = self.wire_plan(node).clone();
        let root = self.component_sites[node.index()].expect("wire root site was validated during preparation");
        let root_slot = self.capture_walk_site(walk, root);
        if let Some(heading) = walk.sites[root_slot].heading {
            return WireWalkNeighbors {
                heading: heading as usize,
                slots: walk
                    .cached_neighbors(root_slot)
                    .unwrap_or_else(|| plan.canonical_neighbors.map(|site| walk.slot(site).expect("discovered wire neighbor"))),
            };
        }
        let slots = plan.canonical_neighbors.map(|site| self.capture_walk_site(walk, site));

        let visited = slots.map(|slot| walk.sites[slot].visited);
        let from_west = visited[0] || visited[7] || visited[8];
        let from_east = visited[1] || visited[12] || visited[13];
        let from_north = visited[4] || visited[17] || visited[20];
        let from_south = visited[5] || visited[18] || visited[21];
        let mut x_bias = i32::from(from_west) - i32::from(from_east);
        let mut z_bias = i32::from(from_north) - i32::from(from_south);
        let (old_x_bias, old_z_bias) = {
            let root = &walk.sites[root_slot];
            (root.x_bias, root.z_bias)
        };
        let heading = if x_bias == 0 && z_bias == 0 {
            x_bias = old_x_bias;
            z_bias = old_z_bias;
            crate::redstone::wire::compute_heading(x_bias, z_bias)
        } else {
            if x_bias != 0 && z_bias != 0 {
                if old_x_bias != 0 {
                    z_bias = 0;
                }
                if old_z_bias != 0 {
                    x_bias = 0;
                }
            }
            crate::redstone::wire::compute_heading(x_bias, z_bias)
        };
        for slot in slots {
            let neighbor = &mut walk.sites[slot];
            neighbor.x_bias = x_bias;
            neighbor.z_bias = z_bias;
        }
        walk.sites[root_slot].heading = Some(heading as u8);
        walk.cache_neighbors(root_slot, slots);
        #[cfg(test)]
        graph_test_trace!(self, format!("wire heading {node:?} bias=({x_bias},{z_bias}) order={heading}"));
        WireWalkNeighbors { heading, slots }
    }

    #[inline]
    pub(super) fn wire_route_admitted(&self, wire_index:usize, route:RouteId, slot:usize, target:SiteId, observed:&ObservedSite)->bool {
        if let Some(plan) = self.program.wires()[wire_index].execution.callback_selection
            && plan.known & (1 << slot) != 0 {
            return plan.admitted & (1 << slot) != 0;
        }
        self.wire_route_delivery(wire_index,route,slot,target,observed).is_some()
    }

    pub(super) fn wire_route_delivery(
        &self,
        wire_index: usize,
        route: RouteId,
        slot: usize,
        target: SiteId,
        observed: &ObservedSite,
    ) -> Option<RouteDelivery> {
        let deliveries = &self.program.routes()[route.index()].deliveries;
        let index = self.program.turbo_callback_index(wire_index);
        let candidates = index.map_or(deliveries.as_ref(), |index| {
            &deliveries[index.offsets[slot] as usize..index.offsets[slot + 1] as usize]
        });
        let matched = first_turbo_delivery(candidates, target, observed, |guard, target, observed| {
            self.guard_matches_for_site(guard, target, observed)
        });
        matched
    }

    fn propagate_wire_changes(&mut self, node: NodeId, walk: &mut WireWalk, layer: u32) {
        let plan = self.wire_plan(node).clone();
        let Some(route) = plan.turbo_route else {
            return;
        };
        let CircuitNodeKind::Wire(wire_index) = self.program.nodes()[node.index()].kind else {
            unreachable!("Turbo callback lookup received a non-wire node");
        };
        let neighbors = self.identify_wire_neighbors(node, walk);
        for &slot in &crate::redstone::wire::TURBO_ORDER[neighbors.heading] {
            let walk_slot = neighbors.slots[slot];
            let site = walk.sites[walk_slot].site;
            let next_layer = layer + 1;
            let changed = {
                let neighbor = &mut walk.sites[walk_slot];
                if next_layer > neighbor.layer {
                    neighbor.layer = next_layer;
                    true
                } else {
                    false
                }
            };
            if changed
                && self
                    .wire_route_admitted(wire_index, route, slot, site, &walk.sites[walk_slot].observed)
            {
                walk.queues[1].push_back(walk_slot);
            }
        }
        for &slot in &crate::redstone::wire::TURBO_ORDER[neighbors.heading][..4] {
            let walk_slot = neighbors.slots[slot];
            let site = walk.sites[walk_slot].site;
            let next_layer = layer + 2;
            let changed = {
                let neighbor = &mut walk.sites[walk_slot];
                if next_layer > neighbor.layer {
                    neighbor.layer = next_layer;
                    true
                } else {
                    false
                }
            };
            if changed
                && self
                    .wire_route_admitted(wire_index, route, slot, site, &walk.sites[walk_slot].observed)
            {
                walk.queues[2].push_back(walk_slot);
            }
        }
    }

    fn shift_wire_queues(&self, walk: &mut WireWalk) {
        walk.queues.rotate_left(1);
        walk.queues[2].clear();
    }

    #[cfg(test)]
    pub(super) fn run_wire_tape_reference(&mut self, node: NodeId) -> Result<(), GraphExecutionError> {
        self.counts.wire_walks += 1;
        super::fixed_point::with_mode(false, || self.evaluate_turbo_wire(node))
    }

    #[inline(always)]
    fn evaluate_turbo_wire(&mut self, node: NodeId) -> Result<(), GraphExecutionError> {
        if super::fixed_point::enabled() {
            return self.evaluate_fixed_point_wire(node);
        }
        if let Some(instant) = self.wire_plan(node).execution.instant_reset {
            let plan = self.wire_plan(node).clone();
            return self.evaluate_instant_output(&plan, instant);
        }
        if self.wire_plan(node).execution.query_facts.is_some_and(|facts| facts.isolated) {
            let plan = self.wire_plan(node).clone();
            return self.evaluate_isolated_turbo_wire(&plan);
        }
        let result = self.evaluate_turbo_wire_inner(node);
        result
    }

    fn evaluate_isolated_turbo_wire(&mut self, plan: &WirePlan) -> Result<(), GraphExecutionError> {
        let Some(route) = plan.turbo_route else {
            return Ok(());
        };
        let CircuitNodeKind::Wire(wire_index) = self.program.nodes()[plan.node.index()].kind else {
            unreachable!("isolated Turbo received a non-wire node");
        };
        let observed = plan.canonical_neighbors.map(|site| {
            self.resolve_site(site, None)
                .expect("wire traversal site was validated during preparation")
                .observed()
        });
        #[cfg(test)]
        if let Some(tapes) = &self.wire_tapes {
            for (site, observed) in plan.canonical_neighbors.iter().zip(&observed) {
                tapes.capture(self, *site, observed);
            }
        }
        // Both original enqueue loops finish before any callback can change a guard.
        let selected: [bool; 24] = std::array::from_fn(|slot| {
            self.wire_route_admitted(wire_index, route, slot, plan.canonical_neighbors[slot], &observed[slot])
        });
        // A fresh walk has no visited neighbors or bias: compute_heading(0, 0) == 3.
        let order = &crate::redstone::wire::TURBO_ORDER[3];
        #[cfg(test)]
        graph_test_trace!(self, format!("wire isolated replay {:?}", plan.node));
        for (slots, layer) in [(order.as_slice(), 1), (&order[..4], 2)] {
            for &slot in slots {
                if selected[slot] {
                    self.dispatch_turbo_nonwire(plan.canonical_neighbors[slot], &observed[slot], layer)?;
                }
            }
        }
        Ok(())
    }

    fn evaluate_turbo_wire_inner(&mut self, node: NodeId) -> Result<(), GraphExecutionError> {
        let compiled_root = self.wire_plan(node).execution.hop_root;
        let root = compiled_root
            .or(self.component_sites[node.index()])
            .expect("wire root site was validated during preparation");
        let mut walk = if compiled_root.is_some() {
            self.wire_walks.pop().unwrap_or_else(|| WireWalk::compiled(self.program.sites().len()))
        } else {
            WireWalk::new()
        };
        // Always return scratch, including when a nested command reports an error.
        let result = self.run_wire_hops(node, root, &mut walk);
        if compiled_root.is_some() {
            walk.clear();
            self.wire_walks.push(walk);
        }
        result
    }

    fn run_wire_hops(&mut self, node: NodeId, root: SiteId, walk: &mut WireWalk) -> Result<(), GraphExecutionError> {
        let root_slot = self.capture_walk_site(walk, root);
        walk.sites[root_slot].visited = true;
        #[cfg(test)]
        graph_test_trace!(self, format!("wire turbo start {node:?}"));
        self.propagate_wire_changes(node, walk, 0);
        self.shift_wire_queues(walk);
        walk.layer = 1;
        while !walk.queues[0].is_empty() || !walk.queues[1].is_empty() {
            let count = walk.queues[0].len();
            for _ in 0..count {
                let slot = walk.queues[0].pop_front().unwrap();
                self.dispatch_turbo_site(slot, walk)?;
            }
            self.shift_wire_queues(walk);
            walk.layer += 1;
        }
        #[cfg(test)]
        graph_test_trace!(self, format!("wire turbo end {node:?}"));
        Ok(())
    }

    fn dispatch_turbo_site(&mut self, slot: usize, walk: &mut WireWalk) -> Result<(), GraphExecutionError> {
        let site = walk.sites[slot].site;
        let observed = walk.sites[slot].observed.clone();
        if let Some(receiver @ CallbackReceiver::Component(node)) = observed.receiver
            && matches!(self.program.nodes()[node.index()].kind, CircuitNodeKind::Wire(_))
        {
            self.record_turbo_delivery(site, receiver, walk.layer);
            return self.evaluate_turbo_wire_node(node, walk);
        }
        self.dispatch_turbo_nonwire(site, &observed, walk.layer)
    }

    fn record_turbo_delivery(&mut self, _site: SiteId, _receiver: CallbackReceiver, _layer: u32) {
        self.counts.callbacks += 1;
        // Dust-to-dust steps stay inside the walk; native dispatches only the rest.
        #[cfg(test)]
        if self.test_recording
            && self.has_semantic_callback(_receiver)
            && !matches!(_receiver, CallbackReceiver::Component(node)
                if matches!(self.program.nodes()[node.index()].kind, CircuitNodeKind::Wire(_)))
        {
            self.test_recorder.observed_operations.push(GraphObservedOperation::Callback {
                target: self.program.sites()[_site.index()].position,
                direction: None,
            });
        }
        #[cfg(test)]
        graph_test_trace!(
            self,
            format!(
                "wire deliver {:?} layer={} Update {{ direction: None }} receiver={_receiver:?}",
                self.program.sites()[_site.index()].position,
                _layer
            )
        );
    }

    pub(super) fn dispatch_turbo_nonwire(&mut self, site: SiteId, observed: &ObservedSite, layer: u32) -> Result<(), GraphExecutionError> {
        let Some(receiver) = observed.receiver else {
            return Ok(());
        };
        #[cfg(test)]
        if let Some(tapes) = &self.wire_tapes {
            tapes.call(self, site, observed, layer);
        }
        self.record_turbo_delivery(site, receiver, layer);
        match receiver {
            CallbackReceiver::Component(node) => self.update_component(node, DeliveryKind::Update { direction: None }, Some(observed)),
            CallbackReceiver::PistonBase(piston) | CallbackReceiver::PistonHead(piston) => {
                if self.classic_proof.is_some() {
                    return Err(GraphExecutionError::UnsupportedComponent {
                        position: Some(self.program.sites()[site.index()].position),
                        kind: "classic proof piston callback",
                    });
                }
                self.sample_piston(piston)
            }
            CallbackReceiver::Contact(_) => {
                if self.classic_proof.is_some() {
                    return Err(GraphExecutionError::UnsupportedComponent {
                        position: Some(self.program.sites()[site.index()].position),
                        kind: "classic proof contact callback",
                    });
                }
                Ok(())
            }
        }
    }

    fn evaluate_turbo_wire_node(&mut self, node: NodeId, walk: &mut WireWalk) -> Result<(), GraphExecutionError> {
        #[cfg(test)]
        if let Some(tapes) = &self.wire_tapes {
            tapes.visit(node);
        }
        let root = self.component_sites[node.index()].expect("wire root site was validated during preparation");
        let root_slot = self.capture_walk_site(walk, root);
        walk.sites[root_slot].visited = true;
        let ordered = self.identify_wire_neighbors(node, walk);
        let plan = self.wire_plan(node).clone();
        let mut wire = match walk.sites[root_slot].observed.properties {
            ObservedProperties::Wire(wire) => wire,
            _ => unreachable!("captured wire site has non-wire properties"),
        };
        #[cfg(test)]
        let (new_power, block_power, wire_power) = self.turbo_wire_power(&plan, &ordered, walk);
        #[cfg(not(test))]
        let (new_power, _, _) = self.turbo_wire_power(&plan, &ordered, walk);
        #[cfg(test)]
        graph_test_trace!(
            self,
            format!(
                "wire query {node:?} layer={} operands=captured-walk/live-strong block={block_power} direct={wire_power}",
                walk.layer,
            )
        );
        if wire.power == new_power {
            return Ok(());
        }
        #[cfg(test)]
        let old_power = wire.power;
        wire.power = new_power;
        walk.sites[root_slot].observed.properties = ObservedProperties::Wire(wire);
        self.set_wire(node, wire);
        #[cfg(test)]
        graph_test_trace!(self, format!("wire strength {node:?} layer={} {old_power} -> {new_power}", walk.layer));
        self.propagate_wire_changes(node, walk, walk.layer);
        Ok(())
    }

    pub(super) fn turbo_wire_power(&mut self, plan: &WirePlan, neighbors: &WireWalkNeighbors, walk: &WireWalk) -> (u8, u8, u8) {
        if let Some((input_site, input_face, is_input, adjacent)) = {
            self.program.dust_path_for_wire(plan.node).map(|path| {
                let index = path
                    .wires
                    .iter()
                    .position(|&node| node == plan.node)
                    .expect("validated dust path contains its wire");
                let adjacent = [
                    index.checked_sub(1).map(|index| path.wires[index]),
                    (index + 1 < path.wires.len()).then(|| path.wires[index + 1]),
                ];
                (path.input_site, path.input_face, index == 0, adjacent)
            })
        } {
            let input_power = if is_input {
                let captured = &walk.sites[walk.slot(input_site).expect("certified input was discovered")].observed;
                self.power_no_dust(input_site, input_face, Some(captured))
            } else {
                0
            };
            let mut wire_power = 0;
            if input_power < 15 {
                for neighbor in adjacent.into_iter().flatten() {
                    let site = self.component_sites[neighbor.index()].expect("certified neighboring wire has a root site");
                    let Some(slot) = walk.slot(site) else {
                        return self.turbo_wire_power_general(plan, neighbors, walk);
                    };
                    let observed = &walk.sites[slot].observed;
                    let ObservedProperties::Wire(wire) = observed.properties else {
                        return self.turbo_wire_power_general(plan, neighbors, walk);
                    };
                    wire_power = wire_power.max(wire.power);
                }
            }
            self.counts.dust_path_power_samples_elided += 6 - u64::from(is_input);
            return (input_power.max(wire_power.saturating_sub(1)), wire_power, input_power);
        }
        if let Some(transfer) = plan.execution.query_facts.and_then(|facts| facts.transfer) {
            let input_power = BlockFace::values()
                .into_iter()
                .zip([3, 2, 4, 5, 1, 0])
                .map(|(face, slot)| {
                    let observed = &walk.sites[neighbors.slots[slot]].observed;
                    self.neighbor_power_no_dust(plan, slot, face, Some(observed))
                })
                .max()
                .unwrap_or(0);
            let mut wire_power = 0;
            if input_power < 15 {
                let mut inputs = transfer
                    .turbo
                    .selected(true, |slot| self.captured_neighbor_geometry(plan, slot, neighbors, walk));
                while inputs != 0 {
                    let slot = inputs.trailing_zeros() as usize;
                    inputs &= inputs - 1;
                    wire_power = self.walk_wire_strength(neighbors.slots[slot], walk, wire_power);
                }
            }
            return (input_power.max(wire_power.saturating_sub(1)), wire_power, input_power);
        }
        self.turbo_wire_power_general(plan, neighbors, walk)
    }

    pub(super) fn turbo_wire_power_general(&mut self, plan: &WirePlan, neighbors: &WireWalkNeighbors, walk: &WireWalk) -> (u8, u8, u8) {
        let mut wire_power = 0;
        for (face, index) in BlockFace::values().into_iter().zip([3, 2, 4, 5, 1, 0]) {
            let observed = &walk.sites[neighbors.slots[index]].observed;
            wire_power = wire_power.max(self.neighbor_power_no_dust(plan, index, face, Some(observed)));
        }
        let mut block_power = 0;
        if wire_power < 15 {
            let order = crate::redstone::wire::TURBO_ORDER[neighbors.heading];
            let (center_up_solid, _) = self.captured_neighbor_geometry(plan, order[1], neighbors, walk);
            let horizontal = [4, 5, 6, 7];
            let down = [8, 10, 12, 14];
            let up = [9, 11, 13, 15];
            for index in 0..4 {
                let neighbor = neighbors.slots[order[horizontal[index]]];
                let (solid, transparent) = self.captured_neighbor_geometry(plan, order[horizontal[index]], neighbors, walk);
                block_power = self.walk_wire_strength(neighbor, walk, block_power);
                if !solid {
                    block_power = self.walk_wire_strength(neighbors.slots[order[down[index]]], walk, block_power);
                } else if !center_up_solid && !transparent {
                    block_power = self.walk_wire_strength(neighbors.slots[order[up[index]]], walk, block_power);
                }
            }
        }
        (wire_power.max(block_power.saturating_sub(1)), block_power, wire_power)
    }

    fn captured_neighbor_geometry(&self, plan: &WirePlan, index: usize, neighbors: &WireWalkNeighbors, walk: &WireWalk) -> (bool, bool) {
        if let Some(geometry) = plan.execution.query_facts.and_then(|facts| facts.geometry_at(index)) {
            return geometry;
        }
        let site = self.walk_site(neighbors.slots[index], walk);
        (site.solid, site.transparent)
    }

    fn walk_site(&self, slot: usize, walk: &WireWalk) -> ResolvedSiteView {
        let site = walk.sites[slot].site;
        let observed = &walk.sites[slot].observed;
        self.resolve_site_view(site, Some(observed)).expect("captured wire site is valid")
    }

    fn walk_wire_strength(&self, slot: usize, walk: &WireWalk, strength: u8) -> u8 {
        match walk.sites[slot].observed.properties {
            ObservedProperties::Wire(wire) => strength.max(wire.power),
            _ => strength,
        }
    }
}
