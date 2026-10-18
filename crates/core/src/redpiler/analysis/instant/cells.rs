//! Derive pull-computation predicates from prepared power, activation and payload rules.
//!
//! At a stationary entry every admitted base is extended and every token is enabled at
//! its shared Far. Computation only retracts bases: a consumed base becomes unavailable
//! and cannot extend again before publication. A pull removes its selected source even
//! when another shared owner later retracts empty. Reset/early-drop entries need their
//! own contracts; reset provenance alone never authorizes contracting their execution.
use super::ownership::close_ownership;
use crate::redpiler::TaskMonitor;
use crate::redpiler::analysis::MAX_GRAPH_PREPARATION_BYTES;
use crate::redpiler::analysis::geometry::is_supported_payload_material;
use crate::redpiler::passes::utils::instant_mechanisms::{PowerIndex, discover_indexed_mechanisms, index_power};
use crate::redpiler::program_graph::*;
use mchprs_blocks::blocks::Block;
use rustc_hash::FxHashSet;

// This share bounds retained pin and operand payloads; boxed-slice/vector allocator overhead is excluded.
const MAX_CELL_TABLE_BYTES: usize = MAX_GRAPH_PREPARATION_BYTES / 8;

#[derive(Debug)]
pub(crate) struct CellDiscovery {
    /// These complete ownership groups have a stationary pull contract for every actor.
    pub(crate) cells: Vec<CellContract>,
    /// These complete groups remain general, with a concrete reason rather than partial coverage.
    pub(crate) declined: Vec<CellDecline>,
    /// These original piston IDs map only fully admitted groups to their cell index.
    pub(crate) actor_owners: Vec<Option<u32>>,
    /// These conserved payload IDs map only fully admitted groups to their cell index.
    pub(crate) resource_owners: Vec<Option<u32>>,
    /// These possible occupied positions map only fully admitted groups to their cell index.
    pub(crate) contact_owners: Vec<Option<u32>>,
}

pub(crate) use crate::redpiler::program_graph::{CellDecline, CellDeclineReason};

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
    /// This bounds activation-row payload bytes against the graph preparation allocation share.
    activation_bytes: usize,
}

#[derive(Clone, Copy)]
struct CellResetProvenance {
    /// This is the existing observer that supplies reset power to the recognized piston.
    observer: NodeId,
    /// This is the distinct piston base whose source routes trigger that observer.
    watched_actor: PistonId,
}

