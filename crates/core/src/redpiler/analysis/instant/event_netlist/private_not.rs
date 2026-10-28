//! Narrow experimental idealized NOT frame. This plan is not native-timing-equivalent.
use crate::redpiler::TaskMonitor;
use crate::redpiler::analysis::instant::boundaries::OwnerMap;
use crate::redpiler::analysis::instant::layers::{compile_native_power_cuts, trace_native_power_targets};
use crate::redpiler::components::NodeType;
use crate::redpiler::program_graph::*;

/// The caller must keep the actor exclusively owned until the pulse is restored.
pub(crate) struct PrivateNot {
    pub(crate) actor: PistonId,
    pub(crate) resource: ResourceId,
    pub(crate) far: ContactId,
    pub(crate) near: ContactId,
    pub(crate) observer: NodeId,
    pub(crate) route: RouteId,
    pub(crate) deliveries: Box<[(u32, RouteDelivery)]>,
    pub(crate) activations: Box<[RouteOccurrence]>,
    pub(crate) ingress_routes: Box<[RouteId]>,
    pub(crate) power: ReducedLayerProgram,
}

/// Admit only a single isolated observer-reset source and one native lamp.
/// Unsupported mutable consumers, captured output, and observable skipped
/// callbacks remain on Graph. This does not certify arbitrary timing or handoff.
pub(crate) fn lower_private_not(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    owners: &OwnerMap,
    output: NodeId,
    monitor: &TaskMonitor,
) -> Result<PrivateNot, String> {
    if monitor.cancelled() {
        return Err("cancelled while admitting private NOT".into());
    }
    if graph.pistons.len() != 1 || graph.resources.len() != 1 || catalog.cells.len() != 1 || !catalog.corridors.is_empty() {
        return Err("private NOT requires one isolated actor/resource and no wire corridor".into());
    }
    let cell = &catalog.cells[0];
    if cell.uses.len() != 1 || cell.resources.len() != 1 {
        return Err("private NOT requires one cell use and one payload".into());
    }
    let use_ = &cell.uses[0];
    let pin = &cell.resources[0];
    if pin.owners.as_ref() != [use_.actor]
        || owners.actors.get(use_.actor.index()).copied().flatten() != Some(0)
        || owners.resources.get(pin.resource.index()).copied().flatten() != Some(0)
    {
        return Err("private NOT payload ownership is incomplete".into());
    }
    let observer = use_.reset.observer().ok_or("private NOT requires an observer reset")?;
    let spec = *graph.pistons.get(use_.actor.index()).ok_or("private NOT actor is missing")?;
    if !spec.sticky || !matches!(spec.lane, PistonLane::SingleResource { far } if far == pin.source) {
        return Err("private NOT requires its exclusive sticky Far resource".into());
    }
    let resource = graph.resources.get(pin.resource.index()).ok_or("private NOT resource is missing")?;
    if !matches!(
        graph.profiles.get(resource.material.index()).map(|p| p.electrical),
        Some(ProfileElectrical::RedstoneSource)
    ) {
        return Err("private NOT requires a redstone payload".into());
    }
    for (index, node) in graph.nodes.iter().enumerate() {
        let id = NodeId::new(index as u32);
        let supported = match node.kind {
            CircuitNodeKind::Piston(actor) => actor == use_.actor,
            CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. } => id == observer,
            CircuitNodeKind::Contact(_) => true,
            CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Constant) => true,
            CircuitNodeKind::Electrical(NodeType::Lamp) => id == output,
            _ => false,
        };
        if !supported {
            return Err(format!("private NOT has an unsupported node {id:?}"));
        }
    }
    if !matches!(
        graph.nodes.get(output.index()).map(|node| &node.kind),
        Some(CircuitNodeKind::Electrical(NodeType::Lamp))
    ) {
        return Err("private NOT output is not a native lamp".into());
    }
    let route = spec.routes.far_changed.ok_or("private NOT lacks its Far route")?;
    for other in [spec.routes.base_changed, spec.routes.near_changed] {
        if graph.routes[other.index()]
            .deliveries
            .iter()
            .any(|delivery| delivery.receiver == Some(CallbackReceiver::Component(output)))
        {
            return Err("private NOT skipped actuator callback reaches the lamp".into());
        }
    }
    if graph
        .routes
        .iter()
        .filter(|r| r.source == observer)
        .any(|r| r.deliveries.iter().any(|d| d.receiver == Some(CallbackReceiver::Component(output))))
    {
        return Err("private NOT skipped observer callback reaches the lamp".into());
    }
    let trace = trace_native_power_targets(
        graph,
        &[PowerTarget::Input {
            node: output,
            port: InputPort::Main,
        }],
        monitor,
    )?;
    for source in trace.inputs {
        match source {
            LayerInputSource::SourceAbove { site, .. } if matches!(graph.sites[site.index()].source, SiteSource::Component(node) if node == observer || node == spec.node) =>
            {
                return Err("private NOT lamp reads skipped actor or observer state".into());
            }
            LayerInputSource::State(StateTest::PistonExtended(_) | StateTest::PistonHeadPresent(_) | StateTest::PistonAvailability { .. }) => {
                return Err("private NOT lamp reads skipped actor pose".into());
            }
            LayerInputSource::BoundaryState {
                test: StateTest::PistonExtended(_) | StateTest::PistonHeadPresent(_) | StateTest::PistonAvailability { .. },
                ..
            } => return Err("private NOT lamp reads skipped actor pose".into()),
            _ => {}
        }
    }
    let mut deliveries = Vec::new();
    for (ordinal, delivery) in graph.routes[route.index()].deliveries.iter().copied().enumerate() {
        if delivery.receiver != Some(CallbackReceiver::Component(output)) {
            continue;
        }
        if delivery.view != OperandView::Live {
            return Err("private NOT captured output stays native".into());
        }
        if !output_guard_supported(graph, delivery.guard, pin.resource, spec.node, observer)? {
            return Err("private NOT output guard reads skipped actor or contact state".into());
        }
        deliveries.push((u32::try_from(ordinal).map_err(|_| "private NOT output ordinal overflow")?, delivery));
    }
    if deliveries.is_empty() {
        return Err("private NOT lacks a native lamp callback".into());
    }
    if use_.activations.is_empty() {
        return Err("private NOT lacks source-qualified trigger events".into());
    }
    let mut ingress_routes = use_.activations.iter().map(|event| event.route).collect::<Vec<_>>();
    ingress_routes.sort_unstable();
    ingress_routes.dedup();
    ingress_routes.retain(|route| source_route_equivalent(graph, *route, use_.actor, spec.node));
    if ingress_routes.is_empty() {
        return Err("private NOT has no unambiguous live trigger route".into());
    }
    let power = compile_native_power_cuts(
        graph,
        &[
            (
                PowerTarget::Input {
                    node: spec.node,
                    port: InputPort::Main,
                },
                0,
            ),
            (
                PowerTarget::Input {
                    node: spec.node,
                    port: InputPort::Quasi,
                },
                0,
            ),
        ],
        monitor,
    )?;
    Ok(PrivateNot {
        actor: use_.actor,
        resource: pin.resource,
        far: pin.source,
        near: spec.near,
        observer,
        route,
        deliveries: deliveries.into_boxed_slice(),
        activations: use_.activations.clone(),
        ingress_routes: ingress_routes.into_boxed_slice(),
        power,
    })
}

