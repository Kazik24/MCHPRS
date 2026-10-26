//! O6 data and timing kernels. Physical topology is used only at activation/projection.
use super::logical_clock::LogicalClock;
use super::*;
use crate::redpiler::LogicalDeviceAdmission;
use crate::redpiler::program_graph::settled_machine::SettledMachine;
use crate::redpiler::program_graph::{SettledEvaluation, SettledValueKind};

const MAX_FRAME_PASSES: usize = 128;

struct Register {
    storage: PistonId,
    resource: ResourceId,
    near: ContactId,
    far: ContactId,
    source: ContactId,
    output: usize,
}

#[derive(Clone, Copy)]
enum FrameBoundary {
    Power {
        slot: usize,
        source: LayerInputSource,
        clock_node: Option<NodeId>,
        threshold: u8,
    },
    ClockState {
        slot: usize,
        test: StateTest,
    },
}

#[derive(Clone, Copy)]
struct FeedbackSlot {
    slot: usize,
    output: usize,
    polarity: bool,
}

struct FeedbackResource {
    resource: ResourceId,
    source: ContactId,
    other: ContactId,
    output: usize,
}

struct SettledFrame {
    evaluation: SettledEvaluation,
    inputs: Box<[bool]>,
    boundaries: Box<[FrameBoundary]>,
    feedback_slots: Box<[FeedbackSlot]>,
    feedback_resources: Box<[FeedbackResource]>,
    outputs: Box<[(NodeId, usize, SettledValueKind)]>,
    button_nodes: Box<[NodeId]>,
    retained_actors: Box<[PistonId]>,
    retained_observers: Box<[NodeId]>,
    retained_commands: Box<[NodeId]>,
    retained_routes: Box<[RouteOccurrence]>,
    retained_resources: Box<[ResourceId]>,
    retained_power_inputs: Box<[FramePowerInput]>,
    terminal_repeater_inputs: Box<[FrameRepeaterInput]>,
    computed: bool,
}

fn retained_cell_callback_is_owned(
    program: &CircuitProgram,
    delivery: &RouteDelivery,
    retained_actors: &FxHashSet<PistonId>,
    retained_contacts: &FxHashSet<ContactId>,
    retained_observers: &[NodeId],
    retained_commands: &[NodeId],
    retained_repeaters: &FxHashSet<NodeId>,
) -> Result<bool, String> {
    let source = program.sites()[delivery.target.index()].source;
    let receiver = delivery.receiver.or_else(|| match (source, delivery.kind) {
        (SiteSource::Component(node), DeliveryKind::Update { .. }) => Some(CallbackReceiver::Component(node)),
        _ => None,
    });
    Ok(match receiver {
        Some(CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor)) if retained_actors.contains(&actor) => true,
        Some(CallbackReceiver::PistonBase(_) | CallbackReceiver::PistonHead(_)) => {
            return Err("retained output cell callback reaches an unowned piston".into());
        }
        Some(CallbackReceiver::Contact(contact)) if retained_contacts.contains(&contact) => true,
        Some(CallbackReceiver::Contact(contact)) if inert_cell_contact(program, contact) => false,
        Some(CallbackReceiver::Contact(_)) => return Err("retained output cell callback reaches an observable unowned contact".into()),
        Some(CallbackReceiver::Component(node))
            if retained_observers.contains(&node) || retained_commands.contains(&node) || retained_repeaters.contains(&node) =>
        {
            true
        }
        Some(CallbackReceiver::Component(node)) if matches!(program.nodes()[node.index()].kind, CircuitNodeKind::Wire(_)) => false,
        Some(CallbackReceiver::Component(_)) => return Err("retained output cell callback reaches an unowned component".into()),
        // Match Graph::deliver_callback: contact/constant deliveries without an explicit
        // receiver, and component shapes, are inert. Component updates have already
        // acquired their implicit Component receiver above.
        None => false,
    })
}

fn retained_cell_observation_is_owned(
    backend: &GraphBackend,
    kind: BoundaryPortKind,
    retained_actors: &FxHashSet<PistonId>,
    retained_contacts: &FxHashSet<ContactId>,
    retained_cells: &[CellUse],
    retained_observers: &[NodeId],
    retained_resources: &[ResourceId],
) -> bool {
    match kind {
        BoundaryPortKind::GuardState { test, .. } => match test {
            StateTest::PistonExtended(actor) | StateTest::PistonHeadPresent(actor) | StateTest::PistonAvailability { piston: actor, .. } => {
                retained_actors.contains(&actor)
            }
            StateTest::ResourceAt { resource, contact } => {
                retained_resources.contains(&resource) && backend.program.resources()[resource.index()].contacts.contains(&contact)
            }
            StateTest::ResourceEnabled(resource) => retained_resources.contains(&resource),
        },
        BoundaryPortKind::ContactAlternative { contact, .. } => retained_contacts.contains(&contact),
        BoundaryPortKind::ObserverWatch { observer, site } => {
            retained_observers.contains(&observer)
                && retained_cells.iter().any(|use_| {
                    use_.reset.observer() == Some(observer)
                        && backend.program.sites()[site.index()].source == SiteSource::Contact(backend.program.pistons()[use_.actor.index()].base)
                })
        }
        BoundaryPortKind::GuardSite { site, .. } => match backend.program.sites()[site.index()].source {
            SiteSource::Contact(contact) => retained_contacts.contains(&contact),
            SiteSource::Component(node) => {
                retained_observers.contains(&node) || retained_actors.iter().any(|actor| backend.program.pistons()[actor.index()].node == node)
            }
            SiteSource::Constant(_) => false,
        },
        _ => false,
    }
}

#[derive(Clone, Copy)]
struct FramePowerInput {
    node: NodeId,
    port: InputPort,
    output: usize,
    reset: NodeId,
}

