//! Lower source-proven, single-owner stationary cells into finite Boolean request layers.
use super::regions::{self, RegionComponent};
use crate::redpiler::TaskMonitor;
use crate::redpiler::analysis::MAX_GRAPH_PREPARATION_BYTES;
use crate::redpiler::passes::utils::instant_mechanisms::{PowerIndex, conductor_inputs, index_power};
use crate::redpiler::program_graph::*;
use mchprs_blocks::BlockFace;
use rustc_hash::FxHashMap;

const MAX_LAYER_BYTES: usize = MAX_GRAPH_PREPARATION_BYTES / 8;
const MAX_LAYER_WORK: usize = 8 * 1024 * 1024;

mod composition;
mod reduction;
mod signals;
pub(crate) use reduction::reduce as lower_region;

/// Build deterministic cell regions and compile only the narrow cases with proven request cuts.
pub(crate) fn compile_layers(graph: &CompileGraph, catalog: &BoundaryCatalog, monitor: &TaskMonitor) -> Result<LayeredInstantPlan, String> {
    check_cancelled(monitor)?;
    let partition = regions::partition(graph, catalog, monitor)?;
    let perimeters = regions::perimeters(graph, &partition, monitor)?;
    let power_index = index_power(graph);
    let mut compiled = Vec::new();
    let mut declined = Vec::new();
    for (region_index, component) in partition.regions.iter().enumerate() {
        check_cancelled(monitor)?;
        match compile_component(
            graph,
            catalog,
            component,
            region_index,
            &partition.owners,
            &partition.owner_region,
            &power_index,
            &perimeters[region_index],
            monitor,
        )? {
            Ok(region) => compiled.push(region),
            Err(decline) => declined.push(decline),
        }
    }
    compiled.sort_unstable_by_key(|region| region.id);
    declined.sort_unstable_by_key(|decline| decline.cells.first().copied().unwrap_or(u32::MAX));
    let programs = compiled
        .iter()
        .map(|region| lower_region(region, monitor))
        .collect::<Result<Box<[_]>, _>>()?;
    check_cancelled(monitor)?;
    Ok(LayeredInstantPlan {
        regions: compiled.into_boxed_slice(),
        programs,
        declines: declined.into_boxed_slice(),
    })
}

