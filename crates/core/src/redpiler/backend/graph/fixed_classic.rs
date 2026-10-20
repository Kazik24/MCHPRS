//! Typed execution of a fully lowered, contactless fixed redstone circuit.
//!
//! This deliberately keeps `CircuitState` and `GraphFuture` as the owners of
//! component state and scheduled ticks. The executor replaces only graph
//! lookup, route selection, and wire-walk dispatch with dense typed tables.
//! Each episode captures a neighborhood once, then uses local u32 indices for
//! its inputs and ordered frontiers. Nested episodes retain separate scratch.
use super::*;

const NO_SLOT: u32 = u32::MAX;
const NO_HEADING: u8 = u8::MAX;

#[cfg(test)]
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct FixedClassicCounts {
    pub(super) seed_calls: u64,
    pub(super) episodes: u64,
    pub(super) wire_steps: u64,
    pub(super) gate_ops: u64,
    pub(super) route_ops: u64,
}

#[derive(Debug, Clone, Copy)]
enum FixedCapture {
    Wire(RedstoneWire),
    Gate { powered: bool, locked: bool },
}

#[derive(Debug, Clone, Copy)]
struct WalkSite {
    site: SiteId,
    /// The state owner is resolved once when this alias is first captured.
    node: NodeId,
    capture: FixedCapture,
    visited_wire: bool,
    x_bias: i8,
    z_bias: i8,
    layer: u32,
    heading: u8,
    /// Index of this root's captured neighborhood, or NO_SLOT before discovery.
    neighbors: u32,
}

/// Scratch is sparse in discovered sites. `slots` is the only dense per-program
/// array; clearing touches only the sites discovered by the previous walk.
pub(super) struct FixedClassicScratch {
    slots: Vec<u32>,
    sites: Vec<WalkSite>,
    /// Local capture indices; NO_SLOT denotes an immutable/inert neighbor.
    neighbors: Vec<[u32; 24]>,
    /// Each FIFO layer is read once in order; propagation appends only to later layers.
    queues: [Vec<u32>; 3],
    layer: u32,
}

impl FixedClassicScratch {
    pub(super) fn new(site_count: usize) -> Self {
        Self {
            slots: vec![NO_SLOT; site_count],
            sites: Vec::with_capacity(64),
            neighbors: Vec::with_capacity(16),
            queues: std::array::from_fn(|_| Vec::new()),
            layer: 0,
        }
    }

    fn clear(&mut self) {
        for site in &self.sites {
            self.slots[site.site.index()] = NO_SLOT;
        }
        self.sites.clear();
        self.neighbors.clear();
        for queue in &mut self.queues {
            queue.clear();
        }
        self.layer = 0;
    }

    fn slot(&self, site: SiteId) -> Option<usize> {
        let slot = *self.slots.get(site.index())?;
        (slot != NO_SLOT).then_some(slot as usize)
    }

    fn capture(&mut self, backend: &GraphBackend, program: &FixedClassicProgram, site: SiteId) -> Option<usize> {
        // Constant/inert sites have no mutable captured properties, can never be
        // visited wires, and their callback dispatch is a no-op.
        if matches!(program.sites[site.index()], FixedClassicSite::Inert) {
            return None;
        }
        if let Some(slot) = self.slot(site) {
            return Some(slot);
        }
        let (node, capture) = match program.sites[site.index()] {
            FixedClassicSite::Inert => unreachable!(),
            FixedClassicSite::Wire { node } => (node, FixedCapture::Wire(backend.wire_state(node))),
            FixedClassicSite::Gate(node) => {
                let state = backend.node_state(node);
                (
                    node,
                    FixedCapture::Gate {
                        powered: state.powered,
                        locked: state.repeater_locked,
                    },
                )
            }
        };
        let slot = self.sites.len();
        self.slots[site.index()] = slot as u32;
        self.sites.push(WalkSite {
            site,
            node,
            capture,
            visited_wire: false,
            x_bias: 0,
            z_bias: 0,
            layer: 0,
            heading: NO_HEADING,
            neighbors: NO_SLOT,
        });
        Some(slot)
    }
}

impl GraphBackend {
    pub(super) fn fixed_classic_input_strength(&self, node: NodeId, port: InputPort) -> u8 {
        let program = self.fixed_classic.as_deref().expect("fixed query requires an attached fixed program");
        let query = program.nodes[node.index()].inputs[port as usize];
        self.fixed_classic_query(program, query, None)
    }

