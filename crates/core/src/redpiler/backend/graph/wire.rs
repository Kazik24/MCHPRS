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
mod turbo_callback_selection_tests {
    use super::*;

    fn delivery(target: SiteId, guard: u32, view: OperandView, receiver: Option<CallbackReceiver>, sequence: u32) -> RouteDelivery {
        RouteDelivery {
            target,
            kind: DeliveryKind::Update { direction: None },
            guard: GuardId::new(guard),
            view,
            sequence,
            receiver,
        }
    }

    #[test]
    fn indexed_and_linear_selection_keep_the_same_first_passing_row() {
        let receiver = CallbackReceiver::Component(NodeId::new(7));
        let target = SiteId::new(200);
        let other_targets: Vec<_> = (0..10).map(|slot| SiteId::new(slot * 11 + 3)).collect();
        let mut route = other_targets
            .iter()
            .map(|&site| delivery(site, 9, OperandView::Captured, Some(receiver), 17))
            .collect::<Vec<_>>();
        route.extend([
            delivery(target, 9, OperandView::Captured, None, 30),
            delivery(target, 9, OperandView::Live, Some(receiver), 30),
            delivery(target, 8, OperandView::Captured, Some(receiver), 90),
            delivery(target, 9, OperandView::Captured, Some(receiver), 90),
        ]);
        let observed = ObservedSite {
            profile: None,
            receiver: Some(receiver),
            properties: ObservedProperties::Passive,
        };
        let matches_guard = |guard, _, observed: &ObservedSite| guard == GuardId::new(9) && observed.receiver == Some(receiver);
        let linear = first_turbo_delivery(&route, target, &observed, matches_guard);
        let indexed = first_turbo_delivery(&route[10..], target, &observed, matches_guard);
        assert_eq!(linear, indexed);
        assert_eq!(linear, Some(route[13]));

        let miss_guard = |_, _, _: &ObservedSite| false;
        let linear_miss = first_turbo_delivery(&route, target, &observed, miss_guard);
        let indexed_miss = first_turbo_delivery(&route[10..], target, &observed, miss_guard);
        assert_eq!(linear_miss, indexed_miss);
        assert_eq!(linear_miss, None);
    }
}

impl GraphBackend {
    pub(super) fn wire_plan(&self, node: NodeId) -> &WirePlan {
        let CircuitNodeKind::Wire(index) = &self.program.nodes()[node.index()].kind else {
            unreachable!("wire handler received a non-wire node");
        };
        &self.program.wires()[*index]
    }

    pub(super) fn wire_state(&self, node: NodeId) -> RedstoneWire {
        match &self.state.components[node.index()] {
            ComponentState::Wire(wire) => *wire,
            _ => unreachable!("wire node has non-wire state"),
        }
    }

    fn set_wire(&mut self, node: NodeId, wire: RedstoneWire) {
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
        wire.power = power;
        self.set_wire(node, wire);
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
        self.seed_power_general(plan)
    }

