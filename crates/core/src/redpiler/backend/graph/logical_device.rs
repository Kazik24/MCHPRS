//! O6 data and timing kernels. Physical topology is used only at activation/projection.
use super::logical_clock::LogicalClock;
use super::*;
use crate::redpiler::LogicalDeviceAdmission;
use crate::redpiler::program_graph::settled_machine::SettledMachine;
use crate::redpiler::program_graph::{SettledEvaluation, SettledValueKind};

const MAX_FRAME_PASSES: usize = 128;

fn report_activation_decline(monitor: &TaskMonitor, index: usize, witness: Option<mchprs_blocks::BlockPos>, reason: &str) {
    let location = witness.map(|pos| format!(" at ({}, {}, {})", pos.x, pos.y, pos.z)).unwrap_or_default();
    monitor.record_warning(format!(
        "Fast device {index}{location} could not start: {reason}. Its work may run on Graph or another device."
    ));
}

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
    retained_actors: Box<[PistonId]>,
}

fn retained_feedback_entry_is_ready(state: &CircuitState, feedback: &FeedbackResource) -> bool {
    let resource = state.resources[feedback.resource.index()];
    resource.enabled
        && resource.at == feedback.source
        && feedback.retained_actors.iter().all(|&actor| {
            let piston = state.pistons[actor.index()];
            piston.extended && piston.head_present && piston.availability == PistonAvailability::Ready
        })
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
    retained_native_routes: Box<[RouteId]>,
    retained_resources: Box<[ResourceId]>,
    retained_power_inputs: Box<[FramePowerInput]>,
    terminal_repeater_inputs: Box<[FrameRepeaterInput]>,
    power_input_offset: usize,
}

enum DeviceClockDriver {
    Logical(LogicalClock),
    NativeAction { clock: PistonId, last_trigger: bool },
}