fn compile_component(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    component: &RegionComponent,
    region_index: usize,
    owners: &super::boundaries::OwnerMap,
    owner_region: &[u32],
    power_index: &PowerIndex,
    perimeter: &rustc_hash::FxHashSet<(BoundaryDirection, BoundaryPortKind)>,
    monitor: &TaskMonitor,
) -> Result<Result<LayeredRegion, LayerDecline>, String> {
    if component.cells.len() != 1 || !component.corridors.is_empty() {
        if !component.cells.is_empty() {
            return composition::compile_connected(graph, catalog, component, region_index, power_index, perimeter, monitor);
        }
        let witness = first_unproven_power(graph, catalog, component, region_index, owners, owner_region, monitor)?.unwrap_or_else(|| {
            if !component.corridors.is_empty() {
                component
                    .corridors
                    .first()
                    .and_then(|corridor| catalog.corridors.get(*corridor as usize))
                    .and_then(|corridor| corridor.wires.first())
                    .copied()
                    .map(LayerWitness::Wire)
                    .unwrap_or_else(|| first_witness(graph, catalog, component))
            } else {
                first_witness(graph, catalog, component)
            }
        });
        let reason = if component.corridors.is_empty() {
            LayerDeclineReason::SharedOwnerOrderUnknown
        } else {
            LayerDeclineReason::SignalArrivalUnproven
        };
        return Ok(Err(decline(component, witness, reason)));
    }
    let cell_id = component.cells[0];
    let Some(cell) = catalog.cells.get(cell_id as usize) else {
        return Err("instant region references a missing cell contract".into());
    };
    if cell.uses.len() != 1 || cell.resources.len() != 1 {
        let actor = cell.uses.first().map_or(PistonId::new(0), |use_| use_.actor);
        return Ok(Err(decline(
            component,
            LayerWitness::Actor(actor),
            LayerDeclineReason::SharedOwnerOrderUnknown,
        )));
    }
    let use_ = &cell.uses[0];
    let private_resource = &cell.resources[0];
    let Some(transfer) = use_.retraction.transfer else {
        return Ok(Err(decline(
            component,
            LayerWitness::Actor(use_.actor),
            LayerDeclineReason::SharedOwnerOrderUnknown,
        )));
    };
    let actor_spec = graph.pistons.get(use_.actor.index()).ok_or("cell actor is outside the source graph")?;
    if private_resource.owners.as_ref() != [use_.actor]
        || private_resource.resource != transfer.resource
        || private_resource.source != transfer.source
        || !matches!(actor_spec.lane, PistonLane::SingleResource { far } if far == private_resource.source)
        || actor_spec.near != transfer.destination
    {
        return Ok(Err(decline(
            component,
            LayerWitness::Actor(use_.actor),
            LayerDeclineReason::SharedOwnerOrderUnknown,
        )));
    }
    let activations = match activation_bindings(graph, catalog, cell_id, use_)? {
        Ok(activations) => activations,
        Err((witness, reason)) => return Ok(Err(decline(component, witness, reason))),
    };
    if activations.boundary.is_empty() {
        return Ok(Err(decline(
            component,
            LayerWitness::Actor(use_.actor),
            LayerDeclineReason::CallbackSemanticsUnsupported,
        )));
    }
    if let Some((witness, reason)) = unsupported_single_cell(graph, catalog, cell_id, cell, use_, private_resource, power_index)? {
        return Ok(Err(decline(component, witness, reason)));
    }

    let mut builder = LayerBuilder::new(monitor)?;
    let mut request_pins = Vec::with_capacity(cell.pins.len());
    for pin in &cell.pins {
        let value = compile_pin(
            graph,
            catalog,
            cell_id,
            use_,
            pin,
            private_resource,
            power_index,
            LayerInputRole::RequestSample,
            LayerInputRole::ActivationEvent,
            &activations.boundary,
            &request_pins,
            &mut builder,
        )?;
        request_pins.push(value);
    }
    let requested = request_pins[use_.requested.index()];

    let mut acceptance_pins = Vec::with_capacity(cell.pins.len());
    for pin in &cell.pins {
        let value = compile_pin(
            graph,
            catalog,
            cell_id,
            use_,
            pin,
            private_resource,
            power_index,
            LayerInputRole::AcceptanceRevalidation,
            LayerInputRole::ActivationEvent,
            &activations.boundary,
            &acceptance_pins,
            &mut builder,
        )?;
        acceptance_pins.push(value);
    }
    let valid = acceptance_pins[use_.valid.index()];
    let consumed_before = builder.constant(false)?;
    let not_consumed = builder.not(consumed_before)?;
    let accepted = builder.all([requested, valid, not_consumed])?;
    let consumed_after = builder.any([consumed_before, accepted])?;
    let stationary_entry = request_pins
        .get(cell.stationary_entry.index())
        .copied()
        .ok_or("stationary-entry pin is outside the cell contract")?;

    let present_at_acceptance = *acceptance_pins
        .get(private_resource.present.index())
        .ok_or("private resource source-presence pin is outside the cell contract")?;
    let absent_at_acceptance = *acceptance_pins
        .get(private_resource.absent.index())
        .ok_or("private resource source-absence pin is outside the cell contract")?;
    let transfer_when = builder.all([accepted, present_at_acceptance])?;
    let source_absent_after = builder.any([absent_at_acceptance, transfer_when])?;
    let resource_outputs = vec![LayerResourceOutput {
        cell: cell_id,
        resource: private_resource.resource,
        source: private_resource.source,
        destination: transfer.destination,
        transfer_when,
        source_absent_after,
    }];

    let mut pin_values = Vec::with_capacity(acceptance_pins.len());
    for (pin, &value) in acceptance_pins.iter().enumerate() {
        builder.visit()?;
        pin_values.push(LayerPinValue {
            cell: cell_id,
            pin: CellPinId(u32::try_from(pin).map_err(|_| "instant cell has too many pins")?),
            value,
        });
    }
    let lifecycle = match lifecycle_occurrences(graph, catalog, cell_id, use_, &activations.lifecycle, &mut builder)? {
        Ok(lifecycle) => lifecycle,
        Err((witness, reason)) => return Ok(Err(decline(component, witness, reason))),
    };
    let outputs = match compile_outputs(
        graph,
        catalog,
        component,
        cell,
        use_,
        private_resource,
        transfer,
        power_index,
        accepted,
        transfer_when,
        &lifecycle,
        &mut builder,
    )? {
        Ok(outputs) => outputs,
        Err((witness, reason)) => return Ok(Err(decline(component, witness, reason))),
    };
    let actor = use_.actor;
    let layer = CausalLayer {
        order: LayerOrder::Fifo,
        uses: vec![LayerUse {
            use_id: LayerUseId(0),
            actor,
            requested,
            accepted,
            consumed_before,
            consumed_after,
            cause: UseCause {
                boundary_occurrences: activations.boundary.into_boxed_slice(),
            },
        }]
        .into_boxed_slice(),
    };
    Ok(Ok(LayeredRegion {
        id: LayerRegionId(u32::try_from(region_index).map_err(|_| "too many instant regions")?),
        cells: component.cells.clone(),
        corridors: component.corridors.clone(),
        stationary_entry,
        inputs: builder.inputs.into_boxed_slice(),
        outputs,
        resource_outputs: resource_outputs.into_boxed_slice(),
        cold_lifecycle_occurrences: lifecycle.into_boxed_slice(),
        expressions: builder.expressions.into_boxed_slice(),
        pin_values: pin_values.into_boxed_slice(),
        layers: vec![layer].into_boxed_slice(),
    }))
}

fn first_unproven_power(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    component: &RegionComponent,
    region_index: usize,
    owners: &super::boundaries::OwnerMap,
    owner_region: &[u32],
    monitor: &TaskMonitor,
) -> Result<Option<LayerWitness>, String> {
    let mut work = 0usize;
    for &cell_id in &component.cells {
        let cell = catalog
            .cells
            .get(cell_id as usize)
            .ok_or("instant region references a missing cell contract")?;
        for use_ in &cell.uses {
            let mut pending = vec![use_.powered];
            let mut visited = rustc_hash::FxHashSet::default();
            while let Some(pin) = pending.pop() {
                work = work.checked_add(1).ok_or("layer support walk work overflow")?;
                if work > MAX_LAYER_WORK {
                    return Err("layer support walk exceeded its preparation work budget".into());
                }
                if work & 0x3ff == 0 {
                    check_cancelled(monitor)?;
                }
                if !visited.insert(pin) {
                    continue;
                }
                match cell.pins.get(pin.index()).ok_or("cell powered pin references a missing equation")? {
                    CellPin::Read(CellRead::Contribution(link))
                        if source_is_internal_wire(graph, catalog, owners, owner_region, region_index, *link) =>
                    {
                        return Ok(Some(LayerWitness::PowerAtCut {
                            actor: use_.actor,
                            link: *link,
                            phase: LayerInputRole::RequestSample,
                        }));
                    }
                    CellPin::Gate { inputs, .. } => pending.extend(inputs.iter().rev().copied()),
                    _ => {}
                }
            }
        }
    }
    check_cancelled(monitor)?;
    Ok(None)
}

fn source_is_internal_wire(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    owners: &super::boundaries::OwnerMap,
    owner_region: &[u32],
    region_index: usize,
    link: PowerLink,
) -> bool {
    let Some(source_owner) = owners.sites.get(link.from.index()).copied().flatten() else {
        return false;
    };
    if owner_region.get(source_owner).copied() != Some(region_index as u32) {
        return false;
    }
    match graph.sites.get(link.from.index()).map(|site| site.source) {
        Some(SiteSource::Component(node)) => graph
            .nodes
            .get(node.index())
            .is_some_and(|node| matches!(node.kind, CircuitNodeKind::Wire(_))),
        Some(SiteSource::Constant(_)) => source_owner >= catalog.cells.len(),
        _ => false,
    }
}