    pub(super) fn fixed_classic_seed(&mut self, node: NodeId, captured: Option<&ObservedSite>) -> Result<(), GraphExecutionError> {
        let program = self.fixed_classic.as_deref().expect("fixed seed requires an attached fixed program");
        #[cfg(test)]
        {
            self.test_recorder.fixed_classic_counts.seed_calls += 1;
        }
        let wire_index = match program.nodes[node.index()].kind {
            FixedClassicKind::Wire(index) => index as usize,
            _ => unreachable!("fixed wire seed received a gate"),
        };
        let wire_plan = &program.wires[wire_index];
        #[cfg(test)]
        if let Some(activity) = self.test_recorder.fixed_classic_activity.get_mut(wire_index) {
            activity[0] += 1;
        }
        let mut wire = captured
            .and_then(|site| match site.properties {
                ObservedProperties::Wire(wire) => Some(wire),
                _ => None,
            })
            .unwrap_or_else(|| self.wire_state(node));
        let power = self.fixed_classic_seed_power(program, wire_plan);
        if wire.power == power {
            return Ok(());
        }
        #[cfg(test)]
        if let Some(activity) = self.test_recorder.fixed_classic_activity.get_mut(wire_index) {
            activity[1] += 1;
        }
        // Only a changing seed enters recursive execution and needs an owned handle.
        let program = self.fixed_classic.clone().unwrap();
        wire.power = power;
        self.set_wire(node, wire);
        #[cfg(test)]
        {
            self.test_recorder.fixed_classic_counts.episodes += 1;
        }

        let mut scratch = self
            .fixed_classic_scratch
            .pop()
            .unwrap_or_else(|| FixedClassicScratch::new(program.sites.len()));
        let result = self.fixed_classic_run_wire(&program, node, &mut scratch);
        scratch.clear();
        self.fixed_classic_scratch.push(scratch);
        result
    }

    pub(super) fn fixed_classic_shape(
        &mut self,
        node: NodeId,
        direction: BlockFace,
        captured: Option<&ObservedSite>,
    ) -> Result<(), GraphExecutionError> {
        let (raw_sides, shape_route) = {
            let program = self.fixed_classic.as_deref().expect("fixed shape requires an attached fixed program");
            let wire_index = match program.nodes[node.index()].kind {
                FixedClassicKind::Wire(index) => index as usize,
                _ => unreachable!("fixed wire shape received a gate"),
            };
            let wire = &program.wires[wire_index];
            (wire.raw_sides, wire.shape_route)
        };
        let wire = captured
            .and_then(|site| match site.properties {
                ObservedProperties::Wire(wire) => Some(wire),
                _ => None,
            })
            .unwrap_or_else(|| self.wire_state(node));
        let raw = Self::fixed_classic_raw_sides(raw_sides, wire.power);
        let updated = crate::redstone::wire::on_neighbor_changed_from(wire, raw, direction);
        if updated != wire {
            self.set_wire(node, updated);
            if let Some(route) = shape_route {
                self.fixed_classic_route(route, None)?;
            }
        }
        Ok(())
    }

    pub(super) fn fixed_classic_route(&mut self, route: RouteId, _captured: Option<&[Option<ObservedSite>]>) -> Result<(), GraphExecutionError> {
        let range = self
            .fixed_classic
            .as_deref()
            .expect("fixed route requires an attached fixed program")
            .routes[route.index()]
        .clone();
        for index in range {
            let operation = self.fixed_classic.as_deref().unwrap().ops[index];
            #[cfg(test)]
            {
                self.test_recorder.fixed_classic_counts.route_ops += 1;
            }
            self.counts.callbacks += 1;
            #[cfg(test)]
            if self.test_recorder.callback_census_enabled {
                let kind = match operation.kind {
                    FixedClassicOpKind::Shape(direction) => DeliveryKind::Shape { direction },
                    FixedClassicOpKind::Seed | FixedClassicOpKind::Update { .. } => DeliveryKind::Update { direction: None },
                };
                self.record_accepted_callback(kind, CallbackReceiver::Component(operation.target));
            }
            match operation.kind {
                FixedClassicOpKind::Seed => self.fixed_classic_seed(operation.target, None)?,
                FixedClassicOpKind::Shape(direction) => self.fixed_classic_shape(operation.target, direction, None)?,
                FixedClassicOpKind::Update { direction: _ } => self.fixed_classic_update(operation.target, None)?,
            }
        }
        Ok(())
    }