    pub(super) fn seed_power_general(&mut self, plan: &WirePlan) -> (u8, u8, u8) {
        let up_solid = self.live_site(plan.canonical_neighbors[3]).solid;
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
            let site = plan.canonical_neighbors[index];
            let neighbor = self.live_site(site);
            block_power = block_power.max(self.power_no_dust(site, face, None));
            if let Block::RedstoneWire { wire } = neighbor.block {
                wire_power = wire_power.max(wire.power);
            }
            if let (Some(above), Some(below)) = (above, below) {
                if !up_solid && !neighbor.transparent {
                    if let Block::RedstoneWire { wire } = self.site_block(plan.canonical_neighbors[above]) {
                        wire_power = wire_power.max(wire.power);
                    }
                }
                if !neighbor.solid {
                    if let Block::RedstoneWire { wire } = self.site_block(plan.canonical_neighbors[below]) {
                        wire_power = wire_power.max(wire.power);
                    }
                }
            }
        }
        (block_power.max(wire_power.saturating_sub(1)), block_power, wire_power)
    }

    pub(super) fn raw_wire_sides(&self, plan: &WirePlan, wire: RedstoneWire) -> RedstoneWire {
        let neighbors = plan.canonical_neighbors.map(|site| self.site_block(site));
        crate::redstone::wire::get_raw_sides_from_neighbors(wire, &neighbors)
    }

    fn capture_walk_site(&mut self, walk: &mut WireWalk, site: SiteId) -> usize {
        if let Some(&slot) = walk.site_slots.get(&site) {
            return slot;
        }
        let observed = self
            .resolve_site(site, None)
            .expect("wire traversal site was validated during preparation")
            .observed();
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
                slots: plan.canonical_neighbors.map(|site| walk.site_slots[&site]),
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
        #[cfg(test)]
        graph_test_trace!(self, format!("wire heading {node:?} bias=({x_bias},{z_bias}) order={heading}"));
        WireWalkNeighbors { heading, slots }
    }

    fn wire_route_delivery(&self, wire_index: usize, route: RouteId, slot: usize, target: SiteId, observed: &ObservedSite) -> Option<RouteDelivery> {
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
                    .wire_route_delivery(wire_index, route, slot, site, &walk.sites[walk_slot].observed)
                    .is_some()
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
                    .wire_route_delivery(wire_index, route, slot, site, &walk.sites[walk_slot].observed)
                    .is_some()
            {
                walk.queues[2].push_back(walk_slot);
            }
        }
    }

    fn shift_wire_queues(&self, walk: &mut WireWalk) {
        walk.queues.rotate_left(1);
        walk.queues[2].clear();
    }

    #[inline(always)]
    fn evaluate_turbo_wire(&mut self, node: NodeId) -> Result<(), GraphExecutionError> {
        let result = self.evaluate_turbo_wire_inner(node);
        result
    }

    fn evaluate_turbo_wire_inner(&mut self, node: NodeId) -> Result<(), GraphExecutionError> {
        let root = self.component_sites[node.index()].expect("wire root site was validated during preparation");
        let mut walk = WireWalk::new();
        let root_slot = self.capture_walk_site(&mut walk, root);
        walk.sites[root_slot].visited = true;
        #[cfg(test)]
        graph_test_trace!(self, format!("wire turbo start {node:?}"));
        self.propagate_wire_changes(node, &mut walk, 0);
        self.shift_wire_queues(&mut walk);
        walk.layer = 1;
        while !walk.queues[0].is_empty() || !walk.queues[1].is_empty() {
            let count = walk.queues[0].len();
            for _ in 0..count {
                let slot = walk.queues[0].pop_front().unwrap();
                self.dispatch_turbo_site(slot, &mut walk)?;
            }
            self.shift_wire_queues(&mut walk);
            walk.layer += 1;
        }
        #[cfg(test)]
        graph_test_trace!(self, format!("wire turbo end {node:?}"));
        Ok(())
    }

    fn dispatch_turbo_site(&mut self, slot: usize, walk: &mut WireWalk) -> Result<(), GraphExecutionError> {
        let _site = walk.sites[slot].site;
        let observed = walk.sites[slot].observed.clone();
        let Some(receiver) = observed.receiver else {
            return Ok(());
        };
        self.counts.callbacks += 1;
        // Dust-to-dust steps stay inside the walk; native dispatches only the rest.
        #[cfg(test)]
        if self.test_recording
            && self.has_semantic_callback(receiver)
            && !matches!(receiver, CallbackReceiver::Component(node)
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
                "wire deliver {:?} layer={} Update {{ direction: None }} receiver={receiver:?}",
                self.program.sites()[_site.index()].position,
                walk.layer
            )
        );
        match receiver {
            CallbackReceiver::Component(node) if matches!(self.program.nodes()[node.index()].kind, CircuitNodeKind::Wire(_)) => {
                self.evaluate_turbo_wire_node(node, walk)
            }
            CallbackReceiver::Component(node) => self.update_component(node, DeliveryKind::Update { direction: None }, Some(&observed)),
            CallbackReceiver::PistonBase(piston) | CallbackReceiver::PistonHead(piston) => self.sample_piston(piston),
            CallbackReceiver::Contact(_) => Ok(()),
        }
    }

    fn evaluate_turbo_wire_node(&mut self, node: NodeId, walk: &mut WireWalk) -> Result<(), GraphExecutionError> {
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
                let captured = &walk.sites[walk.site_slots[&input_site]].observed;
                self.power_no_dust(input_site, input_face, Some(captured))
            } else {
                0
            };
            let mut wire_power = 0;
            if input_power < 15 {
                for neighbor in adjacent.into_iter().flatten() {
                    let site = self.component_sites[neighbor.index()].expect("certified neighboring wire has a root site");
                    let Some(&slot) = walk.site_slots.get(&site) else {
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
        self.turbo_wire_power_general(plan, neighbors, walk)
    }

    pub(super) fn turbo_wire_power_general(&mut self, plan: &WirePlan, neighbors: &WireWalkNeighbors, walk: &WireWalk) -> (u8, u8, u8) {
        let mut wire_power = 0;
        for (face, index) in BlockFace::values().into_iter().zip([3, 2, 4, 5, 1, 0]) {
            let site = plan.canonical_neighbors[index];
            let observed = &walk.sites[neighbors.slots[index]].observed;
            wire_power = wire_power.max(self.power_no_dust(site, face, Some(observed)));
        }
        let mut block_power = 0;
        if wire_power < 15 {
            let order = crate::redstone::wire::TURBO_ORDER[neighbors.heading];
            let center_up_solid = self.walk_site(neighbors.slots[order[1]], walk).solid;
            let horizontal = [4, 5, 6, 7];
            let down = [8, 10, 12, 14];
            let up = [9, 11, 13, 15];
            for index in 0..4 {
                let neighbor = neighbors.slots[order[horizontal[index]]];
                let neighbor_site = self.walk_site(neighbor, walk);
                block_power = self.walk_wire_strength(neighbor, walk, block_power);
                if !neighbor_site.solid {
                    block_power = self.walk_wire_strength(neighbors.slots[order[down[index]]], walk, block_power);
                } else if !center_up_solid && !neighbor_site.transparent {
                    block_power = self.walk_wire_strength(neighbors.slots[order[up[index]]], walk, block_power);
                }
            }
        }
        (wire_power.max(block_power.saturating_sub(1)), block_power, wire_power)
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