/// Boundary activations start this wave; owner callbacks and restoration routes stay cold.
struct ActivationBindings {
    /// These callback rows are true external event inputs for the current request wave.
    boundary: Vec<RouteOccurrence>,
    /// These source callbacks remain explicit Graph lifecycle work outside this finite wave.
    lifecycle: Vec<LayerLifecycleOccurrence>,
}

fn activation_bindings(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    cell_id: u32,
    use_: &CellUse,
) -> Result<Result<ActivationBindings, (LayerWitness, LayerDeclineReason)>, String> {
    let mut bindings = ActivationBindings {
        boundary: Vec::new(),
        lifecycle: Vec::new(),
    };
    let actor = use_.actor;
    let Some(actor_spec) = graph.pistons.get(actor.index()) else {
        return Err("cell activation actor is outside the source graph".into());
    };
    for &occurrence in &use_.activations {
        let Some(route) = graph.routes.get(occurrence.route.index()) else {
            return Err("cell activation route is outside the source graph".into());
        };
        let Some(delivery) = route.deliveries.get(occurrence.ordinal as usize) else {
            return Err("cell activation ordinal is outside its source route".into());
        };
        if let Some(phase) = later_graph_route_phase(graph, actor_spec.routes, use_, occurrence.route)? {
            bindings.lifecycle.push(LayerLifecycleOccurrence {
                actor,
                occurrence,
                phase,
                reason: LayerLifecycleReason::OrdinaryGraphLifecycle,
            });
            continue;
        }
        if retraction_route_phase(use_, occurrence.route).is_some() && is_inert_cell_callback(graph, delivery, use_) {
            bindings.lifecycle.push(LayerLifecycleOccurrence {
                actor,
                occurrence,
                phase: retraction_route_phase(use_, occurrence.route).expect("route phase was checked"),
                reason: LayerLifecycleReason::InertCellCallback,
            });
            continue;
        }
        if reset_occurrence(use_, occurrence) && callback_targets_reset(graph, delivery, use_.reset) {
            bindings.lifecycle.push(LayerLifecycleOccurrence {
                actor,
                occurrence,
                phase: LayerLifecyclePhase::ResetObserverLifecycle,
                reason: LayerLifecycleReason::ResetObserverUpdate { observer: use_.reset },
            });
            continue;
        }
        if find_port(
            catalog,
            BoundaryOwner::Cell(cell_id),
            BoundaryDirection::Input,
            BoundaryPortKind::Callback { occurrence },
        )
        .is_some()
        {
            bindings.boundary.push(occurrence);
            continue;
        }
        return Ok(Err((
            LayerWitness::Callback(occurrence),
            LayerDeclineReason::CallbackSemanticsUnsupported,
        )));
    }
    Ok(Ok(bindings))
}

fn retraction_route_phase(use_: &CellUse, route: RouteId) -> Option<LayerLifecyclePhase> {
    if route == use_.retraction.routes.base_changed {
        Some(LayerLifecyclePhase::RetractionBaseChanged)
    } else if Some(route) == use_.retraction.routes.far_changed {
        Some(LayerLifecyclePhase::RetractionFarChanged)
    } else if route == use_.retraction.routes.near_changed {
        Some(LayerLifecyclePhase::RetractionNearChanged)
    } else {
        None
    }
}

fn deferred_route_phase(graph: &CompileGraph, routes: PistonRoutes, use_: &CellUse, route: RouteId) -> Option<LayerLifecyclePhase> {
    if route == use_.retraction.wake.base_restored || route == routes.base_restored {
        Some(LayerLifecyclePhase::DeferredBaseRestoration)
    } else if route == routes.head_restored {
        Some(LayerLifecyclePhase::DeferredHeadRestoration)
    } else if use_.retraction.wake.resource.is_some_and(|wake| wake.route == route)
        || graph.pistons[use_.actor.index()].routes.resource_restored_near == Some(route)
        || graph.pistons[use_.actor.index()].routes.resource_restored_far == Some(route)
    {
        Some(LayerLifecyclePhase::DeferredResourceRestoration)
    } else {
        None
    }
}

fn later_graph_route_phase(
    graph: &CompileGraph,
    routes: PistonRoutes,
    use_: &CellUse,
    route: RouteId,
) -> Result<Option<LayerLifecyclePhase>, String> {
    let reset = graph
        .nodes
        .get(use_.reset.index())
        .ok_or("cell reset observer is outside the source graph")?;
    if let Some(output_route) = reset.output_route {
        if graph.routes.get(output_route.index()).is_none_or(|row| row.source != use_.reset) {
            return Err("reset observer output route has a mismatched source".into());
        }
        if route == output_route {
            return Ok(Some(LayerLifecyclePhase::ResetOutput));
        }
    }
    if let Some(shape_route) = routes.far_shape_changed {
        let actor = graph.pistons.get(use_.actor.index()).ok_or("cell actor is outside the source graph")?;
        if graph.routes.get(shape_route.index()).is_none_or(|row| row.source != actor.node) {
            return Err("piston far-shape route has a mismatched source".into());
        }
        if route == shape_route {
            return Ok(Some(LayerLifecyclePhase::ResetFarShapeChange));
        }
    }
    Ok(deferred_route_phase(graph, routes, use_, route))
}

/// Recognize only callbacks proven inert for this cell at its exact accepted-route phase.
fn is_inert_cell_callback(graph: &CompileGraph, delivery: &RouteDelivery, use_: &CellUse) -> bool {
    if delivery.view != OperandView::Live {
        return false;
    }
    match delivery.kind {
        DeliveryKind::Update { .. } => match resolved_receiver(graph, delivery) {
            Some(CallbackReceiver::Contact(contact)) => contact_is_local_to_actor(graph, contact, use_.actor),
            Some(receiver) => receiver_is_actor(graph, receiver, use_.actor),
            None => false,
        },
        DeliveryKind::Shape { .. } => match resolved_receiver(graph, delivery) {
            Some(CallbackReceiver::Contact(contact)) => contact_is_local_to_actor(graph, contact, use_.actor),
            Some(CallbackReceiver::Component(node)) if node == use_.reset => graph
                .nodes
                .get(node.index())
                .is_some_and(|node| matches!(&node.kind, CircuitNodeKind::Observer { .. })),
            _ => false,
        },
    }
}

