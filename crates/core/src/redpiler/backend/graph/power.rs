use super::*;
use crate::redpiler::power_rules::{emits_strong, emits_weak};

impl GraphBackend {
    fn site_source_strength(&self, site: SiteId, resolved: &ResolvedSiteView) -> u8 {
        #[cfg(test)]
        if let Some(tapes) = &self.test_recorder.wire_tapes {
            tapes.source_site(self, site);
        }
        if let Some(proof) = &self.classic_proof
            && self.program.sites()[site.index()].source == SiteSource::Component(proof.source)
        {
            return proof.signal;
        }
        if let Some(CallbackReceiver::Component(node)) = resolved.receiver {
            if matches!(
                self.program.nodes()[node.index()].kind,
                CircuitNodeKind::Electrical(NodeType::Comparator { .. })
            ) {
                return self.node_state(node).output_strength;
            }
        }
        let fixed_component = matches!(resolved.receiver, Some(CallbackReceiver::Component(_)));
        let selectable_contact = matches!(self.program.sites()[site.index()].source, SiteSource::Contact(_));
        if selectable_contact && !fixed_component {
            return self.contact_or_boundary_strength(site, resolved.profile);
        }
        match resolved.block {
            Block::RedstoneTorch { lit: true } | Block::RedstoneWallTorch { lit: true, .. } => 15,
            Block::RedstoneBlock => 15,
            Block::StonePressurePlate { powered: true } => 15,
            block if block.pressure_plate_powered() == Some(true) => 15,
            Block::Lever { lever } if lever.powered => 15,
            Block::StoneButton { button } if button.powered => 15,
            Block::RedstoneRepeater { repeater } if repeater.powered => 15,
            Block::RedstoneComparator { .. } => self.program.sites()[site.index()].source_strength,
            Block::RedstoneWire { wire } => wire.power,
            Block::Observer { observer } if observer.powered => 15,
            _ if resolved.receiver.is_none() => self.program.sites()[site.index()].source_strength,
            _ => 0,
        }
    }

    /// Read a wire's non-dust input using live or captured emitter properties.
    /// Strong power conducted through a solid block is always read live.
    pub(super) fn power_no_dust(&self, site: SiteId, face: BlockFace, captured_emitter: Option<&ObservedSite>) -> u8 {
        let resolved = self
            .resolve_site_view(site, captured_emitter)
            .expect("wire query site was validated during preparation");
        if resolved.solid {
            return self.strong_power_at(site);
        }
        if matches!(resolved.block, Block::RedstoneWire { .. }) {
            return 0;
        }
        let strength = self.site_source_strength(site, &resolved);
        if strength > 0 && emits_weak(resolved.block, face) { strength } else { 0 }
    }

    fn strong_power_at(&self, site: SiteId) -> u8 {
        self.conducted_power(site, false)
    }

    /// Read a component's main, side, or quasi input strength.
    /// Uses a specialized query when available, otherwise follows the power links.
    pub(super) fn input_strength(&self, node: NodeId, port: InputPort) -> u8 {
        if let Some(binding) = self
            .active_frame_power_inputs
            .iter()
            .find(|binding| binding.node == node && binding.port == port)
        {
            let Some(reset_source) = binding.reset else {
                return binding.data_strength;
            };
            let target = PowerTarget::Input { node, port };
            let mut reset_strength = 0u8;
            for link in self.program.power_links_to(target) {
                if !self.guard_matches(link.guard, None) {
                    continue;
                }
                if self.frame_reset_source_matches(reset_source, link) {
                    reset_strength = reset_strength.max(self.link_strength(link));
                    continue;
                }
                let SiteSource::Constant(profile) = self.program.sites()[link.from.index()].source else {
                    continue;
                };
                if !self.program.profiles()[profile.index()].solid || !self.program.profiles()[profile.index()].cube {
                    continue;
                }
                let PowerPort::Conducted { include_dust } = link.from_port else {
                    continue;
                };
                let conductor = PowerTarget::Conductor {
                    site: link.from,
                    include_dust,
                };
                for reset_link in self.program.power_links_to(conductor) {
                    if self.frame_reset_source_matches(reset_source, reset_link) && self.guard_matches(reset_link.guard, None) {
                        reset_strength = reset_strength.max(self.link_strength(reset_link).saturating_sub(link.attenuation));
                    }
                }
            }
            return binding.data_strength.max(reset_strength);
        }
        if self.fixed_classic.is_some() {
            return self.fixed_classic_input_strength(node, port);
        }
        if port == InputPort::Quasi
            && let Some(read) = self.program.bud_memory_read(node)
        {
            let state = self.state.resources[read.resource.index()];
            return u8::from(state.enabled && state.at == read.source) * 15;
        }
        if port == InputPort::Main
            && let Some((net, attenuation, source_attenuation)) = self.program.classic().input_by_node.get(node.index()).copied().flatten()
            && self.state.classic_active[net as usize]
        {
            let source = self.program.classic().nets[net as usize].source;
            return self.state.classic_signals[net as usize]
                .saturating_sub(attenuation)
                .max(self.node_state(source).output_strength.saturating_sub(source_attenuation));
        }
        self.query_power(PowerTarget::Input { node, port })
    }