impl CellIndex {
    fn build(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<Self, String> {
        let mut index = Self {
            power: index_power(graph),
            activations: vec![Vec::new(); graph.pistons.len()],
            bases: vec![Vec::new(); graph.contacts.len()],
            heads: vec![Vec::new(); graph.contacts.len()],
            resources: vec![Vec::new(); graph.contacts.len()],
            activation_bytes: 0,
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
                check_cancelled(monitor)?;
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
                    index.add_receiver(graph, receiver, occurrence, monitor)?;
                }
            }
        }
        for occurrences in &mut index.activations {
            occurrences.dedup();
        }
        Ok(index)
    }

    fn add_receiver(
        &mut self,
        graph: &CompileGraph,
        receiver: CallbackReceiver,
        occurrence: RouteOccurrence,
        monitor: &TaskMonitor,
    ) -> Result<(), String> {
        match receiver {
            CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor) => self.add_activation(actor, occurrence)?,
            CallbackReceiver::Component(node) => {
                if let CircuitNodeKind::Piston(actor) = graph.nodes[node.index()].kind {
                    self.add_activation(actor, occurrence)?;
                }
            }
            CallbackReceiver::Contact(contact) => {
                let mut visited = FxHashSet::default();
                let mut pending = vec![contact];
                while let Some(contact) = pending.pop() {
                    check_cancelled(monitor)?;
                    if !visited.insert(contact) {
                        continue;
                    }
                    for variant in &graph.contacts[contact.index()].variants {
                        check_cancelled(monitor)?;
                        match variant.receiver {
                            Some(CallbackReceiver::Contact(other)) => pending.push(other),
                            Some(receiver) => self.add_receiver(graph, receiver, occurrence, monitor)?,
                            None => {}
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn add_activation(&mut self, actor: PistonId, occurrence: RouteOccurrence) -> Result<(), String> {
        let row_bytes = std::mem::size_of::<RouteOccurrence>();
        if self
            .activation_bytes
            .checked_add(row_bytes)
            .is_none_or(|bytes| bytes > MAX_CELL_TABLE_BYTES)
        {
            return Err("instant-cell activation rows exceed the bounded preparation allocation".into());
        }
        self.activation_bytes += row_bytes;
        self.activations[actor.index()].push(occurrence);
        Ok(())
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
        resets[mechanism.piston.index()] = Some(CellResetProvenance {
            observer: mechanism.observer,
            watched_actor: mechanism.watched_actor,
        });
    }
    let mut discovery = CellDiscovery {
        cells: Vec::new(),
        declined: Vec::new(),
        actor_owners: vec![None; graph.pistons.len()],
        resource_owners: vec![None; graph.resources.len()],
        contact_owners: vec![None; graph.contacts.len()],
    };
    let mut group_cells = vec![None; closure.groups.len()];
    let mut retained_pin_total = 0usize;
    for (group_index, group) in closure.groups.into_iter().enumerate() {
        check_cancelled(monitor)?;
        let actors = group.actors;
        let resources = group.resources;
        match build_cell(graph, &index, &actors, &resources, &resets, monitor)? {
            Ok(cell) => {
                let cell_bytes = estimate_pin_bytes(&cell)?;
                if retained_pin_total
                    .checked_add(cell_bytes)
                    .is_none_or(|bytes| bytes > MAX_CELL_TABLE_BYTES)
                {
                    discovery.declined.push(CellDecline {
                        actors,
                        reason: CellDeclineReason::PinTableBudgetExceeded,
                    });
                } else {
                    retained_pin_total += cell_bytes;
                    let cell_index = u32::try_from(discovery.cells.len()).map_err(|_| "too many instant cells")?;
                    group_cells[group_index] = Some(cell_index);
                    for actor in actors {
                        discovery.actor_owners[actor.index()] = Some(cell_index);
                    }
                    for resource in resources {
                        discovery.resource_owners[resource.index()] = Some(cell_index);
                    }
                    discovery.cells.push(cell);
                }
            }
            Err(reason) => discovery.declined.push(CellDecline { actors, reason }),
        }
    }
    for (contact, owner) in closure.contacts.into_iter().enumerate() {
        if let Some(cell) = owner.and_then(|group| group_cells[group]) {
            discovery.contact_owners[contact] = Some(cell);
        }
    }
    check_cancelled(monitor)?;
    Ok(discovery)
}

fn estimate_pin_bytes(cell: &CellContract) -> Result<usize, String> {
    let pins = cell
        .pins
        .len()
        .checked_mul(std::mem::size_of::<CellPin>())
        .ok_or("instant-cell pin table size overflow")?;
    let operands = cell.pins.iter().try_fold(0usize, |total, pin| {
        let count = match pin {
            CellPin::Read(_) => 0,
            CellPin::Gate { inputs, .. } => inputs.len(),
        };
        total
            .checked_add(
                count
                    .checked_mul(std::mem::size_of::<CellPinId>())
                    .ok_or("instant-cell operand table size overflow")?,
            )
            .ok_or("instant-cell operand table size overflow")
    })?;
    pins.checked_add(operands).ok_or_else(|| "instant-cell table size overflow".into())
}

/// Check source witnesses once for the whole table, including omissions and altered polarity.
#[cfg(test)]
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
    resets: &[Option<CellResetProvenance>],
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
        let owners: Vec<_> = actors
            .iter()
            .copied()
            .filter(|&actor| {
                let spec = graph.pistons[actor.index()];
                matches!(spec.lane, PistonLane::SingleResource { far } if far == source) && contacts.contains(&spec.near)
            })
            .collect();
        outputs.push(CellResourcePin {
            resource,
            source,
            present,
            absent,
            owners: owners.into_boxed_slice(),
            selection: CellTokenSelection::FirstAcceptedPullRequestInFifo,
        });
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
        let transfer = outputs
            .iter()
            .find(|output| output.owners.contains(&actor))
            .map(|output| CellResourceTransfer {
                resource: output.resource,
                source: output.source,
                destination: spec.near,
                resource_revision_increment: 1,
                when_at_source: output.present,
            });
        let resource_wake = if let Some(transfer) = transfer {
            let Some(route) = spec.routes.resource_restored_near else {
                return Ok(Err(CellDeclineReason::MissingDeferredWake(actor)));
            };
            Some(CellResourceWake {
                resource: transfer.resource,
                contact: transfer.destination,
                route,
            })
        } else {
            None
        };
        let provenance = resets[actor.index()].expect("all owners checked");
        let reset = provenance.observer;
        let Some(reset_wake) = reset_wake(graph, provenance, monitor)? else {
            return Ok(Err(CellDeclineReason::MissingResetWake(actor)));
        };
        uses.push(CellUse {
            actor,
            reset,
            powered,
            requested,
            valid,
            activations: index.activations[actor.index()].clone().into_boxed_slice(),
            retraction: CellRetractionEffect {
                actor_revision_increment: 1,
                transfer,
                routes: CellRetractionRoutes {
                    base_changed: spec.routes.base_changed,
                    far_changed: transfer.and(spec.routes.far_changed),
                    near_changed: spec.routes.near_changed,
                },
                wake: CellDeferredWake {
                    ticks_after_acceptance: 2,
                    base_restored: spec.routes.base_restored,
                    resource: resource_wake,
                },
                reset: reset_wake,
            },
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

fn reset_wake(graph: &CompileGraph, provenance: CellResetProvenance, monitor: &TaskMonitor) -> Result<Option<CellResetWake>, String> {
    let reset = provenance.observer;
    let watched_routes = graph.pistons[provenance.watched_actor.index()].routes;
    let expected_direction = match graph.nodes[reset.index()].kind {
        CircuitNodeKind::Observer { facing, .. } | CircuitNodeKind::BuiltinReset { facing, .. } => facing,
        _ => return Ok(None),
    };
    let collect = |route: RouteId| -> Result<Box<[RouteOccurrence]>, String> {
        let mut occurrences = Vec::new();
        for (ordinal, delivery) in graph.routes[route.index()].deliveries.iter().enumerate() {
            check_cancelled(monitor)?;
            if delivery.receiver == Some(CallbackReceiver::Component(reset))
                && delivery.kind
                    == (DeliveryKind::Update {
                        direction: Some(expected_direction),
                    })
                && delivery.view == OperandView::Live
                && matches!(graph.guards[delivery.guard.index()], Guard::Always)
            {
                occurrences.push(RouteOccurrence {
                    route,
                    ordinal: ordinal as u32,
                });
            }
        }
        Ok(occurrences.into_boxed_slice())
    };
    let on_base_changed = collect(watched_routes.base_changed)?;
    let on_base_restored = collect(watched_routes.base_restored)?;
    if on_base_changed.is_empty() || on_base_restored.is_empty() {
        return Ok(None);
    }
    Ok(Some(CellResetWake {
        watched_actor: provenance.watched_actor,
        on_base_changed,
        on_base_restored,
    }))
}

#[derive(Default)]
struct Pins {
    /// These pins are appended only after their operands, so construction cannot create cycles.
    values: Vec<CellPin>,
    /// These retained pin and operand bytes stay within the shared graph-preparation cap.
    estimated_bytes: usize,
}

impl Pins {
    fn push(&mut self, pin: CellPin) -> Result<CellPinId, String> {
        self.push_with_operands(pin, 0)
    }

    fn check_budget(&self, bytes: usize) -> Result<(), String> {
        if self.values.len() >= u32::MAX as usize || self.estimated_bytes.checked_add(bytes).is_none_or(|total| total > MAX_CELL_TABLE_BYTES) {
            return Err("instant-cell pins exceed the bounded preparation allocation".into());
        }
        Ok(())
    }

    fn push_with_operands(&mut self, pin: CellPin, operand_count: usize) -> Result<CellPinId, String> {
        let bytes = std::mem::size_of::<CellPin>()
            .checked_add(
                operand_count
                    .checked_mul(std::mem::size_of::<CellPinId>())
                    .ok_or("instant-cell operand size overflow")?,
            )
            .ok_or("instant-cell pin size overflow")?;
        self.check_budget(bytes)?;
        let id = u32::try_from(self.values.len()).map_err(|_| "too many local instant-cell pins")?;
        self.values.push(pin);
        self.estimated_bytes += bytes;
        Ok(CellPinId(id))
    }

    fn state(&mut self, test: StateTest) -> Result<CellPinId, String> {
        self.push(CellPin::Read(CellRead::State(test)))
    }

    fn gate(&mut self, kind: CellGateKind, inputs: Vec<CellPinId>) -> Result<CellPinId, String> {
        let operand_count = inputs.len();
        let operand_bytes = operand_count
            .checked_mul(std::mem::size_of::<CellPinId>())
            .ok_or("instant-cell operand size overflow")?;
        self.check_budget(
            std::mem::size_of::<CellPin>()
                .checked_add(operand_bytes)
                .ok_or("instant-cell pin size overflow")?,
        )?;
        self.push_with_operands(
            CellPin::Gate {
                kind,
                inputs: inputs.into_boxed_slice(),
            },
            operand_count,
        )
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