fn contact_is_local_to_actor(graph: &CompileGraph, contact: ContactId, actor: PistonId) -> bool {
    let Some(spec) = graph.pistons.get(actor.index()) else {
        return false;
    };
    [spec.base, spec.near, spec.far].contains(&contact) && contact_receivers_are_local(graph, contact, actor)
}

fn contact_receivers_are_local(graph: &CompileGraph, contact: ContactId, actor: PistonId) -> bool {
    let Some(contact) = graph.contacts.get(contact.index()) else {
        return false;
    };
    !contact.variants.is_empty()
        && contact
            .variants
            .iter()
            .all(|variant| variant.receiver.is_none_or(|receiver| receiver_is_actor(graph, receiver, actor)))
}

fn receiver_is_actor(graph: &CompileGraph, receiver: CallbackReceiver, actor: PistonId) -> bool {
    match receiver {
        CallbackReceiver::PistonBase(target) | CallbackReceiver::PistonHead(target) => target == actor,
        CallbackReceiver::Component(node) => graph
            .nodes
            .get(node.index())
            .is_some_and(|node| matches!(&node.kind, CircuitNodeKind::Piston(target) if *target == actor)),
        CallbackReceiver::Contact(_) => false,
    }
}

fn callback_targets_reset(graph: &CompileGraph, delivery: &RouteDelivery, reset: NodeId) -> bool {
    matches!(resolved_receiver(graph, delivery), Some(CallbackReceiver::Component(node)) if node == reset)
}

fn resolved_receiver(graph: &CompileGraph, delivery: &RouteDelivery) -> Option<CallbackReceiver> {
    delivery.receiver.or_else(|| match graph.sites.get(delivery.target.index())?.source {
        SiteSource::Component(node) => Some(CallbackReceiver::Component(node)),
        SiteSource::Contact(contact) => Some(CallbackReceiver::Contact(contact)),
        SiteSource::Constant(_) => None,
    })
}

fn reset_occurrence(use_: &CellUse, occurrence: RouteOccurrence) -> bool {
    use_.retraction.reset.on_base_changed.contains(&occurrence) || use_.retraction.reset.on_base_restored.contains(&occurrence)
}

fn lifecycle_occurrences(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    cell_id: u32,
    use_: &CellUse,
    activation_lifecycle: &[LayerLifecycleOccurrence],
    builder: &mut LayerBuilder<'_>,
) -> Result<Result<Vec<LayerLifecycleOccurrence>, (LayerWitness, LayerDeclineReason)>, String> {
    let mut lifecycle = activation_lifecycle.to_vec();
    let actor_spec = *graph
        .pistons
        .get(use_.actor.index())
        .ok_or("cell lifecycle actor is outside the source graph")?;
    let routes = [
        Some((use_.retraction.routes.base_changed, LayerLifecyclePhase::RetractionBaseChanged)),
        use_.retraction
            .routes
            .far_changed
            .map(|route| (route, LayerLifecyclePhase::RetractionFarChanged)),
        Some((use_.retraction.routes.near_changed, LayerLifecyclePhase::RetractionNearChanged)),
    ];
    for (route, phase) in routes.into_iter().flatten() {
        let source = graph.routes.get(route.index()).ok_or("retraction lifecycle route is outside the graph")?;
        for (ordinal, delivery) in source.deliveries.iter().enumerate() {
            builder.visit()?;
            let occurrence = RouteOccurrence {
                route,
                ordinal: u32::try_from(ordinal).map_err(|_| "too many callback occurrences in one route")?,
            };
            let (reason, lifecycle_phase) = if is_inert_cell_callback(graph, delivery, use_) {
                (LayerLifecycleReason::InertCellCallback, phase)
            } else if reset_occurrence(use_, occurrence) && callback_targets_reset(graph, delivery, use_.reset) {
                (
                    LayerLifecycleReason::ResetObserverUpdate { observer: use_.reset },
                    LayerLifecyclePhase::ResetObserverLifecycle,
                )
            } else {
                if has_port(catalog, cell_id, BoundaryDirection::Output, BoundaryPortKind::Callback { occurrence }) {
                    continue;
                }
                return Ok(Err((LayerWitness::Callback(occurrence), LayerDeclineReason::BoundaryEffectUnsupported)));
            };
            let row = LayerLifecycleOccurrence {
                actor: use_.actor,
                occurrence,
                phase: lifecycle_phase,
                reason,
            };
            if !lifecycle.contains(&row) {
                lifecycle.push(row);
            }
        }
    }
    let deferred = [
        Some((use_.retraction.wake.base_restored, LayerLifecyclePhase::DeferredBaseRestoration)),
        Some((actor_spec.routes.head_restored, LayerLifecyclePhase::DeferredHeadRestoration)),
        use_.retraction
            .wake
            .resource
            .map(|wake| (wake.route, LayerLifecyclePhase::DeferredResourceRestoration)),
        actor_spec
            .routes
            .resource_restored_far
            .map(|route| (route, LayerLifecyclePhase::DeferredResourceRestoration)),
    ];
    for (route, phase) in deferred.into_iter().flatten() {
        let source = graph.routes.get(route.index()).ok_or("deferred lifecycle route is outside the graph")?;
        for (ordinal, _) in source.deliveries.iter().enumerate() {
            builder.visit()?;
            let occurrence = RouteOccurrence {
                route,
                ordinal: u32::try_from(ordinal).map_err(|_| "too many callback occurrences in one route")?,
            };
            let row = LayerLifecycleOccurrence {
                actor: use_.actor,
                occurrence,
                phase,
                reason: LayerLifecycleReason::OrdinaryGraphLifecycle,
            };
            if !lifecycle.contains(&row) {
                lifecycle.push(row);
            }
        }
    }
    for occurrence in use_
        .retraction
        .reset
        .on_base_changed
        .iter()
        .chain(&use_.retraction.reset.on_base_restored)
    {
        builder.visit()?;
        let row = LayerLifecycleOccurrence {
            actor: use_.actor,
            occurrence: *occurrence,
            phase: LayerLifecyclePhase::ResetObserverLifecycle,
            reason: LayerLifecycleReason::ResetObserverUpdate { observer: use_.reset },
        };
        if !lifecycle.contains(&row) {
            lifecycle.push(row);
        }
    }
    let reset = graph
        .nodes
        .get(use_.reset.index())
        .ok_or("cell reset observer is outside the source graph")?;
    let later_routes = [
        reset.output_route.map(|route| (route, LayerLifecyclePhase::ResetOutput)),
        actor_spec
            .routes
            .far_shape_changed
            .map(|route| (route, LayerLifecyclePhase::ResetFarShapeChange)),
    ];
    for (route, phase) in later_routes.into_iter().flatten() {
        let source = graph.routes.get(route.index()).ok_or("later lifecycle route is outside the graph")?;
        let expected_source = if phase == LayerLifecyclePhase::ResetOutput {
            use_.reset
        } else {
            actor_spec.node
        };
        if source.source != expected_source {
            return Err("later lifecycle route has a mismatched source".into());
        }
        for (ordinal, _) in source.deliveries.iter().enumerate() {
            builder.visit()?;
            let occurrence = RouteOccurrence {
                route,
                ordinal: u32::try_from(ordinal).map_err(|_| "too many callback occurrences in one route")?,
            };
            let row = LayerLifecycleOccurrence {
                actor: use_.actor,
                occurrence,
                phase,
                reason: LayerLifecycleReason::OrdinaryGraphLifecycle,
            };
            if !lifecycle.contains(&row) {
                lifecycle.push(row);
            }
        }
    }
    Ok(Ok(lifecycle))
}