    pub(super) fn fixed_classic_tick(&mut self, node: NodeId) -> Result<(), GraphExecutionError> {
        let kind = self
            .fixed_classic
            .as_deref()
            .expect("fixed tick requires an attached fixed program")
            .nodes[node.index()]
        .kind;
        match kind {
            FixedClassicKind::Wire(_) => Ok(()),
            FixedClassicKind::Torch => {
                #[cfg(test)]
                {
                    self.test_recorder.fixed_classic_counts.gate_ops += 1;
                }
                self.tick_torch(node)
            }
            FixedClassicKind::Repeater { delay, .. } => {
                #[cfg(test)]
                {
                    self.test_recorder.fixed_classic_counts.gate_ops += 1;
                }
                self.tick_repeater(node, delay)
            }
            FixedClassicKind::Comparator { mode, facing_diode } => {
                #[cfg(test)]
                {
                    self.test_recorder.fixed_classic_counts.gate_ops += 1;
                }
                self.fixed_classic_tick_comparator(node, mode, facing_diode)
            }
            FixedClassicKind::Lamp => {
                #[cfg(test)]
                {
                    self.test_recorder.fixed_classic_counts.gate_ops += 1;
                }
                self.tick_lamp(node)
            }
            FixedClassicKind::Hopper | FixedClassicKind::Lever | FixedClassicKind::PressurePlate | FixedClassicKind::Constant => Ok(()),
            FixedClassicKind::Button => {
                #[cfg(test)]
                {
                    self.test_recorder.fixed_classic_counts.gate_ops += 1;
                }
                if self.node_state(node).powered {
                    self.effects.push(PresentationEffect::Sound {
                        position: self.position(node),
                        id: crate::sound::event_id("block.stone_button.click_off"),
                        category: 4,
                        volume: 1.0,
                        pitch: 1.0,
                    });
                    self.set_node(node, false, 0, None, true)?;
                }
                Ok(())
            }
        }
    }

    fn fixed_classic_seed_power(&self, program: &FixedClassicProgram, wire: &FixedClassicWire) -> u8 {
        let block_power = self.fixed_classic_query(program, wire.block_input, None);
        if block_power == 15 {
            return 15;
        }
        let mut wire_power = 0;
        if block_power < 15 {
            let mut inputs = wire.seed_mask;
            while inputs != 0 {
                let slot = inputs.trailing_zeros() as usize;
                inputs &= inputs - 1;
                let FixedClassicSite::Wire { node } = program.sites[wire.neighbors[slot].index()] else {
                    unreachable!("compiled seed mask references a non-wire site");
                };
                wire_power = wire_power.max(self.wire_state(node).power);
            }
        }
        block_power.max(wire_power.saturating_sub(1))
    }

    fn fixed_classic_run_wire(
        &mut self,
        program: &FixedClassicProgram,
        node: NodeId,
        scratch: &mut FixedClassicScratch,
    ) -> Result<(), GraphExecutionError> {
        #[cfg(test)]
        {
            self.test_recorder.fixed_classic_counts.wire_steps += 1;
        }
        let wire_index = match program.nodes[node.index()].kind {
            FixedClassicKind::Wire(index) => index as usize,
            _ => unreachable!("fixed Turbo received a gate"),
        };
        let root = program.wires[wire_index].root;
        let root_slot = scratch.capture(self, program, root).expect("wire root is a captured wire site");
        scratch.sites[root_slot].visited_wire = true;
        self.fixed_classic_propagate(program, wire_index, scratch, 0);
        Self::fixed_classic_shift_queues(scratch);
        scratch.layer = 1;
        while !scratch.queues[0].is_empty() || !scratch.queues[1].is_empty() {
            let count = scratch.queues[0].len();
            for index in 0..count {
                let slot = scratch.queues[0][index];
                self.fixed_classic_dispatch_site(program, slot, scratch)?;
            }
            Self::fixed_classic_shift_queues(scratch);
            scratch.layer += 1;
        }
        Ok(())
    }

