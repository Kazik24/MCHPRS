//! Region ownership and lazily bound inputs for boundary-scoped event compilation.
use super::*;
use crate::redpiler::components::NodeType;
pub(crate) use crate::redpiler::program_graph::{RegionRead, RegionStateSlot};

#[derive(Clone)]
pub(crate) struct RegionScope {
    /// These are component states whose propagation handlers run inside the event plan.
    pub(crate) nodes: Vec<bool>,
    /// These are piston states retained and updated by the event plan.
    pub(crate) actors: Vec<bool>,
    /// These payload states are logical memory outside an instant region unless explicitly owned.
    pub(crate) resources: Vec<bool>,
    /// These contacts resolve to state owned by the region's actors or payloads.
    contacts: Vec<bool>,
}

impl RegionScope {
    /// Build masks from the graph's already validated actor/resource/reset/wire closure.
    pub(crate) fn new(graph: &CompileGraph, region: &InstantRegion) -> Self {
        let mut scope = Self {
            nodes: vec![false; graph.nodes.len()],
            actors: vec![false; graph.pistons.len()],
            resources: vec![false; graph.resources.len()],
            contacts: vec![false; graph.contacts.len()],
        };
        for &actor in &region.actors {
            scope.actors[actor.index()] = true;
            scope.nodes[graph.pistons[actor.index()].node.index()] = true;
        }
        for &resource in &region.resources {
            scope.resources[resource.index()] = true;
        }
        for &contact in &region.contacts {
            scope.contacts[contact.index()] = true;
            scope.nodes[graph.contacts[contact.index()].node.index()] = true;
        }
        for &node in region.resets.iter().chain(&region.wires) {
            scope.nodes[node.index()] = true;
        }
        scope
    }

    /// Check whether a callback receiver belongs to the regional interpreter being replaced.
    pub(crate) fn owns(&self, receiver: CallbackReceiver) -> bool {
        match receiver {
            CallbackReceiver::Component(node) => self.nodes[node.index()],
            CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor) => self.actors[actor.index()],
            CallbackReceiver::Contact(contact) => self.contacts[contact.index()],
        }
    }
}

impl<'a> Compiler<'a> {
    /// Initialize one region's mutable owners; external components become inputs only when read.
    pub(crate) fn new_region(graph: &'a CompileGraph, region: &InstantRegion, limits: Limits) -> Result<Self> {
        Self::new_region_with_cancellation(graph, region, limits, None)
    }

    /// Initialize a scoped compiler that checks task cancellation during every semantic step.
    pub(crate) fn new_region_monitored(
        graph: &'a CompileGraph,
        region: &InstantRegion,
        limits: Limits,
        monitor: &crate::redpiler::TaskMonitor,
    ) -> Result<Self> {
        Self::new_region_with_cancellation(graph, region, limits, Some(monitor.cancellation_flag()))
    }

    fn new_region_with_cancellation(
        graph: &'a CompileGraph,
        region: &InstantRegion,
        limits: Limits,
        cancellation: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    ) -> Result<Self> {
        let mut compiler = Self::with_state(graph, limits, State::initial_region(graph)?, true)?;
        compiler.cancellation = cancellation;
        compiler.region = Some(RegionScope::new(graph, region));
        for &node in region.resets.iter().chain(&region.wires) {
            compiler.symbolize_component(node)?;
        }
        for &actor in &region.actors {
            compiler.symbolize_actor(actor)?;
        }
        for &resource in &region.resources {
            compiler.symbolize_resource(resource)?;
        }
        compiler.region_entry_state = Some(compiler.state.clone());
        Ok(compiler)
    }

    /// Intern one external runtime read so every occurrence sees the same event-time value.
    pub(crate) fn region_input(&mut self, read: RegionRead) -> Result<Formula> {
        if let Some(&value) = self.region_inputs.get(&read) {
            return Ok(value);
        }
        let index = self.region_reads.len();
        let value = self.logic.input(index)?;
        self.region_reads.push(read);
        self.region_inputs.insert(read, value);
        Ok(value)
    }

