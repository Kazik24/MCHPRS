//! Compiler-only scalar bindings; no concrete input histories or physical motion state.
use super::*;

fn boolean(value: bool) -> Formula {
    if value { Formula::TRUE } else { Formula::FALSE }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Component {
    /// This is the symbolic stored strength, independent of dust directional eligibility.
    pub(crate) signal: Signal,
    /// This is the logical powered bit read by component transitions and captured callbacks.
    pub(crate) powered: Formula,
    /// This retains repeater locking independently from its current output.
    pub(crate) locked: Formula,
    /// These are stored dust sides; electrical emission may regulate them against live geometry.
    pub(crate) shape: Option<WireShape>,
}

#[derive(Debug, Clone)]
pub(crate) struct Actor {
    /// This is the logical extension bit sampled by piston actions.
    pub(crate) extended: Formula,
    /// This records whether the selected head is currently present.
    pub(crate) head: Formula,
    /// These mutually exclusive predicates encode the five discrete availability states.
    pub(crate) availability: [Formula; 5],
    /// This retains revision-changing writes for event validity and exact cold reconstruction.
    pub(crate) revision: Revision,
}

#[derive(Debug, Clone)]
pub(crate) struct Resource {
    /// These mutually exclusive contact predicates preserve the identity of one shared payload.
    pub(crate) contacts: Vec<(ContactId, Formula)>,
    /// This distinguishes a stationary electrical payload from a pending publication.
    pub(crate) enabled: Formula,
    /// This retains transfer/publication writes independently from payload material or position.
    pub(crate) revision: Revision,
}

#[derive(Debug, Clone)]
pub(crate) struct Revision {
    /// This is the original revision, including values close to the wrapping boundary.
    pub(crate) base: u64,
    /// Each selected predicate contributes one wrapping increment, in source order.
    pub(crate) writes: Vec<Formula>,
}

impl Revision {
    pub(crate) fn stamp(&self) -> usize {
        self.writes.len()
    }

    pub(crate) fn unchanged(&self, stamp: usize, logic: &mut Diagram) -> Result<Formula> {
        let mut changed = Formula::FALSE;
        for &write in &self.writes[stamp..] {
            changed = logic.or(changed, write)?;
        }
        // Construction is bounded far below 2^64 writes, so any selected increment invalidates a witness.
        Ok(logic.not(changed)?)
    }
}

impl Resource {
    pub(crate) fn at(&self, contact: ContactId) -> Formula {
        self.contacts
            .iter()
            .find(|(candidate, _)| *candidate == contact)
            .map_or(Formula::FALSE, |(_, value)| *value)
    }
}

#[derive(Clone)]
pub(crate) struct State {
    /// These bindings follow existing node identities while symbolic execution is constructed.
    pub(crate) components: Vec<Component>,
    /// These bindings retain discrete actor state under guarded writes.
    pub(crate) actors: Vec<Actor>,
    /// These bindings retain shared-resource ownership and publication state under guarded writes.
    pub(crate) resources: Vec<Resource>,
}

impl State {
    /// Substitute only constants proved by the entry equations; all other memory remains symbolic.
    pub(crate) fn bind_inputs(&mut self, logic: &mut Diagram, bindings: &[(usize, bool)]) -> Result<()> {
        let mut values = logic.bind_roots(&self.scalar_roots(), bindings)?.into_iter();
        let mut next = || values.next().expect("binding preserves the scalar layout");
        for component in &mut self.components {
            component.signal = Signal::from_bits(std::array::from_fn(|_| next()));
            component.powered = next();
            component.locked = next();
            if let Some(shape) = &mut component.shape {
                for side in &mut shape.sides {
                    side.up = next();
                    side.flat = next();
                }
            }
        }
        for actor in &mut self.actors {
            actor.extended = next();
            actor.head = next();
            for value in &mut actor.availability {
                *value = next();
            }
        }
        for resource in &mut self.resources {
            for (_, at) in &mut resource.contacts {
                *at = next();
            }
            resource.enabled = next();
        }
        assert!(values.next().is_none(), "binding consumes the complete scalar layout");
        Ok(())
    }

    /// Count revision writes without binding them to any concrete runtime origin.
    pub(crate) fn revision_counts(&self, logic: &mut Diagram) -> Result<Vec<Vec<Formula>>> {
        let mut counts = Vec::with_capacity(self.actors.len() + self.resources.len());
        for actor in &self.actors {
            counts.push(logic.count(&actor.revision.writes)?);
        }
        for resource in &self.resources {
            counts.push(logic.count(&resource.revision.writes)?);
        }
        Ok(counts)
    }

    /// These compiler-only roots compare complete scalar state without treating revisions as physical memory.
    pub(crate) fn scalar_roots(&self) -> Vec<Formula> {
        let mut roots = Vec::new();
        for component in &self.components {
            roots.extend(component.signal.bits());
            roots.extend([component.powered, component.locked]);
            if let Some(shape) = component.shape {
                for side in shape.sides {
                    roots.extend([side.up, side.flat]);
                }
            }
        }
        for actor in &self.actors {
            roots.extend([actor.extended, actor.head]);
            roots.extend(actor.availability);
        }
        for resource in &self.resources {
            roots.extend(resource.contacts.iter().map(|(_, at)| *at));
            roots.push(resource.enabled);
        }
        roots
    }

    pub(crate) fn initial(graph: &CompileGraph) -> Result<Self> {
        Self::initial_impl(graph, true)
    }

    pub(crate) fn initial_region(graph: &CompileGraph) -> Result<Self> {
        Self::initial_impl(graph, false)
    }

    fn initial_impl(graph: &CompileGraph, reject_host_state: bool) -> Result<Self> {
        let mut components = Vec::new();
        for (index, node) in graph.nodes.iter().enumerate() {
            let (strength, powered, locked, shape) = match &node.state {
                ComponentState::Electrical(state) => {
                    if reject_host_state && (state.command_entity.is_some() || state.note_instrument.is_some()) {
                        return Err(CompileError::UnsupportedNode(NodeId::new(index as u32)));
                    }
                    (state.output_strength, state.powered, state.repeater_locked, None)
                }
                ComponentState::Wire(wire) => (wire.power, false, false, Some(WireShape::literal(*wire))),
                ComponentState::Specialized => (0, false, false, None),
            };
            components.push(Component {
                signal: Signal::literal(strength)?,
                powered: boolean(powered),
                locked: boolean(locked),
                shape,
            });
        }
        Ok(Self {
            components,
            actors: graph
                .initial_pistons
                .iter()
                .map(|state| Actor {
                    extended: boolean(state.extended),
                    head: boolean(state.head_present),
                    availability: std::array::from_fn(|index| boolean(index == state.availability as usize)),
                    revision: Revision {
                        base: state.revision,
                        writes: Vec::new(),
                    },
                })
                .collect(),
            resources: graph
                .initial_resources
                .iter()
                .zip(&graph.resources)
                .map(|(state, spec)| Resource {
                    contacts: spec.contacts.iter().map(|&contact| (contact, boolean(contact == state.at))).collect(),
                    enabled: boolean(state.enabled),
                    revision: Revision {
                        base: state.revision,
                        writes: Vec::new(),
                    },
                })
                .collect(),
        })
    }
}

impl Compiler<'_> {
    pub(crate) fn write_actor(
        &mut self,
        actor: PistonId,
        when: Formula,
        extended: Option<Formula>,
        head: Option<Formula>,
        availability: Option<PistonAvailability>,
    ) -> Result<()> {
        if when == Formula::FALSE {
            return Ok(());
        }
        self.step()?;
        let state = &mut self.state.actors[actor.index()];
        if let Some(value) = extended {
            state.extended = self.logic.select(when, value, state.extended)?;
        }
        if let Some(value) = head {
            state.head = self.logic.select(when, value, state.head)?;
        }
        if let Some(value) = availability {
            for (index, condition) in state.availability.iter_mut().enumerate() {
                *condition = self.logic.select(when, boolean(index == value as usize), *condition)?;
            }
        }
        self.state_written();
        Ok(())
    }

    pub(crate) fn revise_actor(&mut self, actor: PistonId, when: Formula) -> Result<usize> {
        self.step()?;
        let revision = &mut self.state.actors[actor.index()].revision;
        if when != Formula::FALSE {
            revision.writes.push(when);
        }
        Ok(revision.stamp())
    }

    pub(crate) fn write_resource(&mut self, resource: ResourceId, contact: ContactId, enabled: bool, when: Formula) -> Result<Formula> {
        self.step()?;
        if !self.graph.resources[resource.index()].contacts.contains(&contact) {
            return Err(CompileError::InvalidResourceContact { resource, contact });
        }
        let current = &self.state.resources[resource.index()];
        let enabled_matches = if enabled { current.enabled } else { self.logic.not(current.enabled)? };
        let same = self.logic.and(current.at(contact), enabled_matches)?;
        let differs = self.logic.not(same)?;
        let changed = self.logic.and(when, differs)?;
        if changed == Formula::FALSE {
            return Ok(changed);
        }
        let current = &mut self.state.resources[resource.index()];
        for (candidate, condition) in &mut current.contacts {
            *condition = self.logic.select(changed, boolean(*candidate == contact), *condition)?;
        }
        current.enabled = self.logic.select(changed, boolean(enabled), current.enabled)?;
        current.revision.writes.push(changed);
        self.state_written();
        Ok(changed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WireSide {
    /// This side climbs; the up and flat predicates cannot both hold.
    pub(crate) up: Formula,
    /// This side connects horizontally; both predicates false means no connection.
    pub(crate) flat: Formula,
}

impl WireSide {
    fn literal(side: RedstoneWireSide) -> Self {
        Self {
            up: boolean(side == RedstoneWireSide::Up),
            flat: boolean(side == RedstoneWireSide::Side),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WireShape {
    /// These stored sides are ordered north, south, east and west.
    pub(crate) sides: [WireSide; 4],
}

impl WireShape {
    pub(crate) fn literal(wire: RedstoneWire) -> Self {
        Self {
            sides: [wire.north, wire.south, wire.east, wire.west].map(WireSide::literal),
        }
    }
}
