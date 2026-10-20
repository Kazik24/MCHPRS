//! Lower proven waves to small executables; declined lifecycle families keep ordinary Graph.
use super::*;
use rustc_hash::FxHashSet;
use std::sync::Arc;

#[cfg(test)]
pub(crate) fn lower(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<Vec<Arc<InstantKernel>>, String> {
    let (Some(catalog), Some(plan)) = (&graph.plans.instant.boundaries, &graph.plans.instant.layers) else {
        return lower_with_context(graph, monitor, None);
    };
    if plan.regions.is_empty() {
        return lower_with_context(graph, monitor, None);
    }
    let context = LayerCompilationContext::prepare(graph, catalog, monitor)?;
    lower_with_context(graph, monitor, Some(&context))
}

pub(crate) fn lower_with_context(
    graph: &CompileGraph,
    monitor: &TaskMonitor,
    context: Option<&LayerCompilationContext>,
) -> Result<Vec<Arc<InstantKernel>>, String> {
    let mut kernels = lower_pull_waves(graph, monitor, context)?;
    if !graph.plans.instant_events.is_empty() {
        let actors: FxHashSet<_> = graph
            .plans
            .instant_events
            .iter()
            .flat_map(|kernel| kernel.actors.iter().copied())
            .collect();
        kernels.retain(|kernel| !kernel.actors.iter().any(|actor| actors.contains(actor)));
        kernels.extend(graph.plans.instant_events.iter().cloned().map(Arc::new));
    }
    Ok(kernels)
}

fn lower_pull_waves(
    graph: &CompileGraph,
    monitor: &TaskMonitor,
    context: Option<&LayerCompilationContext>,
) -> Result<Vec<Arc<InstantKernel>>, String> {
    let (Some(catalog), Some(plan)) = (&graph.plans.instant.boundaries, &graph.plans.instant.layers) else {
        return Ok(Vec::new());
    };
    if plan.regions.is_empty() {
        return Ok(Vec::new());
    }
    let context = context.ok_or("kernel lowering requires validated layer analysis")?;
    let partition = &context.partition;
    let power = &context.power;
    let mut kernels = Vec::new();
    for (region, program) in plan.regions.iter().zip(&plan.programs) {
        check_cancelled(monitor)?;
        let component = partition.regions.get(region.id.0 as usize).ok_or("kernel region is missing")?;
        let retained = context.arrivals.get(region.id.0 as usize).and_then(|proof| proof.as_deref());
        let reconstructed = if retained.is_none() {
            match composition::corridors::certify(graph, catalog, component, power, monitor) {
                Ok(arrivals) => arrivals,
                Err(ConstructionError::BudgetExceeded) => continue,
                Err(ConstructionError::Invalid(message)) => return Err(message),
            }
        } else {
            None
        };
        let Some(arrivals) = retained.or(reconstructed.as_deref()) else {
            continue;
        };
        if let Some(kernel) = lower_region(graph, catalog, region, program, arrivals, monitor)? {
            kernels.push(Arc::new(kernel));
        }
    }
    Ok(kernels)
}

fn lower_region(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    region: &LayeredRegion,
    program: &ReducedLayerProgram,
    arrivals: &[composition::corridors::Arrival],
    monitor: &TaskMonitor,
) -> Result<Option<InstantKernel>, String> {
    let roots = program
        .roots
        .iter()
        .copied()
        .filter(|(kind, _)| {
            matches!(
                kind,
                LayerRoot::StationaryEntry | LayerRoot::Requested { .. } | LayerRoot::Accepted { .. }
            )
        })
        .collect();
    let program = reduction::reduce_roots(&program.inputs, &program.expressions, roots, monitor)?;
    let mut actors: Vec<_> = region
        .cells
        .iter()
        .flat_map(|&cell| catalog.cells[cell as usize].uses.iter().map(|use_| use_.actor))
        .collect();
    actors.sort_unstable();
    actors.dedup();
    let resets: FxHashSet<_> = region
        .cells
        .iter()
        .flat_map(|&cell| catalog.cells[cell as usize].uses.iter().filter_map(|use_| use_.reset.observer()))
        .collect();
    let wires: FxHashSet<_> = arrivals.iter().flat_map(|arrival| arrival.wires.iter().copied()).collect();
    let owns = |node: NodeId| {
        resets.contains(&node)
            || wires.contains(&node)
            || matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Piston(actor) if actors.binary_search(&actor).is_ok())
    };
    let entries: FxHashSet<_> = region
        .layers
        .iter()
        .flat_map(|layer| &layer.uses)
        .flat_map(|use_| &use_.cause.boundary_occurrences)
        .filter(|event| !owns(graph.routes[event.route.index()].source))
        .map(|event| event.route)
        .collect();
    if entries.is_empty() {
        return Ok(None);
    }
    let mut entries: Vec<_> = entries.into_iter().collect();
    entries.sort_unstable();
    let root = |kind| {
        program
            .roots
            .iter()
            .find_map(|(candidate, root)| (*candidate == kind).then_some(*root))
            .ok_or("kernel root is missing".to_owned())
    };
    let mut pulls = Vec::new();
    for (layer, frontier) in region.layers.iter().enumerate() {
        for (occurrence, use_) in frontier.uses.iter().enumerate() {
            pulls.push(KernelPull {
                actor: use_.actor,
                requested: root(LayerRoot::Requested { layer, occurrence })?,
                accepted: root(LayerRoot::Accepted { layer, occurrence })?,
                external: use_
                    .cause
                    .boundary_occurrences
                    .iter()
                    .any(|event| entries.binary_search(&event.route).is_ok()),
            });
        }
    }
    let Some(routes) = lower_routes(graph, &actors, &resets, &wires, arrivals) else {
        return Ok(None);
    };
    let mut wire_levels: Vec<_> = wires
        .into_iter()
        .map(|node| {
            let ComponentState::Wire(wire) = graph.nodes[node.index()].state else {
                unreachable!("certified corridor is a wire")
            };
            (node, wire.power)
        })
        .collect();
    wire_levels.sort_unstable_by_key(|&(node, _)| node);
    Ok(Some(InstantKernel {
        entries: entries.into(),
        program: KernelProgram::PullWave {
            stationary: root(LayerRoot::StationaryEntry)?,
            program,
            pulls: pulls.into(),
        },
        actors: actors.into(),
        wires: wire_levels.into(),
        routes: routes.into(),
    }))
}