    fn fixed_classic_dispatch_site(
        &mut self,
        program: &FixedClassicProgram,
        slot: u32,
        scratch: &mut FixedClassicScratch,
    ) -> Result<(), GraphExecutionError> {
        let captured = scratch.sites[slot as usize];
        let node = captured.node;
        match captured.capture {
            FixedCapture::Wire(_) => {
                self.counts.callbacks += 1;
                #[cfg(test)]
                if self.test_recorder.callback_census_enabled {
                    self.record_accepted_callback(DeliveryKind::Update { direction: None }, CallbackReceiver::Component(node));
                }
                #[cfg(test)]
                {
                    self.test_recorder.fixed_classic_counts.wire_steps += 1;
                }
                self.fixed_classic_evaluate_wire_node(program, node, scratch)
            }
            FixedCapture::Gate { powered, locked } => {
                self.counts.callbacks += 1;
                #[cfg(test)]
                if self.test_recorder.callback_census_enabled {
                    self.record_accepted_callback(DeliveryKind::Update { direction: None }, CallbackReceiver::Component(node));
                }
                let observed = ObservedSite {
                    profile: None,
                    receiver: Some(CallbackReceiver::Component(node)),
                    properties: ObservedProperties::Electrical { powered, locked },
                };
                self.fixed_classic_update(node, Some(&observed))
            }
        }
    }

    fn fixed_classic_propagate(&mut self, program: &FixedClassicProgram, wire_index: usize, scratch: &mut FixedClassicScratch, layer: u32) {
        let wire = &program.wires[wire_index];
        let neighbors = self.fixed_classic_identify_neighbors(program, wire_index, scratch);
        let root_slot = scratch.slot(wire.root).unwrap();
        let heading = scratch.sites[root_slot].heading as usize;
        let neighbor_slots = &scratch.neighbors[neighbors];
        for &canonical_slot in &crate::redstone::wire::TURBO_ORDER[heading] {
            // Preserve layer state even for currently unadmitted sites: another
            // wire may admit the same SiteId later in this walk. Inert sites are
            // the only descriptors omitted from scratch and have no effect.
            let walk_slot = neighbor_slots[canonical_slot];
            if walk_slot == NO_SLOT {
                continue;
            }
            let walk_slot = walk_slot as usize;
            let next_layer = layer + 1;
            let changed = if next_layer > scratch.sites[walk_slot].layer {
                scratch.sites[walk_slot].layer = next_layer;
                true
            } else {
                false
            };
            if changed && wire.admitted & (1 << canonical_slot) != 0 {
                scratch.queues[1].push(walk_slot as u32);
            }
        }
        for &canonical_slot in &crate::redstone::wire::TURBO_ORDER[heading][..4] {
            let walk_slot = neighbor_slots[canonical_slot];
            if walk_slot == NO_SLOT {
                continue;
            }
            let walk_slot = walk_slot as usize;
            let next_layer = layer + 2;
            let changed = if next_layer > scratch.sites[walk_slot].layer {
                scratch.sites[walk_slot].layer = next_layer;
                true
            } else {
                false
            };
            if changed && wire.admitted & (1 << canonical_slot) != 0 {
                scratch.queues[2].push(walk_slot as u32);
            }
        }
    }