#[derive(Clone, Copy)]
struct FrameRepeaterInput {
    node: NodeId,
    output: usize,
    strength: u8,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ActiveFramePowerInput {
    pub(super) node: NodeId,
    pub(super) port: InputPort,
    pub(super) data_strength: u8,
    pub(super) reset: Option<NodeId>,
}

pub(super) struct LogicalDevice {
    machine: Option<SettledMachine>,
    frame: Option<SettledFrame>,
    clock: LogicalClock,
    trigger: NodeId,
    external: Box<[bool]>,
    external_nodes: Box<[NodeId]>,
    outputs: Box<[(NodeId, usize)]>,
    registers: Box<[Register]>,
}

impl SettledFrame {
    fn new(backend: &GraphBackend, circuit: &SettledCircuit, clock: &LogicalClock) -> Result<Self, String> {
        if !circuit.declines.is_empty() {
            return Err("logical frame requires a complete circuit".into());
        }

        let mut feedback_outputs = FxHashMap::<ResourceId, (ContactId, usize)>::default();
        let mut outputs = Vec::new();
        let mut retained_power_inputs = Vec::new();
        let mut terminal_repeater_inputs = Vec::new();
        let power_output_nodes: FxHashSet<_> = backend
            .program
            .power_links()
            .iter()
            .filter_map(|link| match backend.program.sites()[link.from.index()].source {
                SiteSource::Component(node) => Some(node),
                _ => None,
            })
            .collect();
        let retained_actors: FxHashSet<_> = circuit.retained_cells.iter().map(|use_| use_.actor).collect();
        let mut retained_actor_order = circuit.retained_cells.iter().map(|use_| use_.actor).collect::<Vec<_>>();
        retained_actor_order.sort_unstable();
        retained_actor_order.dedup();
        let retained_resets: FxHashMap<_, _> = circuit
            .retained_cells
            .iter()
            .map(|use_| {
                use_.reset
                    .observer()
                    .map(|node| (use_.actor, node))
                    .ok_or_else(|| "logical frame retained output cell has no observer reset".to_owned())
            })
            .collect::<Result<_, _>>()?;
        let mut retained_observers = retained_resets.values().copied().collect::<Vec<_>>();
        retained_observers.sort_unstable();
        retained_observers.dedup();
        let retained_contacts = retained_actors
            .iter()
            .flat_map(|actor| {
                let spec = backend.program.pistons()[actor.index()];
                [spec.base, spec.near, spec.far]
            })
            .collect::<FxHashSet<_>>();
        let mut retained_resources = Vec::new();
        for use_ in &circuit.retained_cells {
            if let Some(resource) = use_.retraction.wake.resource {
                retained_resources.push(resource.resource);
            }
            if let Some(transfer) = use_.retraction.transfer {
                retained_resources.push(transfer.resource);
            }
        }
        retained_resources.sort_unstable();
        retained_resources.dedup();
        for (index, output) in circuit.outputs.iter().enumerate() {
            match output.target {
                SettledTarget::Power(PowerTarget::Input { node, port }) => {
                    let retained_actor = backend
                        .program
                        .pistons()
                        .iter()
                        .enumerate()
                        .find_map(|(actor, spec)| (spec.node == node).then_some(PistonId::new(actor as u32)))
                        .filter(|actor| retained_actors.contains(actor));
                    if let Some(actor) = retained_actor {
                        if output.kind != SettledValueKind::Bool || !matches!(port, InputPort::Main | InputPort::Quasi) {
                            return Err("retained output cell power roots must be Boolean Main/Quasi queries".into());
                        }
                        let reset = retained_resets[&actor];
                        retained_power_inputs.push(FramePowerInput {
                            node,
                            port,
                            output: index,
                            reset,
                        });
                    } else if port == InputPort::Main
                        && matches!(
                            backend.program.nodes()[node.index()].kind,
                            CircuitNodeKind::Electrical(NodeType::Repeater { .. })
                        )
                    {
                        if output.kind != SettledValueKind::Bool {
                            return Err("terminal repeater input roots must be Boolean".into());
                        }
                        if !backend
                            .program
                            .power_links_to(PowerTarget::Input { node, port: InputPort::Side })
                            .is_empty()
                        {
                            return Err("terminal repeater with a side input is not owned by the logical frame".into());
                        }
                        if power_output_nodes.contains(&node) {
                            return Err("logical frame repeater output has a downstream consumer".into());
                        }
                        terminal_repeater_inputs.push(FrameRepeaterInput {
                            node,
                            output: index,
                            strength: 0,
                        });
                    } else if port == InputPort::Main {
                        outputs.push((node, index, output.kind));
                    } else {
                        return Err("logical frame has an unsupported public input port".into());
                    }
                }
                SettledTarget::ResourceState { resource, source } => {
                    if output.kind != SettledValueKind::Bool {
                        return Err("logical frame feedback output must be Boolean".into());
                    }
                    let contacts = &backend.program.resources()[resource.index()].contacts;
                    if contacts.len() < 2 || !contacts.contains(&source) {
                        return Err("logical frame feedback must include its canonical source contact".into());
                    }
                    if feedback_outputs.insert(resource, (source, index)).is_some() {
                        return Err("logical frame resource has multiple next-state outputs".into());
                    }
                }
                _ => return Err("logical frame has an unsupported output target".into()),
            }
        }
        let retained_repeaters: FxHashSet<_> = terminal_repeater_inputs.iter().map(|input| input.node).collect();
        for &actor in &retained_actors {
            let node = backend.program.pistons()[actor.index()].node;
            let reset = retained_resets[&actor];
            for port in [InputPort::Main, InputPort::Quasi] {
                let target = PowerTarget::Input { node, port };
                if backend
                    .program
                    .power_links_to(target)
                    .iter()
                    .any(|link| retained_unbound_reset_source(&backend.program, reset, link))
                {
                    return Err("retained output cell actor input has an unbound reset source".into());
                }
                if !retained_power_inputs.iter().any(|input| input.node == node && input.port == port) {
                    return Err("retained output cell is missing a compiled Main/Quasi data root".into());
                }
            }
            if ![InputPort::Main, InputPort::Quasi].into_iter().any(|port| {
                backend
                    .program
                    .power_links_to(PowerTarget::Input { node, port })
                    .iter()
                    .any(|link| retained_reset_input_path_is_owned(&backend.program, actor, reset, link))
            }) {
                return Err("retained output cell reset is not bound to its actor power ports".into());
            }
            let use_ = circuit.retained_cells.iter().find(|use_| use_.actor == actor).unwrap();
            let transfer = use_.retraction.transfer.ok_or("retained output cell has no certified payload transfer")?;
            let piston_state = backend.state.pistons[actor.index()];
            let resource_state = backend.state.resources[transfer.resource.index()];
            if !piston_state.extended
                || !piston_state.head_present
                || piston_state.availability != PistonAvailability::Ready
                || !resource_state.enabled
                || resource_state.at != transfer.source
            {
                return Err("retained output cell is not at its certified stationary entry state".into());
            }
        }
        let mut retained_commands = Vec::new();
        for effect in &circuit.retained_effects {
            match effect.kind {
                BoundaryPortKind::Power { link } => match backend.program.power_links()[link as usize].to {
                    PowerTarget::Input { node, port: InputPort::Main }
                        if matches!(
                            backend.program.nodes()[node.index()].kind,
                            CircuitNodeKind::Electrical(NodeType::CommandBlock { .. })
                        ) =>
                    {
                        retained_commands.push(node)
                    }
                    PowerTarget::Input { node, .. }
                        if backend
                            .program
                            .pistons()
                            .iter()
                            .enumerate()
                            .any(|(actor, spec)| spec.node == node && retained_actors.contains(&PistonId::new(actor as u32))) => {}
                    PowerTarget::Input { node, .. } if matches!(backend.program.nodes()[node.index()].kind, CircuitNodeKind::Wire(_)) => {
                        // The compiler records this exact Power link only when it is
                        // paired with the certified frame-owned corridor input.
                    }
                    PowerTarget::Conductor { .. }
                        if retained_resets.iter().any(|(&actor, &reset)| {
                            retained_reset_conductor_is_owned(&backend.program, actor, reset, backend.program.power_links()[link as usize])
                        }) => {}
                    target => {
                        return Err(format!(
                            "logical frame retained output effect targets an unsupported power sink {target:?}"
                        ));
                    }
                },
                BoundaryPortKind::Callback { .. } => {}
                BoundaryPortKind::WireNeighborhood { .. } | BoundaryPortKind::WireRelation { .. } | BoundaryPortKind::CapturedWire { .. }
                    if circuit
                        .retained_effects
                        .iter()
                        .any(|candidate| candidate.direction == BoundaryDirection::Output && candidate.kind == effect.kind) =>
                {
                    // The compiler only emits these retained effects when the paired
                    // corridor input is certified as part of the settled frame.
                }
                kind if retained_cell_observation_is_owned(
                    backend,
                    kind,
                    &retained_actors,
                    &retained_contacts,
                    &circuit.retained_cells,
                    &retained_observers,
                    &retained_resources,
                ) => {}
                _ => return Err("logical frame retained output effect has an unsupported kind".into()),
            }
        }
        retained_commands.sort_unstable();
        retained_commands.dedup();

        let mut retained_routes = FxHashSet::default();
        let mut retained_route_ids = FxHashSet::default();
        let mut required_occurrences = FxHashSet::default();
        let mut command_output_occurrences = FxHashSet::default();
        let mut terminal_boundary_occurrences = FxHashSet::default();
        let retained_output_callbacks: FxHashSet<_> = circuit
            .retained_effects
            .iter()
            .filter_map(|effect| match effect.kind {
                BoundaryPortKind::Callback { occurrence } => Some(occurrence),
                _ => None,
            })
            .collect();
        for use_ in &circuit.retained_cells {
            required_occurrences.extend(use_.activations.iter().copied());
            let spec = backend.program.pistons()[use_.actor.index()];
            let mut routes = vec![
                use_.retraction.routes.base_changed,
                use_.retraction.routes.near_changed,
                use_.retraction.wake.base_restored,
                spec.routes.base_changed,
                spec.routes.near_changed,
                spec.routes.base_restored,
                spec.routes.head_restored,
            ];
            routes.extend(
                [use_.retraction.routes.far_changed, spec.routes.far_changed, spec.routes.far_shape_changed]
                    .into_iter()
                    .flatten(),
            );
            routes.extend(
                [spec.routes.resource_restored_near, spec.routes.resource_restored_far]
                    .into_iter()
                    .flatten(),
            );
            if let Some(resource) = use_.retraction.wake.resource {
                retained_route_ids.insert(resource.route);
            }
            if let Some(wake) = use_.reset.observer_wake() {
                required_occurrences.extend(wake.on_base_changed.iter().copied());
                required_occurrences.extend(wake.on_base_restored.iter().copied());
            }
            retained_route_ids.extend(routes);
        }
        let mut command_route_nodes = retained_commands.clone();
        command_route_nodes.extend(circuit.outputs.iter().filter_map(|output| match output.target {
            SettledTarget::Power(PowerTarget::Input { node, port: InputPort::Main })
                if matches!(
                    backend.program.nodes()[node.index()].kind,
                    CircuitNodeKind::Electrical(NodeType::CommandBlock { .. })
                ) =>
            {
                Some(node)
            }
            _ => None,
        }));
        command_route_nodes.sort_unstable();
        command_route_nodes.dedup();
        for &command in &command_route_nodes {
            if let Some(route) = backend.program.nodes()[command.index()].output_route {
                retained_route_ids.insert(route);
                command_output_occurrences.extend(backend.program.routes()[route.index()].deliveries.iter().enumerate().map(|(ordinal, _)| {
                    RouteOccurrence {
                        route,
                        ordinal: ordinal as u32,
                    }
                }));
            }
        }
        let public_wire_outputs: FxHashSet<_> = outputs
            .iter()
            .filter_map(|(node, _, _)| matches!(backend.program.nodes()[node.index()].kind, CircuitNodeKind::Wire(_)).then_some(*node))
            .collect();
        // The frame writes these wire values directly; their native callbacks must not overwrite them later.
        for node in retained_repeaters.iter().copied() {
            if let Some(route) = backend.program.nodes()[node.index()].output_route {
                retained_route_ids.insert(route);
                terminal_boundary_occurrences.extend(backend.program.routes()[route.index()].deliveries.iter().enumerate().map(|(ordinal, _)| {
                    RouteOccurrence {
                        route,
                        ordinal: ordinal as u32,
                    }
                }));
            }
        }
        let mut occurrences_to_classify = required_occurrences.clone();
        for route in retained_route_ids {
            occurrences_to_classify.extend(backend.program.routes()[route.index()].deliveries.iter().enumerate().map(|(ordinal, _)| {
                RouteOccurrence {
                    route,
                    ordinal: ordinal as u32,
                }
            }));
        }
        for occurrence in occurrences_to_classify {
            let Some(delivery) = backend.program.routes()[occurrence.route.index()]
                .deliveries
                .get(occurrence.ordinal as usize)
            else {
                return Err("retained output cell certificate names a missing callback occurrence".into());
            };
            let source = backend.program.sites()[delivery.target.index()].source;
            let receiver = delivery.receiver.or_else(|| match (source, delivery.kind) {
                (SiteSource::Component(node), DeliveryKind::Update { .. }) => Some(CallbackReceiver::Component(node)),
                _ => None,
            });
            let owned = retained_cell_callback_is_owned(
                &backend.program,
                delivery,
                &retained_actors,
                &retained_contacts,
                &retained_observers,
                &retained_commands,
                &retained_repeaters,
            )
            .map_err(|reason| {
                let route = &backend.program.routes()[occurrence.route.index()];
                let route_position = backend.program.nodes()[route.source.index()].position;
                let receiver_position = match receiver {
                    Some(CallbackReceiver::Component(node)) => backend.program.nodes()[node.index()].position,
                    Some(CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor)) => {
                        let node = backend.program.pistons()[actor.index()].node;
                        backend.program.nodes()[node.index()].position
                    }
                    Some(CallbackReceiver::Contact(contact)) => Some(backend.program.contacts()[contact.index()].position),
                    None => None,
                };
                format!(
                    "{reason}; {occurrence:?} from {:?} to {receiver:?} at {:?}",
                    route_position, receiver_position
                )
            })?;
            if matches!(receiver, Some(CallbackReceiver::Component(node)) if matches!(backend.program.nodes()[node.index()].kind, CircuitNodeKind::Wire(_)))
                && !retained_output_callbacks.contains(&occurrence)
                && !matches!(receiver, Some(CallbackReceiver::Component(node)) if public_wire_outputs.contains(&node))
            {
                return Err("retained output cell callback reaches an unowned wire outside its certified corridor".into());
            }
            if owned {
                retained_routes.insert(occurrence);
            } else if terminal_boundary_occurrences.contains(&occurrence) {
                match receiver {
                    // A repeater with only inert shape/constant listeners has no further
                    // native effect to preserve beyond its own scheduled state change.
                    None => {
                        retained_routes.insert(occurrence);
                    }
                    // The frame publishes these wires from its equation result; a later
                    // physical neighbor callback must not replace that value with stale
                    // graph state.
                    Some(CallbackReceiver::Component(node))
                        if public_wire_outputs.contains(&node) && matches!(backend.program.nodes()[node.index()].kind, CircuitNodeKind::Wire(_)) => {}
                    _ => return Err("terminal repeater output route reaches an unowned receiver".into()),
                }
            } else if command_output_occurrences.contains(&occurrence) {
                if receiver.is_some() && !retained_output_callbacks.contains(&occurrence) {
                    return Err("retained command output route reaches an unowned callback receiver".into());
                }
                if retained_output_callbacks.contains(&occurrence) {
                    retained_routes.insert(occurrence);
                }
            }
        }
        let mut retained_routes = retained_routes.into_iter().collect::<Vec<_>>();
        retained_routes.sort_unstable_by_key(|occurrence| (occurrence.route, occurrence.ordinal));