fn lower_routes(
    graph: &CompileGraph,
    actors: &[PistonId],
    resets: &FxHashSet<NodeId>,
    wires: &FxHashSet<NodeId>,
    arrivals: &[composition::corridors::Arrival],
) -> Option<Vec<KernelRoute>> {
    let owns = |node: NodeId| {
        wires.contains(&node) || matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Piston(actor) if actors.binary_search(&actor).is_ok())
    };
    let mut routes = Vec::new();
    for &actor in actors {
        let spec = graph.pistons[actor.index()];
        for route in [Some(spec.routes.base_changed), spec.routes.far_changed, Some(spec.routes.near_changed)]
            .into_iter()
            .flatten()
        {
            let mut deliveries = Vec::new();
            for &delivery in &graph.routes[route.index()].deliveries {
                let receiver = delivery.receiver.or_else(|| match graph.sites[delivery.target.index()].source {
                    SiteSource::Component(node) => Some(CallbackReceiver::Component(node)),
                    _ => None,
                });
                match receiver {
                    Some(CallbackReceiver::Component(node)) if resets.contains(&node) => deliveries.push(delivery),
                    Some(CallbackReceiver::Component(node)) if owns(node) => {}
                    Some(CallbackReceiver::Component(node))
                        if matches!(
                            graph.nodes[node.index()].kind,
                            CircuitNodeKind::Electrical(
                                crate::redpiler::components::NodeType::Lever
                                    | crate::redpiler::components::NodeType::Button
                                    | crate::redpiler::components::NodeType::PressurePlate
                            )
                        ) => {}
                    Some(CallbackReceiver::PistonBase(other) | CallbackReceiver::PistonHead(other)) if actors.binary_search(&other).is_ok() => {}
                    None | Some(CallbackReceiver::Contact(_)) => {}
                    // A singleton has no later owned action to invalidate. Keep its outside
                    // handlers in place, including their synchronous feedback and captures.
                    _ if actors.len() == 1 => deliveries.push(delivery),
                    // shortcut: multi-actor external feedback needs an inter-action proof.
                    _ => return None,
                }
            }
            let mut clear_wires: Vec<_> = arrivals
                .iter()
                .filter(|arrival| arrival.producer == actor && arrival.start.route == route)
                .flat_map(|arrival| arrival.wires.iter().copied())
                .collect();
            clear_wires.sort_unstable();
            routes.push(KernelRoute {
                route,
                clear_wires: clear_wires.into(),
                deliveries: deliveries.into(),
            });
        }
    }
    routes.sort_unstable_by_key(|route| route.route);
    routes.dedup_by_key(|route| route.route);
    Some(routes)
}