/// Graph records the source route, not the occurrence, on an already queued
/// request. Every possible actor delivery on an admitted route must therefore
/// have the same live piston-sampling semantics.
fn source_route_equivalent(graph: &CompileGraph, route: RouteId, actor: PistonId, actor_node: NodeId) -> bool {
    let Some(route) = graph.routes.get(route.index()) else { return false };
    let mut found = false;
    for delivery in &route.deliveries {
        let targets_actor = match delivery.receiver {
            Some(CallbackReceiver::PistonBase(id) | CallbackReceiver::PistonHead(id)) => id == actor,
            Some(CallbackReceiver::Component(node)) => node == actor_node,
            None => graph
                .sites
                .get(delivery.target.index())
                .is_some_and(|site| site.source == SiteSource::Component(actor_node)),
            Some(CallbackReceiver::Contact(_)) => false,
        };
        if targets_actor {
            if delivery.view != OperandView::Live || !matches!(delivery.kind, DeliveryKind::Update { .. }) {
                return false;
            }
            found = true;
        }
    }
    found
}

fn output_guard_supported(graph: &CompileGraph, root: GuardId, resource: ResourceId, actor: NodeId, observer: NodeId) -> Result<bool, String> {
    let mut pending = vec![root];
    let mut seen = rustc_hash::FxHashSet::default();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        if seen.len() > 4096 {
            return Err("private NOT output guard budget exceeded".into());
        }
        match graph.guards.get(id.index()).ok_or("private NOT output guard is missing")? {
            Guard::Always => {}
            Guard::State(StateTest::ResourceAt { resource: id, .. } | StateTest::ResourceEnabled(id)) if *id == resource => {}
            Guard::State(_) => return Ok(false),
            Guard::Site {
                site,
                view: OperandView::Live,
                ..
            } => match graph.sites.get(site.index()).ok_or("private NOT guard site is missing")?.source {
                SiteSource::Constant(_) => {}
                SiteSource::Component(node) if node != actor && node != observer => {}
                _ => return Ok(false),
            },
            Guard::Site { .. } => return Ok(false),
            Guard::Not(child) => pending.push(*child),
            Guard::All(children) | Guard::Any(children) => pending.extend(children.iter().copied()),
        }
    }
    Ok(true)
}