        let mut boundaries = Vec::new();
        let mut button_nodes = FxHashSet::default();
        let mut feedback_slots = Vec::new();
        let mut feedback_resources = Vec::new();
        let mut feedback_seen = FxHashSet::default();
        for (slot, input) in circuit.program.inputs.iter().enumerate() {
            match input.source {
                LayerInputSource::State(StateTest::ResourceAt { resource, contact })
                | LayerInputSource::BoundaryState {
                    test: StateTest::ResourceAt { resource, contact },
                    ..
                } => {
                    let Some(&(source, output)) = feedback_outputs.get(&resource) else {
                        return Err("logical frame has an unbound resource-state input".into());
                    };
                    let contacts = &backend.program.resources()[resource.index()].contacts;
                    if !contacts.contains(&contact) {
                        return Err("logical frame reads a non-member feedback contact".into());
                    }
                    // shortcut: retain canonical-source occupancy, not which of up to five near owners wins.
                    feedback_slots.push(FeedbackSlot {
                        slot,
                        output,
                        polarity: contact == source,
                    });
                    if feedback_seen.insert(resource) {
                        let other = contacts.iter().copied().find(|&candidate| candidate != source).unwrap();
                        feedback_resources.push(FeedbackResource {
                            resource,
                            source,
                            other,
                            output,
                        });
                    }
                }
                LayerInputSource::State(test) | LayerInputSource::BoundaryState { test, .. } => {
                    if clock.state_value(test).is_none() {
                        return Err(format!("logical frame has an unbound external state input: {test:?}"));
                    }
                    boundaries.push(FrameBoundary::ClockState { slot, test });
                }
                LayerInputSource::SourceAbove { site, port, threshold } => {
                    let site_spec = &backend.program.sites()[site.index()];
                    let clock_node = match site_spec.source {
                        SiteSource::Component(node) if clock.source_value(node).is_some() => Some(node),
                        SiteSource::Component(node) => {
                            match backend.program.nodes()[node.index()].kind {
                                CircuitNodeKind::Electrical(NodeType::Button) => {
                                    button_nodes.insert(node);
                                }
                                CircuitNodeKind::Electrical(NodeType::Lever | NodeType::PressurePlate) => {}
                                _ => {
                                    return Err(format!(
                                        "logical frame external power source is not a lever, button, or pressure plate: {node:?}"
                                    ));
                                }
                            }
                            None
                        }
                        SiteSource::Constant(_) => None,
                        SiteSource::Contact(contact) => {
                            return Err(format!("logical frame has an unbound contact power input: {contact:?}"));
                        }
                    };
                    if clock_node.is_some() && !matches!(port, PowerPort::Output | PowerPort::Weak(_) | PowerPort::Strong(_)) {
                        return Err("logical frame clock boundary requires a direct component power port".into());
                    }
                    boundaries.push(FrameBoundary::Power {
                        slot,
                        source: input.source,
                        clock_node,
                        threshold,
                    });
                }
                other => return Err(format!("logical frame has an unsupported input binding: {other:?}")),
            }
        }
        if feedback_seen.len() != feedback_outputs.len() {
            return Err("logical frame has feedback output without a matching input".into());
        }

