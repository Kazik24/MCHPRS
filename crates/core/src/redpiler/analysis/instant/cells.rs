//! Derive pull-computation predicates from prepared power, activation and payload rules.
//!
//! At a stationary entry every admitted base is extended and every token is enabled at
//! its shared Far. Computation only retracts bases: a consumed base becomes unavailable
//! and cannot extend again before publication. A pull removes its selected source even
//! when another shared owner later retracts empty. Reset/early-drop entries need their
//! own contracts; reset provenance alone never authorizes contracting their execution.
use super::ownership::close_ownership;
use crate::redpiler::TaskMonitor;
use crate::redpiler::analysis::geometry::is_supported_payload_material;
use crate::redpiler::passes::utils::instant_mechanisms::{PowerIndex, discover_indexed_mechanisms, index_power};
use crate::redpiler::program_graph::*;
use mchprs_blocks::blocks::Block;

#[derive(Debug)]
pub(crate) struct CellDiscovery {
    /// These complete ownership groups have a stationary pull contract for every actor.
    pub(crate) cells: Vec<CellContract>,
    /// These complete groups remain general, with a concrete reason rather than partial coverage.
    pub(crate) declined: Vec<CellDecline>,
}

#[derive(Debug)]
pub(crate) struct CellDecline {
    /// These actors belong to the indivisible group which was not certified.
    pub(crate) actors: Vec<PistonId>,
    /// This identifies the unsupported owner, material or source-contact rule.
    pub(crate) reason: CellDeclineReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CellDeclineReason {
    /// This actor has no recognized reset provenance and may be ordinary memory.
    UnrecognizedActor(PistonId),
    /// This resource has different Far contacts among its owners, requiring another local contract.
    DifferentPullSources(ResourceId),
    /// This resource intersects the group but has no actor with a complete Near/Far pull lane.
    NoPullSource(ResourceId),
    /// This contact would need to contain two different stationary payloads at entry.
    ConflictingSourceContact(ContactId),
    /// This resource is not an admitted entity-free cube/redstone source.
    UnsupportedMaterial(ResourceId),
}

struct CellIndex {
    /// These are all guarded incoming contributions, indexed once by typed channel.
    power: PowerIndex,
    /// These potential occurrences sample each actor after ordinary guard/receiver resolution.
    activations: Vec<Vec<RouteOccurrence>>,
    /// These are base owners at each contact, including unrecognized obstructions.
    bases: Vec<Vec<PistonId>>,
    /// These are head owners at each contact, independent of the entry pose.
    heads: Vec<Vec<PistonId>>,
    /// These are all conserved tokens which may occupy each contact.
    resources: Vec<Vec<ResourceId>>,
}

impl CellIndex {
    fn build(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<Self, String> {
        let mut index = Self {
            power: index_power(graph),
            activations: vec![Vec::new(); graph.pistons.len()],
            bases: vec![Vec::new(); graph.contacts.len()],
            heads: vec![Vec::new(); graph.contacts.len()],
            resources: vec![Vec::new(); graph.contacts.len()],
        };
        for (actor, spec) in graph.pistons.iter().enumerate() {
            check_cancelled(monitor)?;
            let actor = PistonId::new(actor as u32);
            index.bases[spec.base.index()].push(actor);
            index.heads[spec.near.index()].push(actor);
        }
        for (resource, spec) in graph.resources.iter().enumerate() {
            check_cancelled(monitor)?;
            for contact in &spec.contacts {
                index.resources[contact.index()].push(ResourceId::new(resource as u32));
            }
        }
        for (route_id, route) in graph.routes.iter().enumerate() {
            check_cancelled(monitor)?;
            for (ordinal, delivery) in route.deliveries.iter().enumerate() {
                if !matches!(delivery.kind, DeliveryKind::Update { .. }) {
                    continue;
                }
                let occurrence = RouteOccurrence {
                    route: RouteId::new(route_id as u32),
                    ordinal: ordinal as u32,
                };
                let receiver = delivery.receiver.or_else(|| match graph.sites[delivery.target.index()].source {
                    SiteSource::Component(node) => Some(CallbackReceiver::Component(node)),
                    SiteSource::Contact(contact) => Some(CallbackReceiver::Contact(contact)),
                    SiteSource::Constant(_) => None,
                });
                if let Some(receiver) = receiver {
                    index.add_receiver(graph, receiver, occurrence);
                }
            }
        }
        for occurrences in &mut index.activations {
            occurrences.dedup();
        }
        Ok(index)
    }

    fn add_receiver(&mut self, graph: &CompileGraph, receiver: CallbackReceiver, occurrence: RouteOccurrence) {
        match receiver {
            CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor) => self.activations[actor.index()].push(occurrence),
            CallbackReceiver::Component(node) => {
                if let CircuitNodeKind::Piston(actor) = graph.nodes[node.index()].kind {
                    self.activations[actor.index()].push(occurrence);
                }
            }
            CallbackReceiver::Contact(contact) => {
                let mut visited = Vec::new();
                let mut pending = vec![contact];
                while let Some(contact) = pending.pop() {
                    if visited.contains(&contact) {
                        continue;
                    }
                    visited.push(contact);
                    for variant in &graph.contacts[contact.index()].variants {
                        match variant.receiver {
                            Some(CallbackReceiver::Contact(other)) => pending.push(other),
                            Some(receiver) => self.add_receiver(graph, receiver, occurrence),
                            None => {}
                        }
                    }
                }
            }
        }
    }
}

/// No World reads, input-mask enumeration, or symbolic Turbo walk is used here.
pub(crate) fn discover_cells(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<CellDiscovery, String> {
    check_cancelled(monitor)?;
    let index = CellIndex::build(graph, monitor)?;
    let mechanisms = discover_indexed_mechanisms(graph, &index.power, monitor)?;
    let closure = close_ownership(graph, &mechanisms, monitor)?;
    let mut resets = vec![None; graph.pistons.len()];
    for mechanism in mechanisms {
        resets[mechanism.piston.index()] = Some(mechanism.observer);
    }
    let mut discovery = CellDiscovery {
        cells: Vec::new(),
        declined: Vec::new(),
    };
    for group in closure.groups {
        check_cancelled(monitor)?;
        match build_cell(graph, &index, &group.actors, &group.resources, &resets, monitor)? {
            Ok(cell) => discovery.cells.push(cell),
            Err(reason) => discovery.declined.push(CellDecline {
                actors: group.actors,
                reason,
            }),
        }
    }
    check_cancelled(monitor)?;
    Ok(discovery)
}

/// Check source witnesses once for the whole table, including omissions and altered polarity.
pub(crate) fn validate_cells(graph: &CompileGraph, cells: &[CellContract], monitor: &TaskMonitor) -> Result<(), String> {
    let expected = discover_cells(graph, monitor)?;
    if expected.cells != cells {
        let index = expected
            .cells
            .iter()
            .zip(cells)
            .position(|(expected, actual)| expected != actual)
            .unwrap_or(expected.cells.len().min(cells.len()));
        return Err(format!("instant cell {index} does not match its graph-derived contract"));
    }
    Ok(())
}

fn build_cell(
    graph: &CompileGraph,
    index: &CellIndex,
    actors: &[PistonId],
    resources: &[ResourceId],
    resets: &[Option<NodeId>],
    monitor: &TaskMonitor,
) -> Result<Result<CellContract, CellDeclineReason>, String> {
    if let Some(&actor) = actors.iter().find(|actor| resets[actor.index()].is_none()) {
        return Ok(Err(CellDeclineReason::UnrecognizedActor(actor)));
    }
    let mut pins = Pins::default();
    let mut entry_terms = Vec::new();
    let mut outputs = Vec::new();
    for &resource in resources {
        check_cancelled(monitor)?;
        let profile = graph.profiles[graph.resources[resource.index()].material.index()];
        if !profile.cube || !is_supported_payload_material(Block::from_id(profile.raw_state)) {
            return Ok(Err(CellDeclineReason::UnsupportedMaterial(resource)));
        }
        let contacts = &graph.resources[resource.index()].contacts;
        let mut source = None;
        for &actor in actors {
            check_cancelled(monitor)?;
            let spec = graph.pistons[actor.index()];
            if let PistonLane::SingleResource { far } = spec.lane
                && contacts.contains(&spec.near)
                && contacts.contains(&far)
            {
                if source.is_some_and(|source| source != far) {
                    return Ok(Err(CellDeclineReason::DifferentPullSources(resource)));
                }
                source = Some(far);
            }
        }
        let Some(source) = source else {
            return Ok(Err(CellDeclineReason::NoPullSource(resource)));
        };
        if outputs.iter().any(|output: &CellResourcePin| output.source == source) {
            return Ok(Err(CellDeclineReason::ConflictingSourceContact(source)));
        }
        let present = pins.enabled_at(resource, source)?;
        entry_terms.push(present);
        let absent = pins.not(present)?;
        outputs.push(CellResourcePin { resource, source, absent });
    }
    let mut uses = Vec::new();
    for &actor in actors {
        check_cancelled(monitor)?;
        let spec = graph.pistons[actor.index()];
        let ready = pins.state(StateTest::PistonAvailability {
            piston: actor,
            is: PistonAvailability::Ready,
        })?;
        let extended = pins.state(StateTest::PistonExtended(actor))?;
        entry_terms.extend([ready, extended]);
        let main = pins.channel(&index.power, spec.node, InputPort::Main)?;
        let quasi = pins.channel(&index.power, spec.node, InputPort::Quasi)?;
        let powered = pins.gate(CellGateKind::Any, vec![main, quasi])?;
        let unpowered = pins.not(powered)?;
        let activation = pins.push(CellPin::Read(CellRead::Activation(actor)))?;
        let requested = pins.gate(CellGateKind::All, vec![activation, ready, extended, unpowered])?;
        let mut validity = vec![ready, extended, unpowered];
        if let PistonLane::SingleResource { far } = spec.lane {
            for &resource in &index.resources[far.index()] {
                let selected = pins.enabled_at(resource, far)?;
                let free = pins.pull_destination(index, spec.near, actor, resource)?;
                let not_selected = pins.not(selected)?;
                validity.push(pins.gate(CellGateKind::Any, vec![not_selected, free])?);
            }
        }
        let valid = pins.gate(CellGateKind::All, validity)?;
        uses.push(CellUse {
            actor,
            reset: resets[actor.index()].expect("all owners checked"),
            powered,
            requested,
            valid,
            activations: index.activations[actor.index()].clone().into_boxed_slice(),
        });
    }
    let stationary_entry = pins.gate(CellGateKind::All, entry_terms)?;
    Ok(Ok(CellContract {
        uses: uses.into_boxed_slice(),
        pins: pins.values.into_boxed_slice(),
        stationary_entry,
        resources: outputs.into_boxed_slice(),
    }))
}

#[derive(Default)]
struct Pins {
    /// These pins are appended only after their operands, so construction cannot create cycles.
    values: Vec<CellPin>,
}

impl Pins {
    fn push(&mut self, pin: CellPin) -> Result<CellPinId, String> {
        let id = u32::try_from(self.values.len()).map_err(|_| "too many local instant-cell pins")?;
        self.values.push(pin);
        Ok(CellPinId(id))
    }

    fn state(&mut self, test: StateTest) -> Result<CellPinId, String> {
        self.push(CellPin::Read(CellRead::State(test)))
    }

    fn gate(&mut self, kind: CellGateKind, inputs: Vec<CellPinId>) -> Result<CellPinId, String> {
        self.push(CellPin::Gate {
            kind,
            inputs: inputs.into_boxed_slice(),
        })
    }

    fn not(&mut self, input: CellPinId) -> Result<CellPinId, String> {
        self.gate(CellGateKind::Invert, vec![input])
    }

    fn enabled_at(&mut self, resource: ResourceId, contact: ContactId) -> Result<CellPinId, String> {
        let at = self.state(StateTest::ResourceAt { resource, contact })?;
        let enabled = self.state(StateTest::ResourceEnabled(resource))?;
        self.gate(CellGateKind::All, vec![at, enabled])
    }

    fn channel(&mut self, power: &PowerIndex, node: NodeId, port: InputPort) -> Result<CellPinId, String> {
        let mut contributions = Vec::new();
        for &link in power.get(&PowerTarget::Input { node, port }).into_iter().flatten() {
            if link.attenuation < 15 {
                contributions.push(self.push(CellPin::Read(CellRead::Contribution(link)))?);
            }
        }
        let kind = if contributions.len() == 1 {
            CellGateKind::Buffer
        } else {
            CellGateKind::Any
        };
        self.gate(kind, contributions)
    }

    /// Vacating this actor's own head is allowed; other heads, bases and tokens still block.
    fn pull_destination(&mut self, index: &CellIndex, contact: ContactId, actor: PistonId, resource: ResourceId) -> Result<CellPinId, String> {
        let mut clear = Vec::new();
        for &other in &index.resources[contact.index()] {
            if other != resource {
                let at = self.state(StateTest::ResourceAt { resource: other, contact })?;
                clear.push(self.not(at)?);
            }
        }
        for &other in &index.bases[contact.index()] {
            let pending = [PistonAvailability::BasePending, PistonAvailability::BasePendingHeadPresent]
                .into_iter()
                .map(|is| self.state(StateTest::PistonAvailability { piston: other, is }))
                .collect::<Result<Vec<_>, _>>()?;
            clear.push(self.gate(CellGateKind::Any, pending)?);
        }
        for &other in &index.heads[contact.index()] {
            if other != actor {
                let present = self.state(StateTest::PistonHeadPresent(other))?;
                let phases = [
                    PistonAvailability::Ready,
                    PistonAvailability::HeadPending,
                    PistonAvailability::BasePendingHeadPresent,
                ]
                .into_iter()
                .map(|is| self.state(StateTest::PistonAvailability { piston: other, is }))
                .collect::<Result<Vec<_>, _>>()?;
                let head_phase = self.gate(CellGateKind::Any, phases)?;
                let occupied = self.gate(CellGateKind::All, vec![present, head_phase])?;
                clear.push(self.not(occupied)?);
            }
        }
        self.gate(CellGateKind::All, clear)
    }
}

fn check_cancelled(monitor: &TaskMonitor) -> Result<(), String> {
    if monitor.cancelled() {
        Err("cancelled while deriving instant cells".into())
    } else {
        Ok(())
    }
}