    /// Read the retained actor's data input with its certified reset contribution
    /// removed. This is used only to seed the frame adapter; subsequent computes
    /// replace it with the reset-excluded equation output.
    pub(super) fn input_strength_without_frame_reset(&self, node: NodeId, port: InputPort, reset: super::logical_device::FrameResetSource) -> u8 {
        let target = PowerTarget::Input { node, port };
        let mut maximum = 0;
        for link in self.program.power_links_to(target) {
            if !self.guard_matches(link.guard, None) || self.frame_reset_source_matches(reset, link) {
                continue;
            }
            if let PowerPort::Conducted { include_dust } = link.from_port {
                let conductor = PowerTarget::Conductor {
                    site: link.from,
                    include_dust,
                };
                for reset_link in self.program.power_links_to(conductor) {
                    if self.guard_matches(reset_link.guard, None) && !self.frame_reset_source_matches(reset, reset_link) {
                        maximum = maximum.max(self.link_strength(reset_link).saturating_sub(link.attenuation));
                    }
                }
            } else {
                maximum = maximum.max(self.link_strength(link));
            }
        }
        maximum
    }

    fn frame_reset_source_matches(&self, reset: super::logical_device::FrameResetSource, link: &PowerLink) -> bool {
        match reset {
            super::logical_device::FrameResetSource::Observer(node) => {
                matches!(link.from_port, PowerPort::Output | PowerPort::Stored)
                    && matches!(self.program.sites()[link.from.index()].source, SiteSource::Component(source) if source == node)
            }
            super::logical_device::FrameResetSource::InputRestoration { resource, contact } => {
                let resource_state = self.state.resources[resource.index()];
                resource_state.at == contact
                    && self
                        .program
                        .sites()
                        .get(link.from.index())
                        .is_some_and(|site| site.source == SiteSource::Contact(contact))
            }
        }
    }

    /// Return the strongest allowed input after attenuation, or zero if there are none.
    /// The general path checks guards against live state. This runs no callbacks.
    fn query_power(&self, target: PowerTarget) -> u8 {
        if let Some((constant, inputs)) = self.program.fixed_signal_query(target) {
            return inputs.iter().fold(constant, |maximum, input| {
                if input.require_cross && crate::redstone::wire::is_dot(self.wire_state(input.node)) {
                    return maximum;
                }
                maximum.max(self.stored_component_strength(input.node).saturating_sub(input.attenuation))
            });
        }
        let mut maximum = 0;
        for link in self.program.power_links_to(target) {
            #[cfg(test)]
            self.test_recorder
                .power_links_examined
                .set(self.test_recorder.power_links_examined.get() + 1);
            if !self.guard_matches(link.guard, None) {
                continue;
            }
            maximum = maximum.max(self.link_strength(link));
        }
        maximum
    }

    fn link_strength(&self, link: &PowerLink) -> u8 {
        self.site_power(link.from, link.from_port).saturating_sub(link.attenuation)
    }