    /// Import the live pending-membership bit before a reset handler adds or deduplicates work.
    pub(crate) fn pending_input(&mut self, node: NodeId) -> Result<Formula> {
        self.region_input(RegionRead::Pending(node))
    }

    /// Build the exact callback snapshot from runtime payload inputs, never from the later live site.
    pub(crate) fn capture_site(&mut self, site: SiteId) -> Result<SiteView> {
        let present = self.region_input(RegionRead::CapturePresent(site))?;
        match self.graph.sites[site.index()].source {
            SiteSource::Component(node) => {
                let block = Block::from_id(self.graph.nodes[node.index()].raw_state.ok_or(CompileError::UnsupportedNode(node))?);
                let component = self.capture_component(site, node)?;
                Ok(SiteView {
                    variants: vec![SiteVariant {
                        when: present,
                        profile: None,
                        block,
                        solid: block.is_solid(),
                        transparent: block.is_transparent(),
                        receiver: Some(CallbackReceiver::Component(node)),
                        captured_actor_extended: None,
                        component: Some(component),
                    }],
                })
            }
            SiteSource::Constant(profile) => Ok(SiteView {
                variants: vec![self.profile_view(profile, None, present)],
            }),
            SiteSource::Contact(contact) => {
                let alternatives = self.graph.contacts[contact.index()].variants.to_vec();
                let mut variants = Vec::with_capacity(alternatives.len());
                for alternative in alternatives {
                    let selected = self.region_input(RegionRead::CaptureAlternative {
                        site,
                        profile: alternative.profile,
                        receiver: alternative.receiver,
                    })?;
                    let when = self.logic.and(present, selected)?;
                    let mut view = self.profile_view(alternative.profile, alternative.receiver, when);
                    if matches!(
                        alternative.receiver,
                        Some(CallbackReceiver::PistonBase(_) | CallbackReceiver::PistonHead(_))
                    ) {
                        view.captured_actor_extended = Some(self.region_input(RegionRead::CaptureActorExtended(site))?);
                    }
                    if let Some(CallbackReceiver::Component(node)) = alternative.receiver {
                        view.component = Some(self.capture_component(site, node)?);
                    }
                    variants.push(view);
                }
                Ok(SiteView { variants })
            }
        }
    }

    fn capture_component(&mut self, site: SiteId, node: NodeId) -> Result<Component> {
        self.symbolize_component(node)?;
        let mut component = self.state.components[node.index()].clone();
        component.powered = self.region_input(RegionRead::CapturePowered(site))?;
        component.locked = self.region_input(RegionRead::CaptureLocked(site))?;
        if matches!(self.graph.nodes[node.index()].kind, CircuitNodeKind::Wire(_)) {
            let mut bits = [Formula::FALSE; 4];
            for (bit, value) in bits.iter_mut().enumerate() {
                *value = self.region_input(RegionRead::CaptureWireSignalBit { site, bit: bit as u8 })?;
            }
            component.signal = Signal::from_bits(bits);
        }
        if let Some(mut shape) = component.shape {
            for (side, value) in shape.sides.iter_mut().enumerate() {
                let low = self.region_input(RegionRead::CaptureWireSideBit {
                    site,
                    side: side as u8,
                    bit: 0,
                })?;
                value.up = self.region_input(RegionRead::CaptureWireSideBit {
                    site,
                    side: side as u8,
                    bit: 1,
                })?;
                let not_up = self.logic.not(value.up)?;
                value.flat = self.logic.and(low, not_up)?;
            }
            component.shape = Some(shape);
        }
        if matches!(self.graph.nodes[node.index()].kind, CircuitNodeKind::Electrical(NodeType::CopperBulb)) {
            let lit = self.region_input(RegionRead::CaptureLit(site))?;
            component.signal = self.logic.signal_select(lit, Signal::literal(15)?, Signal::literal(0)?)?;
        }
        Ok(component)
    }

