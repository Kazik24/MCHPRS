//! Symbolic live/captured power reads; geometry is reduced during compilation.
use super::*;
use crate::redpiler::{components::NodeType, power_rules};

impl Compiler<'_> {
    pub(crate) fn input_power(&mut self, node: NodeId, port: InputPort) -> Result<Signal> {
        self.query_power(PowerTarget::Input { node, port })
    }

    pub(crate) fn query_power(&mut self, target: PowerTarget) -> Result<Signal> {
        if let Some(&(epoch, value)) = self.power.get(&target)
            && epoch == self.epoch
        {
            return Ok(value);
        }
        if !self.active_power.insert(target) {
            return Err(CompileError::RecursivePower(target));
        }
        let result = self.aggregate_power(target);
        self.active_power.remove(&target);
        if let Ok(value) = result {
            self.power.insert(target, (self.epoch, value));
        }
        result
    }

    fn aggregate_power(&mut self, target: PowerTarget) -> Result<Signal> {
        let indices = self.power_inputs.get(&target).cloned().unwrap_or_default();
        let mut maximum = Signal::literal(0)?;
        for index in indices {
            let link = self.graph.power_links[index];
            let enabled = self.guard(link.guard, None)?;
            if enabled == Formula::FALSE {
                continue;
            }
            let value = self.site_power(link.from, link.from_port)?;
            let value = self.logic.attenuate(value, link.attenuation)?;
            let value = self.logic.signal_select(enabled, value, Signal::literal(0)?)?;
            maximum = self.logic.maximum(maximum, value)?;
        }
        Ok(maximum)
    }

    pub(crate) fn site_power(&mut self, site: SiteId, port: PowerPort) -> Result<Signal> {
        if matches!(port, PowerPort::Output | PowerPort::Stored)
            && let SiteSource::Component(node) = self.graph.sites[site.index()].source
        {
            self.symbolize_component(node)?;
            return Ok(self.state.components[node.index()].signal);
        }
        // Conducted reads always follow live neighbors, even if the emitter was captured.
        if let PowerPort::Conducted { include_dust } = port {
            return self.query_power(PowerTarget::Conductor { site, include_dust });
        }
        let observed = self.observe(site)?;
        let mut maximum = Signal::literal(0)?;
        for variant in observed.variants {
            let stored = self.stored_strength(site, &variant)?;
            let emission = match port {
                PowerPort::Weak(face) => self.emits(site, &variant, face, false)?,
                PowerPort::Strong(face) => self.emits(site, &variant, face, true)?,
                PowerPort::Stored | PowerPort::Output => Formula::TRUE,
                PowerPort::Conducted { .. } => unreachable!(),
            };
            let selected = self.logic.and(variant.when, emission)?;
            let value = self.logic.signal_select(selected, stored, Signal::literal(0)?)?;
            maximum = self.logic.maximum(maximum, value)?;
        }
        Ok(maximum)
    }

    fn stored_strength(&mut self, site: SiteId, variant: &SiteVariant) -> Result<Signal> {
        if let Some(CallbackReceiver::Component(node)) = variant.receiver {
            self.symbolize_component(node)?;
            return Ok(self.state.components[node.index()].signal);
        }
        if matches!(self.graph.sites[site.index()].source, SiteSource::Constant(_)) {
            return Ok(Signal::literal(self.graph.sites[site.index()].source_strength)?);
        }
        let source = variant
            .profile
            .is_some_and(|profile| self.graph.profiles[profile.index()].electrical == ProfileElectrical::RedstoneSource);
        Ok(Signal::literal(if source { 15 } else { 0 })?)
    }

    pub(crate) fn power_no_dust(&mut self, site: SiteId, face: BlockFace, captured: Option<&SiteView>) -> Result<Signal> {
        let observed = match captured {
            Some(value) => value.clone(),
            None => self.observe(site)?,
        };
        let mut maximum = Signal::literal(0)?;
        for variant in observed.variants {
            let strength = if variant.solid {
                self.query_power(PowerTarget::Conductor { site, include_dust: false })?
            } else if matches!(variant.block, Block::RedstoneWire { .. }) {
                Signal::literal(0)?
            } else if power_rules::emits_weak(variant.block, face) {
                self.source_strength(site, &variant)?
            } else {
                Signal::literal(0)?
            };
            let selected = self.logic.signal_select(variant.when, strength, Signal::literal(0)?)?;
            maximum = self.logic.maximum(maximum, selected)?;
        }
        Ok(maximum)
    }

    fn source_strength(&mut self, site: SiteId, variant: &SiteVariant) -> Result<Signal> {
        if let Some(CallbackReceiver::Component(node)) = variant.receiver
            && matches!(
                self.graph.nodes[node.index()].kind,
                CircuitNodeKind::Electrical(NodeType::Comparator { .. })
            )
        {
            self.symbolize_component(node)?;
            return Ok(self.state.components[node.index()].signal);
        }
        if matches!(self.graph.sites[site.index()].source, SiteSource::Contact(_)) && variant.component.is_none() {
            return self.stored_strength(site, variant);
        }
        if let Some(component) = &variant.component {
            return match variant.block {
                Block::RedstoneBlock => Ok(Signal::literal(15)?),
                Block::RedstoneTorch { .. }
                | Block::RedstoneWallTorch { .. }
                | Block::Lever { .. }
                | Block::StoneButton { .. }
                | Block::StonePressurePlate { .. }
                | Block::RedstoneRepeater { .. }
                | Block::Observer { .. } => Ok(self.logic.signal_select(component.powered, Signal::literal(15)?, Signal::literal(0)?)?),
                Block::RedstoneWire { .. } => Ok(component.signal),
                block if block.pressure_plate_powered().is_some() => {
                    Ok(self.logic.signal_select(component.powered, Signal::literal(15)?, Signal::literal(0)?)?)
                }
                _ => Ok(Signal::literal(0)?),
            };
        }
        let strength = match variant.block {
            Block::RedstoneTorch { lit: true }
            | Block::RedstoneWallTorch { lit: true, .. }
            | Block::RedstoneBlock
            | Block::StonePressurePlate { powered: true } => 15,
            block if block.pressure_plate_powered() == Some(true) => 15,
            Block::Lever { lever } if lever.powered => 15,
            Block::StoneButton { button } if button.powered => 15,
            Block::RedstoneRepeater { repeater } if repeater.powered => 15,
            Block::Observer { observer } if observer.powered => 15,
            _ => self.graph.sites[site.index()].source_strength,
        };
        Ok(Signal::literal(strength)?)
    }

    fn emits(&mut self, site: SiteId, variant: &SiteVariant, face: BlockFace, strong: bool) -> Result<Formula> {
        if matches!(variant.block, Block::RedstoneWire { .. })
            && let SiteSource::Component(node) = self.graph.sites[site.index()].source
        {
            return self.wire_emits(node, face);
        }
        let value = if strong {
            power_rules::emits_strong(variant.block, face)
        } else {
            power_rules::emits_weak(variant.block, face)
        };
        Ok(if value { Formula::TRUE } else { Formula::FALSE })
    }
}