fn has_port(catalog: &BoundaryCatalog, cell_id: u32, direction: BoundaryDirection, kind: BoundaryPortKind) -> bool {
    catalog
        .ports
        .iter()
        .any(|port| port.owner == BoundaryOwner::Cell(cell_id) && port.direction == direction && port.kind == kind)
}

fn unsupported_single_cell(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    cell_id: u32,
    cell: &CellContract,
    use_: &CellUse,
    private_resource: &CellResourcePin,
    power_index: &PowerIndex,
) -> Result<Option<(LayerWitness, LayerDeclineReason)>, String> {
    for pin in &cell.pins {
        match pin {
            CellPin::Read(CellRead::Contribution(link)) => {
                let Some(index) = graph.power_links.iter().position(|candidate| candidate == link) else {
                    return Err("cell power contribution is absent from the graph".into());
                };
                let link_index = u32::try_from(index).map_err(|_| "too many graph power links")?;
                let external = find_port(
                    catalog,
                    BoundaryOwner::Cell(cell_id),
                    BoundaryDirection::Input,
                    BoundaryPortKind::Power { link: link_index },
                )
                .is_some();
                if !external && local_reset_output_link(graph, power_index, use_, *link).is_none() {
                    return Ok(Some((LayerWitness::Power(*link), LayerDeclineReason::SignalArrivalUnproven)));
                }
            }
            CellPin::Read(CellRead::Activation(actor)) if *actor != use_.actor => {
                return Ok(Some((LayerWitness::Actor(*actor), LayerDeclineReason::CallbackSemanticsUnsupported)));
            }
            CellPin::Read(CellRead::State(test)) if !state_is_owned_stationary_read(*test, use_.actor, private_resource, graph) => {
                return Ok(Some((LayerWitness::Actor(use_.actor), LayerDeclineReason::SharedOwnerOrderUnknown)));
            }
            _ => {}
        }
    }
    Ok(None)
}

/// Accept only the observer output that the existing recognizer already proved is this cell's reset source.
fn local_reset_output_link(graph: &CompileGraph, power: &PowerIndex, use_: &CellUse, contribution: PowerLink) -> Option<PowerLink> {
    let actor_node = graph.pistons.get(use_.actor.index())?.node;
    if contribution.to
        != (PowerTarget::Input {
            node: actor_node,
            port: InputPort::Quasi,
        })
        || contribution.attenuation != 0
        || !matches!(graph.guards.get(contribution.guard.index()), Some(Guard::Always))
    {
        return None;
    }
    let sources = match contribution.from_port {
        PowerPort::Conducted { .. } => conductor_inputs(graph, power, contribution)?,
        _ => vec![contribution],
    };
    if sources.len() != 1 {
        return None;
    }
    let source = sources[0];
    if source.attenuation != 0
        || !matches!(graph.guards.get(source.guard.index()), Some(Guard::Always))
        || !matches!(source.from_port, PowerPort::Strong(BlockFace::Bottom) | PowerPort::Output)
        || !matches!(source.to, PowerTarget::Conductor { .. } | PowerTarget::Input { .. })
        || !matches!(graph.sites.get(source.from.index())?.source, SiteSource::Component(node) if node == use_.reset)
        || !matches!(graph.nodes.get(use_.reset.index())?.kind, CircuitNodeKind::Observer { .. })
    {
        return None;
    }
    Some(source)
}

