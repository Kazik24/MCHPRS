//! Lower proven waves to small executables; declined lifecycle families keep ordinary Graph.
use super::*;
use rustc_hash::FxHashSet;
use std::sync::Arc;

#[cfg(test)]
pub(crate) fn lower(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<Vec<Arc<InstantKernel>>, String> {
    let (Some(catalog), Some(plan)) = (&graph.plans.instant.boundaries, &graph.plans.instant.layers) else {
        return lower_with_context(graph, monitor, None);
    };
    let (expected, context) = compile_layers_with_context(graph, catalog, monitor)?;
    if plan != &expected {
        return Err("kernel lowering received a stale layer plan".into());
    }
    lower_with_context(graph, monitor, Some(&context))
}

pub(crate) fn lower_with_context(
    graph: &CompileGraph,
    monitor: &TaskMonitor,
    context: Option<&LayerCompilationContext>,
) -> Result<Vec<Arc<InstantKernel>>, String> {
    let mut kernels = lower_pull_waves(graph, monitor, context)?;
    if !graph.plans.instant_events.is_empty() {
        if graph.plans.settled_circuit.is_some() {
            let event_actors: FxHashSet<_> = graph
                .plans
                .instant_events
                .iter()
                .flat_map(|kernel| kernel.actors.iter().copied())
                .collect();
            // An event kernel already provides the cheaper exact path for a one-actor
            // wave that owns no wire corridor. Keep PullWaves where they contract
            // multiple actors or eliminate owned wire propagation.
            kernels.retain(|kernel| {
                let KernelProgram::PullWave { .. } = &kernel.program else {
                    return true;
                };
                kernel.actors.len() != 1 || !kernel.wires.is_empty() || !event_actors.contains(&kernel.actors[0])
            });
            let owned: FxHashSet<_> = kernels.iter().flat_map(|kernel| kernel.actors.iter().copied()).collect();
            for kernel in &graph.plans.instant_events {
                let KernelProgram::Events(entries) = &kernel.program else {
                    return Err("instant event table contains a non-event kernel".into());
                };
                let remaining: Vec<_> = entries.iter().filter(|entry| !owned.contains(&entry.actor)).cloned().collect();
                if remaining.is_empty() {
                    continue;
                }
                let mut fallback = kernel.clone();
                fallback.actors = remaining.iter().map(|entry| entry.actor).collect();
                fallback.program = KernelProgram::Events(remaining.into_boxed_slice());
                kernels.push(Arc::new(fallback));
            }
            return Ok(kernels);
        }
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
                Err(ConstructionError::Unsupported(message)) => return Err(message),
                Err(ConstructionError::FeedbackRequiresRegister(source)) => {
                    return Err(format!("logical feedback requires a register: {source:?}"));
                }
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
            let causes = use_.cause.boundary_occurrences.as_ref();
            let triggers = use_.cause.enqueue_triggers.as_ref();
            let all_boundary_sources = !causes.is_empty()
                && causes.iter().all(|event| entries.binary_search(&event.route).is_ok())
                && causes == triggers;
            let origin = if all_boundary_sources {
                KernelPullOrigin::Boundary(causes.to_vec().into_boxed_slice())
            } else {
                // An internal request must retain one exact source occurrence and its
                // recorded callback cuts. Never infer a producer from a global route search.
                let [cause] = causes else { return Ok(None) };
                if triggers.is_empty() {
                    return Ok(None);
                }
                KernelPullOrigin::Internal {
                    cause: *cause,
                    enqueue_triggers: triggers.to_vec().into_boxed_slice(),
                }
            };
            pulls.push(KernelPull {
                actor: use_.actor,
                requested: root(LayerRoot::Requested { layer, occurrence })?,
                accepted: root(LayerRoot::Accepted { layer, occurrence })?,
                origin,
            });
        }
    }
    let Some(routes) = lower_routes(
        graph,
        &actors,
        &resets,
        &wires,
        arrivals,
        &pulls,
    ) else {
        return Ok(None);
    };
    let mut expected_wire_levels = FxHashMap::default();
    for arrival in arrivals {
        for &(node, expected) in arrival.expected_wire_levels.iter() {
            if !arrival.wires.contains(&node) || expected_wire_levels.insert(node, expected).is_some_and(|old| old != expected) {
                return Ok(None);
            }
        }
    }
    if expected_wire_levels.len() != wires.len() || expected_wire_levels.keys().any(|node| !wires.contains(node)) {
        return Ok(None);
    }
    let mut wire_levels: Vec<_> = expected_wire_levels.into_iter().collect();
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
    pulls: &[KernelPull],
) -> Option<Vec<KernelRoute>> {
    let owns = |node: NodeId| {
        wires.contains(&node) || matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Piston(actor) if actors.binary_search(&actor).is_ok())
    };
    let mut enqueue_at = Vec::new();
    for (pull_index, pull) in pulls.iter().enumerate().filter(|(_, pull)| !pull.origin.is_boundary()) {
        let Some(cause) = pull.origin.internal_cause() else { return None };
        let cause_delivery = *graph.routes.get(cause.route.index())?.deliveries.get(cause.ordinal as usize)?;
        // Piston sampling is an Update callback. A shape row at the same target is
        // not a valid recipe for creating the child request.
        let direct_receiver = matches!(cause_delivery.kind, DeliveryKind::Update { .. }).then(|| {
            cause_delivery.receiver.or_else(|| match graph.sites[cause_delivery.target.index()].source {
                SiteSource::Component(node) => Some(CallbackReceiver::Component(node)),
                _ => None,
            })
        }).flatten();
        let target_actor = match direct_receiver {
            Some(CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor)) => Some(actor),
            Some(CallbackReceiver::Component(node)) => match graph.nodes[node.index()].kind {
                CircuitNodeKind::Piston(actor) => Some(actor),
                _ => None,
            },
            _ => None,
        };
        let direct = target_actor == Some(pull.actor)
            && matches!(graph.nodes[graph.routes[cause.route.index()].source.index()].kind, CircuitNodeKind::Piston(source) if actors.binary_search(&source).is_ok());
        let enqueue_triggers = pull.origin.enqueue_triggers();
        if enqueue_triggers.is_empty() {
            return None;
        }
        for &trigger in enqueue_triggers {
            let direct_trigger = trigger == cause && direct;
            let certified_arrival = arrivals.iter().any(|arrival| {
                arrival.start == trigger
                    && arrival.finishes.iter().any(|finish| finish.actor == pull.actor && finish.occurrence == cause)
            });
            if !direct_trigger && !certified_arrival {
                return None;
            }
            enqueue_at.push((trigger, pull_index));
        }
    }
    let mut routes = Vec::new();
    for &actor in actors {
        let spec = graph.pistons[actor.index()];
        for route in [Some(spec.routes.base_changed), spec.routes.far_changed, Some(spec.routes.near_changed)]
            .into_iter()
            .flatten()
        {
            let mut steps = Vec::new();
            for (ordinal, &delivery) in graph.routes[route.index()].deliveries.iter().enumerate() {
                let occurrence = RouteOccurrence {
                    route,
                    ordinal: ordinal as u32,
                };
                let mut clear_wires: Vec<_> = arrivals
                    .iter()
                    .filter(|arrival| arrival.start == occurrence)
                    .flat_map(|arrival| arrival.wires.iter().copied())
                    .collect();
                clear_wires.sort_unstable();
                clear_wires.dedup();
                if !clear_wires.is_empty() {
                    steps.push(KernelRouteStep {
                        ordinal: ordinal as u32,
                        kind: KernelRouteStepKind::ClearWires(clear_wires.into()),
                    });
                }
                let receiver = delivery.receiver.or_else(|| {
                    matches!(delivery.kind, DeliveryKind::Update { .. })
                        .then(|| match graph.sites[delivery.target.index()].source {
                            SiteSource::Component(node) => Some(CallbackReceiver::Component(node)),
                            _ => None,
                        })
                        .flatten()
                });
                match receiver {
                    Some(CallbackReceiver::Component(node)) if resets.contains(&node) => {
                        steps.push(KernelRouteStep {
                            ordinal: ordinal as u32,
                            kind: KernelRouteStepKind::Deliver(delivery),
                        });
                    }
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
                    // No later owned actor can observe a changed read in a singleton wave.
                    _ if actors.len() == 1 => {
                        steps.push(KernelRouteStep {
                            ordinal: ordinal as u32,
                            kind: KernelRouteStepKind::Deliver(delivery),
                        });
                    }
                    // Later reads may observe this callback's immediate writes.
                    _ => {
                        steps.push(KernelRouteStep {
                            ordinal: ordinal as u32,
                            kind: KernelRouteStepKind::NativeHandoff(delivery),
                        });
                    }
                }
                // The native wire update and its nested callbacks finish before
                // the resulting piston request is appended to Graph's FIFO.
                for &(_, pull_index) in enqueue_at.iter().filter(|(trigger, _)| *trigger == occurrence) {
                    steps.push(KernelRouteStep {
                        ordinal: ordinal as u32,
                        kind: KernelRouteStepKind::EnqueuePull(pull_index),
                    });
                }
            }
            routes.push(KernelRoute {
                route,
                steps: steps.into(),
            });
        }
    }
    routes.sort_unstable_by_key(|route| route.route);
    routes.dedup_by_key(|route| route.route);
    Some(routes)
}