    /// Bind a component's runtime scalar state once without replacing later SSA writes.
    pub(crate) fn symbolize_component(&mut self, node: NodeId) -> Result<()> {
        if self.region.is_none() || self.symbolized_nodes[node.index()] {
            return Ok(());
        }
        let mut component = self.state.components[node.index()].clone();
        let mut signal_bits = [Formula::FALSE; 4];
        for (bit, value) in signal_bits.iter_mut().enumerate() {
            *value = self.region_input(RegionRead::ComponentSignal { node, bit: bit as u8 })?;
        }
        component.signal = Signal::from_bits(signal_bits);
        if let Some(mut shape) = component.shape {
            for (side, value) in shape.sides.iter_mut().enumerate() {
                let low = self.region_input(RegionRead::WireSideBit {
                    node,
                    side: side as u8,
                    bit: 0,
                })?;
                value.up = self.region_input(RegionRead::WireSideBit {
                    node,
                    side: side as u8,
                    bit: 1,
                })?;
                let not_up = self.logic.not(value.up)?;
                value.flat = self.logic.and(low, not_up)?;
            }
            component.shape = Some(shape);
            component.powered = Formula::FALSE;
            component.locked = Formula::FALSE;
        } else {
            component.powered = self.region_input(RegionRead::ComponentPowered(node))?;
            component.locked = self.region_input(RegionRead::ComponentLocked(node))?;
        }
        self.state.components[node.index()] = component;
        self.symbolized_nodes[node.index()] = true;
        Ok(())
    }

    /// Bind an actor's entry pose and availability before its event handler can change them.
    pub(crate) fn symbolize_actor(&mut self, actor: PistonId) -> Result<()> {
        if self.region.is_none() || self.symbolized_actors[actor.index()] {
            return Ok(());
        }
        let extended = self.region_input(RegionRead::ActorExtended(actor))?;
        let head = self.region_input(RegionRead::ActorHead(actor))?;
        let mut availability = [Formula::FALSE; 5];
        for (index, value) in availability.iter_mut().enumerate() {
            *value = self.region_input(RegionRead::ActorAvailability { actor, state: index as u8 })?;
        }
        let state = &mut self.state.actors[actor.index()];
        state.extended = extended;
        state.head = head;
        state.availability = availability;
        self.symbolized_actors[actor.index()] = true;
        Ok(())
    }

    /// Bind only an explicitly owned movable payload; ordinary external BUD memory stays live.
    pub(crate) fn symbolize_resource(&mut self, resource: ResourceId) -> Result<()> {
        if self.region.is_none() || self.symbolized_resources[resource.index()] {
            return Ok(());
        }
        let contacts: Vec<_> = self.state.resources[resource.index()]
            .contacts
            .iter()
            .map(|(contact, _)| *contact)
            .collect();
        let mut values = Vec::with_capacity(contacts.len());
        for contact in contacts {
            values.push((contact, self.region_input(RegionRead::ResourceAt { resource, contact })?));
        }
        let enabled = self.region_input(RegionRead::ResourceEnabled(resource))?;
        let state = &mut self.state.resources[resource.index()];
        for (contact, symbolic) in values {
            state.contacts.iter_mut().find(|(candidate, _)| *candidate == contact).unwrap().1 = symbolic;
        }
        state.enabled = enabled;
        self.symbolized_resources[resource.index()] = true;
        Ok(())
    }