fn compile_pin(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    cell_id: u32,
    use_: &CellUse,
    pin: &CellPin,
    private_resource: &CellResourcePin,
    power_index: &PowerIndex,
    read_role: LayerInputRole,
    activation_role: LayerInputRole,
    activations: &[RouteOccurrence],
    previous_pins: &[LayerExprId],
    builder: &mut LayerBuilder<'_>,
) -> Result<LayerExprId, String> {
    builder.visit()?;
    match pin {
        CellPin::Read(CellRead::Contribution(link)) => {
            if let Some(reset_link) = local_reset_output_link(graph, power_index, use_, *link) {
                let input = builder.input(
                    LayerInputSource::LocalResetOutput {
                        observer: use_.reset,
                        link: reset_link,
                    },
                    read_role,
                )?;
                return builder.input_expr(input);
            }
            let link_index = graph
                .power_links
                .iter()
                .position(|candidate| candidate == link)
                .ok_or("cell power contribution is absent from the graph")?;
            let Some(port) = find_port(
                catalog,
                BoundaryOwner::Cell(cell_id),
                BoundaryDirection::Input,
                BoundaryPortKind::Power {
                    link: u32::try_from(link_index).map_err(|_| "too many graph power links")?,
                },
            ) else {
                return Err(format!(
                    "cell {} reads internal or unbound power link {link_index} at acceptance/request time",
                    use_.actor.index()
                ));
            };
            let input = builder.input(
                LayerInputSource::PowerContribution {
                    port: port as u32,
                    link: *link,
                },
                read_role,
            )?;
            builder.input_expr(input)
        }
        CellPin::Read(CellRead::Activation(actor)) => {
            if *actor != use_.actor {
                return Err("cell equation reads a different actor activation".into());
            }
            let mut events = Vec::with_capacity(activations.len());
            for &occurrence in activations {
                builder.visit()?;
                let Some(port) = find_port(
                    catalog,
                    BoundaryOwner::Cell(cell_id),
                    BoundaryDirection::Input,
                    BoundaryPortKind::Callback { occurrence },
                ) else {
                    return Err(format!(
                        "actor {} activation occurrence {:?} has no external callback input",
                        actor.index(),
                        occurrence
                    ));
                };
                let input = builder.input(
                    LayerInputSource::BoundaryEvent {
                        port: port as u32,
                        occurrence,
                    },
                    activation_role,
                )?;
                events.push(builder.input_expr(input)?);
            }
            builder.any(events)
        }
        CellPin::Read(CellRead::State(test)) => {
            if !state_is_owned_stationary_read(*test, use_.actor, private_resource, graph) {
                return Err(format!("actor {} has a nonlocal mutable state read {:?}", use_.actor.index(), test));
            }
            let input = builder.input(LayerInputSource::State(*test), read_role)?;
            builder.input_expr(input)
        }
        CellPin::Gate { kind, inputs } => {
            let mut values = Vec::with_capacity(inputs.len());
            for input in inputs.iter() {
                let value = *previous_pins.get(input.index()).ok_or("cell pin references a missing or later operand")?;
                values.push(value);
            }
            builder.gate(*kind, values)
        }
    }
}

fn state_is_owned_stationary_read(test: StateTest, actor: PistonId, resource: &CellResourcePin, graph: &CompileGraph) -> bool {
    matches!(
        test,
        StateTest::PistonExtended(piston) if piston == actor
    ) || matches!(
        test,
        StateTest::PistonAvailability {
            piston,
            is: PistonAvailability::Ready,
        } if piston == actor
    ) || matches!(test, StateTest::ResourceEnabled(id) if id == resource.resource)
        || matches!(test, StateTest::ResourceAt { resource: id, contact } if id == resource.resource
            && graph.resources.get(id.index()).is_some_and(|spec| spec.contacts.contains(&contact)))
}

fn compile_outputs(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    component: &RegionComponent,
    cell: &CellContract,
    use_: &CellUse,
    private_resource: &CellResourcePin,
    transfer: CellResourceTransfer,
    power_index: &PowerIndex,
    accepted: LayerExprId,
    transfer_when: LayerExprId,
    lifecycle: &[LayerLifecycleOccurrence],
    builder: &mut LayerBuilder<'_>,
) -> Result<Result<Box<[LayerOutput]>, (LayerWitness, LayerDeclineReason)>, String> {
    let mut outputs = Vec::new();
    let mut value_outputs = Vec::new();
    let phases = [
        (Some(use_.retraction.routes.base_changed), accepted),
        (use_.retraction.routes.far_changed, transfer_when),
        (Some(use_.retraction.routes.near_changed), accepted),
    ];
    for &port_index in &component.ports {
        builder.visit()?;
        let port = catalog.ports[port_index as usize];
        if port.direction != BoundaryDirection::Output {
            continue;
        }
        if let BoundaryPortKind::Power { link: link_index } = port.kind {
            let link = graph
                .power_links
                .get(link_index as usize)
                .ok_or("power output port references a missing source link")?;
            if cell.pins.iter().any(|pin| {
                matches!(pin, CellPin::Read(CellRead::Contribution(contribution))
                    if local_reset_output_link(graph, power_index, use_, *contribution) == Some(*link))
            }) {
                let input = builder.input(
                    LayerInputSource::LocalResetOutput {
                        observer: use_.reset,
                        link: *link,
                    },
                    LayerInputRole::AcceptanceRevalidation,
                )?;
                let emitted_when = builder.input_expr(input)?;
                value_outputs.push(LayerOutput {
                    port: port_index,
                    emitted_when,
                });
                continue;
            }
        }
        if let BoundaryPortKind::GuardState { test, .. } = port.kind {
            let Some(emitted_when) = compile_state_output(graph, use_.actor, private_resource, transfer, test, accepted, transfer_when, builder)?
            else {
                return Ok(Err((LayerWitness::Actor(use_.actor), LayerDeclineReason::BoundaryEffectUnsupported)));
            };
            value_outputs.push(LayerOutput {
                port: port_index,
                emitted_when,
            });
            continue;
        }
        let BoundaryPortKind::Callback { occurrence } = port.kind else {
            return Ok(Err((LayerWitness::Actor(use_.actor), LayerDeclineReason::BoundaryEffectUnsupported)));
        };
        let route = graph
            .routes
            .get(occurrence.route.index())
            .ok_or("callback port route is outside the graph")?;
        let Some(delivery) = route.deliveries.get(occurrence.ordinal as usize) else {
            return Err("callback port ordinal is outside the graph route".into());
        };
        // A cold row is omitted here only when its original occurrence was retained above.
        if lifecycle.iter().any(|row| row.occurrence == occurrence) {
            continue;
        }
        let is_immediate = phases.iter().any(|(route, _)| *route == Some(occurrence.route));
        if !is_immediate || !matches!(graph.guards.get(delivery.guard.index()), Some(Guard::Always)) || delivery.view != OperandView::Live {
            return Ok(Err((
                LayerWitness::Callback(occurrence),
                LayerDeclineReason::CallbackSemanticsUnsupported,
            )));
        }
        for (phase, (route, emitted_when)) in phases.iter().enumerate() {
            if *route == Some(occurrence.route) {
                outputs.push((
                    phase,
                    occurrence.ordinal,
                    LayerOutput {
                        port: port_index,
                        emitted_when: *emitted_when,
                    },
                ));
            }
        }
    }
    // Route IDs are storage indices, not event order; a reused route runs once per phase.
    outputs.sort_by_key(|&(phase, ordinal, _)| (phase, ordinal));
    let mut outputs: Vec<_> = outputs.into_iter().map(|(_, _, output)| output).collect();
    outputs.extend(value_outputs);
    Ok(Ok(outputs.into_boxed_slice()))
}