    /// Return the local neighborhood index, preserving its first capture and heading.
    fn fixed_classic_identify_neighbors(&self, program: &FixedClassicProgram, wire_index: usize, scratch: &mut FixedClassicScratch) -> usize {
        let wire = &program.wires[wire_index];
        let root_slot = scratch.capture(self, program, wire.root).expect("wire root is a captured wire site");
        let cached = scratch.sites[root_slot].neighbors;
        if cached != NO_SLOT {
            return cached as usize;
        }
        // Eager capture keeps canonical order, including unadmitted mutable sites.
        // Inert sites have no captured state and need no runtime descriptor lookup.
        let mut slots = [NO_SLOT; 24];
        let mut pending = wire.mutable_neighbors;
        while pending != 0 {
            let canonical_slot = pending.trailing_zeros() as usize;
            pending &= pending - 1;
            slots[canonical_slot] = scratch
                .capture(self, program, wire.neighbors[canonical_slot])
                .expect("compiled mutable mask references a mutable site") as u32;
        }
        let visited = slots.map(|slot| slot != NO_SLOT && scratch.sites[slot as usize].visited_wire);
        let from_west = visited[0] || visited[7] || visited[8];
        let from_east = visited[1] || visited[12] || visited[13];
        let from_north = visited[4] || visited[17] || visited[20];
        let from_south = visited[5] || visited[18] || visited[21];
        let mut x_bias = i32::from(from_west) - i32::from(from_east);
        let mut z_bias = i32::from(from_north) - i32::from(from_south);
        let (old_x_bias, old_z_bias) = {
            let root = scratch.sites[root_slot];
            (i32::from(root.x_bias), i32::from(root.z_bias))
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
        for slot in slots.into_iter().filter(|slot| *slot != NO_SLOT) {
            scratch.sites[slot as usize].x_bias = x_bias as i8;
            scratch.sites[slot as usize].z_bias = z_bias as i8;
        }
        scratch.sites[root_slot].heading = heading as u8;
        let index = scratch.neighbors.len();
        scratch.neighbors.push(slots);
        scratch.sites[root_slot].neighbors = index as u32;
        index
    }

    fn fixed_classic_shift_queues(scratch: &mut FixedClassicScratch) {
        scratch.queues.rotate_left(1);
        // The consumed layer becomes the spare buffer; retain its allocation.
        scratch.queues[2].clear();
    }

    fn fixed_classic_turbo_power(
        &self,
        program: &FixedClassicProgram,
        wire: &FixedClassicWire,
        scratch: &FixedClassicScratch,
        neighbors: usize,
    ) -> u8 {
        let block_power = self.fixed_classic_query(program, wire.block_input, Some(scratch));
        if block_power == 15 {
            return 15;
        }
        let mut wire_power = 0;
        if block_power < 15 {
            let mut inputs = wire.turbo_mask;
            while inputs != 0 {
                let canonical_slot = inputs.trailing_zeros() as usize;
                inputs &= inputs - 1;
                let walk_slot = scratch.neighbors[neighbors][canonical_slot];
                debug_assert_ne!(walk_slot, NO_SLOT, "compiled Turbo mask references an undiscovered site");
                let FixedCapture::Wire(input) = scratch.sites[walk_slot as usize].capture else {
                    unreachable!("compiled Turbo mask references a non-wire site");
                };
                wire_power = wire_power.max(input.power);
            }
        }
        block_power.max(wire_power.saturating_sub(1))
    }

    /// A queued wire is evaluated inside its caller's walk. It keeps the
    /// caller's capture table, heading, layers, and FIFO frontier intact.
    fn fixed_classic_evaluate_wire_node(
        &mut self,
        program: &FixedClassicProgram,
        node: NodeId,
        scratch: &mut FixedClassicScratch,
    ) -> Result<(), GraphExecutionError> {
        let wire_index = match program.nodes[node.index()].kind {
            FixedClassicKind::Wire(index) => index as usize,
            _ => unreachable!("fixed wire visit received a gate"),
        };
        let wire_plan = &program.wires[wire_index];
        let root_slot = scratch.capture(self, program, wire_plan.root).expect("wire root is a captured wire site");
        #[cfg(test)]
        if let Some(activity) = self.test_recorder.fixed_classic_activity.get_mut(wire_index) {
            activity[2] += 1;
        }
        scratch.sites[root_slot].visited_wire = true;
        let neighbors = self.fixed_classic_identify_neighbors(program, wire_index, scratch);
        let FixedCapture::Wire(mut wire) = scratch.sites[root_slot].capture else {
            unreachable!("fixed wire root has a gate capture");
        };
        let power = self.fixed_classic_turbo_power(program, wire_plan, scratch, neighbors);
        if wire.power == power {
            return Ok(());
        }
        #[cfg(test)]
        if let Some(activity) = self.test_recorder.fixed_classic_activity.get_mut(wire_index) {
            activity[3] += 1;
        }
        wire.power = power;
        scratch.sites[root_slot].capture = FixedCapture::Wire(wire);
        self.set_wire(node, wire);
        let layer = scratch.layer;
        self.fixed_classic_propagate(program, wire_index, scratch, layer);
        Ok(())
    }

    fn fixed_classic_query(&self, program: &FixedClassicProgram, query: FixedClassicQuery, scratch: Option<&FixedClassicScratch>) -> u8 {
        if query.constant == 15 || query.start == query.end {
            return query.constant;
        }
        let mut maximum = query.constant;
        for operand in &program.operands[query.start as usize..query.end as usize] {
            let (source, node) = match operand.source {
                FixedClassicSource::LiveOutput(node) => (self.fixed_classic_live_output(node), node),
                FixedClassicSource::CapturedPowered { site, node } => (self.fixed_classic_captured_powered(program, scratch, site, node), node),
            };
            if operand.require_cross
                && matches!(program.nodes[node.index()].kind, FixedClassicKind::Wire(_))
                && crate::redstone::wire::is_dot(self.wire_state(node))
            {
                continue;
            }
            maximum = maximum.max(source.saturating_sub(operand.attenuation));
            if maximum == 15 {
                break;
            }
        }
        maximum
    }

    fn fixed_classic_live_output(&self, node: NodeId) -> u8 {
        match &self.state.components[node.index()] {
            ComponentState::Electrical(state) => state.output_strength,
            ComponentState::Wire(_) => self.wire_state(node).power,
            ComponentState::Specialized => 0,
        }
    }

    fn fixed_classic_captured_powered(&self, program: &FixedClassicProgram, scratch: Option<&FixedClassicScratch>, site: SiteId, node: NodeId) -> u8 {
        if let Some(scratch) = scratch
            && let Some(slot) = scratch.slot(site)
        {
            return match scratch.sites[slot].capture {
                FixedCapture::Wire(wire) => wire.power,
                FixedCapture::Gate { powered, .. } => match program.nodes[node.index()].kind {
                    FixedClassicKind::Constant | FixedClassicKind::Comparator { .. } => self.fixed_classic_live_output(node),
                    _ => u8::from(powered) * 15,
                },
            };
        }
        match program.nodes[node.index()].kind {
            FixedClassicKind::Wire(_) => self.wire_state(node).power,
            FixedClassicKind::Comparator { .. } | FixedClassicKind::Constant => self.fixed_classic_live_output(node),
            _ => u8::from(self.node_state(node).powered) * 15,
        }
    }

    fn fixed_classic_update(&mut self, node: NodeId, captured: Option<&ObservedSite>) -> Result<(), GraphExecutionError> {
        let kind = self
            .fixed_classic
            .as_deref()
            .expect("fixed update requires an attached fixed program")
            .nodes[node.index()]
        .kind;
        match kind {
            FixedClassicKind::Wire(_) => self.fixed_classic_seed(node, captured),
            FixedClassicKind::Torch => {
                #[cfg(test)]
                {
                    self.test_recorder.fixed_classic_counts.gate_ops += 1;
                }
                self.update_torch(node, captured)
            }
            FixedClassicKind::Repeater { .. } => {
                #[cfg(test)]
                {
                    self.test_recorder.fixed_classic_counts.gate_ops += 1;
                }
                self.update_repeater(node, captured)
            }
            FixedClassicKind::Comparator { .. } => {
                #[cfg(test)]
                {
                    self.test_recorder.fixed_classic_counts.gate_ops += 1;
                }
                self.fixed_classic_update_comparator(node)
            }
            FixedClassicKind::Lamp => {
                #[cfg(test)]
                {
                    self.test_recorder.fixed_classic_counts.gate_ops += 1;
                }
                self.update_lamp(node, captured)
            }
            FixedClassicKind::Hopper => {
                #[cfg(test)]
                {
                    self.test_recorder.fixed_classic_counts.gate_ops += 1;
                }
                self.update_hopper(node, captured)
            }
            FixedClassicKind::Lever | FixedClassicKind::Button | FixedClassicKind::PressurePlate | FixedClassicKind::Constant => {
                #[cfg(test)]
                {
                    self.test_recorder.fixed_classic_counts.gate_ops += 1;
                }
                Ok(())
            }
        }
    }

    fn fixed_classic_comparator_input(&self, node: NodeId) -> u8 {
        let fixed = self.fixed_classic.as_ref().unwrap().nodes[node.index()]
            .comparator
            .expect("fixed comparator has a compiled input plan");
        if let Some(source) = fixed.direct_override {
            return self.fixed_classic_override(source);
        }
        let rear = self.input_strength(node, InputPort::Main);
        if rear >= fixed.far_threshold {
            return rear;
        }
        fixed.far_override.map_or(rear, |source| self.fixed_classic_override(source))
    }

    fn fixed_classic_override(&self, value: ComparatorOverride) -> u8 {
        match value {
            ComparatorOverride::Literal(value) => value,
            ComparatorOverride::LiveComponentOutput(node) => self.fixed_classic_live_output(node),
        }
    }

    fn fixed_classic_update_comparator(&mut self, node: NodeId) -> Result<(), GraphExecutionError> {
        #[cfg(test)]
        if self.test_recording {
            self.test_recorder.comparator_updates.push(self.position(node));
        }
        if self.is_pending(node) {
            return Ok(());
        }
        let input = self.fixed_classic_comparator_input(node);
        let side = self.input_strength(node, InputPort::Side);
        let kind = self.fixed_classic.as_ref().unwrap().nodes[node.index()].kind;
        let (mode, facing_diode) = match kind {
            FixedClassicKind::Comparator { mode, facing_diode } => (mode, facing_diode),
            _ => unreachable!(),
        };
        let strength = crate::redpiler::power_rules::comparator_output_strength(input, side, mode);
        let powered = crate::redpiler::power_rules::comparator_is_powered(input, side, mode);
        let current = self.node_state(node);
        if strength != current.output_strength || powered != current.powered {
            self.schedule(node, 1, if facing_diode { TickPriority::High } else { TickPriority::Normal });
        }
        Ok(())
    }

    fn fixed_classic_tick_comparator(&mut self, node: NodeId, mode: ComparatorMode, _facing_diode: bool) -> Result<(), GraphExecutionError> {
        let input = self.fixed_classic_comparator_input(node);
        let side = self.input_strength(node, InputPort::Side);
        let strength = crate::redpiler::power_rules::comparator_output_strength(input, side, mode);
        let current = self.node_state(node);
        if strength != current.output_strength || mode == ComparatorMode::Compare {
            let powered = crate::redpiler::power_rules::comparator_is_powered(input, side, mode);
            self.set_node(node, powered, strength, None, false)?;
            self.fire_route(node)?;
        }
        Ok(())
    }

    fn fixed_classic_raw_sides(bits: u8, power: u8) -> RedstoneWire {
        let side = |shift: u32| RedstoneWireSide::from_id(u32::from((bits >> shift) & 3u8));
        RedstoneWire::new(side(0), side(2), side(4), side(6), power)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plot::PlotWorld;
    use crate::world::storage::Chunk;
    use mchprs_blocks::BlockDirection;
    use mchprs_blocks::blocks::{Lever, LeverFace, RedstoneComparator};

    #[test]
    fn contiguous_frontiers_preserve_fifo_duplicates_and_two_layer_deliveries() {
        let mut scratch = FixedClassicScratch::new(0);
        scratch.queues[1].extend([7, 3, 7]);
        scratch.queues[2].extend([9, 2]);
        GraphBackend::fixed_classic_shift_queues(&mut scratch);
        let mut delivered = Vec::new();
        for index in 0..scratch.queues[0].len() {
            let slot = scratch.queues[0][index];
            delivered.push(slot);
            if slot == 7 {
                scratch.queues[1].push(11);
                scratch.queues[2].push(20);
            }
        }
        assert_eq!(delivered, [7, 3, 7]);
        GraphBackend::fixed_classic_shift_queues(&mut scratch);
        assert_eq!(scratch.queues[0], [9, 2, 11, 11]);
        assert_eq!(scratch.queues[1], [20, 20]);
        assert!(scratch.queues[2].is_empty());
        GraphBackend::fixed_classic_shift_queues(&mut scratch);
        assert_eq!(scratch.queues[0], [20, 20]);
        scratch.clear();
        assert!(scratch.queues.iter().all(Vec::is_empty));
    }

    #[test]
    fn aggregate_block_input_keeps_live_analog_and_held_powered_samples_across_nested_walks() {
        let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
        let source = BlockPos::new(5, 20, 5);
        let dust = source.offset(BlockFace::East);
        let analog = dust.offset(BlockFace::East);
        for position in [source, dust, analog] {
            world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
        }
        world.set_block(
            source,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
            },
        );
        world.set_block(dust, Block::RedstoneWire { wire: Default::default() });
        world.set_block(
            analog,
            Block::RedstoneComparator {
                comparator: RedstoneComparator::new(BlockDirection::East, ComparatorMode::Compare, true),
            },
        );
        world.set_block_entity(analog, BlockEntity::Comparator { output_strength: 3 });
        let wire = crate::redstone::wire::get_state_for_placement(&world, dust);
        world.set_block(dust, Block::RedstoneWire { wire });
        let mut backend = GraphBackend::compile_with_options(
            &world,
            (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15)),
            vec![],
            &crate::redpiler::CompilerOptions {
                optimization_level: Some(2),
                ..Default::default()
            },
            Arc::default(),
        )
        .unwrap();
        let program = backend.fixed_classic.clone().unwrap();
        let wire_plan = &program.wires[0];
        let node = backend.positions[&source];
        let analog_node = backend.positions[&analog];
        let operands = &program.operands[wire_plan.block_input.start as usize..wire_plan.block_input.end as usize];
        assert!(
            operands
                .iter()
                .any(|operand| { matches!(operand.source, FixedClassicSource::CapturedPowered { node: captured, .. } if captured == node) })
        );
        assert!(
            operands
                .iter()
                .any(|operand| operand.source == FixedClassicSource::LiveOutput(analog_node))
        );
        let assert_block_input = |backend: &GraphBackend, scratch: Option<&FixedClassicScratch>, expected| {
            let separate = wire_plan
                .no_dust
                .iter()
                .map(|query| backend.fixed_classic_query(&program, *query, scratch))
                .max()
                .unwrap();
            assert_eq!(separate, expected);
            assert_eq!(backend.fixed_classic_query(&program, wire_plan.block_input, scratch), separate);
        };
        assert_block_input(&backend, None, 3);
        for constant in [0, 9, 15] {
            assert_eq!(
                backend.fixed_classic_query(
                    &program,
                    FixedClassicQuery {
                        constant,
                        ..Default::default()
                    },
                    None
                ),
                constant
            );
        }
        assert_eq!(
            backend.fixed_classic_query(
                &program,
                FixedClassicQuery {
                    constant: 15,
                    ..wire_plan.block_input
                },
                None
            ),
            15
        );
        let mut outer = FixedClassicScratch::new(program.sites.len());
        let neighbors = backend.fixed_classic_identify_neighbors(&program, 0, &mut outer);
        let source_slot = outer.sites.iter().position(|site| site.node == node).expect("eager source capture");
        let source_site = outer.sites[source_slot].site;
        let source_neighbor = wire_plan.neighbors.iter().position(|site| *site == source_site).unwrap();
        assert_ne!(
            wire_plan.mutable_neighbors & (1 << source_neighbor),
            0,
            "unadmitted source still has capture state"
        );
        assert_eq!(wire_plan.admitted & (1 << source_neighbor), 0, "lever source has no Turbo receiver");
        for (canonical_slot, site) in wire_plan.neighbors.iter().enumerate() {
            let mutable = !matches!(program.sites[site.index()], FixedClassicSite::Inert);
            assert_eq!(wire_plan.mutable_neighbors & (1 << canonical_slot) != 0, mutable);
            assert_eq!(outer.neighbors[neighbors][canonical_slot] != NO_SLOT, mutable);
        }
        assert!(matches!(outer.sites[source_slot].capture, FixedCapture::Gate { powered: false, .. }));
        assert_block_input(&backend, Some(&outer), 3);

        // A later live write must not refresh an existing episode's sample.
        let ComponentState::Electrical(state) = &mut backend.state.components[node.index()] else {
            panic!("source state")
        };
        state.powered = true;
        state.output_strength = 15;
        let ComponentState::Electrical(state) = &mut backend.state.components[analog_node.index()] else {
            panic!("analog source state")
        };
        state.output_strength = 7;
        assert_eq!(backend.fixed_classic_identify_neighbors(&program, 0, &mut outer), neighbors);
        assert!(matches!(outer.sites[source_slot].capture, FixedCapture::Gate { powered: false, .. }));
        // The Boolean source stays held off while the analog operand reads seven live.
        assert_block_input(&backend, Some(&outer), 7);
        assert_block_input(&backend, None, 15);

        let mut nested = FixedClassicScratch::new(program.sites.len());
        let nested_slot = nested.capture(&backend, &program, source_site).unwrap();
        assert!(matches!(nested.sites[nested_slot].capture, FixedCapture::Gate { powered: true, .. }));
        assert!(matches!(outer.sites[source_slot].capture, FixedCapture::Gate { powered: false, .. }));
        assert_block_input(&backend, Some(&nested), 15);
        outer.clear();
        let next = outer.capture(&backend, &program, source_site).unwrap();
        assert!(matches!(outer.sites[next].capture, FixedCapture::Gate { powered: true, .. }));
        assert!(outer.neighbors.is_empty());
    }
}