        Ok(Self {
            evaluation: SettledEvaluation::new(circuit)?,
            inputs: vec![false; circuit.program.inputs.len()].into_boxed_slice(),
            boundaries: boundaries.into_boxed_slice(),
            feedback_slots: feedback_slots.into_boxed_slice(),
            feedback_resources: feedback_resources.into_boxed_slice(),
            outputs: outputs.into_boxed_slice(),
            button_nodes: button_nodes.into_iter().collect::<Vec<_>>().into_boxed_slice(),
            retained_actors: retained_actor_order.into_boxed_slice(),
            retained_observers: retained_observers.into_boxed_slice(),
            retained_commands: retained_commands.into_boxed_slice(),
            retained_routes: retained_routes.into_boxed_slice(),
            retained_resources: retained_resources.into_boxed_slice(),
            retained_power_inputs: retained_power_inputs.into_boxed_slice(),
            terminal_repeater_inputs: terminal_repeater_inputs.into_boxed_slice(),
            computed: false,
        })
    }

    fn evaluate(&mut self) -> Result<usize, &'static str> {
        for slot in &self.feedback_slots {
            self.inputs[slot.slot] = slot.polarity;
        }
        let mut operations = 0;
        for _ in 0..MAX_FRAME_PASSES {
            operations += self.evaluation.evaluate(&self.inputs)?;
            let mut changed = false;
            for slot in &self.feedback_slots {
                let next = (self.evaluation.outputs()[slot.output] != 0) == slot.polarity;
                changed |= self.inputs[slot.slot] != next;
                self.inputs[slot.slot] = next;
            }
            if !changed {
                return Ok(operations);
            }
        }
        Err("logical frame feedback did not converge within 128 passes")
    }
}

