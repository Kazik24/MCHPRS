//! Typed bindings for ordinary single-owner movable-payload BUD pistons.
//!
//! These bindings retain graph IDs and original callback rows. They do not assign a
//! Boolean polarity to a payload or replace the Graph owner by themselves.
use super::*;
use rustc_hash::{FxHashMap, FxHashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LogicalBudOwnerPlan {
    pub(crate) actor: PistonId,
    pub(crate) resource: ResourceId,
    pub(crate) near: ContactId,
    pub(crate) far: ContactId,
    /// The cold-load state is a validation witness, never an inferred constant.
    pub(crate) initial_piston: PistonState,
    pub(crate) initial_resource: ResourceState,
    /// Exact live input dependencies, kept separate because piston acceptance samples both ports.
    pub(crate) main_links: Box<[PowerLink]>,
    pub(crate) quasi_links: Box<[PowerLink]>,
    /// Every original base/head callback occurrence that can sample the actor.
    pub(crate) sample_routes: Box<[RouteOccurrence]>,
    pub(crate) transition: LogicalBudTransitionPlan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LogicalBudTransitionPlan {
    pub(crate) publication_delay_ticks: u8,
    /// Ordered original route occurrences, including repeated route emissions.
    pub(crate) events: Box<[LogicalBudRouteEvent]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LogicalBudRouteEvent {
    pub(crate) sequence: u32,
    pub(crate) cut: LogicalBudEventCut,
    pub(crate) condition: LogicalBudEventCondition,
    pub(crate) occurrence: RouteOccurrence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogicalBudEventCut {
    ExtendFarShape,
    ExtendNearAfterTransfer,
    ExtendBase,
    RetractNearBeforeBase,
    RetractBaseBeforeTransfer,
    RetractBaseAfterNear,
    RetractResourceRestoredFar,
    RetractFarPulled,
    RetractNearAfterTransfer,
    DeferredResourceFar,
    DeferredHead,
    DeferredBase,
    DeferredResourceNear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogicalBudEventCondition {
    Always,
    ResourceMoved,
    InterruptedHead,
    NotInterruptedHead,
    RestoreMatchingExtension,
    PulledResource,
    ActorRevisionCurrent,
    ResourceRevisionCurrent,
}

/// Discover complete, exclusive, two-contact redstone payload owners without depending on
/// the narrower instant-cell or `BudMemoryRead` recognizers.
pub(crate) fn compile_logical_bud_owners(graph: &CompileGraph, monitor: &crate::redpiler::TaskMonitor) -> Result<Vec<LogicalBudOwnerPlan>, String> {
    const PLAN_BYTE_BUDGET: usize = crate::redpiler::analysis::MAX_GRAPH_PREPARATION_BYTES / 8;
    let deliveries = graph
        .routes
        .iter()
        .try_fold(0usize, |sum, route| sum.checked_add(route.deliveries.len()))
        .ok_or("logical BUD route index size overflow")?;
    let index_estimate = graph
        .resources
        .len()
        .saturating_mul(64)
        .saturating_add(
            graph
                .resources
                .iter()
                .map(|resource| resource.contacts.len())
                .sum::<usize>()
                .saturating_mul(std::mem::size_of::<ResourceId>()),
        )
        .saturating_add(graph.contacts.len().saturating_mul(std::mem::size_of::<Vec<PistonId>>()))
        .saturating_add(
            graph
                .pistons
                .len()
                .saturating_mul(std::mem::size_of::<Vec<RouteOccurrence>>() + 3 * std::mem::size_of::<PistonId>()),
        )
        .saturating_add(graph.power_links.len().saturating_mul(std::mem::size_of::<PowerLink>() + 32))
        .saturating_add(deliveries.saturating_mul(std::mem::size_of::<RouteOccurrence>()));
    if index_estimate > PLAN_BYTE_BUDGET {
        return Err("logical BUD owner indices exceed the graph preparation budget".into());
    }
    let mut resources_by_pair: FxHashMap<(ContactId, ContactId), Vec<ResourceId>> = FxHashMap::default();
    let mut resources_by_contact = vec![Vec::new(); graph.contacts.len()];
    for (index, resource) in graph.resources.iter().enumerate() {
        if index & 0x0fff == 0 && monitor.cancelled() {
            return Err("cancelled while indexing logical BUD resources".into());
        }
        if resource.contacts.len() == 2 {
            let first = resource.contacts[0];
            let second = resource.contacts[1];
            let pair = if first < second { (first, second) } else { (second, first) };
            resources_by_pair.entry(pair).or_default().push(ResourceId::new(index as u32));
        }
        let resource_id = ResourceId::new(index as u32);
        for &contact in &resource.contacts {
            resources_by_contact[contact.index()].push(resource_id);
        }
    }
    let mut actors_by_contact = vec![Vec::new(); graph.contacts.len()];
    for (index, piston) in graph.pistons.iter().enumerate() {
        if index & 0x0fff == 0 && monitor.cancelled() {
            return Err("cancelled while indexing logical BUD contact owners".into());
        }
        let actor = PistonId::new(index as u32);
        for contact in [piston.base, piston.near, piston.far] {
            actors_by_contact[contact.index()].push(actor);
        }
    }
    let mut input_links: FxHashMap<PowerTarget, Vec<PowerLink>> = FxHashMap::default();
    for (index, &link) in graph.power_links.iter().enumerate() {
        if index & 0x0fff == 0 && monitor.cancelled() {
            return Err("cancelled while indexing logical BUD power inputs".into());
        }
        if matches!(link.to, PowerTarget::Input { .. }) {
            input_links.entry(link.to).or_default().push(link);
        }
    }
    let mut sample_routes = vec![Vec::new(); graph.pistons.len()];
    for (route_index, route) in graph.routes.iter().enumerate() {
        if route_index & 0x0fff == 0 && monitor.cancelled() {
            return Err("cancelled while indexing logical BUD sample routes".into());
        }
        for (ordinal, delivery) in route.deliveries.iter().enumerate() {
            if ordinal & 0x0fff == 0 && monitor.cancelled() {
                return Err("cancelled while indexing logical BUD callback deliveries".into());
            }
            let receiver_actor = match delivery.receiver {
                Some(CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor)) => Some(actor),
                Some(CallbackReceiver::Component(node)) => match graph.nodes[node.index()].kind {
                    CircuitNodeKind::Piston(actor) => Some(actor),
                    _ => None,
                },
                None if matches!(delivery.kind, DeliveryKind::Update { .. }) => {
                    let SiteSource::Component(node) = graph.sites[delivery.target.index()].source else {
                        continue;
                    };
                    match graph.nodes[node.index()].kind {
                        CircuitNodeKind::Piston(actor) => Some(actor),
                        _ => None,
                    }
                }
                _ => None,
            };
            if let Some(actor) = receiver_actor {
                sample_routes[actor.index()].push(RouteOccurrence {
                    route: RouteId::new(route_index as u32),
                    ordinal: ordinal as u32,
                });
            }
        }
    }
    let mut instant_actors = FxHashSet::default();
    if let Some(catalog) = &graph.plans.instant.boundaries {
        instant_actors.extend(catalog.cells.iter().flat_map(|cell| cell.uses.iter().map(|use_| use_.actor)));
    }
    instant_actors.extend(graph.plans.instant_events.iter().flat_map(|kernel| match &kernel.program {
        KernelProgram::Events(entries) => entries.iter().map(|entry| entry.actor).collect::<Vec<_>>(),
        KernelProgram::PullWave { pulls, .. } => pulls.iter().map(|pull| pull.actor).collect::<Vec<_>>(),
    }));

    let mut plans = Vec::new();
    for (actor_index, spec) in graph.pistons.iter().enumerate() {
        if monitor.cancelled() {
            return Err("cancelled during logical BUD owner discovery".into());
        }
        let actor = PistonId::new(actor_index as u32);
        if !spec.sticky || instant_actors.contains(&actor) {
            continue;
        }
        let PistonLane::SingleResource { far } = spec.lane else {
            continue;
        };
        let pair = if spec.near < far { (spec.near, far) } else { (far, spec.near) };
        let Some([resource_id]) = resources_by_pair.get(&pair).map(Vec::as_slice) else {
            continue;
        };
        let resource_id = *resource_id;
        let resource = &graph.resources[resource_id.index()];
        if Block::from_id(graph.profiles[resource.material.index()].raw_state) != Block::RedstoneBlock {
            continue;
        }
        if !exclusive_contacts(graph, actor, resource_id, spec.near, far, &actors_by_contact, &resources_by_contact) {
            continue;
        }

        let initial_piston = graph.initial_pistons[actor.index()];
        let initial_resource = graph.initial_resources[resource_id.index()];
        // The first runtime primitive starts only from a settled, internally consistent snapshot.
        // Dynamic updates still use ResourceState; these values are never constant-folded.
        if initial_piston.availability != PistonAvailability::Ready
            || initial_piston.head_present != initial_piston.extended
            || !initial_resource.enabled
            // Native extend supports an empty retracted base when the sticky payload already
            // occupies its far contact. Either endpoint is valid; pose and payload side are
            // separate state and must not be inferred from one another.
            || (initial_resource.at != spec.near && initial_resource.at != far)
        {
            continue;
        }

        let target = |port| PowerTarget::Input { node: spec.node, port };
        let main_links = input_links.get(&target(InputPort::Main)).cloned().unwrap_or_default();
        let quasi_links = input_links.get(&target(InputPort::Quasi)).cloned().unwrap_or_default();
        // An unlinked input is an exact constant zero at runtime, not an unsupported port.
        // A normally QC-only BUD is valid even when Main has no links.
        if sample_routes[actor.index()].is_empty() {
            continue;
        }
        let transition = transition_plan(graph, spec.routes);
        plans.push(LogicalBudOwnerPlan {
            actor,
            resource: resource_id,
            near: spec.near,
            far,
            initial_piston,
            initial_resource,
            main_links: main_links.into_boxed_slice(),
            quasi_links: quasi_links.into_boxed_slice(),
            sample_routes: sample_routes[actor.index()].clone().into_boxed_slice(),
            transition,
        });
        if estimated_bytes(&plans) > PLAN_BYTE_BUDGET {
            return Err("logical BUD owner plans exceed the graph preparation budget".into());
        }
    }
    Ok(plans)
}

pub(crate) fn estimated_bytes(plans: &[LogicalBudOwnerPlan]) -> usize {
    plans.iter().fold(std::mem::size_of_val(plans), |total, plan| {
        let dynamic = plan
            .main_links
            .len()
            .saturating_mul(std::mem::size_of::<PowerLink>())
            .saturating_add(plan.quasi_links.len().saturating_mul(std::mem::size_of::<PowerLink>()))
            .saturating_add(plan.sample_routes.len().saturating_mul(std::mem::size_of::<RouteOccurrence>()))
            .saturating_add(plan.transition.events.len().saturating_mul(std::mem::size_of::<LogicalBudRouteEvent>()));
        total.saturating_add(dynamic)
    })
}

fn exclusive_contacts(
    graph: &CompileGraph,
    actor: PistonId,
    resource: ResourceId,
    near: ContactId,
    far: ContactId,
    actors_by_contact: &[Vec<PistonId>],
    resources_by_contact: &[Vec<ResourceId>],
) -> bool {
    let Some(spec) = graph.resources.get(resource.index()) else {
        return false;
    };
    if spec.contacts.len() != 2 || !spec.contacts.contains(&near) || !spec.contacts.contains(&far) || near == far {
        return false;
    }
    [near, far]
        .into_iter()
        .all(|contact| actors_by_contact[contact.index()].iter().all(|&owner| owner == actor))
        && [near, far]
            .into_iter()
            .all(|contact| resources_by_contact[contact.index()].iter().all(|&other| other == resource))
}

fn transition_plan(graph: &CompileGraph, routes: PistonRoutes) -> LogicalBudTransitionPlan {
    let mut events = Vec::new();
    let mut sequence = 0u32;
    let mut add_route = |route: Option<RouteId>, cut, condition| {
        let Some(route) = route else { return };
        if let Some(spec) = graph.routes.get(route.index()) {
            for ordinal in 0..spec.deliveries.len() {
                let occurrence = RouteOccurrence {
                    route,
                    ordinal: ordinal as u32,
                };
                events.push(LogicalBudRouteEvent {
                    sequence,
                    cut,
                    condition,
                    occurrence,
                });
                sequence = sequence.saturating_add(1);
            }
        }
    };

    add_route(
        routes.far_shape_changed,
        LogicalBudEventCut::ExtendFarShape,
        LogicalBudEventCondition::ResourceMoved,
    );
    add_route(
        Some(routes.near_changed),
        LogicalBudEventCut::ExtendNearAfterTransfer,
        LogicalBudEventCondition::Always,
    );
    add_route(
        Some(routes.near_changed),
        LogicalBudEventCut::ExtendNearAfterTransfer,
        LogicalBudEventCondition::ResourceMoved,
    );
    add_route(
        Some(routes.base_changed),
        LogicalBudEventCut::ExtendBase,
        LogicalBudEventCondition::Always,
    );
    add_route(
        Some(routes.near_changed),
        LogicalBudEventCut::RetractNearBeforeBase,
        LogicalBudEventCondition::InterruptedHead,
    );
    add_route(
        Some(routes.base_changed),
        LogicalBudEventCut::RetractBaseBeforeTransfer,
        LogicalBudEventCondition::NotInterruptedHead,
    );
    add_route(
        Some(routes.base_changed),
        LogicalBudEventCut::RetractBaseAfterNear,
        LogicalBudEventCondition::InterruptedHead,
    );
    add_route(
        routes.resource_restored_far,
        LogicalBudEventCut::RetractResourceRestoredFar,
        LogicalBudEventCondition::RestoreMatchingExtension,
    );
    add_route(
        routes.far_changed,
        LogicalBudEventCut::RetractFarPulled,
        LogicalBudEventCondition::PulledResource,
    );
    add_route(
        Some(routes.near_changed),
        LogicalBudEventCut::RetractNearAfterTransfer,
        LogicalBudEventCondition::Always,
    );
    add_route(
        routes.resource_restored_far,
        LogicalBudEventCut::DeferredResourceFar,
        LogicalBudEventCondition::ResourceRevisionCurrent,
    );
    add_route(
        Some(routes.head_restored),
        LogicalBudEventCut::DeferredHead,
        LogicalBudEventCondition::ActorRevisionCurrent,
    );
    add_route(
        Some(routes.base_restored),
        LogicalBudEventCut::DeferredBase,
        LogicalBudEventCondition::ActorRevisionCurrent,
    );
    add_route(
        routes.resource_restored_near,
        LogicalBudEventCut::DeferredResourceNear,
        LogicalBudEventCondition::ResourceRevisionCurrent,
    );

    LogicalBudTransitionPlan {
        publication_delay_ticks: 2,
        events: events.into_boxed_slice(),
    }
}