    pub(super) fn site_power(&self, site: SiteId, port: PowerPort) -> u8 {
        if let SiteSource::Component(node) = self.program.sites()[site.index()].source {
            if matches!(port, PowerPort::Output | PowerPort::Stored) {
                return self.stored_component_strength(node);
            }
            if let PowerPort::Weak(face) | PowerPort::Strong(face) = port
                && let CircuitNodeKind::Wire(index) = self.program.nodes()[node.index()].kind
                && let Some(shape) = self.program.wires()[index].execution.shape
                && let Some(emits) = match face {
                    BlockFace::Top => Some(true),
                    BlockFace::Bottom => Some(false),
                    _ => shape.emits(face.unwrap_direction().opposite()),
                }
            {
                return if emits { self.stored_component_strength(node) } else { 0 };
            }
        }
        let Some(resolved) = self.resolve_site(site, None) else {
            return 0;
        };
        let stored = match resolved.receiver {
            Some(CallbackReceiver::Component(node)) => self.stored_component_strength(node),
            Some(CallbackReceiver::PistonBase(_) | CallbackReceiver::PistonHead(_) | CallbackReceiver::Contact(_)) | None => {
                self.contact_or_boundary_strength(site, resolved.profile)
            }
        };
        match port {
            PowerPort::Output | PowerPort::Stored => stored,
            PowerPort::Weak(face) => {
                if self.emits_weak(site, resolved.block, face) {
                    stored
                } else {
                    0
                }
            }
            PowerPort::Strong(face) => {
                if self.emits_strong(site, resolved.block, face) {
                    stored
                } else {
                    0
                }
            }
            PowerPort::Conducted { include_dust } => self.conducted_power(site, include_dust),
        }
    }

    pub(super) fn stored_component_strength(&self, node: NodeId) -> u8 {
        #[cfg(test)]
        if let Some(tapes) = &self.test_recorder.wire_tapes {
            tapes.read(self, node);
        }
        match &self.state.components[node.index()] {
            ComponentState::Electrical(state) => state.output_strength,
            ComponentState::Wire(_) => self.wire_state(node).power,
            ComponentState::Specialized => 0,
        }
    }

    fn contact_or_boundary_strength(&self, site: SiteId, profile: Option<ProfileId>) -> u8 {
        if matches!(self.program.sites()[site.index()].source, SiteSource::Constant(_)) {
            return self.program.sites()[site.index()].source_strength;
        }
        let Some(profile) = profile else {
            return 0;
        };
        match self.program.profiles()[profile.index()].electrical {
            ProfileElectrical::RedstoneSource => 15,
            ProfileElectrical::None | ProfileElectrical::Component => 0,
        }
    }

    fn emits_weak(&self, site: SiteId, block: Block, side: BlockFace) -> bool {
        let Block::RedstoneWire { wire } = block else {
            return emits_weak(block, side);
        };
        if side == BlockFace::Top {
            return true;
        }
        if side == BlockFace::Bottom {
            return false;
        }
        let SiteSource::Component(node) = self.program.sites()[site.index()].source else {
            // shortcut: moved wire contacts lack per-position plans until Step 08.
            return crate::redstone::wire::get_current_side(wire, side.unwrap_direction().opposite()) != RedstoneWireSide::None;
        };
        let CircuitNodeKind::Wire(_) = self.program.nodes()[node.index()].kind else {
            return crate::redstone::wire::get_current_side(wire, side.unwrap_direction().opposite()) != RedstoneWireSide::None;
        };
        self.wire_emits(node, wire, side)
    }

    pub(super) fn wire_emits(&self, node: NodeId, wire: RedstoneWire, side: BlockFace) -> bool {
        if side == BlockFace::Top {
            return true;
        }
        if side == BlockFace::Bottom {
            return false;
        }
        let plan = self.wire_plan(node);
        let raw = self.raw_wire_sides(plan, wire);
        let regulated = crate::redstone::wire::regulate_sides(wire, raw);
        crate::redstone::wire::get_current_side(regulated, side.unwrap_direction().opposite()) != RedstoneWireSide::None
    }

    fn emits_strong(&self, site: SiteId, block: Block, side: BlockFace) -> bool {
        if matches!(block, Block::RedstoneWire { .. }) {
            self.emits_weak(site, block, side)
        } else {
            emits_strong(block, side)
        }
    }

    pub(super) fn conducted_power(&self, site: SiteId, include_dust: bool) -> u8 {
        self.query_power(PowerTarget::Conductor { site, include_dust })
    }
}