fn inert_cell_contact(program: &CircuitProgram, contact: ContactId) -> bool {
    program.resources().iter().all(|resource| !resource.contacts.contains(&contact))
        && program.contacts()[contact.index()].variants.iter().all(|variant| {
            let profile = program.profiles()[variant.profile.index()];
            profile.electrical == ProfileElectrical::None && variant.receiver.is_none()
        })
}

fn retained_reset_conductor_is_owned(program: &CircuitProgram, actor: PistonId, reset: NodeId, link: PowerLink) -> bool {
    let PowerTarget::Conductor { site, include_dust } = link.to else {
        return false;
    };
    if !matches!(link.from_port, PowerPort::Output | PowerPort::Stored)
        || !matches!(program.sites()[link.from.index()].source, SiteSource::Component(node) if node == reset)
    {
        return false;
    }
    let SiteSource::Constant(profile) = program.sites()[site.index()].source else {
        return false;
    };
    let profile = program.profiles()[profile.index()];
    if !profile.solid || !profile.cube {
        return false;
    }
    if !program.power_links().iter().any(|candidate| candidate.from == site) {
        return true;
    }
    let foreign_dynamic_source = program.power_links().iter().any(|candidate| {
        matches!(candidate.to, PowerTarget::Conductor { site: candidate_site, include_dust: dust } if candidate_site == site && dust == include_dust)
            && matches!(program.sites()[candidate.from.index()].source, SiteSource::Component(node) if node != reset)
    });
    let piston_node = program.pistons()[actor.index()].node;
    !foreign_dynamic_source
        && program.power_links().iter().any(|actor_link| {
            actor_link.from == site
                && matches!(actor_link.from_port, PowerPort::Conducted { include_dust: dust } if dust == include_dust)
                && matches!(actor_link.to, PowerTarget::Input { node, port: InputPort::Main | InputPort::Quasi } if node == piston_node)
        })
}

fn retained_reset_input_path_is_owned(program: &CircuitProgram, actor: PistonId, reset: NodeId, input: &PowerLink) -> bool {
    if matches!(program.sites()[input.from.index()].source, SiteSource::Component(node) if node == reset) {
        return true;
    }
    let SiteSource::Constant(profile) = program.sites()[input.from.index()].source else {
        return false;
    };
    if !program.profiles()[profile.index()].solid || !program.profiles()[profile.index()].cube {
        return false;
    }
    let PowerPort::Conducted { include_dust } = input.from_port else {
        return false;
    };
    let conductor = PowerTarget::Conductor {
        site: input.from,
        include_dust,
    };
    program
        .power_links_to(conductor)
        .iter()
        .any(|link| retained_reset_conductor_is_owned(program, actor, reset, *link))
}

fn retained_unbound_reset_source(program: &CircuitProgram, reset: NodeId, input: &PowerLink) -> bool {
    let is_other_reset = |node: NodeId| {
        node != reset
            && matches!(
                program.nodes()[node.index()].kind,
                CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. } | CircuitNodeKind::Electrical(NodeType::Observer { .. })
            )
    };
    if matches!(program.sites()[input.from.index()].source, SiteSource::Component(node) if is_other_reset(node)) {
        return true;
    }
    let SiteSource::Constant(profile) = program.sites()[input.from.index()].source else {
        return false;
    };
    if !program.profiles()[profile.index()].solid || !program.profiles()[profile.index()].cube {
        return false;
    }
    let PowerPort::Conducted { include_dust } = input.from_port else {
        return false;
    };
    program
        .power_links_to(PowerTarget::Conductor {
            site: input.from,
            include_dust,
        })
        .iter()
        .any(|link| matches!(program.sites()[link.from.index()].source, SiteSource::Component(node) if is_other_reset(node)))
}

impl GraphBackend {
    pub(crate) fn logical_device_admission(&self) -> &LogicalDeviceAdmission {
        &self.logical_device_admission
    }