/// Lower an owned actor or private-token predicate at the post-pull cut without exposing intermediate state.
fn compile_state_output(
    graph: &CompileGraph,
    actor: PistonId,
    resource: &CellResourcePin,
    transfer: CellResourceTransfer,
    test: StateTest,
    accepted: LayerExprId,
    transfer_when: LayerExprId,
    builder: &mut LayerBuilder<'_>,
) -> Result<Option<LayerExprId>, String> {
    let state_is_owned = match test {
        StateTest::PistonExtended(piston) | StateTest::PistonHeadPresent(piston) | StateTest::PistonAvailability { piston, .. } => piston == actor,
        StateTest::ResourceEnabled(id) => id == resource.resource && id == transfer.resource && graph.resources.get(id.index()).is_some(),
        StateTest::ResourceAt { resource: id, contact } => {
            id == resource.resource && id == transfer.resource && graph.resources.get(id.index()).is_some_and(|spec| spec.contacts.contains(&contact))
        }
    };
    if !state_is_owned {
        return Ok(None);
    }
    let input = builder.input(LayerInputSource::State(test), LayerInputRole::AcceptanceRevalidation)?;
    let old_value = builder.input_expr(input)?;
    let value = match test {
        StateTest::PistonExtended(_) => old_value,
        StateTest::PistonHeadPresent(_) => {
            let not_accepted = builder.not(accepted)?;
            builder.all([old_value, not_accepted])?
        }
        StateTest::PistonAvailability {
            is: PistonAvailability::BasePending,
            ..
        } => builder.any([old_value, accepted])?,
        StateTest::PistonAvailability { .. } => {
            let not_accepted = builder.not(accepted)?;
            builder.all([old_value, not_accepted])?
        }
        StateTest::ResourceEnabled(_) => {
            let no_transfer = builder.not(transfer_when)?;
            builder.all([old_value, no_transfer])?
        }
        StateTest::ResourceAt { contact, .. } if contact == transfer.destination => builder.any([old_value, transfer_when])?,
        StateTest::ResourceAt { .. } => {
            let no_transfer = builder.not(transfer_when)?;
            builder.all([old_value, no_transfer])?
        }
    };
    Ok(Some(value))
}

fn find_port(catalog: &BoundaryCatalog, owner: BoundaryOwner, direction: BoundaryDirection, kind: BoundaryPortKind) -> Option<usize> {
    catalog
        .ports
        .iter()
        .position(|port| port.owner == owner && port.direction == direction && port.kind == kind)
}

fn first_witness(graph: &CompileGraph, catalog: &BoundaryCatalog, component: &RegionComponent) -> LayerWitness {
    for &port in &component.ports {
        match catalog.ports[port as usize].kind {
            BoundaryPortKind::Power { link } => {
                if let Some(link) = graph.power_links.get(link as usize) {
                    return LayerWitness::Power(*link);
                }
            }
            BoundaryPortKind::Callback { occurrence } => return LayerWitness::Callback(occurrence),
            BoundaryPortKind::WireRelation { relation } => {
                if let Some(relation) = graph.wire_relations.get(relation as usize) {
                    return LayerWitness::Wire(relation.from);
                }
            }
            BoundaryPortKind::CapturedWire { wire, .. } | BoundaryPortKind::WireNeighborhood { wire, .. } => return LayerWitness::Wire(wire),
            _ => {}
        }
    }
    component
        .cells
        .first()
        .and_then(|cell| catalog.cells.get(*cell as usize))
        .and_then(|cell| cell.uses.first())
        .map(|use_| LayerWitness::Actor(use_.actor))
        .unwrap_or_else(|| {
            let wire = component
                .corridors
                .first()
                .and_then(|corridor| catalog.corridors.get(*corridor as usize))
                .and_then(|corridor| corridor.wires.first())
                .copied()
                .unwrap_or(NodeId::new(0));
            LayerWitness::Wire(wire)
        })
}

fn decline(component: &RegionComponent, witness: LayerWitness, reason: LayerDeclineReason) -> LayerDecline {
    LayerDecline {
        cells: component.cells.clone(),
        corridors: component.corridors.clone(),
        witness,
        reason,
    }
}

struct LayerBuilder<'a> {
    /// This monitor cancels pin and expression walks within a large component.
    monitor: &'a TaskMonitor,
    /// These source bindings are shared by request and acceptance expressions.
    inputs: Vec<LayerInput>,
    /// These topologically appended expressions are the region-wide Boolean arena.
    expressions: Vec<LayerExpr>,
    /// This map interns structurally identical expressions into stable roots.
    interned: FxHashMap<LayerExpr, LayerExprId>,
    /// This map gives one local input ID to each exact source/phase pair.
    input_ids: FxHashMap<(LayerInputSource, LayerInputRole), LayerInputId>,
    /// This counts retained expression/input payload and construction visits.
    budget: LayerBudget,
}