    /// Return the changed owned scalar fields relative to this entry's live input state.
    pub(crate) fn changed_region_state(&self) -> Vec<(RegionStateSlot, Formula)> {
        let (Some(scope), Some(entry)) = (&self.region, &self.region_entry_state) else {
            return Vec::new();
        };
        let mut changed = Vec::new();
        for (index, component) in self.state.components.iter().enumerate() {
            if !scope.nodes[index] {
                continue;
            }
            let node = NodeId::new(index as u32);
            for (bit, (value, original)) in component.signal.bits().into_iter().zip(entry.components[index].signal.bits()).enumerate() {
                if value != original {
                    changed.push((RegionStateSlot::ComponentSignal { node, bit: bit as u8 }, value));
                }
            }
            if component.powered != entry.components[index].powered {
                changed.push((RegionStateSlot::ComponentPowered(node), component.powered));
            }
            if component.locked != entry.components[index].locked {
                changed.push((RegionStateSlot::ComponentLocked(node), component.locked));
            }
            if let (Some(shape), Some(old)) = (component.shape, entry.components[index].shape) {
                for (side, (value, original)) in shape.sides.into_iter().zip(old.sides).enumerate() {
                    if value.up != original.up {
                        changed.push((RegionStateSlot::WireUp { node, side: side as u8 }, value.up));
                    }
                    if value.flat != original.flat {
                        changed.push((RegionStateSlot::WireFlat { node, side: side as u8 }, value.flat));
                    }
                }
            }
        }
        for (index, actor) in self.state.actors.iter().enumerate() {
            if !scope.actors[index] {
                continue;
            }
            let id = PistonId::new(index as u32);
            let old = &entry.actors[index];
            if actor.extended != old.extended {
                changed.push((RegionStateSlot::ActorExtended(id), actor.extended));
            }
            if actor.head != old.head {
                changed.push((RegionStateSlot::ActorHead(id), actor.head));
            }
            for (state, (value, original)) in actor.availability.iter().zip(old.availability).enumerate() {
                if *value != original {
                    changed.push((
                        RegionStateSlot::ActorAvailability {
                            actor: id,
                            state: state as u8,
                        },
                        *value,
                    ));
                }
            }
            for (write, &value) in actor.revision.writes.iter().enumerate().skip(old.revision.writes.len()) {
                changed.push((
                    RegionStateSlot::ActorRevision {
                        actor: id,
                        write: write as u32,
                    },
                    value,
                ));
            }
        }
        for (index, resource) in self.state.resources.iter().enumerate() {
            if !scope.resources[index] {
                continue;
            }
            let id = ResourceId::new(index as u32);
            let old = &entry.resources[index];
            for &(contact, value) in &resource.contacts {
                let original = old.at(contact);
                if value != original {
                    changed.push((RegionStateSlot::ResourceAt { resource: id, contact }, value));
                }
            }
            if resource.enabled != old.enabled {
                changed.push((RegionStateSlot::ResourceEnabled(id), resource.enabled));
            }
            for (write, &value) in resource.revision.writes.iter().enumerate().skip(old.revision.writes.len()) {
                changed.push((
                    RegionStateSlot::ResourceRevision {
                        resource: id,
                        write: write as u32,
                    },
                    value,
                ));
            }
        }
        changed
    }

    /// Return one original value needed to rewind an event-time callback overlay.
    pub(crate) fn entry_region_value(&self, slot: RegionStateSlot) -> Formula {
        let Some(entry) = &self.region_entry_state else {
            return Formula::FALSE;
        };
        match slot {
            RegionStateSlot::ComponentSignal { node, bit } => entry.components[node.index()].signal.bits()[bit as usize],
            RegionStateSlot::ComponentPowered(node) => entry.components[node.index()].powered,
            RegionStateSlot::ComponentLocked(node) => entry.components[node.index()].locked,
            RegionStateSlot::WireUp { node, side } => entry.components[node.index()].shape.unwrap().sides[side as usize].up,
            RegionStateSlot::WireFlat { node, side } => entry.components[node.index()].shape.unwrap().sides[side as usize].flat,
            RegionStateSlot::ActorExtended(actor) => entry.actors[actor.index()].extended,
            RegionStateSlot::ActorHead(actor) => entry.actors[actor.index()].head,
            RegionStateSlot::ActorAvailability { actor, state } => entry.actors[actor.index()].availability[state as usize],
            RegionStateSlot::ResourceAt { resource, contact } => entry.resources[resource.index()].at(contact),
            RegionStateSlot::ResourceEnabled(resource) => entry.resources[resource.index()].enabled,
            RegionStateSlot::ActorRevision { .. } | RegionStateSlot::ResourceRevision { .. } => Formula::FALSE,
        }
    }
}