    pub(super) fn install_logical_device(&mut self) -> Result<LogicalDeviceAdmission, String> {
        let Some(circuit) = self.program.settled_circuit() else {
            return Ok(LogicalDeviceAdmission::NotApplicable);
        };
        let Some(timing) = &circuit.timing else {
            return Ok(LogicalDeviceAdmission::Declined("no certified logical-device timing".into()));
        };
        let is_frame = circuit
            .outputs
            .iter()
            .any(|output| matches!(output.target, SettledTarget::ResourceState { .. }));
        if !self.future.pending.is_empty() || !self.future.piston_requests.is_empty() || !self.future.deferred_pistons.is_empty() {
            // Keep entry-time graph work on the ordinary executor; never abandon a live queue.
            #[cfg(test)]
            eprintln!("logical device activation declined: pending graph work; frame={is_frame}");
            return Ok(LogicalDeviceAdmission::Declined("pending graph work at entry".into()));
        }
        if is_frame && self.future.scheduler.iter().next().is_some() {
            #[cfg(test)]
            eprintln!("logical device activation declined: scheduled work; frame=true");
            return Ok(LogicalDeviceAdmission::Declined("scheduled graph work at entry".into()));
        }
        let clock = match LogicalClock::new(&self.program, &self.state, timing) {
            Ok(clock) => clock,
            Err(error) if is_frame => {
                #[cfg(test)]
                eprintln!("logical device activation declined: clock: {error}");
                return Ok(LogicalDeviceAdmission::Declined(format!("logical clock: {error}")));
            }
            Err(error) => return Err(error),
        };
        if is_frame {
            let frame = match SettledFrame::new(self, circuit, &clock) {
                Ok(frame) => frame,
                Err(_error) => {
                    #[cfg(test)]
                    eprintln!("logical device activation declined: frame: {_error}");
                    return Ok(LogicalDeviceAdmission::Declined(format!("logical frame: {_error}")));
                }
            };
            self.logical_device = Some(LogicalDevice {
                machine: None,
                frame: Some(frame),
                external: Box::new([]),
                external_nodes: Box::new([]),
                outputs: Box::new([]),
                registers: Box::new([]),
                clock,
                trigger: timing.trigger,
            });
            return Ok(LogicalDeviceAdmission::Active);
        }

        let inputs = circuit
            .program
            .inputs
            .iter()
            .map(|input| self.sample_settled_input(input.source))
            .collect::<Result<Vec<_>, _>>()?;
        let machine = SettledMachine::new(circuit, &inputs)?;
        let mut external_nodes = Vec::new();
        for input in machine.external_inputs() {
            let LayerInputSource::SourceAbove { site, .. } = input.source else {
                return Err(format!("logical device has unresolved physical input {:?}", input.source));
            };
            let SiteSource::Component(node) = self.program.sites()[site.index()].source else {
                return Err("logical device has a non-component boundary source".into());
            };
            if clock.source_value(node).is_none() {
                return Err(format!("logical device reads outside its clock kernel: {node:?}"));
            }
            external_nodes.push(node);
        }
        let mut outputs = Vec::new();
        let mut registers = Vec::new();
        for (index, output) in circuit.outputs.iter().enumerate() {
            match output.target {
                SettledTarget::Power(PowerTarget::Input { node, port: InputPort::Main }) => outputs.push((node, index)),
                SettledTarget::Register(read) => {
                    let spec = self.program.pistons()[read.storage.index()];
                    let PistonLane::SingleResource { far } = spec.lane else {
                        return Err("logical register has no canonical lane".into());
                    };
                    registers.push(Register {
                        storage: read.storage,
                        resource: read.resource,
                        near: spec.near,
                        far,
                        source: read.source,
                        output: index,
                    });
                }
                _ => return Err("logical device has an unsupported public output".into()),
            }
        }
        self.logical_device = Some(LogicalDevice {
            external: vec![false; external_nodes.len()].into_boxed_slice(),
            external_nodes: external_nodes.into_boxed_slice(),
            machine: Some(machine),
            frame: None,
            clock,
            trigger: timing.trigger,
            outputs: outputs.into_boxed_slice(),
            registers: registers.into_boxed_slice(),
        });
        Ok(LogicalDeviceAdmission::Active)
    }