impl<'a> LayerBuilder<'a> {
    fn new(monitor: &'a TaskMonitor) -> Result<Self, String> {
        let mut builder = Self {
            monitor,
            inputs: Vec::new(),
            expressions: Vec::new(),
            interned: FxHashMap::default(),
            input_ids: FxHashMap::default(),
            budget: LayerBudget { bytes: 0, work: 0 },
        };
        // The two constants are stable roots shared by all subsequent local equations.
        builder.constant(false)?;
        builder.constant(true)?;
        Ok(builder)
    }

    fn visit(&mut self) -> Result<(), String> {
        self.budget.visit(self.monitor)
    }

    fn input(&mut self, source: LayerInputSource, role: LayerInputRole) -> Result<LayerInputId, String> {
        self.visit()?;
        let key = (source, role);
        if let Some(&id) = self.input_ids.get(&key) {
            return Ok(id);
        }
        self.budget.retain(std::mem::size_of::<LayerInput>())?;
        let id = LayerInputId(u32::try_from(self.inputs.len()).map_err(|_| "too many region inputs")?);
        self.inputs.push(LayerInput { source, role });
        self.input_ids.insert(key, id);
        Ok(id)
    }

    fn input_expr(&mut self, input: LayerInputId) -> Result<LayerExprId, String> {
        self.intern(LayerExpr::Input(input))
    }

    fn constant(&mut self, value: bool) -> Result<LayerExprId, String> {
        self.intern(LayerExpr::Constant(value))
    }

    fn not(&mut self, value: LayerExprId) -> Result<LayerExprId, String> {
        match self.expression(value)?.clone() {
            LayerExpr::Constant(value) => self.constant(!value),
            LayerExpr::Not(inner) => Ok(inner),
            _ => self.intern(LayerExpr::Not(value)),
        }
    }

    fn all(&mut self, values: impl IntoIterator<Item = LayerExprId>) -> Result<LayerExprId, String> {
        self.combine(values, true)
    }

    fn any(&mut self, values: impl IntoIterator<Item = LayerExprId>) -> Result<LayerExprId, String> {
        self.combine(values, false)
    }

    fn combine(&mut self, values: impl IntoIterator<Item = LayerExprId>, all: bool) -> Result<LayerExprId, String> {
        let identity = if all { true } else { false };
        let absorbing = !identity;
        let mut operands = Vec::new();
        let mut pending: Vec<_> = values.into_iter().collect();
        let mut seen = rustc_hash::FxHashSet::default();
        while let Some(value) = pending.pop() {
            self.visit()?;
            if !seen.insert(value) {
                continue;
            }
            if self.interned.get(&LayerExpr::Not(value)).is_some_and(|opposite| seen.contains(opposite)) {
                return self.constant(absorbing);
            }
            match self.expression(value)?.clone() {
                LayerExpr::Constant(value) if value == absorbing => return self.constant(absorbing),
                LayerExpr::Constant(value) if value == identity => {}
                LayerExpr::Not(child) if seen.contains(&child) => return self.constant(absorbing),
                _ => operands.push(value),
            }
        }
        operands.sort_unstable();
        if operands.is_empty() {
            return self.constant(identity);
        }
        if operands.len() == 1 {
            return Ok(operands[0]);
        }
        let payload = operands
            .len()
            .checked_mul(std::mem::size_of::<LayerExprId>())
            .ok_or("layer expression operand size overflow")?;
        self.budget.retain(payload)?;
        let operands = operands.into_boxed_slice();
        self.intern(if all { LayerExpr::All(operands) } else { LayerExpr::Any(operands) })
    }

    fn gate(&mut self, kind: CellGateKind, values: Vec<LayerExprId>) -> Result<LayerExprId, String> {
        match kind {
            CellGateKind::Buffer if values.len() == 1 => Ok(values[0]),
            CellGateKind::Invert if values.len() == 1 => self.not(values[0]),
            CellGateKind::All => self.all(values),
            CellGateKind::Any => self.any(values),
            _ => Err("cell contract has a malformed unary gate".into()),
        }
    }

    fn expression(&self, value: LayerExprId) -> Result<&LayerExpr, String> {
        self.expressions
            .get(value.0 as usize)
            .ok_or_else(|| "layer expression references a missing root".into())
    }

    fn intern(&mut self, expression: LayerExpr) -> Result<LayerExprId, String> {
        if let Some(&id) = self.interned.get(&expression) {
            return Ok(id);
        }
        let operand_bytes = match &expression {
            LayerExpr::All(values) | LayerExpr::Any(values) => values.len().checked_mul(std::mem::size_of::<LayerExprId>()),
            _ => Some(0),
        }
        .ok_or("layer expression size overflow")?;
        self.budget.retain(
            std::mem::size_of::<LayerExpr>()
                .checked_add(operand_bytes)
                .ok_or("layer expression byte size overflow")?,
        )?;
        let id = LayerExprId(u32::try_from(self.expressions.len()).map_err(|_| "too many region expressions")?);
        self.expressions.push(expression.clone());
        self.interned.insert(expression, id);
        Ok(id)
    }
}

struct LayerBudget {
    /// This is the retained expression/input payload charged before it is appended.
    bytes: usize,
    /// This counts pin, input and Boolean-operation visits.
    work: usize,
}

impl LayerBudget {
    fn retain(&mut self, bytes: usize) -> Result<(), String> {
        self.bytes = self.bytes.checked_add(bytes).ok_or("layer retained byte counter overflow")?;
        if self.bytes > MAX_LAYER_BYTES {
            return Err("instant layer retained-table budget exceeded".into());
        }
        Ok(())
    }

    fn visit(&mut self, monitor: &TaskMonitor) -> Result<(), String> {
        check_cancelled(monitor)?;
        self.work = self.work.checked_add(1).ok_or("layer work counter overflow")?;
        if self.work > MAX_LAYER_WORK {
            return Err("instant layer construction-work budget exceeded".into());
        }
        Ok(())
    }
}

fn check_cancelled(monitor: &TaskMonitor) -> Result<(), String> {
    if monitor.cancelled() {
        Err("cancelled while compiling instant causal layers".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