fn native_clock_state_is_bound(timing: &SettledTimingIsland, test: StateTest) -> bool {
    match test {
        StateTest::PistonExtended(actor) | StateTest::PistonHeadPresent(actor) | StateTest::PistonAvailability { piston: actor, .. } => {
            timing.actors.contains(&actor)
        }
        StateTest::ResourceAt { resource, .. } | StateTest::ResourceEnabled(resource) => {
            timing.actor_specs.iter().any(|actor| actor.resource == Some(resource))
        }
    }
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
                    backend.program.sites()[site.index()].source == SiteSource::Contact(backend.program.pistons()[use_.actor.index()].base)
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
    reset: FrameResetSource,
    data_strength: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FrameResetSource {
    Observer(NodeId),
    InputRestoration { resource: ResourceId, contact: ContactId },
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
    pub(super) reset: Option<FrameResetSource>,
}

pub(super) struct LogicalDevice {
    machine: Option<SettledMachine>,
    frame: Option<SettledFrame>,
    clock_driver: DeviceClockDriver,
    trigger: NodeId,
    external: Box<[bool]>,
    external_nodes: Box<[NodeId]>,
    outputs: Box<[(NodeId, usize)]>,
    registers: Box<[Register]>,
}

impl SettledFrame {
    fn new(
        backend: &GraphBackend,
        circuit: &SettledCircuit,
        scope: &SettledDeviceScope,
        clock: Option<&LogicalClock>,
        native_timing: Option<&SettledTimingIsland>,
    ) -> Result<Self, String> {
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
                let reset = match &use_.reset {
                    CellReset::Observer { node, .. } => FrameResetSource::Observer(*node),
                    CellReset::InputRestoration { resource, contact, .. } => {
                        if !backend
                            .program
                            .resources()
                            .get(resource.index())
                            .is_some_and(|spec| spec.contacts.contains(contact))
                        {
                            return Err("retained input-restoration reset contact is outside its resource".to_owned());
                        }
                        FrameResetSource::InputRestoration {
                            resource: *resource,
                            contact: *contact,
                        }
                    }
                    CellReset::Payload { .. } => {
                        return Err("logical frame retained output cell has an unsupported payload reset".to_owned());
                    }
                };
                Ok((use_.actor, reset))
            })
            .collect::<Result<_, String>>()?;
        let mut retained_observers = retained_resets
            .values()
            .filter_map(|reset| match reset {
                FrameResetSource::Observer(node) => Some(*node),
                FrameResetSource::InputRestoration { .. } => None,
            })
            .collect::<Vec<_>>();
        for effect in &circuit.retained_effects {
            let BoundaryPortKind::ObserverWatch { observer, site } = effect.kind else {
                continue;
            };
            let Some(site_spec) = backend.program.sites().get(site.index()) else {
                return Err("retained observer watch references a missing site".into());
            };
            let Some(node_spec) = backend.program.nodes().get(observer.index()) else {
                return Err("retained observer watch references a missing node".into());
            };
            let watched_site = match node_spec.kind {
                CircuitNodeKind::Observer { watched, .. } | CircuitNodeKind::BuiltinReset { watched, .. } => watched,
                _ => return Err("retained observer watch references a non-observer node".into()),
            };
            let watches_retained_base = circuit
                .retained_cells
                .iter()
                .any(|use_| site_spec.source == SiteSource::Contact(backend.program.pistons()[use_.actor.index()].base));
            if watched_site != site || !watches_retained_base || scope.owned_nodes.binary_search(&observer).is_ok() {
                return Err("retained observer watch is not a native observation of a retained cell base".into());
            }
            retained_observers.push(observer);
        }
        retained_observers.sort_unstable();
        retained_observers.dedup();
        let mut retained_contacts = retained_actors
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
            match &use_.reset {
                CellReset::InputRestoration { resource, contact, .. } => {
                    retained_resources.push(*resource);
                    retained_contacts.insert(*contact);
                }
                CellReset::Observer { .. } | CellReset::Payload { .. } => {}
            }
        }
        retained_resources.sort_unstable();
        retained_resources.dedup();
        for (index, output) in circuit.outputs.iter().enumerate() {
            match output.target {
                SettledTarget::Power(PowerTarget::Input { node, port }) => {
                    if matches!(backend.program.nodes()[node.index()].kind, CircuitNodeKind::Wire(_))
                        && scope.owned_nodes.binary_search(&node).is_err()
                    {
                        return Err("logical frame writes a wire outside its certified component scope".into());
                    }
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
                        if matches!(retained_resets[&actor], FrameResetSource::Observer(_)) {
                            retained_power_inputs.push(FramePowerInput {
                                node,
                                port,
                                output: index,
                                reset: retained_resets[&actor],
                                data_strength: backend.input_strength_without_frame_reset(node, port, retained_resets[&actor]),
                            });
                        }
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
                            strength: backend.input_strength(node, InputPort::Main),
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
                SettledTarget::ResetExcludedPower {
                    node,
                    port,
                    resource,
                    contact,
                } => {
                    if output.kind != SettledValueKind::Bool || !matches!(port, InputPort::Main | InputPort::Quasi) {
                        return Err("reset-excluded retained actor power roots must be Boolean Main/Quasi queries".into());
                    }
                    let actor = backend
                        .program
                        .pistons()
                        .iter()
                        .enumerate()
                        .find_map(|(actor, spec)| (spec.node == node).then_some(PistonId::new(actor as u32)))
                        .filter(|actor| retained_actors.contains(actor))
                        .ok_or("reset-excluded power output does not target a retained actor")?;
                    if retained_resets.get(&actor) != Some(&FrameResetSource::InputRestoration { resource, contact }) {
                        return Err("reset-excluded power output does not match its retained input-restoration contract".into());
                    }
                    if retained_power_inputs.iter().any(|input| input.node == node && input.port == port) {
                        return Err("retained actor has multiple compiled power roots for one input port".into());
                    }
                    retained_power_inputs.push(FramePowerInput {
                        node,
                        port,
                        output: index,
                        reset: FrameResetSource::InputRestoration { resource, contact },
                        data_strength: backend.input_strength_without_frame_reset(
                            node,
                            port,
                            FrameResetSource::InputRestoration { resource, contact },
                        ),
                    });
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
                    .any(|link| retained_unbound_reset_source(&backend.program, reset, link, &retained_observers))
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
                    .any(|link| retained_reset_input_path_is_owned(&backend.program, actor, reset, link, &retained_observers))
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
                            retained_reset_conductor_is_owned(
                                &backend.program,
                                actor,
                                reset,
                                &backend.program.power_links()[link as usize],
                                &retained_observers,
                            )
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
        retained_route_ids.extend(retained_output_callbacks.iter().map(|occurrence| occurrence.route));
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
            match &use_.reset {
                CellReset::InputRestoration { notification, sample, .. } => {
                    required_occurrences.extend([*notification, *sample]);
                    retained_route_ids.extend([notification.route, sample.route]);
                }
                CellReset::Observer { .. } => {}
                CellReset::Payload { .. } => return Err("logical frame has an unsupported payload reset contract".into()),
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
        for &observer in &retained_observers {
            if let Some(route) = backend.program.nodes()[observer.index()].output_route {
                retained_route_ids.insert(route);
            }
        }
        let mut retained_native_routes = retained_route_ids.iter().copied().collect::<Vec<_>>();
        retained_native_routes.sort_unstable();
        retained_native_routes.dedup();
        let mut occurrences_to_classify = required_occurrences.clone();
        for &route in &retained_native_routes {
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
            let input_restoration_boundary =
                retained_input_restoration_callback_is_owned(&backend.program, &circuit.retained_cells, occurrence, delivery.kind, receiver);
            if matches!(receiver, Some(CallbackReceiver::Component(node)) if matches!(backend.program.nodes()[node.index()].kind, CircuitNodeKind::Wire(_)))
                && !retained_output_callbacks.contains(&occurrence)
                && !matches!(receiver, Some(CallbackReceiver::Component(node)) if public_wire_outputs.contains(&node))
                && !input_restoration_boundary
            {
                return Err("retained output cell callback reaches an unowned wire outside its certified corridor".into());
            }
            let native_wire_effect = retained_output_callbacks.contains(&occurrence)
                && matches!(receiver, Some(CallbackReceiver::Component(node)) if {
                    matches!(backend.program.nodes()[node.index()].kind, CircuitNodeKind::Wire(_))
                        && scope.owned_nodes.binary_search(&node).is_err()
                });
            if owned {
                retained_routes.insert(occurrence);
            } else if input_restoration_boundary {
                // Keep the exact native notification/sample row from the cataloged
                // InputRestoration contract. Its source route is still dispatched by
                // Graph; this merely prevents an owned-source route filter from
                // dropping the certified row.
                retained_routes.insert(occurrence);
            } else if native_wire_effect {
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
                        let mut retained_actors = circuit
                            .retained_cells
                            .iter()
                            .filter_map(|use_| {
                                use_.retraction
                                    .transfer
                                    .filter(|transfer| transfer.resource == resource && transfer.source == source)
                                    .map(|_| use_.actor)
                            })
                            .collect::<Vec<_>>();
                        retained_actors.sort_unstable();
                        retained_actors.dedup();
                        feedback_resources.push(FeedbackResource {
                            resource,
                            source,
                            other,
                            output,
                            retained_actors: retained_actors.into_boxed_slice(),
                        });
                    }
                }
                LayerInputSource::State(test) | LayerInputSource::BoundaryState { test, .. } => {
                    let bound = clock.is_some_and(|clock| clock.state_value(test).is_some())
                        || native_timing.is_some_and(|timing| native_clock_state_is_bound(timing, test));
                    if !bound {
                        return Err(format!("logical frame has an unbound external state input: {test:?}"));
                    }
                    boundaries.push(FrameBoundary::ClockState { slot, test });
                }
                LayerInputSource::SourceAbove { site, port, threshold } => {
                    let site_spec = &backend.program.sites()[site.index()];
                    let clock_node = match site_spec.source {
                        SiteSource::Component(node)
                            if clock.is_some_and(|clock| clock.source_value(node).is_some())
                                || native_timing.is_some_and(|timing| timing.nodes.contains(&node)) =>
                        {
                            Some(node)
                        }
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
            retained_native_routes: retained_native_routes.into_boxed_slice(),
            retained_resources: retained_resources.into_boxed_slice(),
            retained_power_inputs: retained_power_inputs.into_boxed_slice(),
            terminal_repeater_inputs: terminal_repeater_inputs.into_boxed_slice(),
            power_input_offset: 0,
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

fn retained_input_restoration_callback_is_owned(
    program: &CircuitProgram,
    retained_cells: &[CellUse],
    occurrence: RouteOccurrence,
    kind: DeliveryKind,
    receiver: Option<CallbackReceiver>,
) -> bool {
    if !matches!(kind, DeliveryKind::Update { .. }) {
        return false;
    }
    retained_cells.iter().any(|use_| {
        let CellReset::InputRestoration { notification, sample, .. } = &use_.reset else {
            return false;
        };
        let receiver_is_actor = matches!(
            receiver,
            Some(CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor)) if actor == use_.actor
        ) || matches!(
            receiver,
            Some(CallbackReceiver::Component(node)) if program.pistons()[use_.actor.index()].node == node
        );
        if occurrence == *sample {
            return receiver_is_actor;
        }
        if occurrence != *notification {
            return false;
        }
        if notification == sample {
            return receiver_is_actor;
        }
        matches!(receiver, Some(CallbackReceiver::Component(node)) if matches!(program.nodes()[node.index()].kind, CircuitNodeKind::Wire(_)))
    })
}

fn retained_reset_source_link_is_owned(program: &CircuitProgram, reset: FrameResetSource, link: &PowerLink) -> bool {
    match reset {
        FrameResetSource::Observer(node) => {
            matches!(link.from_port, PowerPort::Output | PowerPort::Stored)
                && matches!(program.sites()[link.from.index()].source, SiteSource::Component(source) if source == node)
        }
        FrameResetSource::InputRestoration { resource, contact } => {
            program
                .resources()
                .get(resource.index())
                .is_some_and(|spec| spec.contacts.contains(&contact))
                && program
                    .sites()
                    .get(link.from.index())
                    .is_some_and(|site| site.source == SiteSource::Contact(contact))
        }
    }
}

fn retained_reset_conductor_is_owned(
    program: &CircuitProgram,
    actor: PistonId,
    reset: FrameResetSource,
    link: &PowerLink,
    retained_observers: &[NodeId],
) -> bool {
    let PowerTarget::Conductor { site, include_dust } = link.to else {
        return false;
    };
    if !retained_reset_source_link_is_owned(program, reset, link) {
        return false;
    }
    let SiteSource::Constant(profile) = program.sites()[site.index()].source else {
        return false;
    };
    let profile = program.profiles()[profile.index()];
    if !profile.solid || !profile.cube {
        return false;
    }
    if let FrameResetSource::Observer(reset_node) = reset && program.power_links().iter().any(|candidate| {
        matches!(candidate.to, PowerTarget::Conductor { site: candidate_site, include_dust: dust } if candidate_site == site && dust == include_dust)
            && matches!(program.sites()[candidate.from.index()].source, SiteSource::Component(node) if node != reset_node)
    }) {
        return false;
    }
    if matches!(reset, FrameResetSource::InputRestoration { .. })
        && program
            .power_links_to(PowerTarget::Conductor { site, include_dust })
            .iter()
            .any(|candidate| retained_unbound_reset_source(program, reset, candidate, retained_observers))
    {
        return false;
    }
    if !program.power_links().iter().any(|candidate| candidate.from == site) {
        return true;
    }
    let piston_node = program.pistons()[actor.index()].node;
    program.power_links().iter().any(|actor_link| {
        actor_link.from == site
            && matches!(actor_link.from_port, PowerPort::Conducted { include_dust: dust } if dust == include_dust)
            && matches!(actor_link.to, PowerTarget::Input { node, port: InputPort::Main | InputPort::Quasi } if node == piston_node)
    })
}

fn retained_reset_input_path_is_owned(
    program: &CircuitProgram,
    actor: PistonId,
    reset: FrameResetSource,
    input: &PowerLink,
    retained_observers: &[NodeId],
) -> bool {
    if retained_reset_source_link_is_owned(program, reset, input) {
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
        .any(|link| retained_reset_conductor_is_owned(program, actor, reset, link, retained_observers))
}

fn retained_unbound_reset_source(program: &CircuitProgram, reset: FrameResetSource, input: &PowerLink, retained_observers: &[NodeId]) -> bool {
    let is_other_reset = |node: NodeId| {
        let is_observer = matches!(
            program.nodes()[node.index()].kind,
            CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. } | CircuitNodeKind::Electrical(NodeType::Observer { .. })
        );
        is_observer
            && match reset {
                FrameResetSource::Observer(reset_node) => reset_node != node,
                FrameResetSource::InputRestoration { .. } => !retained_observers.contains(&node),
            }
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

    pub(crate) fn logical_devices_count(&self) -> usize {
        self.logical_devices.len()
    }

    pub(super) fn logical_node_owned(&self, node: NodeId) -> bool {
        self.logical_device_owner.get(node.index()).is_some_and(Option::is_some)
    }

    pub(super) fn install_logical_device(&mut self, monitor: &TaskMonitor) -> Result<LogicalDeviceAdmission, String> {
        let count = self.program.logical_devices().len();
        self.native_action_clock_owner.fill(None);
        if count == 0 {
            return Ok(if self.program.settled_circuit().is_some() {
                LogicalDeviceAdmission::Declined("no independently certified logical device".into())
            } else {
                LogicalDeviceAdmission::NotApplicable
            });
        }
        self.logical_device_owner = vec![None; self.program.nodes().len()];
        let mut declines = Vec::new();
        for program_index in 0..count {
            let nodes = self.program.logical_devices()[program_index].scope.owned_nodes.clone();
            let clock_driver = self.program.logical_devices()[program_index].scope.clock_driver;
            let witness = match clock_driver {
                SettledClockDriver::NativeAction { clock } => self.program.pistons().get(clock.index()).map(|actor| actor.node),
                _ => nodes.first().copied(),
            }
            .and_then(|node| self.program.nodes().get(node.index()).and_then(|node| node.position));
            let report_decline = |reason: &str| {
                report_activation_decline(monitor, program_index, witness, reason);
            };
            if nodes.iter().any(|&node| self.logical_node_owned(node)) {
                let reason = "overlapping logical-device scopes";
                report_decline(reason);
                declines.push(reason.to_owned());
                continue;
            }
            if let SettledClockDriver::NativeAction { clock } = clock_driver
                && (self.native_action_clock_owner.get(clock.index()).is_none_or(Option::is_some)
                    || self.logical_bud_owner_by_actor.get(clock.index()).is_some_and(Option::is_some))
            {
                let reason = format!("native-action clock {clock:?} has a conflicting or intercepted owner");
                report_decline(&reason);
                declines.push(reason);
                continue;
            }
            match self.prepare_logical_device(program_index) {
                Ok(device) => {
                    let owner = self.logical_devices.len();
                    for &node in &nodes {
                        self.logical_device_owner[node.index()] = Some(owner);
                    }
                    for callback in self.program.logical_devices()[program_index].scope.input_callbacks.iter() {
                        self.logical_input_callbacks.insert(*callback);
                        self.general_delivery_routes[callback.route.index()] = true;
                    }
                    if let Some(frame) = &device.frame {
                        for &route in &frame.retained_native_routes {
                            self.general_delivery_routes[route.index()] = true;
                        }
                    }
                    self.logical_devices.push(device);
                    if let SettledClockDriver::NativeAction { clock } = clock_driver {
                        self.native_action_clock_owner[clock.index()] = Some(owner);
                    }
                }
                Err(reason) => {
                    #[cfg(test)]
                    eprintln!("logical device {program_index} declined: {reason}");
                    report_decline(&reason);
                    declines.push(reason);
                }
            }
        }
        self.activate_frame_context();
        Ok(if self.logical_devices.is_empty() {
            self.logical_device_owner.clear();
            LogicalDeviceAdmission::Declined(declines.join("; "))
        } else {
            LogicalDeviceAdmission::Active
        })
    }

    fn prepare_logical_device(&self, program_index: usize) -> Result<LogicalDevice, String> {
        let plan = &self.program.logical_devices()[program_index];
        let circuit = &plan.circuit;
        let scope = &plan.scope;
        let owns = |node: NodeId| scope.owned_nodes.binary_search(&node).is_ok();
        let owns_actor = |actor: PistonId| owns(self.program.pistons()[actor.index()].node);
        if self.future.pending.keys().any(|tick| tick.node.is_some_and(owns))
            || self.future.piston_requests.iter().any(|request| owns_actor(request.piston))
            || self.future.deferred_pistons.iter().any(|event| owns_actor(event.piston))
            || self.future.scheduler.iter().any(|(tick, _, _)| tick.node.is_some_and(owns))
        {
            return Err("pending graph work in logical-device scope at entry".into());
        }
        let timing = circuit.timing.as_ref().ok_or("no certified logical-device timing")?;
        let clock_driver = match scope.clock_driver {
            SettledClockDriver::Logical => DeviceClockDriver::Logical(LogicalClock::new(&self.program, &self.state, timing)?),
            SettledClockDriver::NativeAction { clock } => self.prepare_native_action_clock(clock, timing)?,
        };
        if matches!(scope.clock_driver, SettledClockDriver::NativeAction { .. })
            || circuit
                .outputs
                .iter()
                .any(|output| matches!(output.target, SettledTarget::ResourceState { .. }))
        {
            let logical_clock = match &clock_driver {
                DeviceClockDriver::Logical(clock) => Some(clock),
                DeviceClockDriver::NativeAction { .. } => None,
            };
            let frame = SettledFrame::new(self, circuit, scope, logical_clock, Some(timing))?;
            if frame
                .feedback_resources
                .iter()
                .any(|feedback| scope.owned_resources.binary_search(&feedback.resource).is_err() && feedback.retained_actors.is_empty())
                || frame.retained_routes.iter().any(|occurrence| {
                    scope.owned_source_routes.binary_search(&occurrence.route).is_err()
                        && frame.retained_native_routes.binary_search(&occurrence.route).is_err()
                })
            {
                return Err("logical frame boundary escapes its certified scope".into());
            }
            return Ok(LogicalDevice {
                machine: None,
                frame: Some(frame),
                external: Box::new([]),
                external_nodes: Box::new([]),
                outputs: Box::new([]),
                registers: Box::new([]),
                clock_driver,
                trigger: timing.trigger,
            });
        }
        let DeviceClockDriver::Logical(clock) = &clock_driver else {
            return Err("native-action clock driver requires a computation frame".into());
        };
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
        Ok(LogicalDevice {
            external: vec![false; external_nodes.len()].into_boxed_slice(),
            external_nodes: external_nodes.into_boxed_slice(),
            machine: Some(machine),
            frame: None,
            clock_driver,
            trigger: timing.trigger,
            outputs: outputs.into_boxed_slice(),
            registers: registers.into_boxed_slice(),
        })
    }

    fn prepare_native_action_clock(&self, clock: PistonId, timing: &SettledTimingIsland) -> Result<DeviceClockDriver, String> {
        if clock != timing.clock {
            return Err("native-action frame clock does not match its certified timing island".into());
        }
        let clock_state = self
            .state
            .pistons
            .get(clock.index())
            .ok_or("native-action frame clock state is missing")?;
        if !clock_state.extended || !clock_state.head_present || clock_state.availability != PistonAvailability::Ready {
            return Err("native-action frame requires an extended stationary clock piston".into());
        }
        if self.logical_bud_owner_by_actor.get(clock.index()).is_some_and(Option::is_some) {
            return Err("native-action frame clock is intercepted by a logical piston owner".into());
        }
        let powered = self.node_state(timing.trigger).powered;
        Ok(DeviceClockDriver::NativeAction {
            clock,
            last_trigger: powered,
        })
    }

    fn activate_frame_context(&mut self) {
        let mut devices = std::mem::take(&mut self.logical_devices);
        self.rebuild_frame_context(&mut devices);
        self.logical_devices = devices;
    }

    fn rebuild_frame_context(&mut self, devices: &mut [LogicalDevice]) {
        self.active_frame_command_outputs.clear();
        self.active_frame_retained_actors.clear();
        self.active_frame_retained_nodes.clear();
        self.active_frame_retained_routes.clear();
        self.active_frame_power_inputs.clear();
        for device in devices {
            if let Some(frame) = &mut device.frame {
                frame.power_input_offset = self.active_frame_power_inputs.len();
                self.active_frame_retained_actors.extend_from_slice(&frame.retained_actors);
                if let DeviceClockDriver::NativeAction { clock, .. } = &device.clock_driver {
                    self.active_frame_retained_actors.push(*clock);
                    self.active_frame_retained_nodes.push(self.program.pistons()[clock.index()].node);
                }
                self.active_frame_retained_nodes.extend(
                    frame
                        .retained_actors
                        .iter()
                        .map(|actor| self.program.pistons()[actor.index()].node)
                        .chain(frame.retained_observers.iter().copied())
                        .chain(frame.retained_commands.iter().copied())
                        .chain(frame.button_nodes.iter().copied())
                        .chain(frame.terminal_repeater_inputs.iter().map(|input| input.node)),
                );
                self.active_frame_retained_routes.extend_from_slice(&frame.retained_routes);
                for &(node, _, _) in &frame.outputs {
                    if matches!(
                        self.program.nodes()[node.index()].kind,
                        CircuitNodeKind::Electrical(NodeType::CommandBlock { .. })
                    ) {
                        self.active_frame_retained_nodes.push(node);
                        self.active_frame_command_outputs.push(node);
                    }
                }
                self.refresh_frame_power_inputs(frame);
            }
        }
        self.active_frame_retained_nodes.sort_unstable();
        self.active_frame_retained_nodes.dedup();
        self.active_frame_retained_routes
            .sort_unstable_by_key(|occurrence| (occurrence.route, occurrence.ordinal));
        self.active_frame_retained_routes.dedup();
    }

    pub(super) fn tick_logical_device<'h>(&mut self, host: &mut Option<&'h mut (dyn GraphCommandHost + 'h)>) -> Result<(), GraphExecutionError> {
        let mut devices = std::mem::take(&mut self.logical_devices);
        self.rebuild_frame_context(&mut devices);
        let result = self.tick_phases_with_hook(host, |backend, _| {
            let step_result = (|| {
                for device in &mut devices {
                    let (logical_step, native_resample) = match &mut device.clock_driver {
                        DeviceClockDriver::Logical(clock) => (
                            Some(
                                clock
                                    .advance(backend.node_state(device.trigger).powered)
                                    .map_err(|kind| backend.unsupported(device.trigger, kind))?,
                            ),
                            false,
                        ),
                        DeviceClockDriver::NativeAction { last_trigger, .. } => {
                            let trigger = backend.node_state(device.trigger).powered;
                            let resample = trigger && trigger != *last_trigger;
                            *last_trigger = trigger;
                            (None, resample)
                        }
                    };
                    if let Some(step) = logical_step {
                        backend.run_logical_device_step(device, step)?;
                    }
                    if native_resample {
                        backend.run_frame_step(
                            device,
                            super::logical_clock::ClockStep {
                                resample: true,
                                ..Default::default()
                            },
                        )?;
                    }
                }
                Ok(())
            })();
            backend.logical_devices = std::mem::take(&mut devices);
            step_result
        });
        if !devices.is_empty() {
            self.logical_devices = devices;
        }
        result
    }

    fn run_logical_device_step(&mut self, device: &mut LogicalDevice, step: super::logical_clock::ClockStep) -> Result<(), GraphExecutionError> {
        if device.frame.is_some() {
            return self.run_frame_step(device, step);
        }
        let mut inputs_changed = false;
        let DeviceClockDriver::Logical(clock) = &device.clock_driver else {
            return Err(self.unsupported(device.trigger, "register machine has a native-action clock"));
        };
        if step.compute || clock.is_quiescent() {
            for (value, &node) in device.external.iter_mut().zip(device.external_nodes.iter()) {
                let sampled = clock.source_value(node).expect("validated logical device input");
                inputs_changed |= *value != sampled;
                *value = sampled;
            }
        }
        if step.compute {
            let machine = device.machine.as_mut().expect("register machine installed");
            let operations = machine.step(&device.external).expect("validated logical device inputs");
            self.counts.logical_kernel_evaluations += 1;
            self.counts.logical_kernel_operations += operations as u64;
        } else if clock.is_quiescent() && inputs_changed {
            let machine = device.machine.as_mut().expect("register machine installed");
            let operations = machine.observe(&device.external).expect("validated logical device inputs");
            self.counts.logical_kernel_evaluations += 1;
            self.counts.logical_kernel_operations += operations as u64;
        }
        if step.publish || (!step.compute && inputs_changed) {
            let machine = device.machine.as_ref().expect("register machine installed");
            for &(node, output) in &device.outputs {
                let strength = 15 * u8::from(machine.outputs()[output] != 0);
                self.set_node(node, strength != 0, strength, None, false)?;
            }
        }
        if step.commit {
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
    }

    pub(super) fn run_native_action_clock_step(&mut self, clock: PistonId, _action: mchprs_world::PistonAction) -> Result<(), GraphExecutionError> {
        let Some(owner) = self.native_action_clock_owner.get(clock.index()).copied().flatten() else {
            return Err(self.unsupported(self.program.pistons()[clock.index()].node, "unbound native-action clock"));
        };
        let mut devices = std::mem::take(&mut self.logical_devices);
        let result = if let Some(device) = devices.get_mut(owner) {
            if matches!(&device.clock_driver, DeviceClockDriver::NativeAction { clock: expected, .. } if *expected == clock) {
                let result = self.run_frame_step(
                    device,
                    super::logical_clock::ClockStep {
                        compute: true,
                        publish: true,
                        commit: true,
                        ..Default::default()
                    },
                );
                #[cfg(test)]
                if result.is_ok() && self.test_recording {
                    self.test_recorder.observed_operations.push(GraphObservedOperation::NativeFrameStep {
                        actor: self.program.nodes()[self.program.pistons()[clock.index()].node.index()].position.unwrap(),
                        action: _action,
                    });
                }
                result
            } else {
                Err(self.unsupported(self.program.pistons()[clock.index()].node, "native-action clock owner mismatch"))
            }
        } else {
            Err(self.unsupported(self.program.pistons()[clock.index()].node, "native-action clock owner is missing"))
        };
        self.logical_devices = devices;
        result
    }

    fn refresh_frame_power_inputs(&mut self, frame: &SettledFrame) {
        let inputs = frame
            .retained_power_inputs
            .iter()
            .map(|input| ActiveFramePowerInput {
                node: input.node,
                port: input.port,
                data_strength: input.data_strength,
                reset: Some(input.reset),
            })
            .chain(frame.terminal_repeater_inputs.iter().map(|input| ActiveFramePowerInput {
                node: input.node,
                port: InputPort::Main,
                data_strength: input.strength,
                reset: None,
            }));
        for (offset, input) in inputs.enumerate() {
            let slot = frame.power_input_offset + offset;
            if slot < self.active_frame_power_inputs.len() {
                self.active_frame_power_inputs[slot] = input;
            } else {
                debug_assert_eq!(slot, self.active_frame_power_inputs.len());
                self.active_frame_power_inputs.push(input);
            }
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
                    source,
                    clock_node: Some(node),
                    threshold,
                    ..
                } => {
                    let powered = match &device.clock_driver {
                        DeviceClockDriver::Logical(clock) => clock.source_value(node).is_some_and(|powered| powered && 15 > threshold),
                        DeviceClockDriver::NativeAction { .. } => {
                            self.sample_settled_input(source).map_err(|kind| self.unsupported(device.trigger, kind))?
                        }
                    };
                    (slot, powered)
                }
                FrameBoundary::Power { slot, source, .. } => (
                    slot,
                    self.sample_settled_input(source).map_err(|kind| self.unsupported(device.trigger, kind))?,
                ),
                FrameBoundary::ClockState { slot, test } => {
                    let value = match &device.clock_driver {
                        DeviceClockDriver::Logical(clock) => clock.state_value(test),
                        DeviceClockDriver::NativeAction { .. } => Some(self.state.matches(test)),
                    }
                    .ok_or_else(|| self.unsupported(device.trigger, "unbound logical clock state"))?;
                    (slot, value)
                }
            };
            frame.inputs[slot] = value;
        }
        if step.compute {
            for feedback in frame.feedback_resources.iter().filter(|feedback| !feedback.retained_actors.is_empty()) {
                if !retained_feedback_entry_is_ready(&self.state, feedback) {
                    return Err(self.unsupported(device.trigger, "retained feedback cell is outside its certified stationary entry"));
                }
            }
        }
        let operations = frame.evaluate().map_err(|kind| self.unsupported(device.trigger, kind))?;
        if step.compute {
            for input in frame.terminal_repeater_inputs.iter_mut() {
                input.strength = if frame.evaluation.outputs()[input.output] != 0 { 15 } else { 0 };
            }
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
            for input in frame.retained_power_inputs.iter_mut() {
                input.data_strength = if frame.evaluation.outputs()[input.output] != 0 { 15 } else { 0 };
            }
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
        } else if matches!(self.program.nodes()[node.index()].kind, CircuitNodeKind::Electrical(NodeType::Lamp)) {
            self.set_node(node, strength != 0, 0, None, false)?;
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
}

#[cfg(feature = "native-clock-test-support")]
#[doc(hidden)]
pub fn native_clock_test_support<W: crate::world::World>(world: &W, bounds: (mchprs_blocks::BlockPos, mchprs_blocks::BlockPos)) -> (u64, u64, u64) {
    use crate::redpiler::program_graph::{LayerExpr, ReducedLayerProgram, SettledCircuit};
    let options = crate::redpiler::CompilerOptions {
        optimization_level: Some(6),
        budget_multiplier: 8,
        ..Default::default()
    };
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let (mut backend, _) = crate::redpiler::Compiler::prepare_graph_backend(world, bounds, vec![], &options, &monitor, None).unwrap();
    let mut device = backend.logical_devices.pop().expect("counter logical device");
    let timing = backend.program.logical_devices()[0].circuit.timing.as_ref().unwrap();
    let trigger = timing.trigger;
    for state in &mut backend.state.components {
        match state {
            ComponentState::Electrical(state) => {
                state.powered = false;
                state.output_strength = 0;
            }
            ComponentState::Wire(wire) => wire.power = 0,
            ComponentState::Specialized => {}
        }
    }
    backend.state.classic_signals.fill(0);
    let clock = (0..backend.program.pistons().len())
        .map(|index| PistonId::new(index as u32))
        .find(|&actor| !backend.piston_powered(actor) && backend.piston_retraction_available(actor, true))
        .expect("counter has an unpowered native piston");
    let circuit = SettledCircuit {
        program: ReducedLayerProgram {
            inputs: Box::new([]),
            expressions: vec![LayerExpr::Constant(false)].into_boxed_slice(),
            roots: Box::new([]),
        },
        outputs: Box::new([]),
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
        retained_native_routes: Box::new([]),
        retained_resources: Box::new([]),
        retained_power_inputs: Box::new([]),
        terminal_repeater_inputs: Box::new([]),
        power_input_offset: 0,
    });
    device.machine = None;
    backend.logical_bud_owner_by_actor[clock.index()] = None;
    device.clock_driver = DeviceClockDriver::NativeAction {
        clock,
        last_trigger: backend.node_state(trigger).powered,
    };
    device.trigger = trigger;
    backend.native_action_clock_owner[clock.index()] = Some(0);
    backend.logical_devices.push(device);
    let request = super::PistonRequest {
        piston: clock,
        action: mchprs_world::PistonAction::RetractWithoutPull,
        source: None,
    };
    let before = backend.counts.logical_kernel_evaluations;
    backend.state.pistons[clock.index()].extended = false;
    assert!(!backend.native_action_accepted(request));
    backend.accept_action_using(request, None).unwrap();
    let rejected = backend.counts.logical_kernel_evaluations;
    backend.state.pistons[clock.index()].extended = true;
    backend.state.pistons[clock.index()].head_present = true;
    backend.state.pistons[clock.index()].availability = PistonAvailability::Ready;
    assert!(backend.native_action_accepted(request), "fixture clock must accept the retraction");
    backend.accept_action_using(request, None).unwrap();
    (before, rejected, backend.counts.logical_kernel_evaluations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::CompilerOptions;
    use crate::redpiler::program_graph::{LayerExpr, ReducedLayerProgram, SettledCircuit};
    use std::sync::Arc;

    #[test]
    fn activation_decline_keeps_reason_and_clickable_witness() {
        let monitor = TaskMonitor::default();
        report_activation_decline(&monitor, 2, Some(mchprs_blocks::BlockPos::new(-7, 64, 12)), "pending scheduled work");
        let warnings = monitor.warnings();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("pending scheduled work"));
        assert!(warnings[0].contains("may run on Graph"));
        let component = crate::player::diagnostic_component(&warnings[0]);
        assert!(
            component["extra"]
                .as_array()
                .unwrap()
                .iter()
                .any(|part| part["click_event"]["command"] == "/hl -7 64 12")
        );
    }

    fn constant_frame(circuit: &SettledCircuit) -> SettledFrame {
        SettledFrame {
            evaluation: SettledEvaluation::new(circuit).unwrap(),
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
            retained_native_routes: Box::new([]),
            retained_resources: Box::new([]),
            retained_power_inputs: Box::new([]),
            terminal_repeater_inputs: Box::new([]),
            power_input_offset: 0,
        }
    }

    fn constant_circuit() -> SettledCircuit {
        SettledCircuit {
            program: ReducedLayerProgram {
                inputs: Box::new([]),
                expressions: vec![LayerExpr::Constant(false)].into_boxed_slice(),
                roots: Box::new([]),
            },
            outputs: Box::new([]),
            retained_cells: Box::new([]),
            retained_effects: Box::new([]),
            declines: Box::new([]),
            timing: None,
        }
    }

    #[test]
    fn settled_frame_publishes_lamp_input() {
        let (mut world, bounds) = super::super::adder_tests::counter_world();
        let lamp_position = bounds.1 + mchprs_blocks::BlockPos::new(2, 0, 0);
        world.set_block(lamp_position, Block::RedstoneLamp { lit: false });
        let bounds = (bounds.0, lamp_position);
        let options = CompilerOptions {
            optimization_level: Some(6),
            budget_multiplier: 8,
            ..Default::default()
        };
        let mut backend =
            GraphBackend::compile_with_options(&world, bounds, world.scheduler().iter_entries().collect(), &options, Arc::default()).unwrap();
        let mut device = backend.logical_devices.pop().expect("counter logical device");
        let lamp = backend.positions[&lamp_position];
        let circuit = SettledCircuit {
            program: ReducedLayerProgram {
                inputs: Box::new([]),
                expressions: Box::new([LayerExpr::Constant(true)]),
                roots: Box::new([(LayerRoot::Output(0), LayerExprId(0))]),
            },
            outputs: Box::new([SettledOutput {
                target: SettledTarget::Power(PowerTarget::Input {
                    node: lamp,
                    port: InputPort::Main,
                }),
                kind: SettledValueKind::Bool,
                roots: Box::new([LayerExprId(0)]),
            }]),
            retained_cells: Box::new([]),
            retained_effects: Box::new([]),
            declines: Box::new([]),
            timing: None,
        };
        let mut frame = constant_frame(&circuit);
        frame.outputs = Box::new([(lamp, 0, SettledValueKind::Bool)]);
        device.frame = Some(frame);
        backend
            .run_frame_step(
                &mut device,
                super::super::logical_clock::ClockStep {
                    compute: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(backend.node_state(lamp).powered);
        assert_eq!(backend.node_state(lamp).output_strength, 0);
        assert_eq!(backend.counts.logical_kernel_evaluations, 1);
    }

    #[test]
    fn retained_feedback_entry_guard_requires_stationary_ready_cell_each_cycle() {
        let resource = ResourceId::new(0);
        let actor = PistonId::new(0);
        let source = ContactId::new(0);
        let other = ContactId::new(1);
        let feedback = FeedbackResource {
            resource,
            source,
            other,
            output: 0,
            retained_actors: Box::new([actor]),
        };
        let mut state = CircuitState {
            classic_signals: Box::new([]),
            classic_active: Box::new([]),
            components: Box::new([]),
            pistons: Box::new([PistonState {
                extended: true,
                head_present: true,
                availability: PistonAvailability::Ready,
                revision: 0,
            }]),
            resources: Box::new([ResourceState {
                at: source,
                enabled: true,
                revision: 0,
            }]),
        };

        assert!(retained_feedback_entry_is_ready(&state, &feedback));
        // The next compute must validate the live native result again; a stale
        // fixed-point value cannot stand in for a token moved during a cycle.
        state.resources[resource.index()].at = other;
        assert!(!retained_feedback_entry_is_ready(&state, &feedback));
        state.resources[resource.index()].at = source;
        state.pistons[actor.index()].availability = PistonAvailability::HeadPending;
        assert!(!retained_feedback_entry_is_ready(&state, &feedback));
    }

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
        let mut device = backend.logical_devices.pop().expect("COUNTER installs its logical device");
        let circuit = constant_circuit();
        device.frame = Some(constant_frame(&circuit));
        let DeviceClockDriver::Logical(clock) = &mut device.clock_driver else {
            panic!("COUNTER should use its source-derived logical clock");
        };
        let step = clock.advance(true).unwrap();
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

    #[test]
    fn native_clock_frame_runs_only_after_acceptance_and_before_retract_effects() {
        let (mut world, bounds) = super::super::adder_tests::counter_world();
        let ticks = world.scheduler().iter_entries().collect();
        let options = CompilerOptions {
            optimization_level: Some(6),
            budget_multiplier: 8,
            ..Default::default()
        };
        let mut backend = GraphBackend::compile_with_options(&world, bounds, ticks, &options, Arc::default()).unwrap();
        let mut device = backend.logical_devices.pop().expect("COUNTER installs a source-clocked logical device");
        let timing = backend.program.logical_devices()[0].circuit.timing.as_ref().unwrap();
        let clock = timing.clock;
        let trigger = timing.trigger;
        device.frame = Some(constant_frame(&constant_circuit()));
        device.machine = None;
        device.clock_driver = DeviceClockDriver::NativeAction {
            clock,
            last_trigger: backend.node_state(trigger).powered,
        };
        device.trigger = trigger;
        backend.native_action_clock_owner[clock.index()] = Some(0);
        backend.logical_devices.push(device);
        backend.set_test_recording(true);

        let request = PistonRequest {
            piston: clock,
            action: mchprs_world::PistonAction::Retract,
            source: None,
        };
        let clock_position = backend.program.pistons()[clock.index()].position;
        backend.accept_action_using(request, Some(false)).unwrap();
        assert!(
            !backend
                .observed_operations
                .iter()
                .any(|operation| matches!(operation, GraphObservedOperation::NativeFrameStep { actor, .. } if *actor == clock_position)),
            "a rejected clock action must not run the native-clock frame"
        );

        let start = backend.observed_operations.len();
        backend.accept_action_using(request, Some(true)).unwrap();
        let operations = &backend.observed_operations[start..];
        let frame = operations
            .iter()
            .position(|operation| matches!(operation, GraphObservedOperation::NativeFrameStep { actor, action } if *actor == clock_position && *action == request.action))
            .expect("accepted native clock retract runs its frame");
        let dispatch = operations
            .iter()
            .position(|operation| matches!(operation, GraphObservedOperation::RetractionDispatch { actor, .. } if *actor == clock_position))
            .expect("accepted retract reaches its shared dispatch boundary");
        let accepted = operations
            .iter()
            .position(|operation| matches!(operation, GraphObservedOperation::Accepted { actor, action } if *actor == clock_position && *action == request.action))
            .expect("accepted retract publishes its accepted action");
        assert!(
            frame < dispatch && dispatch < accepted,
            "frame compute must precede the shared retract dispatch in the action FIFO"
        );

        let no_pull_request = PistonRequest {
            action: mchprs_world::PistonAction::RetractWithoutPull,
            ..request
        };
        let start = backend.observed_operations.len();
        backend.accept_action_using(no_pull_request, Some(true)).unwrap();
        let operations = &backend.observed_operations[start..];
        let frame = operations
            .iter()
            .position(|operation| matches!(operation, GraphObservedOperation::NativeFrameStep { actor, action } if *actor == clock_position && *action == no_pull_request.action))
            .expect("accepted RetractWithoutPull runs the native-clock frame");
        let dispatch = operations
            .iter()
            .position(
                |operation| matches!(operation, GraphObservedOperation::RetractionDispatch { actor, without_pull: true } if *actor == clock_position),
            )
            .expect("RetractWithoutPull reaches its no-pull dispatch");
        let accepted = operations
            .iter()
            .position(|operation| matches!(operation, GraphObservedOperation::Accepted { actor, action } if *actor == clock_position && *action == no_pull_request.action))
            .expect("accepted no-pull retract is published");
        assert!(frame < dispatch && dispatch < accepted);
        assert!(
            backend.future.deferred_pistons.back().unwrap().resource.is_none(),
            "RetractWithoutPull must not enqueue a resource pull"
        );

        assert_eq!(backend.native_action_clock_owner[clock.index()], Some(0));
        backend.reset(&mut world);
        assert!(backend.native_action_clock_owner.iter().all(Option::is_none));
    }

    #[test]
    fn native_action_clock_requires_stationary_pose_at_binding() {
        let (world, bounds) = super::super::adder_tests::counter_world();
        let ticks = world.scheduler().iter_entries().collect();
        let options = CompilerOptions {
            optimization_level: Some(6),
            budget_multiplier: 8,
            ..Default::default()
        };
        let mut backend = GraphBackend::compile_with_options(&world, bounds, ticks, &options, Arc::default()).unwrap();
        let timing = backend.program.logical_devices()[0].circuit.timing.as_ref().unwrap().clone();
        let clock = timing.clock;
        assert!(backend.prepare_native_action_clock(clock, &timing).is_ok());

        backend.state.pistons[clock.index()].availability = crate::redpiler::program_graph::PistonAvailability::BasePending;
        assert!(matches!(
            backend.prepare_native_action_clock(clock, &timing),
            Err(reason) if reason.contains("extended stationary clock piston")
        ));
    }
}