    pub(super) fn tick_logical_device<'h>(&mut self, host: &mut Option<&'h mut (dyn GraphCommandHost + 'h)>) -> Result<(), GraphExecutionError> {
        let mut device = self.logical_device.take().expect("installed logical device");
        if let Some(frame) = device
            .frame
            .as_ref()
            .filter(|frame| !frame.retained_actors.is_empty() || !frame.terminal_repeater_inputs.is_empty())
        {
            self.active_frame_command_outputs.clear();
            self.active_frame_retained_actors = frame.retained_actors.to_vec();
            self.active_frame_retained_nodes = frame
                .retained_actors
                .iter()
                .map(|actor| self.program.pistons()[actor.index()].node)
                .chain(frame.retained_observers.iter().copied())
                .chain(frame.retained_commands.iter().copied())
                .chain(frame.button_nodes.iter().copied())
                .chain(frame.terminal_repeater_inputs.iter().map(|input| input.node))
                .chain(frame.outputs.iter().filter_map(|(node, _, _)| {
                    matches!(
                        self.program.nodes()[node.index()].kind,
                        CircuitNodeKind::Electrical(NodeType::CommandBlock { .. })
                    )
                    .then_some(*node)
                }))
                .collect();
            self.active_frame_retained_nodes.sort_unstable();
            self.active_frame_retained_nodes.dedup();
            self.active_frame_retained_routes = frame.retained_routes.to_vec();
            self.refresh_frame_power_inputs(frame);
            for &(node, _, _) in &frame.outputs {
                if matches!(
                    self.program.nodes()[node.index()].kind,
                    CircuitNodeKind::Electrical(NodeType::CommandBlock { .. })
                ) {
                    self.active_frame_command_outputs.push(node);
                }
            }
            let result = self.tick_phases_with_hook(host, |backend, _| {
                let step = device
                    .clock
                    .advance(backend.node_state(device.trigger).powered)
                    .map_err(|kind| backend.unsupported(device.trigger, kind))?;
                backend.run_frame_step(&mut device, step)
            });
            self.active_frame_command_outputs.clear();
            self.active_frame_retained_actors.clear();
            self.active_frame_retained_nodes.clear();
            self.active_frame_retained_routes.clear();
            self.active_frame_power_inputs.clear();
            self.logical_device = Some(device);
            return result;
        }
        self.counts.ticks += 1;
        self.future.clock.logical_tick += 1;
        self.active_frame_command_outputs.clear();
        if let Some(frame) = &device.frame {
            for &(node, _, _) in &frame.outputs {
                if matches!(
                    self.program.nodes()[node.index()].kind,
                    CircuitNodeKind::Electrical(NodeType::CommandBlock { .. })
                ) {
                    self.active_frame_command_outputs.push(node);
                }
            }
        }
        if let Some(host) = host.as_mut() {
            host.publish_logical_tick(self.future.clock.logical_tick);
        }
        let result = (|| {
            if let Some(frame) = &device.frame {
                self.refresh_frame_power_inputs(frame);
                self.advance_frame_scheduler(device.trigger, &frame.button_nodes, host)?;
            }
            let step = device
                .clock
                .advance(self.node_state(device.trigger).powered)
                .map_err(|kind| self.unsupported(device.trigger, kind))?;
            if device.frame.is_some() {
                self.run_frame_step(&mut device, step)?;
            } else if step.compute {
                let machine = device.machine.as_mut().expect("register machine installed");
                for (value, &node) in device.external.iter_mut().zip(device.external_nodes.iter()) {
                    *value = device.clock.source_value(node).expect("validated logical device input");
                }
                let operations = machine.step(&device.external).expect("validated logical device inputs");
                self.counts.logical_kernel_evaluations += 1;
                self.counts.logical_kernel_operations += operations as u64;
            }
            if step.publish && device.frame.is_none() {
                let machine = device.machine.as_ref().expect("register machine installed");
                for &(node, output) in &device.outputs {
                    let strength = 15 * u8::from(machine.outputs()[output] != 0);
                    self.set_node(node, strength != 0, strength, None, false)?;
                }
            }
            if step.commit && device.frame.is_none() {
                let machine = device.machine.as_ref().expect("register machine installed");
                for register in &device.registers {
                    let at = if machine.outputs()[register.output] != 0 {
                        register.source
                    } else if register.source == register.far {
                        register.near
                    } else {
                        register.far
                    };
                    let piston = &mut self.state.pistons[register.storage.index()];
                    piston.extended = at == register.far;
                    piston.head_present = piston.extended;
                    piston.availability = PistonAvailability::Ready;
                    let resource = &mut self.state.resources[register.resource.index()];
                    resource.at = at;
                    resource.enabled = true;
                }
            }
            Ok(())
        })();
        self.active_frame_command_outputs.clear();
        self.active_frame_power_inputs.clear();
        self.logical_device = Some(device);
        result
    }

    fn refresh_frame_power_inputs(&mut self, frame: &SettledFrame) {
        self.active_frame_power_inputs.clear();
        self.active_frame_power_inputs
            .extend(frame.retained_power_inputs.iter().map(|input| ActiveFramePowerInput {
                node: input.node,
                port: input.port,
                data_strength: if frame.evaluation.outputs()[input.output] != 0 { 15 } else { 0 },
                reset: Some(input.reset),
            }));
        if frame.computed {
            self.active_frame_power_inputs
                .extend(frame.terminal_repeater_inputs.iter().map(|input| ActiveFramePowerInput {
                    node: input.node,
                    port: InputPort::Main,
                    data_strength: input.strength,
                    reset: None,
                }));
        }
    }

    fn run_frame_step(&mut self, device: &mut LogicalDevice, step: super::logical_clock::ClockStep) -> Result<(), GraphExecutionError> {
        let Some(frame) = &mut device.frame else {
            return Ok(());
        };
        let retained_path = !frame.retained_actors.is_empty();
        if !step.compute && !(retained_path && step.resample) {
            return Ok(());
        }
        for boundary in frame.boundaries.iter().copied() {
            let (slot, value) = match boundary {
                FrameBoundary::Power {
                    slot,
                    source: _,
                    clock_node: Some(node),
                    threshold,
                    ..
                } => (slot, device.clock.source_value(node).is_some_and(|powered| powered && 15 > threshold)),
                FrameBoundary::Power { slot, source, .. } => (
                    slot,
                    self.sample_settled_input(source).map_err(|kind| self.unsupported(device.trigger, kind))?,
                ),
                FrameBoundary::ClockState { slot, test } => (
                    slot,
                    device
                        .clock
                        .state_value(test)
                        .ok_or_else(|| self.unsupported(device.trigger, "unbound logical clock state"))?,
                ),
            };
            frame.inputs[slot] = value;
        }
        let operations = frame.evaluate().map_err(|kind| self.unsupported(device.trigger, kind))?;
        if step.compute {
            for input in frame.terminal_repeater_inputs.iter_mut() {
                input.strength = if frame.evaluation.outputs()[input.output] != 0 { 15 } else { 0 };
            }
            frame.computed = true;
        }
        self.counts.logical_kernel_evaluations += 1;
        self.counts.logical_kernel_operations += operations as u64;
        if step.compute {
            for feedback in &frame.feedback_resources {
                if retained_path && frame.retained_resources.contains(&feedback.resource) {
                    continue;
                }
                let at = if frame.evaluation.outputs()[feedback.output] != 0 {
                    feedback.source
                } else {
                    feedback.other
                };
                let resource = &mut self.state.resources[feedback.resource.index()];
                if resource.at != at || !resource.enabled {
                    resource.revision = resource.revision.wrapping_add(1);
                }
                resource.at = at;
                resource.enabled = true;
            }
        }
        if retained_path || (step.compute && !frame.terminal_repeater_inputs.is_empty()) {
            self.refresh_frame_power_inputs(frame);
            for &actor in frame.retained_actors.iter() {
                self.sample_piston(actor)?;
            }
        }
        if step.compute {
            for &(node, output, kind) in &frame.outputs {
                let value = frame.evaluation.outputs()[output];
                let strength = match kind {
                    SettledValueKind::Bool => 15 * u8::from(value != 0),
                    SettledValueKind::Strength => value,
                };
                self.publish_frame_output(node, strength)?;
            }
            for input in frame.terminal_repeater_inputs.iter() {
                self.update_repeater(input.node, None)?;
            }
        }
        Ok(())
    }

    pub(super) fn publish_frame_output(&mut self, node: NodeId, strength: u8) -> Result<(), GraphExecutionError> {
        if matches!(self.program.nodes()[node.index()].kind, CircuitNodeKind::Wire(_)) {
            let mut wire = self.wire_state(node);
            wire.power = strength;
            self.set_wire(node, wire);
        } else if matches!(
            self.program.nodes()[node.index()].kind,
            CircuitNodeKind::Electrical(NodeType::CommandBlock { .. })
        ) {
            self.update_command_powered(node, strength > 0)?;
        } else {
            self.set_node(node, strength != 0, strength, None, false)?;
        }
        Ok(())
    }

    pub(super) fn advance_frame_scheduler<'h>(
        &mut self,
        trigger: NodeId,
        button_nodes: &[NodeId],
        host: &mut Option<&'h mut (dyn GraphCommandHost + 'h)>,
    ) -> Result<(), GraphExecutionError> {
        for (tick, _, _) in self.future.scheduler.iter() {
            if tick.registry_type == tick.current_registry_type
                && !tick.node.is_some_and(|node| {
                    button_nodes.contains(&node)
                        || matches!(
                            self.program.nodes()[node.index()].kind,
                            CircuitNodeKind::Electrical(NodeType::CommandBlock { .. })
                        )
                })
            {
                return Err(self.unsupported(trigger, "logical frame has an unexpected scheduled tick"));
            }
        }
        self.future.clock.phase = GraphPhase::ScheduledTicks;
        self.future.clock.scheduled_advanced = false;
        let result = (|| {
            self.drain_frame_ticks(trigger, button_nodes, host)?;
            self.future.scheduler.end_last_tick_move_next();
            self.future.clock.scheduled_advanced = true;
            self.drain_frame_ticks(trigger, button_nodes, host)
        })();
        self.future.clock.phase = GraphPhase::BetweenTicks;
        self.future.clock.scheduled_advanced = false;
        result
    }

    fn drain_frame_ticks<'h>(
        &mut self,
        trigger: NodeId,
        button_nodes: &[NodeId],
        host: &mut Option<&'h mut (dyn GraphCommandHost + 'h)>,
    ) -> Result<(), GraphExecutionError> {
        loop {
            let Some(tick) = ({ self.future.scheduler.this_tick_ref().iter().next().map(|(tick, _)| *tick) }) else {
                break;
            };
            if tick.registry_type == tick.current_registry_type
                && !tick.node.is_some_and(|node| {
                    button_nodes.contains(&node)
                        || matches!(
                            self.program.nodes()[node.index()].kind,
                            CircuitNodeKind::Electrical(NodeType::CommandBlock { .. })
                        )
                })
            {
                return Err(self.unsupported(trigger, "logical frame has an unexpected scheduled tick"));
            }
            let tick = self.future.scheduler.this_tick().pop_first().expect("peeked scheduler tick");
            self.popped(tick);
            if tick.registry_type != tick.current_registry_type {
                continue;
            }
            let Some(node) = tick.node.filter(|node| button_nodes.contains(node)) else {
                let Some(node) = tick.node.filter(|node| {
                    matches!(
                        self.program.nodes()[node.index()].kind,
                        CircuitNodeKind::Electrical(NodeType::CommandBlock { .. })
                    )
                }) else {
                    return Err(self.unsupported(trigger, "logical frame has an unexpected scheduled tick"));
                };
                self.counts.scheduled_ticks += 1;
                self.tick_command_block(node, host)?;
                continue;
            };
            self.counts.scheduled_ticks += 1;
            if self.node_state(node).powered {
                self.set_node(node, false, 0, None, false)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::CompilerOptions;
    use crate::redpiler::program_graph::{LayerExpr, ReducedLayerProgram, SettledCircuit};
    use std::sync::Arc;

    #[test]
    fn unretained_frame_does_not_evaluate_on_resample() {
        let (world, bounds) = super::super::adder_tests::counter_world();
        let ticks = world.scheduler().iter_entries().collect();
        let options = CompilerOptions {
            optimization_level: Some(6),
            budget_multiplier: 8,
            ..Default::default()
        };
        let mut backend = GraphBackend::compile_with_options(&world, bounds, ticks, &options, Arc::default()).unwrap();
        let mut device = backend.logical_device.take().expect("COUNTER installs its logical device");
        let circuit = SettledCircuit {
            program: ReducedLayerProgram {
                inputs: Box::new([]),
                expressions: vec![LayerExpr::Constant(false)].into_boxed_slice(),
                roots: Box::new([]),
            },
            outputs: Box::new([]),
            firing: None,
            retained_cells: Box::new([]),
            retained_effects: Box::new([]),
            declines: Box::new([]),
            timing: None,
        };
        device.frame = Some(SettledFrame {
            evaluation: SettledEvaluation::new(&circuit).unwrap(),
            inputs: Box::new([]),
            boundaries: Box::new([]),
            feedback_slots: Box::new([]),
            feedback_resources: Box::new([]),
            outputs: Box::new([]),
            button_nodes: Box::new([]),
            retained_actors: Box::new([]),
            retained_observers: Box::new([]),
            retained_commands: Box::new([]),
            retained_routes: Box::new([]),
            retained_resources: Box::new([]),
            retained_power_inputs: Box::new([]),
            terminal_repeater_inputs: Box::new([]),
            computed: false,
        });
        let step = device.clock.advance(true).unwrap();
        assert!(step.resample);
        assert!(!step.compute);

        backend.run_frame_step(&mut device, step).unwrap();
        assert_eq!(backend.counts.logical_kernel_evaluations, 0);
        backend
            .run_frame_step(
                &mut device,
                super::super::logical_clock::ClockStep {
                    compute: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(backend.counts.logical_kernel_evaluations, 1);
    }
}
