//! Compose settled value equations; physical ordering belongs only to the boundary adapter.
use super::*;

const MAX_RETAINED_FEEDBACK_OWNERS: usize = 64;
const MAX_FEEDBACK_OWNER_CLOSURE_WORK: usize = 2 * 1024 * 1024;

fn diagnostic_position(position: Option<mchprs_blocks::BlockPos>) -> String {
    position.map_or_else(|| "position unavailable".into(), |pos| format!("({}, {}, {})", pos.x, pos.y, pos.z))
}

fn computation_output_error(position: Option<mchprs_blocks::BlockPos>, error: ConstructionError) -> String {
    let reason = match error {
        ConstructionError::BudgetExceeded => "equation construction exceeded its budget".to_owned(),
        error => format!("{error:?}"),
    };
    format!("equation output at {}: {reason}", diagnostic_position(position))
}

pub(crate) fn compile_device(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<SettledCircuit, String> {
    let plan = compile_settled(graph, monitor)?;
    compile_device_with_plan(graph, plan, monitor)
}

fn compile_device_with_plan(graph: &CompileGraph, plan: SettledCircuit, monitor: &TaskMonitor) -> Result<SettledCircuit, String> {
    if graph.plans.bud_memory_reads.is_empty() {
        if let Some(computation) = try_computation_frame(graph, monitor) {
            return Ok(computation);
        }
        return Ok(plan);
    }
    let Ok(timing) = super::timing::infer(graph, monitor) else {
        return Ok(plan);
    };
    let mut targets = Vec::new();
    for output in &plan.outputs {
        let SettledTarget::Power(target) = output.target else {
            continue;
        };
        match target {
            PowerTarget::Input { node, .. } if timing.nodes.contains(&node) => {}
            PowerTarget::Input { node, port: InputPort::Main }
                if matches!(
                    graph.nodes[node.index()].kind,
                    CircuitNodeKind::Electrical(NodeType::Repeater { .. } | NodeType::Lamp)
                ) =>
            {
                targets.push(target)
            }
            // An unsupported public consumer must not disappear from the device boundary.
            _ => return Ok(plan),
        }
    }
    if targets.is_empty() {
        return Ok(plan);
    }
    let mut compact = compile_public_boundary(graph, &targets, &graph.plans.bud_memory_reads, monitor)?;
    compact.timing = Some(timing);
    Ok(compact)
}

pub(crate) fn compile_devices(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<(SettledCircuit, Vec<ScopedSettledDevice>), String> {
    let plan = compile_settled(graph, monitor)?;
    let devices = compile_scoped_devices_with_plan(graph, &plan, monitor)?;
    let legacy = compile_device_with_plan(graph, plan, monitor)?;
    Ok((legacy, devices))
}

pub(crate) fn compile_scoped_devices(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<Vec<ScopedSettledDevice>, String> {
    let plan = compile_settled(graph, monitor)?;
    compile_scoped_devices_with_plan(graph, &plan, monitor)
}

fn compile_scoped_devices_with_plan(
    graph: &CompileGraph,
    settled_plan: &SettledCircuit,
    monitor: &TaskMonitor,
) -> Result<Vec<ScopedSettledDevice>, String> {
    let partition = super::islands::partition(graph, monitor)?;
    let clocks = super::timing::candidate_clocks_in_islands(graph, &partition, monitor)?;
    let mut clocks_by_island = vec![Vec::<PistonId>::new(); partition.islands.len()];
    for clock in clocks {
        let node = graph
            .pistons
            .get(clock.index())
            .ok_or("logical clock candidate references a missing piston")?
            .node;
        let island = *partition
            .node_island
            .get(node.index())
            .ok_or("logical clock candidate references a missing component")? as usize;
        clocks_by_island[island].push(clock);
    }

    let physical_ownership = if clocks_by_island.iter().any(|clocks| clocks.len() == 1) {
        let all_actors = (0..graph.pistons.len()).map(|index| PistonId::new(index as u32)).collect::<Vec<_>>();
        Some(
            crate::redpiler::analysis::instant::ownership::close_seeded_ownership(graph, &[], &all_actors, monitor)
                .map_err(|error| format!("scoped native ownership closure: {error}"))?,
        )
    } else {
        None
    };

    let mut devices = Vec::new();
    let mut declined = 0usize;
    let mut examples = Vec::new();
    for (index, island) in partition.islands.iter().enumerate() {
        if monitor.cancelled() {
            return Err("cancelled while compiling scoped logical devices".into());
        }
        if clocks_by_island[index].len() != 1 {
            if clocks_by_island[index].len() > 1 {
                declined += 1;
                if examples.len() < 24 {
                    let clock = clocks_by_island[index][0];
                    let node = graph.pistons[clock.index()].node;
                    tracing::debug!(
                        island = island.id,
                        ?clock,
                        count = clocks_by_island[index].len(),
                        "scoped logical candidate declined: multiple clocks"
                    );
                    examples.push(format!(
                        "Instant circuit candidate at {}: {} clocks share an island; exactly one is required.",
                        diagnostic_position(graph.nodes[node.index()].position),
                        clocks_by_island[index].len()
                    ));
                }
            }
            #[cfg(test)]
            if island.nodes.len() >= 1_000 {
                eprintln!(
                    "scoped logical device decline: island {} has {} candidate clocks",
                    island.id,
                    clocks_by_island[index].len()
                );
            }
            continue;
        }
        let Some(physical_ownership) = physical_ownership.as_ref() else {
            return Err("scoped native ownership closure was not prepared".into());
        };
        let mut reason = String::new();
        let Some(device) = try_scoped_device_for_island(
            graph,
            island,
            clocks_by_island[index][0],
            settled_plan,
            physical_ownership,
            monitor,
            &mut reason,
        ) else {
            declined += 1;
            if examples.len() < 24 {
                let clock = clocks_by_island[index][0];
                let node = graph.pistons[clock.index()].node;
                tracing::debug!(island = island.id, ?clock, %reason, "scoped logical candidate declined");
                examples.push(format!(
                    "Instant circuit candidate at {}: {}.",
                    diagnostic_position(graph.nodes[node.index()].position),
                    if reason.is_empty() {
                        "the required safety proof was unavailable"
                    } else {
                        &reason
                    }
                ));
            }
            #[cfg(test)]
            eprintln!(
                "scoped logical device decline: island {} clock {:?} failed a scoped admission gate",
                island.id, clocks_by_island[index][0]
            );
            continue;
        };
        devices.push(device);
    }
    if declined > 0 {
        monitor.record_warning(format!(
            "Fast devices: {} prepared, {declined} skipped; showing {} reasons. Prepared does not mean running.",
            devices.len(),
            examples.len()
        ));
        for example in examples {
            monitor.record_warning(example);
        }
    }
    Ok(devices)
}

fn try_computation_frame(graph: &CompileGraph, monitor: &TaskMonitor) -> Option<SettledCircuit> {
    let timing = super::timing::infer(graph, monitor).ok()?;
    try_computation_frame_with_timing(graph, None, timing, monitor).map(|(circuit, _)| circuit)
}

fn try_scoped_device_for_island(
    graph: &CompileGraph,
    island: &LogicalIsland,
    clock: PistonId,
    settled_plan: &SettledCircuit,
    physical_ownership: &crate::redpiler::analysis::instant::ownership::OwnershipClosure,
    monitor: &TaskMonitor,
    reason: &mut String,
) -> Option<ScopedSettledDevice> {
    let timing = match super::timing::infer_for_clock_in_island(graph, clock, island, monitor) {
        Ok(timing) => timing,
        Err(error) => {
            *reason = format!("timing inference: {error}");
            #[cfg(test)]
            eprintln!(
                "scoped logical device decline: timing inference for island {} clock {:?}: {error}",
                island.id, clock
            );
            return None;
        }
    };
    let owns_node = |node: NodeId| island.nodes.binary_search(&node).is_ok();
    if timing.nodes.iter().any(|node| !owns_node(*node)) || timing.actor_specs.iter().any(|actor| !owns_node(actor.node)) {
        *reason = "timing actors or nodes escape the island".into();
        #[cfg(test)]
        eprintln!("scoped logical device decline: island {} timing actors/nodes escape island", island.id);
        return None;
    }
    let local_bud_reads = graph
        .plans
        .bud_memory_reads
        .iter()
        .copied()
        .filter(|read| island.resources.binary_search(&read.resource).is_ok())
        .collect::<Vec<_>>();
    #[cfg(test)]
    if island.nodes.len() >= 1_000 {
        eprintln!(
            "scoped logical device: island {} local BUD read count {}",
            island.id,
            local_bud_reads.len()
        );
    }
    if local_bud_reads.is_empty() {
        let (circuit, expanded_components) = match try_computation_frame_with_timing(graph, Some(island), timing, monitor) {
            Some(result) => result,
            None => {
                *reason = "computation-frame compilation did not certify a frame".into();
                #[cfg(test)]
                eprintln!("scoped logical device decline: computation-frame compile failed for island {}", island.id);
                return None;
            }
        };
        let scope = match scoped_computation_device_scope(graph, island, &circuit, &expanded_components, physical_ownership, monitor, reason) {
            Some(scope) => scope,
            None => {
                if reason.is_empty() {
                    *reason = "ownership/scope proof unavailable".into();
                }
                #[cfg(test)]
                eprintln!("scoped logical device decline: ownership/scope proof failed for island {}", island.id);
                return None;
            }
        };
        if !scoped_frame_power_sinks_supported(graph, &scope, &circuit, &expanded_components) {
            *reason = "frame power sink is unsupported".into();
            #[cfg(test)]
            eprintln!("scoped logical device decline: unsupported frame power sink in island {}", island.id);
            return None;
        }
        if !scoped_frame_outputs_are_closed(graph, &scope, &circuit) {
            *reason = "frame outputs are not closed".into();
            #[cfg(test)]
            eprintln!("scoped logical device decline: frame outputs are not closed in island {}", island.id);
            return None;
        }
        return Some(ScopedSettledDevice { circuit, scope });
    }
    if !matches!(
        timing.actor_specs.iter().find(|actor| actor.piston == timing.clock)?.lane,
        PistonLane::Empty
    ) {
        *reason = "BUD reads require an empty clock lane".into();
        #[cfg(test)]
        eprintln!(
            "scoped logical device decline: island {} has BUD reads and non-empty clock lane",
            island.id
        );
        return None;
    }

    let mut targets = Vec::new();
    for output in &settled_plan.outputs {
        let target = match output.target {
            SettledTarget::Power(target @ PowerTarget::Input { node, .. }) => {
                if !owns_node(node) || timing.nodes.contains(&node) {
                    continue;
                }
                target
            }
            // A live conductor output is observable boundary state that this
            // scoped public-input adapter does not currently publish.
            SettledTarget::Power(PowerTarget::Conductor { site, .. }) => {
                if island.sites.binary_search(&site).is_ok() {
                    return None;
                }
                continue;
            }
            _ => continue,
        };
        match target {
            PowerTarget::Input { node, port: InputPort::Main }
                if matches!(
                    graph.nodes[node.index()].kind,
                    CircuitNodeKind::Electrical(NodeType::Repeater { .. } | NodeType::Lamp)
                ) =>
            {
                targets.push(target)
            }
            // Unsupported observable consumers remain on ordinary Graph.
            _ => return None,
        }
    }
    targets.sort_unstable();
    targets.dedup();
    if targets.is_empty() {
        return None;
    }
    let mut compact = compile_public_boundary(graph, &targets, &local_bud_reads, monitor).ok()?;
    compact.timing = Some(timing);
    if compact.outputs.iter().any(|output| match output.target {
        #[cfg(test)]
        SettledTarget::ComponentOutput(_) => true,
        SettledTarget::ResetExcludedPower { .. } => true,
        SettledTarget::Power(PowerTarget::Input { node, .. }) => !owns_node(node),
        SettledTarget::Power(PowerTarget::Conductor { site, .. }) => island.sites.binary_search(&site).is_err(),
        SettledTarget::ResourceState { resource, .. } => island.resources.binary_search(&resource).is_err(),
        SettledTarget::Register(read) => island.resources.binary_search(&read.resource).is_err(),
    }) {
        return None;
    }
    let scope = scoped_device_scope(graph, island, &compact)?;
    Some(ScopedSettledDevice { circuit: compact, scope })
}

/// A scoped frame suppresses ordinary callbacks from every owned source. Keep
/// admission conservative: every active electrical receiver must be represented
/// by a published frame output or by the certified clock/cell boundary.
fn scoped_frame_power_sinks_supported(
    graph: &CompileGraph,
    scope: &SettledDeviceScope,
    circuit: &SettledCircuit,
    expanded_components: &FxHashSet<NodeId>,
) -> bool {
    let owns_node = |node: NodeId| scope.owned_nodes.binary_search(&node).is_ok();
    for &node in scope.owned_nodes.iter() {
        let Some(draft) = graph.nodes.get(node.index()) else {
            return false;
        };
        if scoped_frame_has_unmodeled_native_effect(&draft.kind) {
            #[cfg(test)]
            eprintln!(
                "scoped logical frame sink declined: unmodeled native effect node={node:?} position={:?} kind={:?}",
                draft.position, draft.kind
            );
            return false;
        }
    }
    let timing_nodes = circuit
        .timing
        .as_ref()
        .map(|timing| timing.nodes.iter().copied().collect::<FxHashSet<_>>())
        .unwrap_or_default();
    let mut covered_power = circuit
        .outputs
        .iter()
        .filter_map(|output| match output.target {
            SettledTarget::Power(target) => Some(target),
            SettledTarget::ResetExcludedPower { node, port, .. } => Some(PowerTarget::Input { node, port }),
            _ => None,
        })
        .collect::<FxHashSet<_>>();
    for port in circuit.retained_effects.iter().filter(|port| port.direction == BoundaryDirection::Output) {
        let BoundaryPortKind::Power { link } = port.kind else {
            continue;
        };
        let Some(link) = graph.power_links.get(link as usize) else {
            return false;
        };
        covered_power.insert(link.to);
    }
    let retained_actors = circuit.retained_cells.iter().map(|use_| use_.actor).collect::<FxHashSet<_>>();
    let catalog_actors = graph
        .plans
        .instant
        .boundaries
        .as_ref()
        .into_iter()
        .flat_map(|catalog| catalog.cells.iter())
        .flat_map(|cell| cell.uses.iter().map(|use_| use_.actor))
        .collect::<FxHashSet<_>>();
    let catalog_resets = graph
        .plans
        .instant
        .boundaries
        .as_ref()
        .into_iter()
        .flat_map(|catalog| catalog.cells.iter())
        .flat_map(|cell| cell.uses.iter().filter_map(|use_| use_.reset.observer()))
        .collect::<FxHashSet<_>>();
    let mut all_supported = true;
    #[cfg(test)]
    let mut rejected = Vec::<String>::new();
    for link in &graph.power_links {
        let PowerTarget::Input { node, .. } = link.to else {
            continue;
        };
        if !owns_node(node) || timing_nodes.contains(&node) {
            continue;
        }
        let Some(draft) = graph.nodes.get(node.index()) else {
            return false;
        };
        let value_component_certified =
            expanded_components.contains(&node) && expanded_component_routes_supported(graph, scope, node, &catalog_actors, &catalog_resets);
        let actor_certified = matches!(
            &draft.kind,
            CircuitNodeKind::Piston(actor) if retained_actors.contains(actor) || catalog_actors.contains(actor)
        ) || matches!(&draft.kind, CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. } if catalog_resets.contains(&node))
            || value_component_certified;
        let invariant_comparator_side = matches!(
            link.to,
            PowerTarget::Input {
                node,
                port: InputPort::Side
            } if comparator_side_input_is_inert(graph, node)
        );
        if !scoped_frame_sink_kind_supported(&draft.kind, covered_power.contains(&link.to), actor_certified, invariant_comparator_side) {
            all_supported = false;
            #[cfg(test)]
            {
                let reason = format!("target={:?}, kind={:?}, position={:?}", link.to, draft.kind, draft.position);
                if !rejected.contains(&reason) {
                    rejected.push(reason);
                }
            }
        }
    }
    #[cfg(test)]
    for reason in rejected {
        eprintln!("scoped logical frame sink declined: {reason}");
    }
    all_supported
}

fn scoped_frame_sink_kind_supported(kind: &CircuitNodeKind, power_is_covered: bool, actor_certified: bool, invariant_comparator_side: bool) -> bool {
    match kind {
        // Wires are combinational transport; their downstream semantic
        // receivers are checked separately by their own incoming links.
        CircuitNodeKind::Wire(_) | CircuitNodeKind::Electrical(NodeType::Wire) => true,
        CircuitNodeKind::Piston(_) => power_is_covered || actor_certified,
        CircuitNodeKind::Electrical(NodeType::Repeater { .. }) => power_is_covered || actor_certified,
        CircuitNodeKind::Electrical(NodeType::CommandBlock { .. }) => power_is_covered,
        CircuitNodeKind::Electrical(NodeType::Lamp) => false,
        CircuitNodeKind::Electrical(NodeType::Comparator { .. }) => actor_certified || invariant_comparator_side,
        // Torches, observers, note blocks, bulbs, hoppers, trapdoors, and
        // control sources retain native behavior this frame does not own.
        CircuitNodeKind::Electrical(_) | CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. } | CircuitNodeKind::Contact(_) => {
            actor_certified
        }
    }
}

fn scoped_frame_has_unmodeled_native_effect(kind: &CircuitNodeKind) -> bool {
    matches!(
        kind,
        CircuitNodeKind::Electrical(NodeType::Lamp | NodeType::CopperBulb | NodeType::Hopper | NodeType::Trapdoor | NodeType::NoteBlock { .. })
    )
}

/// Admit only comparator inputs whose every possible Side strength leaves the
/// compiled comparator state unchanged. That prevents its native updater from
/// scheduling the suppressed output route after a Side-only change.
fn comparator_side_input_is_inert(graph: &CompileGraph, node: NodeId) -> bool {
    let Some(CircuitNodeDraft {
        kind: CircuitNodeKind::Electrical(NodeType::Comparator { mode, .. }),
        state: ComponentState::Electrical(state),
        ..
    }) = graph.nodes.get(node.index())
    else {
        return false;
    };
    let Some(plan) = graph.comparator_inputs.iter().find(|plan| plan.node == node) else {
        return false;
    };
    comparator_side_state_is_invariant(
        *mode,
        plan.direct_override,
        plan.far_override.is_some(),
        state.output_strength,
        state.powered,
        state.pending_tick,
    )
}

fn comparator_side_state_is_invariant(
    mode: mchprs_blocks::blocks::ComparatorMode,
    direct_override: Option<ComparatorOverride>,
    has_far_override: bool,
    output_strength: u8,
    powered: bool,
    pending_tick: bool,
) -> bool {
    if pending_tick || has_far_override {
        return false;
    }
    let Some(ComparatorOverride::Literal(input)) = direct_override else {
        return false;
    };
    let expected = (
        crate::redpiler::power_rules::comparator_output_strength(input, 0, mode),
        crate::redpiler::power_rules::comparator_is_powered(input, 0, mode),
    );
    expected == (output_strength, powered)
        && (0..=15).all(|side| {
            (
                crate::redpiler::power_rules::comparator_output_strength(input, side, mode),
                crate::redpiler::power_rules::comparator_is_powered(input, side, mode),
            ) == expected
        })
}

/// Expanded logical value producers may suppress their native callbacks only
/// when every route occurrence ends at a wire or a certified cell/reset actor.
fn expanded_component_routes_supported(
    graph: &CompileGraph,
    scope: &SettledDeviceScope,
    node: NodeId,
    catalog_actors: &FxHashSet<PistonId>,
    catalog_resets: &FxHashSet<NodeId>,
) -> bool {
    let owns_node = |node: NodeId| scope.owned_nodes.binary_search(&node).is_ok();
    if !owns_node(node) {
        return false;
    }
    let Some(route_id) = graph.nodes[node.index()].output_route else {
        return true;
    };
    let Some(route) = graph.routes.get(route_id.index()) else {
        return false;
    };
    route.deliveries.iter().all(|delivery| {
        let Some(site) = graph.sites.get(delivery.target.index()) else {
            return false;
        };
        let receiver = delivery.receiver.or_else(|| match (site.source, delivery.kind) {
            (SiteSource::Component(target), DeliveryKind::Update { .. }) => Some(CallbackReceiver::Component(target)),
            _ => None,
        });
        match receiver {
            None => true,
            Some(CallbackReceiver::Component(target)) if target == node => true,
            Some(CallbackReceiver::Component(target)) if owns_node(target) => match graph.nodes[target.index()].kind {
                CircuitNodeKind::Wire(_) | CircuitNodeKind::Electrical(NodeType::Wire) => true,
                CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. } => catalog_resets.contains(&target),
                _ => false,
            },
            Some(CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor)) => catalog_actors.contains(&actor),
            _ => false,
        }
    })
}

fn scoped_device_scope(graph: &CompileGraph, island: &LogicalIsland, circuit: &SettledCircuit) -> Option<SettledDeviceScope> {
    scoped_device_scope_with_masks(
        graph,
        island,
        circuit,
        island.nodes.iter().copied().collect(),
        FxHashSet::default(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Box::new([]),
        crate::redpiler::program_graph::SettledClockDriver::Logical,
    )
}

/// Frame publication replaces value equations, so it cannot replay an owned
/// source's callback into an outside native component. Such crossings are
/// valid only when the exact occurrence is already owned by a retained cell.
fn scoped_frame_outputs_are_closed(graph: &CompileGraph, scope: &SettledDeviceScope, circuit: &SettledCircuit) -> bool {
    let retained_callbacks = circuit
        .retained_effects
        .iter()
        .filter_map(|port| match (port.direction, port.kind) {
            (BoundaryDirection::Output, BoundaryPortKind::Callback { occurrence }) => Some(occurrence),
            _ => None,
        })
        .collect::<FxHashSet<_>>();
    for &route_id in scope.owned_source_routes.iter() {
        let Some(route) = graph.routes.get(route_id.index()) else {
            return false;
        };
        for (ordinal, delivery) in route.deliveries.iter().enumerate() {
            let Some(target) = resolved_delivery_node(graph, delivery) else {
                continue;
            };
            if scope.owned_nodes.binary_search(&target).is_ok() {
                continue;
            }
            let Ok(ordinal) = u32::try_from(ordinal) else {
                return false;
            };
            if !retained_callbacks.contains(&RouteOccurrence { route: route_id, ordinal }) {
                #[cfg(test)]
                eprintln!(
                    "logical frame decline: owned callback {route_id:?}/{ordinal} from {:?} at {:?} crosses to {:?} at {:?}",
                    route.source,
                    graph.nodes.get(route.source.index()).and_then(|node| node.position),
                    target,
                    graph.nodes.get(target.index()).and_then(|node| node.position)
                );
                return false;
            }
        }
    }
    true
}

fn scoped_computation_device_scope(
    graph: &CompileGraph,
    island: &LogicalIsland,
    circuit: &SettledCircuit,
    expanded_components: &FxHashSet<NodeId>,
    physical_ownership: &crate::redpiler::analysis::instant::ownership::OwnershipClosure,
    monitor: &TaskMonitor,
    reason: &mut String,
) -> Option<SettledDeviceScope> {
    let Some(timing) = circuit.timing.as_ref() else {
        #[cfg(test)]
        eprintln!("logical frame decline: scoped circuit is missing timing");
        return None;
    };
    let Some(retained_cells) = super::native_inputs::retained_cell_boundary(graph, circuit, physical_ownership, timing) else {
        *reason = "retained output-cell boundary proof unavailable".into();
        if let Some((resource, source)) = unretained_resource_output(circuit, &FxHashSet::default()) {
            tracing::debug!(
                island = island.id,
                ?resource,
                ?source,
                reason = "resource output has no complete retained-cell boundary proof",
                "logical device declined"
            );
        } else {
            tracing::debug!(
                island = island.id,
                reason = "retained output-cell boundary is unavailable",
                "logical device declined"
            );
        }
        #[cfg(test)]
        if let Some((resource, source)) = unretained_resource_output(circuit, &FxHashSet::default()) {
            eprintln!("logical frame decline: resource {resource:?} at {source:?} has no complete retained-cell boundary proof");
        }
        return None;
    };
    let owned_state = match computation_owned_state(circuit, graph, &retained_cells, island) {
        Some(state) => state,
        None => {
            *reason = "computation state ownership proof unavailable".into();
            if let Some((resource, source)) = unretained_resource_output(circuit, &retained_cells.resources) {
                tracing::debug!(
                    island = island.id,
                    ?resource,
                    ?source,
                    reason = "resource output owner group is not completely retained in the island",
                    "logical device declined"
                );
            } else {
                tracing::debug!(
                    island = island.id,
                    reason = "computation state ownership is invalid",
                    "logical device declined"
                );
            }
            return None;
        }
    };
    let mut native_nodes = FxHashSet::default();
    let mut trace_targets = Vec::new();
    let mut trace_source_sites = Vec::new();
    let mut trace_source_states = Vec::new();
    let mut equation_clock_sites = FxHashSet::default();
    let feedback_sources = computation_feedback_sources(circuit);
    for input in &circuit.program.inputs {
        match input.source {
            LayerInputSource::SourceAbove { site, .. } => {
                let Some(source_site) = graph.sites.get(site.index()) else {
                    #[cfg(test)]
                    eprintln!("logical frame decline: source input references missing site {site:?}");
                    return None;
                };
                let site_source = source_site.source;
                if let SiteSource::Component(node) = site_source
                    && node != timing.trigger
                    && timing.node_specs.iter().any(|spec| {
                        spec.node == node && matches!(spec.kind, ClockNodeKind::Torch | ClockNodeKind::Observer | ClockNodeKind::BuiltinReset)
                    })
                {
                    equation_clock_sites.insert(site);
                }
                if !retained_cells.contains_source(site_source)
                    && collect_native_source_node(graph, site_source, timing, &mut native_nodes, &mut trace_targets).is_none()
                {
                    #[cfg(test)]
                    eprintln!("native input decline: equation source site {site:?} has unsupported source {site_source:?}");
                    return None;
                }
            }
            LayerInputSource::WireDot { node } => {
                if !matches!(graph.nodes.get(node.index())?.kind, CircuitNodeKind::Wire(_)) {
                    #[cfg(test)]
                    eprintln!("logical frame decline: wire-dot input references non-wire node {node:?}");
                    return None;
                }
                native_nodes.insert(node);
            }
            LayerInputSource::State(test) | LayerInputSource::BoundaryState { test, .. } if timing_source_state(timing, test) => {}
            LayerInputSource::State(StateTest::ResourceAt { resource, contact })
                if retained_cells.resources.contains(&resource)
                    && graph
                        .resources
                        .get(resource.index())
                        .is_some_and(|resource| resource.contacts.contains(&contact)) =>
            {
                // This exact state leaf is sampled from the complete certified
                // Graph-owned feedback cell group at the accepted native clock action.
            }
            LayerInputSource::State(test) if computation_feedback_input_supported(test, &feedback_sources) => {
                // The settled evaluator feeds back only the exact ResourceAt
                // source represented by this frame's ResourceState output.
            }
            // Computation-frame runtime has no native ownership proof for the other layer boundaries.
            unsupported => {
                *reason = format!("unsupported equation input {unsupported:?}");
                #[cfg(test)]
                eprintln!("logical frame decline: unsupported equation input {unsupported:?}");
                return None;
            }
        }
    }

    let lamps = island
        .nodes
        .iter()
        .copied()
        .filter(|node| matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Electrical(NodeType::Lamp)))
        .collect::<Vec<_>>();
    for &lamp in &lamps {
        trace_targets.push(PowerTarget::Input {
            node: lamp,
            port: InputPort::Main,
        });
    }
    if !trace_targets.is_empty() {
        let trace = match super::trace_native_power_targets(graph, &trace_targets, monitor) {
            Ok(trace) => trace,
            Err(error) => {
                *reason = format!("native power source trace failed: {error}");
                #[cfg(test)]
                eprintln!("native input decline: lamp/wire source trace failed: {error}");
                return None;
            }
        };
        if trace.source_sites.iter().any(|site| site.index() >= graph.sites.len()) {
            #[cfg(test)]
            eprintln!("native input decline: source trace returned an invalid site");
            return None;
        }
        trace_source_sites.extend(trace.source_sites.iter().copied());
        for site in trace.source_sites {
            let Some(source_site) = graph.sites.get(site.index()) else {
                #[cfg(test)]
                eprintln!("native input decline: source trace returned missing site {site:?}");
                return None;
            };
            let source = source_site.source;
            if !retained_cells.contains_source(source)
                && collect_native_source_node(graph, source, timing, &mut native_nodes, &mut Vec::new()).is_none()
            {
                #[cfg(test)]
                eprintln!("native input decline: traced site {site:?} has unsupported source {source:?}");
                return None;
            }
        }
        for input in trace.inputs {
            match input {
                LayerInputSource::SourceAbove { site, .. } => {
                    trace_source_sites.push(site);
                    let Some(source_site) = graph.sites.get(site.index()) else {
                        #[cfg(test)]
                        eprintln!("native input decline: traced input references missing site {site:?}");
                        return None;
                    };
                    let source = source_site.source;
                    if !retained_cells.contains_source(source)
                        && collect_native_source_node(graph, source, timing, &mut native_nodes, &mut Vec::new()).is_none()
                    {
                        #[cfg(test)]
                        eprintln!("native input decline: traced input site {site:?} has unsupported source {source:?}");
                        return None;
                    }
                }
                LayerInputSource::WireDot { node } if matches!(graph.nodes.get(node.index())?.kind, CircuitNodeKind::Wire(_)) => {
                    native_nodes.insert(node);
                }
                LayerInputSource::State(test) | LayerInputSource::BoundaryState { test, .. } => trace_source_states.push(test),
                unsupported => {
                    *reason = format!("unsupported power trace input {unsupported:?}");
                    #[cfg(test)]
                    eprintln!("native input decline: unsupported power trace input {unsupported:?}");
                    return None;
                }
            }
        }
        for node in trace.wires {
            if !matches!(graph.nodes.get(node.index())?.kind, CircuitNodeKind::Wire(_)) {
                #[cfg(test)]
                eprintln!("native input decline: power trace returned non-wire node {node:?}");
                return None;
            }
            native_nodes.insert(node);
        }
    }
    native_nodes.extend(lamps);
    // The computation frame follows the qualified controller's actual accepted
    // native actions. Keep the complete timing power island on Graph so its
    // piston, observers and pulse routes remain in their original FIFO.
    native_nodes.extend(timing.nodes.iter().copied());
    native_nodes.extend(timing.actor_specs.iter().map(|actor| actor.node));
    equation_clock_sites.retain(|site| !trace_source_sites.contains(site));

    let native = super::native_inputs::discover(
        graph,
        island,
        expanded_components,
        physical_ownership,
        &retained_cells,
        &native_nodes,
        &trace_source_sites,
        &trace_source_states,
        &equation_clock_sites,
        monitor,
    );
    let Some(mut native) = native else {
        *reason = "physical source/callback ownership closure was not proven".into();
        #[cfg(test)]
        eprintln!("native input decline: complete physical source/callback ownership closure could not be proven");
        return None;
    };
    native_nodes = std::mem::take(&mut native.nodes);

    for output in &circuit.outputs {
        let overlaps_native = match output.target {
            #[cfg(test)]
            SettledTarget::ComponentOutput(_) => true,
            SettledTarget::Power(PowerTarget::Input { node, .. }) => native_nodes.contains(&node),
            SettledTarget::ResetExcludedPower { node, .. } => native_nodes.contains(&node),
            SettledTarget::Power(PowerTarget::Conductor { site, .. }) => {
                native.sites.contains(&site)
                    || matches!(graph.sites.get(site.index()).map(|site| site.source), Some(SiteSource::Contact(contact)) if native.contacts.contains(&contact))
            }
            SettledTarget::ResourceState { resource, .. } | SettledTarget::Register(BudMemoryRead { resource, .. }) => {
                native.resources.contains(&resource)
            }
        };
        if overlaps_native {
            let node = match output.target {
                SettledTarget::Power(PowerTarget::Input { node, .. }) | SettledTarget::ResetExcludedPower { node, .. } => Some(node),
                _ => None,
            };
            tracing::debug!(island = island.id, target = ?output.target, "output overlaps native ownership");
            *reason = format!(
                "output at {} is shared with normal simulation; fast control could not be verified",
                diagnostic_position(node.and_then(|node| graph.nodes.get(node.index()).and_then(|node| node.position)))
            );
            return None;
        }
    }
    if native_nodes.iter().any(|node| island.nodes.binary_search(node).is_err())
        || native.contacts.iter().any(|contact| island.contacts.binary_search(contact).is_err())
        || native.resources.iter().any(|resource| island.resources.binary_search(resource).is_err())
        || native.sites.iter().any(|site| island.sites.binary_search(site).is_err())
    {
        *reason = "native input closure escapes the interaction island".into();
        #[cfg(test)]
        eprintln!("logical frame decline: native input cone escaped its interaction island");
        return None;
    }

    let owned_nodes = match computation_owned_nodes(
        graph,
        island,
        circuit,
        expanded_components,
        &native_nodes,
        &retained_cells,
        &timing,
        reason,
    ) {
        Some(nodes) => nodes,
        None => {
            if reason.is_empty() {
                *reason = "computation node ownership proof unavailable".into();
            }
            return None;
        }
    };
    let owned_set: FxHashSet<_> = owned_nodes.iter().copied().collect();
    let timing_nodes: FxHashSet<_> = timing
        .nodes
        .iter()
        .copied()
        .chain(timing.actor_specs.iter().map(|actor| actor.node))
        .collect();
    let mut input_callbacks = Vec::<RouteOccurrence>::new();
    for &route_id in &island.routes {
        let Some(route) = graph.routes.get(route_id.index()) else {
            #[cfg(test)]
            eprintln!("logical frame decline: island references missing route {route_id:?}");
            return None;
        };
        // Internal callbacks remain part of the compiled graph contract. Every
        // other callback into a frame-owned node must be certified below,
        // including sources which were not reached by native-input discovery.
        if owned_set.contains(&route.source) {
            continue;
        }
        for (ordinal, delivery) in route.deliveries.iter().enumerate() {
            let Some(target) = resolved_delivery_node(graph, delivery) else {
                continue;
            };
            if !owned_set.contains(&target) {
                continue;
            }
            let occurrence = RouteOccurrence {
                route: route_id,
                ordinal: u32::try_from(ordinal).ok()?,
            };
            let inert_wire_shape = matches!(
                delivery.kind,
                DeliveryKind::Shape { .. }
                    if matches!(&graph.nodes.get(target.index())?.kind, CircuitNodeKind::Wire(_))
                        && crate::redpiler::program_graph::WireShape::shape_callbacks_are_inert(graph, target)
            );
            if !incoming_callback_source_has_boundary_proof(route.source, target, &owned_set, &native_nodes)
                || (!matches!(delivery.kind, DeliveryKind::Update { .. }) && !inert_wire_shape)
                || (!expanded_components.contains(&target)
                    && (!timing_nodes.contains(&target) || route.source != timing.trigger)
                    && !inert_wire_shape)
            {
                tracing::debug!(?route_id, ordinal, source = ?route.source, ?target, kind = ?delivery.kind, "callback lacks a supported ownership boundary");
                *reason = format!(
                    "callback from {} to {} lacks a supported ownership boundary",
                    diagnostic_position(graph.nodes.get(route.source.index()).and_then(|node| node.position)),
                    diagnostic_position(graph.nodes.get(target.index()).and_then(|node| node.position))
                );
                #[cfg(test)]
                eprintln!(
                    "logical frame decline: native input callback {:?}/{} from {:?} at {:?} targets unsupported owned node {:?} at {:?} ({:?})",
                    route_id,
                    ordinal,
                    route.source,
                    graph.nodes.get(route.source.index()).and_then(|node| node.position),
                    target,
                    graph.nodes.get(target.index()).and_then(|node| node.position),
                    delivery.kind
                );
                return None;
            }
            input_callbacks.push(occurrence);
        }
    }
    input_callbacks.sort_unstable_by_key(|occurrence| (occurrence.route, occurrence.ordinal));
    input_callbacks.dedup();

    let (owned_sites, owned_contacts, owned_resources) = owned_state;
    let scope = scoped_device_scope_with_masks(
        graph,
        island,
        circuit,
        owned_nodes,
        native_nodes,
        owned_sites,
        owned_contacts,
        owned_resources,
        input_callbacks.into_boxed_slice(),
        crate::redpiler::program_graph::SettledClockDriver::NativeAction { clock: timing.clock },
    );
    if scope.is_none() {
        #[cfg(test)]
        eprintln!("logical frame decline: final scope masks do not partition island {}", island.id);
    }
    scope
}

fn incoming_callback_source_has_boundary_proof(
    source: NodeId,
    target: NodeId,
    owned_nodes: &FxHashSet<NodeId>,
    native_nodes: &FxHashSet<NodeId>,
) -> bool {
    !owned_nodes.contains(&target) || owned_nodes.contains(&source) || native_nodes.contains(&source)
}

/// Return only nodes whose values the reduced equations replace. Nodes retained
/// by Graph's clock, cell, or repeater adapters remain Graph-owned even when
/// the frame samples their inputs.
fn computation_owned_nodes(
    graph: &CompileGraph,
    island: &LogicalIsland,
    circuit: &SettledCircuit,
    equation_support: &FxHashSet<NodeId>,
    native_nodes: &FxHashSet<NodeId>,
    retained_cells: &super::native_inputs::RetainedCellBoundary,
    timing: &SettledTimingIsland,
    reason: &mut String,
) -> Option<Vec<NodeId>> {
    let mut retained = retained_cells.nodes.clone();
    retained.extend(timing.nodes.iter().copied());
    retained.extend(timing.actor_specs.iter().map(|actor| actor.node));
    for output in &circuit.outputs {
        if let SettledTarget::Power(PowerTarget::Input { node, port: InputPort::Main }) = output.target
            && matches!(
                graph.nodes.get(node.index())?.kind,
                CircuitNodeKind::Electrical(NodeType::Repeater { .. })
            )
        {
            retained.insert(node);
        }
    }

    let mut output_nodes = FxHashSet::default();
    for output in &circuit.outputs {
        let node = match output.target {
            #[cfg(test)]
            SettledTarget::ComponentOutput(node) => Some(node),
            SettledTarget::Power(PowerTarget::Input { node, .. }) => Some(node),
            SettledTarget::ResetExcludedPower { node, .. } => Some(node),
            SettledTarget::Power(PowerTarget::Conductor { .. }) | SettledTarget::ResourceState { .. } | SettledTarget::Register(_) => None,
        };
        if let Some(node) = node {
            if island.nodes.binary_search(&node).is_err() || node.index() >= graph.nodes.len() {
                return None;
            }
            output_nodes.insert(node);
        }
    }
    if let Some(node) = first_native_overlap(&island.nodes, equation_support, &output_nodes, &retained, native_nodes) {
        tracing::debug!(island = island.id, ?node, "equation support overlaps native ownership");
        *reason = format!(
            "block at {} is shared with normal simulation; fast control could not be verified",
            diagnostic_position(graph.nodes.get(node.index()).and_then(|node| node.position))
        );
    }
    positive_computation_nodes(&island.nodes, equation_support, &output_nodes, &retained, native_nodes)
}

fn first_native_overlap(
    island_nodes: &[NodeId],
    equation_support: &FxHashSet<NodeId>,
    output_nodes: &FxHashSet<NodeId>,
    retained_nodes: &FxHashSet<NodeId>,
    native_nodes: &FxHashSet<NodeId>,
) -> Option<NodeId> {
    equation_support
        .iter()
        .chain(output_nodes)
        .filter(|node| island_nodes.binary_search(node).is_ok() && !retained_nodes.contains(node) && native_nodes.contains(node))
        .copied()
        .min()
}

fn positive_computation_nodes(
    island_nodes: &[NodeId],
    equation_support: &FxHashSet<NodeId>,
    output_nodes: &FxHashSet<NodeId>,
    retained_nodes: &FxHashSet<NodeId>,
    native_nodes: &FxHashSet<NodeId>,
) -> Option<Vec<NodeId>> {
    let mut owned = FxHashSet::default();
    for node in equation_support.iter().chain(output_nodes) {
        if island_nodes.binary_search(node).is_err() {
            return None;
        }
        if retained_nodes.contains(node) {
            continue;
        }
        if native_nodes.contains(node) {
            return None;
        }
        owned.insert(*node);
    }
    let mut owned = owned.into_iter().collect::<Vec<_>>();
    owned.sort_unstable();
    Some(owned)
}

fn computation_owned_state(
    circuit: &SettledCircuit,
    graph: &CompileGraph,
    retained_cells: &super::native_inputs::RetainedCellBoundary,
    island: &LogicalIsland,
) -> Option<(Vec<SiteId>, Vec<ContactId>, Vec<ResourceId>)> {
    let mut sites = FxHashSet::default();
    let mut contacts = FxHashSet::default();
    let resources = FxHashSet::default();
    for output in &circuit.outputs {
        match output.target {
            #[cfg(test)]
            SettledTarget::ComponentOutput(_) => return None,
            SettledTarget::Power(PowerTarget::Conductor { site, .. }) => {
                if island.sites.binary_search(&site).is_err() || site.index() >= graph.sites.len() {
                    return None;
                }
                sites.insert(site);
                if let SiteSource::Contact(contact) = graph.sites[site.index()].source {
                    contacts.insert(contact);
                }
            }
            SettledTarget::ResourceState { resource, .. } | SettledTarget::Register(BudMemoryRead { resource, .. }) => {
                if island.resources.binary_search(&resource).is_err() || resource.index() >= graph.resources.len() {
                    return None;
                }
                if !retained_cells.resources.contains(&resource) {
                    // The frame has no scheduler adapter for unretained
                    // resource owners; fail closed instead of claiming only
                    // the resource bit while Graph continues to move actors.
                    return None;
                }
            }
            SettledTarget::Power(PowerTarget::Input { .. }) | SettledTarget::ResetExcludedPower { .. } => {}
        }
    }
    if sites.iter().any(|site| island.sites.binary_search(site).is_err())
        || contacts.iter().any(|contact| island.contacts.binary_search(contact).is_err())
        || resources.iter().any(|resource| island.resources.binary_search(resource).is_err())
    {
        return None;
    }
    let mut sites = sites.into_iter().collect::<Vec<_>>();
    let mut contacts = contacts.into_iter().collect::<Vec<_>>();
    let mut resources = resources.into_iter().collect::<Vec<_>>();
    sites.sort_unstable();
    contacts.sort_unstable();
    resources.sort_unstable();
    Some((sites, contacts, resources))
}

fn unretained_resource_output(circuit: &SettledCircuit, retained: &FxHashSet<ResourceId>) -> Option<(ResourceId, ContactId)> {
    circuit.outputs.iter().find_map(|output| match output.target {
        SettledTarget::ResourceState { resource, source } if !retained.contains(&resource) => Some((resource, source)),
        SettledTarget::Register(BudMemoryRead { resource, source, .. }) if !retained.contains(&resource) => Some((resource, source)),
        _ => None,
    })
}

fn collect_native_source_node(
    graph: &CompileGraph,
    source: SiteSource,
    timing: &SettledTimingIsland,
    native_nodes: &mut FxHashSet<NodeId>,
    trace_targets: &mut Vec<PowerTarget>,
) -> Option<()> {
    let SiteSource::Component(node) = source else {
        return Some(());
    };
    let kind = &graph.nodes.get(node.index())?.kind;
    match kind {
        CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::PressurePlate | NodeType::Lamp) => {
            native_nodes.insert(node);
        }
        CircuitNodeKind::Wire(_) => {
            native_nodes.insert(node);
            trace_targets.push(PowerTarget::Input { node, port: InputPort::Main });
        }
        CircuitNodeKind::Electrical(NodeType::Constant) => {
            native_nodes.insert(node);
        }
        CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. } if !timing_source_value(timing, node) => {
            native_nodes.insert(node);
        }
        _ if timing_source_value(timing, node) => {}
        _ => return None,
    }
    Some(())
}

fn timing_source_value(timing: &SettledTimingIsland, node: NodeId) -> bool {
    node == timing.trigger
        || timing
            .node_specs
            .iter()
            .any(|spec| spec.node == node && matches!(spec.kind, ClockNodeKind::Torch | ClockNodeKind::Observer | ClockNodeKind::BuiltinReset))
}

fn timing_source_state(timing: &SettledTimingIsland, test: StateTest) -> bool {
    matches!(test, StateTest::PistonExtended(actor) | StateTest::PistonHeadPresent(actor) if actor == timing.clock)
}

fn resolved_delivery_node(graph: &CompileGraph, delivery: &RouteDelivery) -> Option<NodeId> {
    let receiver = delivery
        .receiver
        .or_else(|| match (graph.sites.get(delivery.target.index())?.source, delivery.kind) {
            (SiteSource::Component(node), DeliveryKind::Update { .. }) => Some(CallbackReceiver::Component(node)),
            _ => None,
        })?;
    Some(match receiver {
        CallbackReceiver::Component(node) => node,
        CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor) => graph.pistons.get(actor.index())?.node,
        CallbackReceiver::Contact(contact) => graph.contacts.get(contact.index())?.node,
    })
}

fn scoped_device_scope_with_masks(
    graph: &CompileGraph,
    island: &LogicalIsland,
    circuit: &SettledCircuit,
    owned_nodes: Vec<NodeId>,
    native_nodes: FxHashSet<NodeId>,
    owned_sites: Vec<SiteId>,
    owned_contacts: Vec<ContactId>,
    owned_resources: Vec<ResourceId>,
    input_callbacks: Box<[RouteOccurrence]>,
    clock_driver: crate::redpiler::program_graph::SettledClockDriver,
) -> Option<SettledDeviceScope> {
    let owns_node = |node: NodeId| owned_nodes.binary_search(&node).is_ok();
    let native_clock_is_owned = match clock_driver {
        crate::redpiler::program_graph::SettledClockDriver::Logical => true,
        crate::redpiler::program_graph::SettledClockDriver::NativeAction { clock } => {
            circuit.timing.as_ref().is_some_and(|timing| timing.clock == clock)
                && graph
                    .pistons
                    .get(clock.index())
                    .is_some_and(|piston| native_nodes.contains(&piston.node) && !owns_node(piston.node))
        }
    };
    if circuit.timing.is_none() || !native_clock_is_owned {
        return None;
    }
    let owned_source_routes = island
        .routes
        .iter()
        .copied()
        .filter(|route| owns_node(graph.routes[route.index()].source))
        .collect::<Vec<_>>();
    Some(SettledDeviceScope {
        component: island.id,
        owned_nodes: owned_nodes.into_boxed_slice(),
        owned_sites: owned_sites.into_boxed_slice(),
        owned_contacts: owned_contacts.into_boxed_slice(),
        owned_resources: owned_resources.into_boxed_slice(),
        owned_source_routes: owned_source_routes.into_boxed_slice(),
        input_callbacks,
        clock_driver,
    })
}

fn try_computation_frame_with_timing(
    graph: &CompileGraph,
    island: Option<&LogicalIsland>,
    timing: crate::redpiler::program_graph::SettledTimingIsland,
    monitor: &TaskMonitor,
) -> Option<(SettledCircuit, FxHashSet<NodeId>)> {
    let Some(clock) = timing.actor_specs.iter().find(|actor| actor.piston == timing.clock) else {
        #[cfg(test)]
        eprintln!("computation-frame decline: clock {:?} missing from timing actor specs", timing.clock);
        return None;
    };
    if !clock.sticky || !matches!(clock.lane, PistonLane::SingleResource { .. }) {
        #[cfg(test)]
        eprintln!(
            "computation-frame decline: clock {:?} is not a sticky single-resource clock",
            timing.clock
        );
        return None;
    }
    let Some((mut targets, mut retained_cells, mut retained_effects)) = frame_retained_output_cells(graph, island) else {
        #[cfg(test)]
        eprintln!(
            "computation-frame decline: retained output cells not certified for island {:?}",
            island.map(|island| island.id)
        );
        return None;
    };
    for use_ in &retained_cells {
        let node = graph.pistons[use_.actor.index()].node;
        targets.extend([
            PowerTarget::Input { node, port: InputPort::Main },
            PowerTarget::Input {
                node,
                port: InputPort::Quasi,
            },
        ]);
    }
    targets.sort_unstable();
    targets.dedup();
    if targets.is_empty() {
        #[cfg(test)]
        eprintln!(
            "computation-frame decline: no power targets for island {:?}",
            island.map(|island| island.id)
        );
        return None;
    }
    const MAX_FEEDBACK_OWNER_ROUNDS: usize = 8;
    let mut retained_feedback_resources = FxHashSet::default();
    let (mut computation, expanded_components) = 'compile: {
        for round in 0..MAX_FEEDBACK_OWNER_ROUNDS {
            check_cancelled(monitor).ok()?;
            let reset_excluded_targets = retained_cells
                .iter()
                .filter_map(|use_| match &use_.reset {
                    CellReset::InputRestoration { resource, contact, .. } => Some((use_.actor, *resource, *contact)),
                    CellReset::Observer { .. } | CellReset::Payload { .. } => None,
                })
                .flat_map(|(actor, resource, contact)| {
                    let node = graph.pistons[actor.index()].node;
                    [InputPort::Main, InputPort::Quasi].map(|port| (PowerTarget::Input { node, port }, (resource, contact)))
                })
                .collect::<FxHashMap<_, _>>();
            let (computation, expanded_components) =
                match compile_computation_with_reset_exclusions(graph, &targets, &reset_excluded_targets, monitor) {
                    Ok(computation) => computation,
                    Err(reason) => {
                        if island.is_none() && !monitor.cancelled() {
                            monitor.record_warning(format!("Whole-plot fast circuit skipped: {reason}. Other regions may still compile."));
                        }
                        #[cfg(test)]
                        eprintln!(
                            "computation-frame decline: equation compilation failed for island {:?}: {reason}",
                            island.map(|island| island.id)
                        );
                        return None;
                    }
                };
            let unsupported = unsupported_computation_frame_inputs(graph, &computation);
            if !computation.declines.is_empty() || !unsupported.is_empty() {
                #[cfg(test)]
                eprintln!(
                    "computation-frame decline: island {:?} has {} compiler declines and {} unsupported inputs",
                    island.map(|island| island.id),
                    computation.declines.len(),
                    unsupported.len()
                );
                return None;
            }
            let feedback = computation
                .outputs
                .iter()
                .filter_map(|output| match output.target {
                    SettledTarget::ResourceState { resource, source } => Some((resource, source)),
                    _ => None,
                })
                .collect::<Vec<_>>();
            let mut added_owner = false;
            for (resource, source) in feedback {
                if retained_feedback_resources.contains(&resource) {
                    continue;
                }
                let Some(owner_closure) = frame_private_feedback_owner_closure(graph, island, resource, source, monitor) else {
                    continue;
                };
                let new_resources = owner_closure
                    .iter()
                    .filter(|(owned_resource, _, _, _)| !retained_feedback_resources.contains(owned_resource))
                    .count();
                if retained_feedback_resources.len() + new_resources > MAX_RETAINED_FEEDBACK_OWNERS {
                    #[cfg(test)]
                    {
                        let witness = owner_closure
                            .iter()
                            .take(24)
                            .map(|(resource, actor, use_, _)| format!("{resource:?}/{actor:?}/{:?}", use_.reset))
                            .collect::<Vec<_>>();
                        eprintln!(
                            "computation-frame feedback owner budget: round={round} retained={} closure={} new={} total={} targets={} closure_witness={witness:?}",
                            retained_feedback_resources.len(),
                            owner_closure.len(),
                            new_resources,
                            retained_feedback_resources.len() + new_resources,
                            targets.len(),
                        );
                    }
                    return None;
                }
                for (owned_resource, actor, use_, effects) in owner_closure {
                    for effect in effects {
                        if !retained_effects.contains(&effect) {
                            retained_effects.push(effect);
                        }
                    }
                    if !retained_cells.iter().any(|retained| retained.actor == actor) {
                        retained_cells.push(use_);
                        let node = graph.pistons.get(actor.index())?.node;
                        targets.extend([
                            PowerTarget::Input { node, port: InputPort::Main },
                            PowerTarget::Input {
                                node,
                                port: InputPort::Quasi,
                            },
                        ]);
                        added_owner = true;
                    }
                    retained_feedback_resources.insert(owned_resource);
                }
            }
            if !added_owner {
                break 'compile (computation, expanded_components);
            }
            if round + 1 == MAX_FEEDBACK_OWNER_ROUNDS {
                #[cfg(test)]
                eprintln!("computation-frame decline: retained feedback-owner roots did not settle within {MAX_FEEDBACK_OWNER_ROUNDS} rounds");
                return None;
            }
            targets.sort_unstable();
            targets.dedup();
        }
        return None;
    };
    if let Some(island) = island {
        let owns_node = |node: NodeId| island.nodes.binary_search(&node).is_ok();
        if computation.outputs.iter().any(|output| match output.target {
            #[cfg(test)]
            SettledTarget::ComponentOutput(_) => true,
            SettledTarget::Power(PowerTarget::Input { node, .. }) => !owns_node(node),
            SettledTarget::ResetExcludedPower { node, .. } => !owns_node(node),
            SettledTarget::Power(PowerTarget::Conductor { site, .. }) => island.sites.binary_search(&site).is_err(),
            SettledTarget::ResourceState { resource, .. } => !island.resources.contains(&resource),
            SettledTarget::Register(read) => !island.resources.contains(&read.resource),
        }) {
            #[cfg(test)]
            eprintln!("computation-frame decline: output lies outside island {:?}", island.id);
            return None;
        }
        for input in &computation.program.inputs {
            if let LayerInputSource::SourceAbove { site, .. } = input.source
                && let Some(SiteSource::Component(node)) = graph.sites.get(site.index()).map(|site| site.source)
                && !owns_node(node)
            {
                #[cfg(test)]
                eprintln!(
                    "computation-frame decline: input source site {site:?} lies outside island {:?}",
                    island.id
                );
                return None;
            }
        }
    }
    for (index, node) in graph.nodes.iter().enumerate() {
        let node_id = NodeId::new(index as u32);
        if !matches!(node.kind, CircuitNodeKind::Electrical(NodeType::CommandBlock { .. }))
            || island.is_some_and(|island| island.nodes.binary_search(&node_id).is_err())
        {
            continue;
        }
        let target = PowerTarget::Input {
            node: node_id,
            port: InputPort::Main,
        };
        let retained = retained_effects
            .iter()
            .any(|port| matches!(port.kind, BoundaryPortKind::Power { link } if graph.power_links[link as usize].to == target));
        let published = computation.outputs.iter().any(|output| match output.target {
            SettledTarget::Power(output_target) => output_target == target,
            SettledTarget::ResetExcludedPower { node, port, .. } => target == PowerTarget::Input { node, port },
            _ => false,
        });
        if retained == published {
            #[cfg(test)]
            eprintln!(
                "computation-frame decline: command output ownership mismatch at {node_id:?} in island {:?}",
                island.map(|island| island.id)
            );
            return None;
        }
    }
    computation.retained_cells = retained_cells.into_boxed_slice();
    computation.retained_effects = retained_effects.into_boxed_slice();
    computation.timing = Some(timing);
    Some((computation, expanded_components))
}

/// Keep command outputs backed by a single certified payload cell in Graph's
/// exact event schedule, while the bulk FPU remains a settled computation.
fn frame_retained_output_cells(graph: &CompileGraph, island: Option<&LogicalIsland>) -> Option<(Vec<PowerTarget>, Vec<CellUse>, Vec<BoundaryPort>)> {
    let Some(catalog) = graph.plans.instant.boundaries.as_ref() else {
        return None;
    };
    let mut targets = island.map_or_else(|| frame_power_targets(graph), |island| frame_power_targets_in(graph, Some(island), false));
    let mut retained_cells = Vec::<CellUse>::new();
    let mut retained_effects = Vec::<BoundaryPort>::new();
    let command_nodes = graph
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            (matches!(node.kind, CircuitNodeKind::Electrical(NodeType::CommandBlock { .. }))
                && island.is_none_or(|island| island.nodes.binary_search(&NodeId::new(index as u32)).is_ok()))
            .then_some(NodeId::new(index as u32))
        })
        .collect::<Vec<_>>();
    for node in command_nodes {
        let target = PowerTarget::Input { node, port: InputPort::Main };
        let links = graph.power_links.iter().filter(|link| link.to == target).collect::<Vec<_>>();
        if links.is_empty() {
            continue;
        }
        let mut selected: Option<(usize, CellUse)> = None;
        for (link_index, link) in graph.power_links.iter().enumerate().filter(|(_, link)| link.to == target) {
            let SiteSource::Contact(contact) = graph.sites[link.from.index()].source else {
                return None;
            };
            let found = catalog.cells.iter().enumerate().find_map(|(cell_index, cell)| {
                if island.is_some_and(|island| {
                    cell.uses
                        .iter()
                        .any(|use_| island.nodes.binary_search(&graph.pistons[use_.actor.index()].node).is_err())
                }) {
                    return None;
                }
                cell.resources.iter().find_map(|resource| {
                    let source_resource = &graph.resources[resource.resource.index()];
                    if !source_resource.contacts.contains(&contact) {
                        return None;
                    }
                    if source_resource.material.index() >= graph.profiles.len() {
                        return None;
                    }
                    if graph.profiles[source_resource.material.index()].electrical != ProfileElectrical::RedstoneSource {
                        return None;
                    }
                    let owners = resource.owners.as_ref();
                    if owners.len() != 1 || cell.uses.len() != 1 {
                        return None;
                    }
                    let actor = owners[0];
                    let Some(piston) = graph.pistons.get(actor.index()) else {
                        return None;
                    };
                    if piston.near != contact || !matches!(piston.lane, PistonLane::SingleResource { far } if far == resource.source) {
                        return None;
                    }
                    let use_ = cell.uses.iter().find(|use_| {
                        use_.actor == actor
                            && use_.reset.observer().is_some()
                            && use_.retraction.transfer.is_some_and(|transfer| transfer.resource == resource.resource)
                    });
                    let Some(use_) = use_ else {
                        return None;
                    };
                    Some((cell_index, use_.clone()))
                })
            });
            let Some((cell_index, use_)) = found else {
                return None;
            };
            if let Some((previous_cell, previous_use)) = &selected {
                if *previous_cell != cell_index || previous_use.actor != use_.actor {
                    return None;
                }
            } else {
                selected = Some((cell_index, use_));
            }
            if !catalog.ports.iter().any(|port| {
                port.owner == BoundaryOwner::Cell(cell_index as u32)
                    && port.direction == BoundaryDirection::Output
                    && port.kind == (BoundaryPortKind::Power { link: link_index as u32 })
            }) {
                return None;
            }
        }
        let Some((cell_index, use_)) = selected else {
            return None;
        };
        let cell = &catalog.cells[cell_index];
        if cell.uses.len() != 1 || use_.activations.is_empty() {
            return None;
        }
        let actor = use_.actor;
        if !retained_cells.iter().any(|retained| retained.actor == actor) {
            retained_cells.push(use_.clone());
        }
        for port in catalog
            .ports
            .iter()
            .filter(|port| port.owner == BoundaryOwner::Cell(cell_index as u32) && port.direction == BoundaryDirection::Output)
        {
            if !retained_cell_effect_is_owned(graph, catalog, cell, &use_, port) {
                return None;
            }
            if !retained_effects.contains(port) {
                retained_effects.push(*port);
            }
        }
        targets.retain(|candidate| *candidate != target);
    }
    retained_cells.sort_unstable_by_key(|use_| use_.actor);
    retained_effects.sort_unstable_by_key(|port| match port.kind {
        BoundaryPortKind::Power { link } => link,
        _ => u32::MAX,
    });
    Some((targets, retained_cells, retained_effects))
}

/// Retain a bounded chain of private owner cells needed by a feedback output.
/// Input-restoration resets pull in their exact resource owners recursively;
/// shared or ambiguous cells remain on Graph.
fn frame_private_feedback_owner_closure(
    graph: &CompileGraph,
    island: Option<&LogicalIsland>,
    resource: ResourceId,
    source: ContactId,
    monitor: &TaskMonitor,
) -> Option<Vec<(ResourceId, PistonId, CellUse, Vec<BoundaryPort>)>> {
    let catalog = graph.plans.instant.boundaries.as_ref()?;
    let mut pending = vec![(resource, Some(source))];
    let mut visited = FxHashSet::default();
    let mut owners = Vec::new();
    let mut work = 0usize;
    while let Some((resource, expected_source)) = pending.pop() {
        check_cancelled(monitor).ok()?;
        if !visited.insert(resource) {
            continue;
        }
        if visited.len() > 64 {
            return None;
        }
        let mut candidate = None;
        for (cell_index, cell) in catalog.cells.iter().enumerate() {
            work = work.checked_add(1)?;
            if work > MAX_FEEDBACK_OWNER_CLOSURE_WORK || work & 0x3fff == 0 && monitor.cancelled() {
                return None;
            }
            let Some(pin) = cell.resources.iter().find(|pin| pin.resource == resource) else {
                continue;
            };
            if candidate.is_some()
                || cell.resources.len() != 1
                || cell.uses.len() != 1
                || pin.owners.len() != 1
                || expected_source.is_some_and(|source| source != pin.source)
            {
                return None;
            }
            candidate = Some((cell_index, cell, pin));
        }
        let (cell_index, cell, pin) = candidate?;
        let use_ = cell.uses.first()?;
        let actor = pin.owners[0];
        let piston = graph.pistons.get(actor.index())?;
        let resource_spec = graph.resources.get(resource.index())?;
        if use_.actor != actor
            || use_.activations.is_empty()
            || use_
                .retraction
                .transfer
                .is_none_or(|transfer| transfer.resource != resource || transfer.source != pin.source)
            || !piston.sticky
            || !matches!(piston.lane, PistonLane::SingleResource { far } if far == pin.source)
            || !resource_spec.contacts.contains(&pin.source)
            || !resource_spec.contacts.contains(&piston.near)
            || resource_spec.contacts.len() != 2
            || island.is_some_and(|island| island.nodes.binary_search(&piston.node).is_err())
            || island.is_some_and(|island| island.resources.binary_search(&resource).is_err())
        {
            return None;
        }
        let reset_dependencies = match &use_.reset {
            CellReset::Observer { wake, .. } if wake.watched_actor != actor => {
                let (resource, source) = private_feedback_resource_for_actor(graph, island, catalog, wake.watched_actor)?;
                vec![(resource, Some(source))]
            }
            CellReset::Observer { .. } => Vec::new(),
            CellReset::InputRestoration { resource, contact, .. }
                if graph.resources.get(resource.index()).is_some_and(|spec| spec.contacts.contains(contact))
                    && island.is_none_or(|island| island.resources.binary_search(resource).is_ok()) =>
            {
                vec![(*resource, None)]
            }
            CellReset::Payload { .. } | CellReset::InputRestoration { .. } => return None,
        };
        let mut effects = Vec::new();
        for port in catalog
            .ports
            .iter()
            .filter(|port| port.owner == BoundaryOwner::Cell(cell_index as u32) && port.direction == BoundaryDirection::Output)
        {
            work = work.checked_add(1)?;
            if work > MAX_FEEDBACK_OWNER_CLOSURE_WORK || work & 0x3fff == 0 && monitor.cancelled() {
                return None;
            }
            effects.push(*port);
        }
        if effects.is_empty() {
            return None;
        }
        owners.push((resource, actor, use_.clone(), effects));
        pending.extend(reset_dependencies);
    }
    let cohort = owners.iter().map(|(_, _, use_, _)| use_.clone()).collect::<Vec<_>>();
    for (resource, _, use_, effects) in &owners {
        let cell_index = catalog
            .cells
            .iter()
            .position(|cell| cell.resources.iter().any(|pin| pin.resource == *resource))?;
        let cell = &catalog.cells[cell_index];
        for port in effects {
            if !retained_cell_effect_is_owned(graph, catalog, cell, use_, port) && !cohort_observer_watch_is_owned(graph, &cohort, port.kind) {
                return None;
            }
        }
    }
    owners.sort_unstable_by_key(|(resource, actor, _, _)| (*resource, *actor));
    Some(owners)
}

fn private_feedback_resource_for_actor(
    graph: &CompileGraph,
    island: Option<&LogicalIsland>,
    catalog: &BoundaryCatalog,
    actor: PistonId,
) -> Option<(ResourceId, ContactId)> {
    let mut selected = None;
    for cell in catalog.cells.iter() {
        let Some(use_) = cell.uses.iter().find(|use_| use_.actor == actor) else {
            continue;
        };
        if selected.is_some() || cell.uses.len() != 1 || cell.resources.len() != 1 || use_.activations.is_empty() {
            return None;
        }
        let pin = &cell.resources[0];
        let piston = graph.pistons.get(actor.index())?;
        if pin.owners.as_ref() != [actor]
            || use_
                .retraction
                .transfer
                .is_none_or(|transfer| transfer.resource != pin.resource || transfer.source != pin.source)
            || island.is_some_and(|island| island.resources.binary_search(&pin.resource).is_err())
            || island.is_some_and(|island| island.nodes.binary_search(&piston.node).is_err())
        {
            return None;
        }
        selected = Some((pin.resource, pin.source));
    }
    let (resource, source) = selected?;
    Some((resource, source))
}

fn cohort_observer_watch_is_owned(graph: &CompileGraph, cohort: &[CellUse], kind: BoundaryPortKind) -> bool {
    let BoundaryPortKind::ObserverWatch { observer, site } = kind else {
        return false;
    };
    if !graph
        .nodes
        .get(observer.index())
        .is_some_and(|node| matches!(node.kind, CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. }))
    {
        return false;
    }
    let Some(site) = graph.sites.get(site.index()) else {
        return false;
    };
    let SiteSource::Contact(contact) = site.source else {
        return false;
    };
    cohort
        .iter()
        .any(|use_| graph.pistons.get(use_.actor.index()).is_some_and(|piston| piston.base == contact))
}

fn retained_cell_effect_is_owned(graph: &CompileGraph, catalog: &BoundaryCatalog, cell: &CellContract, use_: &CellUse, port: &BoundaryPort) -> bool {
    match port.kind {
        BoundaryPortKind::Power { link } => {
            let power_link = graph.power_links[link as usize];
            let target = power_link.to;
            let piston_node = graph.pistons[use_.actor.index()].node;
            if matches!(target, PowerTarget::Input { node, .. } if node == piston_node)
                || matches!(target, PowerTarget::Input { node, port: InputPort::Main } if matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Electrical(NodeType::CommandBlock { .. })))
            {
                return true;
            }
            if retained_cell_reset_conductor_is_owned(graph, use_, power_link) {
                return true;
            }
            let corridor_index = match target {
                PowerTarget::Input { node, .. } if matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Wire(_)) => {
                    catalog.corridors.iter().position(|corridor| corridor.wires.contains(&node))
                }
                PowerTarget::Conductor { site, .. } => catalog.corridors.iter().position(|corridor| corridor.conductive_sites.contains(&site)),
                _ => None,
            };
            corridor_index.is_some_and(|corridor_index| {
                catalog.ports.iter().any(|peer| {
                    peer.owner == BoundaryOwner::Corridor(corridor_index as u32)
                        && peer.direction == BoundaryDirection::Input
                        && peer.kind == port.kind
                })
            })
        }
        BoundaryPortKind::Callback { occurrence } => {
            let Some(delivery) = graph
                .routes
                .get(occurrence.route.index())
                .and_then(|route| route.deliveries.get(occurrence.ordinal as usize))
            else {
                return false;
            };
            let source = graph.sites[delivery.target.index()].source;
            match delivery.receiver.or_else(|| match (source, delivery.kind) {
                (SiteSource::Component(node), DeliveryKind::Update { .. }) => Some(CallbackReceiver::Component(node)),
                _ => None,
            }) {
                Some(CallbackReceiver::Component(node)) if matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Wire(_)) => {
                    retained_cell_corridor_wire_is_owned(catalog, port.kind, node)
                }
                _ => true,
            }
        }
        kind if retained_cell_observation_is_owned(graph, cell, kind) => true,
        BoundaryPortKind::WireRelation { relation } => {
            retained_cell_corridor_wire_is_owned(catalog, port.kind, graph.wire_relations[relation as usize].from)
        }
        BoundaryPortKind::WireNeighborhood { wire, .. } | BoundaryPortKind::CapturedWire { wire, .. } => {
            retained_cell_corridor_wire_is_owned(catalog, port.kind, wire)
        }
        _ => false,
    }
}

fn retained_cell_reset_conductor_is_owned(graph: &CompileGraph, use_: &CellUse, link: PowerLink) -> bool {
    let Some(reset) = use_.reset.observer() else {
        return false;
    };
    let PowerTarget::Conductor { site, include_dust } = link.to else {
        return false;
    };
    if !matches!(link.from_port, PowerPort::Output | PowerPort::Stored)
        || !matches!(graph.sites[link.from.index()].source, SiteSource::Component(node) if node == reset)
    {
        return false;
    }
    let SiteSource::Constant(profile) = graph.sites[site.index()].source else {
        return false;
    };
    let profile = graph.profiles[profile.index()];
    if !profile.solid || !profile.cube {
        return false;
    }
    // The circuit builder can leave the observer's old conductor delivery in
    // the cell after specializing the piston query to the direct observer
    // output. An unqueried solid site has no runtime consumer and is inert.
    if !graph.power_links.iter().any(|candidate| candidate.from == site) {
        return true;
    }
    let piston_node = graph.pistons[use_.actor.index()].node;
    let untracked_dynamic_source = graph.power_links.iter().any(|candidate| {
        matches!(candidate.to, PowerTarget::Conductor { site: candidate_site, include_dust: dust } if candidate_site == site && dust == include_dust)
            && matches!(graph.sites[candidate.from.index()].source, SiteSource::Component(node) if node != reset)
    });
    !untracked_dynamic_source
        && graph.power_links.iter().any(|actor_link| {
            actor_link.from == site
                && matches!(actor_link.from_port, PowerPort::Conducted { include_dust: dust } if dust == include_dust)
                && matches!(actor_link.to, PowerTarget::Input { node, port: InputPort::Main | InputPort::Quasi } if node == piston_node)
        })
}

fn retained_cell_corridor_wire_is_owned(catalog: &BoundaryCatalog, kind: BoundaryPortKind, wire: NodeId) -> bool {
    let Some(corridor_index) = catalog.corridors.iter().position(|corridor| corridor.wires.contains(&wire)) else {
        return false;
    };
    catalog
        .ports
        .iter()
        .any(|peer| peer.owner == BoundaryOwner::Corridor(corridor_index as u32) && peer.direction == BoundaryDirection::Input && peer.kind == kind)
}

fn retained_cell_observation_is_owned(graph: &CompileGraph, cell: &CellContract, kind: BoundaryPortKind) -> bool {
    match kind {
        BoundaryPortKind::GuardState { test, .. } => match test {
            StateTest::PistonExtended(actor) | StateTest::PistonHeadPresent(actor) | StateTest::PistonAvailability { piston: actor, .. } => {
                cell.uses.iter().any(|cell_use| cell_use.actor == actor)
            }
            StateTest::ResourceAt { resource, contact } => {
                cell.resources.iter().any(|pin| pin.resource == resource) && graph.resources[resource.index()].contacts.contains(&contact)
            }
            StateTest::ResourceEnabled(resource) => cell.resources.iter().any(|pin| pin.resource == resource),
        },
        BoundaryPortKind::ContactAlternative { contact, .. } => cell.uses.iter().any(|cell_use| {
            let piston = &graph.pistons[cell_use.actor.index()];
            [piston.base, piston.near, piston.far].contains(&contact)
        }),
        BoundaryPortKind::ObserverWatch { observer, site } => cell.uses.iter().any(|cell_use| {
            let piston = &graph.pistons[cell_use.actor.index()];
            cell_use.reset.observer() == Some(observer) && graph.sites[site.index()].source == SiteSource::Contact(piston.base)
        }),
        BoundaryPortKind::GuardSite { site, .. } => {
            let source = graph.sites[site.index()].source;
            match source {
                SiteSource::Contact(contact) => cell.uses.iter().any(|cell_use| {
                    let piston = &graph.pistons[cell_use.actor.index()];
                    [piston.base, piston.near, piston.far].contains(&contact)
                }),
                SiteSource::Component(node) => cell
                    .uses
                    .iter()
                    .any(|cell_use| node == graph.pistons[cell_use.actor.index()].node || cell_use.reset.observer() == Some(node)),
                SiteSource::Constant(_) => false,
            }
        }
        _ => false,
    }
}

pub(super) fn unsupported_computation_frame_inputs(graph: &CompileGraph, circuit: &SettledCircuit) -> Vec<LayerInputSource> {
    let state_sources = computation_feedback_sources(circuit);
    circuit
        .program
        .inputs
        .iter()
        .filter_map(|input| {
            let supported = match input.source {
                LayerInputSource::State(test) if computation_feedback_input_supported(test, &state_sources) => true,
                LayerInputSource::SourceAbove {
                    site,
                    port: PowerPort::Output | PowerPort::Strong(_) | PowerPort::Weak(_),
                    threshold: 0,
                } => {
                    let Some(SiteSource::Component(node)) = graph.sites.get(site.index()).map(|site| site.source) else {
                        return Some(input.source);
                    };
                    matches!(
                        graph.nodes.get(node.index()).map(|node| &node.kind),
                        Some(CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::PressurePlate))
                    )
                }
                _ => false,
            };
            (!supported).then_some(input.source)
        })
        .collect()
}

fn computation_feedback_sources(circuit: &SettledCircuit) -> FxHashMap<ResourceId, ContactId> {
    circuit
        .outputs
        .iter()
        .filter_map(|output| match output.target {
            SettledTarget::ResourceState { resource, source } => Some((resource, source)),
            _ => None,
        })
        .collect()
}

fn computation_feedback_input_supported(test: StateTest, state_sources: &FxHashMap<ResourceId, ContactId>) -> bool {
    matches!(test, StateTest::ResourceAt { resource, contact } if state_sources.get(&resource) == Some(&contact))
}

#[cfg(test)]
mod scoped_feedback_input_tests {
    use super::*;

    #[test]
    fn scoped_frame_accepts_only_the_bound_resource_feedback_contact() {
        let resource = ResourceId::new(4);
        let source = ContactId::new(9);
        let other = ContactId::new(10);
        let state_sources = FxHashMap::from_iter([(resource, source)]);

        assert!(computation_feedback_input_supported(
            StateTest::ResourceAt { resource, contact: source },
            &state_sources
        ));
        assert!(!computation_feedback_input_supported(
            StateTest::ResourceAt { resource, contact: other },
            &state_sources
        ));
        assert!(!computation_feedback_input_supported(
            StateTest::ResourceAt {
                resource: ResourceId::new(5),
                contact: source
            },
            &state_sources
        ));
        assert!(!computation_feedback_input_supported(
            StateTest::ResourceEnabled(resource),
            &state_sources
        ));
    }
}

/// Compile terminal wires, safe terminal-repeater inputs, and command-block power as public frame outputs.
pub(super) fn frame_power_targets(graph: &CompileGraph) -> Vec<PowerTarget> {
    frame_power_targets_in(graph, None, false)
}

#[cfg(test)]
pub(super) fn selected_frame_power_targets(graph: &CompileGraph) -> Vec<PowerTarget> {
    frame_power_targets_in(graph, None, true)
}

fn frame_power_targets_in(graph: &CompileGraph, island: Option<&LogicalIsland>, include_lamps: bool) -> Vec<PowerTarget> {
    let owns_node = |node: NodeId| island.is_none_or(|island| island.nodes.binary_search(&node).is_ok());
    let outgoing_components: FxHashSet<_> = graph
        .power_links
        .iter()
        .filter_map(|link| match graph.sites.get(link.from.index())?.source {
            SiteSource::Component(node) => Some(node),
            _ => None,
        })
        .collect();
    let side_input_nodes: FxHashSet<_> = graph
        .power_links
        .iter()
        .filter_map(|link| match link.to {
            PowerTarget::Input { node, port: InputPort::Side } => Some(node),
            _ => None,
        })
        .collect();
    let terminal_repeater_inputs: FxHashSet<_> = graph
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            let id = NodeId::new(index as u32);
            if !owns_node(id) {
                return None;
            }
            let has_semantic_output_callback = match node.output_route {
                Some(route_id) => match graph.routes.get(route_id.index()) {
                    Some(route) => route.deliveries.iter().any(|delivery| {
                        let Some(site) = graph.sites.get(delivery.target.index()) else {
                            return true;
                        };
                        let receiver = delivery.receiver.or_else(|| match (site.source, delivery.kind) {
                            (SiteSource::Component(node), DeliveryKind::Update { .. }) => Some(CallbackReceiver::Component(node)),
                            _ => None,
                        });
                        receiver.is_some_and(|receiver| {
                            !terminal_repeater_receiver_is_safe(
                                receiver,
                                id,
                                matches!(receiver, CallbackReceiver::Component(receiver) if matches!(graph.nodes[receiver.index()].kind, CircuitNodeKind::Wire(_))),
                            )
                        })
                    }),
                    None => true,
                },
                None => false,
            };
            (matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Repeater { .. }))
                && !side_input_nodes.contains(&id)
                && !outgoing_components.contains(&id)
                && !has_semantic_output_callback)
                .then_some(PowerTarget::Input {
                    node: id,
                    port: InputPort::Main,
                })
        })
        .collect();
    let repeater_input_wires: FxHashSet<_> = graph
        .power_links
        .iter()
        .filter_map(|link| {
            if !terminal_repeater_inputs.contains(&link.to) {
                return None;
            }
            let SiteSource::Component(node) = graph.sites.get(link.from.index())?.source else {
                return None;
            };
            matches!(graph.nodes.get(node.index())?.kind, CircuitNodeKind::Wire(_)).then_some(node)
        })
        .collect();
    let mut targets = graph
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            let id = NodeId::new(index as u32);
            if !owns_node(id) {
                return None;
            }
            let terminal_wire =
                matches!(node.kind, CircuitNodeKind::Wire(_)) && (!outgoing_components.contains(&id) || repeater_input_wires.contains(&id));
            let command_output = matches!(node.kind, CircuitNodeKind::Electrical(NodeType::CommandBlock { .. }));
            let lamp_output = include_lamps && matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Lamp));
            (terminal_wire || command_output || lamp_output).then_some(PowerTarget::Input {
                node: id,
                port: InputPort::Main,
            })
        })
        .collect::<Vec<_>>();
    targets.extend(terminal_repeater_inputs);
    targets.sort_unstable();
    targets
}

fn terminal_repeater_receiver_is_safe(receiver: CallbackReceiver, repeater: NodeId, is_wire: bool) -> bool {
    match receiver {
        CallbackReceiver::Component(node) => node == repeater || is_wire,
        CallbackReceiver::Contact(_) | CallbackReceiver::PistonBase(_) | CallbackReceiver::PistonHead(_) => false,
    }
}

pub(crate) fn compile_settled(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<SettledCircuit, String> {
    check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
    let mut equations = Equations::new(graph, monitor).map_err(|error| format!("{error:?}"))?;
    equations.retain_classic = true;
    // Inline feed-forward repeaters; side-locked and feedback repeaters remain state leaves.
    equations.inline_repeaters = true;
    equations.stable_wire_modes = true;
    equations.defer_sources = true;
    let (owned, requests) = select_settled_boundary_targets(graph, &equations, monitor)?;
    let catalog = graph.plans.instant.boundaries.as_ref().ok_or("missing settled circuit boundaries")?;
    let external_guards = externally_used_guards(graph, &owned, monitor)?;
    let external_contact_reads = externally_read_contacts(graph, catalog, &owned, &external_guards);
    let retained_effects = catalog
        .ports
        .iter()
        .filter(|port| {
            port.direction == BoundaryDirection::Output && externally_observed(graph, port.kind, &owned, &external_guards, &external_contact_reads)
        })
        .copied()
        .collect::<Vec<_>>();
    let mut outputs = Vec::new();
    let mut declines = Vec::new();
    for target in requests {
        check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
        let kind = power_value_kind(graph, target);
        let count = if kind == SettledValueKind::Strength { 15 } else { 1 };
        let result = (0..count)
            .map(|threshold| equations.query(target, threshold))
            .collect::<Result<Vec<_>, _>>();
        match result {
            Ok(roots) => outputs.push(SettledOutput {
                target: SettledTarget::Power(target),
                kind,
                roots: roots.into_boxed_slice(),
            }),
            Err(error) => declines.push(format!("{target:?}: {error:?}")),
        }
    }
    let mut registers: Vec<_> = equations.registers.values().copied().collect();
    registers.sort_unstable_by_key(|read| read.resource.index());
    for read in registers {
        check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
        let result = equations
            .powered(read.storage)
            .and_then(|root| match graph.pistons[read.storage.index()].lane {
                PistonLane::SingleResource { far } if read.source == far => Ok(root),
                PistonLane::SingleResource { .. } if read.source == graph.pistons[read.storage.index()].near => equations.reduced.not(root),
                _ => Err("logical register source is outside its storage lane".into()),
            });
        match result {
            Ok(root) => outputs.push(SettledOutput {
                target: SettledTarget::Register(read),
                kind: SettledValueKind::Bool,
                roots: Box::new([root]),
            }),
            Err(error) => declines.push(format!("register {:?}: {error:?}", read.resource)),
        }
    }
    // Only stable outputs and next-state equations are roots. Reset acceptance and
    // internal callback occurrences never keep otherwise dead computation alive.
    let mut output_roots: Vec<_> = outputs.iter().flat_map(|output| output.roots.iter().copied()).collect();
    if let Err(error) = super::equations::lower_deferred_sources(&mut equations, &mut output_roots, monitor) {
        match error {
            super::equations::SourceLoweringError::BudgetExceeded(reason) => {
                declines.push(reason);
                return declined_settled(monitor, retained_effects, declines);
            }
            super::equations::SourceLoweringError::Unsupported(reason) => {
                declines.push(reason);
                return declined_settled(monitor, retained_effects, declines);
            }
            super::equations::SourceLoweringError::Fatal(reason) => return Err(reason),
        }
    }
    let mut output_roots = output_roots.into_iter();
    for output in &mut outputs {
        for root in &mut output.roots {
            *root = output_roots.next().ok_or("missing iteratively lowered output root")?;
        }
    }
    let roots = outputs
        .iter()
        .flat_map(|output| output.roots.iter().copied())
        .enumerate()
        .map(|(index, root)| (LayerRoot::Output(index), root))
        .collect();
    let program = reduction::reduce_roots(&equations.reduced.inputs, &equations.reduced.expressions, roots, monitor)?;
    let mut mapped = program.roots.iter();
    for output in &mut outputs {
        for root in &mut output.roots {
            *root = mapped.next().ok_or("missing reduced settled root")?.1;
        }
    }
    let circuit = SettledCircuit {
        program,
        outputs: outputs.into_boxed_slice(),
        retained_cells: Box::new([]),
        retained_effects: retained_effects.into_boxed_slice(),
        declines: declines.into_boxed_slice(),
        timing: None,
    };
    circuit.validate()?;
    Ok(circuit)
}

/// Select the complete, stably ordered power-query boundary for a settled
/// artifact.
fn select_settled_boundary_targets(
    graph: &CompileGraph,
    equations: &Equations<'_>,
    monitor: &TaskMonitor,
) -> Result<(FxHashSet<NodeId>, Vec<PowerTarget>), String> {
    check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
    let catalog = graph.plans.instant.boundaries.as_ref().ok_or("missing settled circuit boundaries")?;
    let mut owned: FxHashSet<_> = catalog.corridors.iter().flat_map(|corridor| corridor.wires.iter().copied()).collect();
    owned.extend(
        catalog
            .cells
            .iter()
            .flat_map(|cell| cell.uses.iter().filter_map(|use_| use_.reset.observer())),
    );
    owned.extend(
        catalog
            .cells
            .iter()
            .flat_map(|cell| cell.uses.iter().map(|use_| graph.pistons[use_.actor.index()].node)),
    );
    owned.extend(equations.registers.values().map(|read| graph.pistons[read.storage.index()].node));
    for actor in catalog
        .cells
        .iter()
        .flat_map(|cell| cell.uses.iter().map(|use_| use_.actor))
        .chain(equations.registers.values().map(|read| read.storage))
    {
        let piston = graph.pistons[actor.index()];
        owned.extend([piston.base, piston.near, piston.far].map(|contact| graph.contacts[contact.index()].node));
    }

    let mut requests = Vec::new();
    let mut seen = FxHashSet::default();
    for link in &graph.power_links {
        check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
        if let PowerTarget::Input { node, .. } = link.to {
            if !owned.contains(&node) && seen.insert(link.to) {
                requests.push(link.to);
            }
        }
    }
    // Stable ordering keeps repeated compilation and publication validation reproducible.
    requests.sort_unstable_by_key(|target| match target {
        PowerTarget::Input { node, port } => (node.index(), *port as usize),
        PowerTarget::Conductor { site, include_dust } => (site.index(), usize::from(*include_dust)),
    });
    Ok((owned, requests))
}

/// Compile a caller-selected set of public power queries and register next-state roots.
///
/// This artifact intentionally has no retained physical-effect frontier: its contract is
/// the selected values at a settled boundary, with the same private reset assumptions
/// used by `compile_settled`. Unsupported queries fail the build instead of becoming
/// implicit constants or partially compiled outputs.
pub(crate) fn compile_public_boundary(
    graph: &CompileGraph,
    power_targets: &[PowerTarget],
    register_reads: &[BudMemoryRead],
    monitor: &TaskMonitor,
) -> Result<SettledCircuit, String> {
    compile_boundary(graph, power_targets, register_reads, BoundaryKind::Settled, &[], monitor)
}

/// Diagnostic counterpart to `compile_public_boundary` that leaves repeaters
/// as state leaves, matching an event boundary rather than a settled frame.
#[cfg(test)]
pub(crate) fn compile_event_boundary(graph: &CompileGraph, power_targets: &[PowerTarget], monitor: &TaskMonitor) -> Result<SettledCircuit, String> {
    compile_boundary(graph, power_targets, &[], BoundaryKind::Event, &[], monitor)
}

/// Trace a live source cycle to the owning piston-input queries after constant propagation.
#[cfg(test)]
pub(crate) fn diagnostic_source_cycle_edges(graph: &CompileGraph, power_targets: &[PowerTarget], monitor: &TaskMonitor) -> Result<String, String> {
    let mut equations = Equations::new(graph, monitor).map_err(|error| format!("{error:?}"))?;
    equations.retain_classic = true;
    equations.inline_repeaters = false;
    equations.defer_sources = true;
    let mut roots = power_targets
        .iter()
        .map(|&target| equations.query(target, 0).map_err(|error| format!("{target:?}: {error:?}")))
        .collect::<Result<Vec<_>, _>>()?;
    match super::equations::lower_deferred_sources(&mut equations, &mut roots, monitor) {
        Err(super::equations::SourceLoweringError::Unsupported(_)) => {}
        Err(super::equations::SourceLoweringError::BudgetExceeded(reason)) => return Err(reason),
        Err(super::equations::SourceLoweringError::Fatal(reason)) => return Err(reason),
        Ok(()) => return Ok("no live post-propagation feedback cycle".into()),
    }
    let cycle = equations.source_cycle_witness.clone();
    if cycle.len() < 2 {
        return Err("source analysis declined without recording a closed cycle witness".into());
    }
    let scc_edges = equations.source_scc_edges.clone();
    let scc_sources: FxHashSet<_> = scc_edges.iter().flat_map(|(source, next)| [*source, *next]).collect();
    if scc_edges.is_empty() {
        return Err("post-propagation cycle had no internal SCC edges".into());
    }
    let mut incoming = FxHashMap::<PowerTarget, Vec<PowerLink>>::default();
    for link in &graph.power_links {
        incoming.entry(link.to).or_default().push(*link);
    }
    let mut resets_by_resource = FxHashMap::<ResourceId, Vec<(PistonId, CellReset)>>::default();
    if let Some(catalog) = graph.plans.instant.boundaries.as_ref() {
        for cell in &catalog.cells {
            for use_ in &cell.uses {
                for pin in &cell.resources {
                    if pin.owners.contains(&use_.actor) {
                        resets_by_resource.entry(pin.resource).or_default().push((use_.actor, use_.reset.clone()));
                    }
                }
            }
        }
    }
    let mut positive_edges = 0usize;
    let mut negative_edges = 0usize;
    let mut mixed_edges = 0usize;
    let mut unknown_edges = 0usize;
    let mut details = Vec::new();
    let mut negative_details = 0usize;
    for (source, next) in &scc_edges {
        let LayerInputSource::State(StateTest::ResourceAt { resource, contact }) = source else {
            unknown_edges += 1;
            if details.len() < 64 {
                details.push(format!("non-resource SCC edge {source:?} -> {next:?}"));
            }
            continue;
        };
        let Some((origin, owners)) = equations.cells.get(resource).cloned() else {
            unknown_edges += 1;
            if details.len() < 64 {
                details.push(format!("unowned SCC edge {resource:?}@{contact:?} -> {next:?}"));
            }
            continue;
        };
        let resource_spec = graph.resources.get(resource.index()).ok_or("cycle resource is missing")?;
        let current = graph.initial_resources.get(resource.index()).copied();
        let mut matched_queries = Vec::new();
        let mut edge_positive = false;
        let mut edge_negative = false;
        for owner in owners.iter().copied() {
            let piston = graph.pistons.get(owner.index()).ok_or("cycle owner piston is missing")?;
            for port in [InputPort::Main, InputPort::Quasi] {
                let query_target = PowerTarget::Input { node: piston.node, port };
                equations.deferred_children.remove(source);
                equations.deferred_parent = Some(*source);
                equations.expanded.clear();
                if let Ok(root) = equations.query(query_target, 0) {
                    let reached = super::equations::reachable_sources(&equations, root)?;
                    if reached.contains(next) && scc_sources.contains(next) {
                        let polarities = source_polarities(&equations, root, *next)?;
                        edge_positive |= polarities.iter().any(|polarity| polarity.starts_with("positive"));
                        edge_negative |= polarities.iter().any(|polarity| polarity.starts_with("negative"));
                        let links = incoming
                            .get(&query_target)
                            .into_iter()
                            .flatten()
                            .map(|link| {
                                let site = &graph.sites[link.from.index()];
                                format!(
                                    "from={:?}@{:?} source={:?} port={:?} attenuation={} guard={:?}",
                                    link.from,
                                    site.position,
                                    site.source,
                                    link.from_port,
                                    link.attenuation,
                                    graph.guards.get(link.guard.index())
                                )
                            })
                            .collect::<Vec<_>>();
                        matched_queries.push(format!("{query_target:?} signs={polarities:?} links={links:?}"));
                    }
                }
                equations.deferred_parent = None;
            }
        }
        if edge_positive && edge_negative {
            mixed_edges += 1;
        } else if edge_positive {
            positive_edges += 1;
        } else if edge_negative {
            negative_edges += 1;
        } else {
            unknown_edges += 1;
        }
        let reset = resets_by_resource
            .get(resource)
            .into_iter()
            .flatten()
            .filter(|(actor, _)| owners.contains(actor))
            .map(|(actor, reset)| format!("{actor:?}:{reset:?}"))
            .collect::<Vec<_>>();
        let owner_specs = owners
            .iter()
            .filter_map(|owner| {
                graph
                    .pistons
                    .get(owner.index())
                    .map(|piston| (*owner, piston.position, piston.facing, piston.sticky, piston.lane))
            })
            .collect::<Vec<_>>();
        if details.len() < 64 || (edge_negative && negative_details < 16) {
            if edge_negative {
                negative_details += 1;
            }
            details.push(format!(
                "{resource:?}@{contact:?} origin={origin:?} contacts={:?} current={current:?} owners={owner_specs:?} resource_rule=AND(owner_powered), owner_rule=OR(main,quasi) reset={reset:?} -> {next:?} signs={} via={matched_queries:?}",
                resource_spec.contacts,
                match (edge_positive, edge_negative) {
                    (true, true) => "both",
                    (true, false) => "positive",
                    (false, true) => "negative",
                    (false, false) => "unknown",
                }
            ));
        }
    }
    let mut report = format!(
        "cycle_witness_nodes={} scc_nodes={} scc_edges={} positive={positive_edges} negative={negative_edges} mixed={mixed_edges} unknown={unknown_edges} (details capped at 64 plus negative edges)\n",
        cycle.len() - 1,
        scc_sources.len(),
        scc_edges.len()
    );
    for detail in details {
        report.push_str(&detail);
        report.push('\n');
    }
    Ok(report)
}

#[cfg(test)]
fn source_polarities(equations: &Equations<'_>, root: LayerExprId, source: LayerInputSource) -> Result<Vec<String>, String> {
    let mut results = Vec::new();
    let mut visited = FxHashSet::default();
    let mut pending = vec![(root, false, Vec::<&'static str>::new())];
    while let Some((expression, negated, path)) = pending.pop() {
        if !visited.insert((expression, negated)) {
            continue;
        }
        match equations
            .reduced
            .expression(expression)
            .map_err(|error| format!("reading cycle expression: {error:?}"))?
        {
            LayerExpr::Constant(_) => {}
            LayerExpr::Input(input) => {
                let binding = equations
                    .reduced
                    .inputs
                    .get(input.0 as usize)
                    .ok_or("cycle expression input is missing")?;
                if binding.source == source {
                    results.push(format!("{} {path:?}", if negated { "negative" } else { "positive" }));
                }
            }
            LayerExpr::Not(child) => {
                let mut child_path = path;
                child_path.push("Not");
                pending.push((*child, !negated, child_path));
            }
            LayerExpr::All(children) | LayerExpr::Any(children) => {
                let operator = if matches!(equations.reduced.expression(expression), Ok(LayerExpr::All(_))) {
                    "All"
                } else {
                    "Any"
                };
                for child in children.iter().copied() {
                    let mut child_path = path.clone();
                    child_path.push(operator);
                    pending.push((child, negated, child_path));
                }
            }
        }
    }
    results.sort();
    results.dedup();
    Ok(results)
}

/// Diagnostic event-boundary extraction that leaves selected catalog-cell writers
/// in Graph and treats their resource feedback as a live state input.
#[cfg(test)]
pub(crate) fn compile_event_boundary_with_retained_resources(
    graph: &CompileGraph,
    power_targets: &[PowerTarget],
    retained_resources: &[ResourceId],
    monitor: &TaskMonitor,
) -> Result<SettledCircuit, String> {
    compile_boundary(graph, power_targets, &[], BoundaryKind::Event, retained_resources, monitor)
}

/// Diagnostic extraction that retains each catalog-cell resource responsible for a
/// typed ResourceAt recursion witness. The resource writer and its callbacks remain
/// authoritative in Graph; no logical next-state update is synthesized here.
#[cfg(test)]
pub(crate) fn compile_event_boundary_cut_feedback(
    graph: &CompileGraph,
    power_targets: &[PowerTarget],
    monitor: &TaskMonitor,
) -> Result<(SettledCircuit, Vec<ResourceId>, Vec<PistonId>), String> {
    const MAX_CUT_RESOURCES: usize = 64;
    if power_targets.is_empty() {
        return Err("event-boundary feedback extraction requires a public power target".into());
    }

    let mut retained_resources = FxHashSet::default();
    let mut retained_owners = FxHashSet::default();
    for _ in 0..=MAX_CUT_RESOURCES {
        check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
        let mut equations = Equations::new(graph, monitor).map_err(|error| format!("{error:?}"))?;
        equations.retain_classic = true;
        equations.inline_repeaters = false;
        if !retained_resources.is_empty() || !retained_owners.is_empty() {
            let resources: Vec<_> = retained_resources.iter().copied().collect();
            let owners: Vec<_> = retained_owners.iter().copied().collect();
            let (resources, owners) = equations
                .retain_cells_touching(&resources, &owners)
                .map_err(|error| format!("retaining cell state in Graph: {error:?}"))?;
            retained_resources.extend(resources);
            retained_owners.extend(owners);
        }

        match compile_boundary_equations(graph, power_targets, &[], &mut equations, monitor) {
            Ok(circuit) => {
                let mut resources: Vec<_> = retained_resources.into_iter().collect();
                resources.sort_unstable();
                let mut owners: Vec<_> = retained_owners.into_iter().collect();
                owners.sort_unstable();
                return Ok((circuit, resources, owners));
            }
            Err(error) => {
                let Some((resource, _contact)) = equations.resource_cycle_witness else {
                    return Err(error);
                };
                if !equations.cells.contains_key(&resource) || retained_resources.contains(&resource) {
                    return Err(error);
                }
                let (resources, owners) = equations
                    .retain_cells_touching(&[resource], &[])
                    .map_err(|cut_error| format!("retaining feedback resource {resource:?}: {cut_error:?}"))?;
                if resources.is_empty() || !resources.contains(&resource) {
                    return Err(format!("feedback resource {resource:?} did not select a catalog cell: {error}"));
                }
                retained_resources.extend(resources);
                retained_owners.extend(owners);
                if retained_resources.len() > MAX_CUT_RESOURCES {
                    return Err(format!("event-boundary feedback cut exceeded {MAX_CUT_RESOURCES} resources: {error}"));
                }
            }
        }
    }
    Err(format!("event-boundary feedback cut exceeded {MAX_CUT_RESOURCES} iterations"))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BoundaryKind {
    Settled,
    #[cfg(test)]
    Event,
}

fn compile_boundary(
    graph: &CompileGraph,
    power_targets: &[PowerTarget],
    register_reads: &[BudMemoryRead],
    kind: BoundaryKind,
    retained_resources: &[ResourceId],
    monitor: &TaskMonitor,
) -> Result<SettledCircuit, String> {
    check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
    if power_targets.is_empty() && register_reads.is_empty() {
        return Err("public-boundary compilation requires at least one requested root".into());
    }
    let mut equations = Equations::new(graph, monitor).map_err(|error| format!("{error:?}"))?;
    equations.retain_classic = true;
    // Event queries observe the repeater's current state; delayed propagation
    // remains owned by the ordinary graph event path. Settled
    // boundaries inline repeaters to model the sampled settled cut.
    equations.inline_repeaters = kind == BoundaryKind::Settled;
    equations.stable_wire_modes = kind == BoundaryKind::Settled;
    equations.defer_sources = true;
    equations.resource_semantics = ResourceSemantics::Settled;
    if !retained_resources.is_empty() {
        equations
            .retain_cells_touching(retained_resources, &[])
            .map_err(|error| format!("retaining cell state in Graph: {error:?}"))?;
    }

    compile_boundary_equations(graph, power_targets, register_reads, &mut equations, monitor)
}

fn compile_boundary_equations(
    graph: &CompileGraph,
    power_targets: &[PowerTarget],
    register_reads: &[BudMemoryRead],
    equations: &mut Equations<'_>,
    monitor: &TaskMonitor,
) -> Result<SettledCircuit, String> {
    check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
    let mut outputs = Vec::with_capacity(power_targets.len() + register_reads.len());
    let mut seen_power = FxHashSet::default();
    for &target in power_targets {
        check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
        if !seen_power.insert(target) {
            return Err(format!("duplicate public power target {target:?}"));
        }
        let kind = power_value_kind(graph, target);
        let count = if kind == SettledValueKind::Strength { 15 } else { 1 };
        let mut roots = Vec::with_capacity(count);
        for threshold in 0..count {
            roots.push(
                equations
                    .query(target, threshold as u8)
                    .map_err(|error| format!("public output {target:?}: {error:?}"))?,
            );
        }
        outputs.push(SettledOutput {
            target: SettledTarget::Power(target),
            kind,
            roots: roots.into_boxed_slice(),
        });
    }

    let mut seen_registers = FxHashSet::default();
    for &read in register_reads {
        check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
        if !seen_registers.insert(read.resource) {
            return Err(format!("duplicate public register root for {:?}", read.resource));
        }
        if equations.registers.get(&read.resource) != Some(&read) {
            return Err(format!("requested register read is not recognized: {read:?}"));
        }
        let root = equations
            .powered(read.storage)
            .and_then(|powered| match graph.pistons[read.storage.index()].lane {
                PistonLane::SingleResource { far } if read.source == far => Ok(powered),
                PistonLane::SingleResource { .. } if read.source == graph.pistons[read.storage.index()].near => equations.reduced.not(powered),
                _ => Err("logical register source is outside its storage lane".into()),
            })
            .map_err(|error| format!("public register {:?}: {error:?}", read.resource))?;
        outputs.push(SettledOutput {
            target: SettledTarget::Register(read),
            kind: SettledValueKind::Bool,
            roots: Box::new([root]),
        });
    }

    lower_output_roots(equations, &mut outputs, monitor)?;
    let roots = outputs
        .iter()
        .flat_map(|output| output.roots.iter().copied())
        .enumerate()
        .map(|(index, root)| (LayerRoot::Output(index), root))
        .collect();
    let program = reduction::reduce_roots(&equations.reduced.inputs, &equations.reduced.expressions, roots, monitor)?;
    let mut mapped = program.roots.iter();
    for output in &mut outputs {
        for root in &mut output.roots {
            *root = mapped.next().ok_or("missing reduced public-boundary root")?.1;
        }
    }
    let circuit = SettledCircuit {
        program,
        outputs: outputs.into_boxed_slice(),
        retained_cells: Box::new([]),
        retained_effects: Box::new([]),
        declines: Box::new([]),
        timing: None,
    };
    circuit.validate()?;
    Ok(circuit)
}

fn lower_output_roots(equations: &mut Equations<'_>, outputs: &mut [SettledOutput], monitor: &TaskMonitor) -> Result<(), String> {
    let mut roots: Vec<_> = outputs.iter().flat_map(|output| output.roots.iter().copied()).collect();
    super::equations::lower_deferred_sources(equations, &mut roots, monitor).map_err(|error| format!("{error:?}"))?;
    let mut lowered = roots.into_iter();
    for output in outputs {
        for root in &mut output.roots {
            *root = lowered.next().ok_or("missing iteratively lowered boundary root")?;
        }
    }
    if lowered.next().is_some() {
        return Err("iterative lowering returned extra boundary roots".into());
    }
    Ok(())
}

fn declined_settled(monitor: &TaskMonitor, retained_effects: Vec<BoundaryPort>, declines: Vec<String>) -> Result<SettledCircuit, String> {
    let empty = LayerBuilder::new(monitor).map_err(|error| format!("creating declined settled program: {error:?}"))?;
    let program = reduction::reduce_roots(&empty.inputs, &empty.expressions, Vec::new(), monitor)?;
    let circuit = SettledCircuit {
        program,
        outputs: Box::new([]),
        retained_cells: Box::new([]),
        retained_effects: retained_effects.into_boxed_slice(),
        declines: declines.into_boxed_slice(),
        timing: None,
    };
    circuit.validate()?;
    Ok(circuit)
}

/// Compile requested stable outputs with explicit state bits for recognized
/// shared-Far payload feedback. Unlike BUD registers, these are ordinary
/// cell resources whose current source location closes a combinational cycle.
#[cfg(test)]
pub(crate) fn compile_computation(graph: &CompileGraph, power_targets: &[PowerTarget], monitor: &TaskMonitor) -> Result<SettledCircuit, String> {
    compile_computation_with_support(graph, power_targets, monitor).map(|(circuit, _)| circuit)
}

#[cfg(test)]
pub(crate) fn computation_support(
    graph: &CompileGraph,
    power_targets: &[PowerTarget],
    monitor: &TaskMonitor,
) -> Result<(SettledCircuit, FxHashSet<NodeId>), String> {
    compile_computation_with_support(graph, power_targets, monitor)
}
#[cfg(test)]
fn compile_computation_with_support(
    graph: &CompileGraph,
    power_targets: &[PowerTarget],
    monitor: &TaskMonitor,
) -> Result<(SettledCircuit, FxHashSet<NodeId>), String> {
    compile_computation_with_reset_exclusions(graph, power_targets, &FxHashMap::default(), monitor)
}

fn compile_computation_with_reset_exclusions(
    graph: &CompileGraph,
    power_targets: &[PowerTarget],
    reset_excluded_targets: &FxHashMap<PowerTarget, (ResourceId, ContactId)>,
    monitor: &TaskMonitor,
) -> Result<(SettledCircuit, FxHashSet<NodeId>), String> {
    compile_computation_with_scope(graph, power_targets, reset_excluded_targets, &[], &[], monitor, None, false, false)
}

/// Experimental lowering for one closed owner region. Unowned reads stay live Graph leaves.
#[cfg(test)]
pub(crate) fn compile_candidate_computation<'a>(
    graph: &'a CompileGraph,
    power_targets: &[PowerTarget],
    owners: &'a crate::redpiler::analysis::instant::boundaries::OwnerMap,
    monitor: &'a TaskMonitor,
) -> Result<(SettledCircuit, FxHashSet<NodeId>), String> {
    compile_computation_with_scope(graph, power_targets, &FxHashMap::default(), &[], &[], monitor, Some(owners), false, false)
}

/// Add candidate Far-state roots for explicitly exposed, unambiguous resources.
/// These equations do not prove that a qualifying source event reached the actor.
#[cfg(test)]
pub(crate) fn compile_candidate_state_computation<'a>(
    graph: &'a CompileGraph,
    power_targets: &[PowerTarget],
    resource_roots: &[(ResourceId, ContactId)],
    owners: &'a crate::redpiler::analysis::instant::boundaries::OwnerMap,
    monitor: &'a TaskMonitor,
) -> Result<(SettledCircuit, FxHashSet<NodeId>), String> {
    compile_computation_with_scope(
        graph,
        power_targets,
        &FxHashMap::default(),
        resource_roots,
        &[],
        monitor,
        Some(owners),
        false,
        false,
    )
}

/// Keep selected resource and actor state live across an event transition.
#[cfg(test)]
pub(crate) fn compile_candidate_live_computation<'a>(
    graph: &'a CompileGraph,
    power_targets: &[PowerTarget],
    resource_roots: &[(ResourceId, ContactId)],
    owners: &'a crate::redpiler::analysis::instant::boundaries::OwnerMap,
    monitor: &'a TaskMonitor,
) -> Result<(SettledCircuit, FxHashSet<NodeId>), String> {
    compile_computation_with_scope(
        graph,
        power_targets,
        &FxHashMap::default(),
        resource_roots,
        &[],
        monitor,
        Some(owners),
        false,
        true,
    )
}

/// Experiment: derive native wire values without transferring their ownership.
#[cfg(test)]
pub(crate) fn compile_candidate_with_native_wires<'a>(
    graph: &'a CompileGraph,
    power_targets: &[PowerTarget],
    resource_roots: &[(ResourceId, ContactId)],
    owners: &'a crate::redpiler::analysis::instant::boundaries::OwnerMap,
    monitor: &'a TaskMonitor,
) -> Result<(SettledCircuit, FxHashSet<NodeId>), String> {
    compile_computation_with_scope(
        graph,
        power_targets,
        &FxHashMap::default(),
        resource_roots,
        &[],
        monitor,
        Some(owners),
        true,
        false,
    )
}

/// Idealized global read cone with explicit state outputs for a closed region.
/// This grants no ownership over the external dependencies it evaluates.
#[cfg(test)]
pub(crate) fn compile_global_boundary_computation<'a>(
    graph: &'a CompileGraph,
    power_targets: &[PowerTarget],
    resource_roots: &[(ResourceId, ContactId)],
    component_roots: &[NodeId],
    owners: &'a crate::redpiler::analysis::instant::boundaries::OwnerMap,
    monitor: &'a TaskMonitor,
) -> Result<(SettledCircuit, FxHashSet<NodeId>), String> {
    compile_global_boundary_with_state(graph, power_targets, resource_roots, component_roots, owners, monitor, false)
}

/// Experimental global read cone whose actor and resource state is supplied by
/// the batched event executor instead of synthesized from steady power.
#[cfg(test)]
pub(crate) fn compile_global_live_computation<'a>(
    graph: &'a CompileGraph,
    power_targets: &[PowerTarget],
    resource_roots: &[(ResourceId, ContactId)],
    component_roots: &[NodeId],
    owners: &'a crate::redpiler::analysis::instant::boundaries::OwnerMap,
    monitor: &'a TaskMonitor,
) -> Result<(SettledCircuit, FxHashSet<NodeId>), String> {
    compile_global_boundary_with_state(graph, power_targets, resource_roots, component_roots, owners, monitor, true)
}

#[cfg(test)]
fn compile_global_boundary_with_state<'a>(
    graph: &'a CompileGraph,
    power_targets: &[PowerTarget],
    resource_roots: &[(ResourceId, ContactId)],
    component_roots: &[NodeId],
    owners: &'a crate::redpiler::analysis::instant::boundaries::OwnerMap,
    monitor: &'a TaskMonitor,
    live_states: bool,
) -> Result<(SettledCircuit, FxHashSet<NodeId>), String> {
    for &(resource, source) in resource_roots {
        let owner = owners
            .resources
            .get(resource.index())
            .copied()
            .flatten()
            .ok_or_else(|| format!("exposed resource {resource:?} is outside the candidate"))?;
        let spec = graph.resources.get(resource.index()).ok_or("missing exposed resource")?;
        if spec
            .contacts
            .iter()
            .any(|contact| owners.contacts.get(contact.index()).copied().flatten() != Some(owner))
        {
            return Err(format!("exposed resource {resource:?} has a contact outside its owner"));
        }
        if graph.pistons.iter().enumerate().any(|(index, piston)| {
            piston.sticky && piston.lane == (PistonLane::SingleResource { far: source }) && owners.actors.get(index).copied().flatten() != Some(owner)
        }) {
            return Err(format!("exposed resource {resource:?} has an actor outside its owner"));
        }
    }
    for &node in component_roots {
        if owners.nodes.get(node.index()).copied().flatten().is_none() {
            return Err(format!("component {node:?} is outside the candidate"));
        }
    }
    compile_computation_with_scope(
        graph,
        power_targets,
        &FxHashMap::default(),
        resource_roots,
        component_roots,
        monitor,
        None,
        false,
        live_states,
    )
}

fn compile_computation_with_scope<'a>(
    graph: &'a CompileGraph,
    power_targets: &[PowerTarget],
    reset_excluded_targets: &FxHashMap<PowerTarget, (ResourceId, ContactId)>,
    resource_roots: &[(ResourceId, ContactId)],
    component_roots: &[NodeId],
    monitor: &'a TaskMonitor,
    owners: Option<&'a crate::redpiler::analysis::instant::boundaries::OwnerMap>,
    expand_native_wires: bool,
    live_states: bool,
) -> Result<(SettledCircuit, FxHashSet<NodeId>), String> {
    check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
    if power_targets.is_empty() && resource_roots.is_empty() && component_roots.is_empty() {
        return Err("logical computation requires at least one public output".into());
    }
    let mut equations = Equations::new(graph, monitor).map_err(|error| format!("{error:?}"))?;
    equations.retain_classic = true;
    equations.inline_repeaters = true;
    equations.resource_semantics = ResourceSemantics::Feedback {
        reject_unrepresented_destinations: true,
    };
    equations.stable_wire_modes = true;
    #[cfg(test)]
    {
        equations.candidate_contact_states = !resource_roots.is_empty() && !live_states;
        equations.candidate_live_states = live_states;
    }
    #[cfg(test)]
    if let Some(owners) = owners {
        equations.candidate_owners = Some(owners);
        equations.wires.retain(|node| owners.nodes.get(node.index()).copied().flatten().is_some());
        if expand_native_wires {
            equations.wires.extend(
                graph
                    .nodes
                    .iter()
                    .enumerate()
                    .filter_map(|(index, node)| matches!(node.kind, CircuitNodeKind::Wire(_)).then_some(NodeId::new(index as u32))),
            );
        }
        equations
            .resets
            .retain(|node| owners.nodes.get(node.index()).copied().flatten().is_some());
        equations
            .actors
            .retain(|actor| owners.actors.get(actor.index()).copied().flatten().is_some());
        equations
            .cells
            .retain(|resource, _| owners.resources.get(resource.index()).copied().flatten().is_some());
        equations
            .unique_cell_resources
            .retain(|resource| owners.resources.get(resource.index()).copied().flatten().is_some());
        equations
            .disabled_destinations
            .retain(|(resource, _)| owners.resources.get(resource.index()).copied().flatten().is_some());
        equations
            .registers
            .retain(|resource, _| owners.resources.get(resource.index()).copied().flatten().is_some());
    } else {
        assume_computation_frame_observers_off(&mut equations);
    }
    #[cfg(not(test))]
    let _ = expand_native_wires;
    #[cfg(not(test))]
    let _ = live_states;
    #[cfg(not(test))]
    {
        let _ = owners;
        assume_computation_frame_observers_off(&mut equations);
    }
    // This test-only extractor deliberately asks whether the local stationary lane
    // is enough to form a candidate state equation. It does not grant production
    // lifecycle/reset admission to the inferred owner.
    for (index, resource) in graph.resources.iter().enumerate() {
        let id = ResourceId::new(index as u32);
        #[cfg(test)]
        if owners.is_some_and(|owners| owners.resources.get(index).copied().flatten().is_none()) {
            continue;
        }
        if equations.cells.contains_key(&id) {
            continue;
        }
        if let Ok(Some((canonical, owners))) = infer_raw_resource_lane(&resource.contacts, &graph.pistons) {
            install_candidate_resource_lane(&mut equations, id, canonical, owners);
        }
    }

    let mut outputs = Vec::with_capacity(power_targets.len());
    let mut seen_power = FxHashSet::default();
    for &target in power_targets {
        check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
        if !seen_power.insert(target) {
            return Err(format!("duplicate logical computation output {target:?}"));
        }
        let kind = power_value_kind(graph, target);
        let reset_exclusion = reset_excluded_targets.get(&target).copied();
        let roots = (0..if kind == SettledValueKind::Strength { 15 } else { 1 })
            .map(|threshold| match reset_exclusion {
                Some((resource, contact)) => {
                    equations.query_with_state_override(target, threshold, StateTest::ResourceAt { resource, contact }, false)
                }
                None => equations.query(target, threshold),
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                let position = match target {
                    PowerTarget::Input { node, .. } => graph.nodes.get(node.index()).and_then(|node| node.position),
                    _ => None,
                };
                computation_output_error(position, error)
            })?;
        let settled_target = match reset_exclusion {
            Some((resource, contact)) => {
                let PowerTarget::Input { node, port } = target else {
                    return Err("reset-excluded computation roots must be piston power inputs".into());
                };
                SettledTarget::ResetExcludedPower {
                    node,
                    port,
                    resource,
                    contact,
                }
            }
            None => SettledTarget::Power(target),
        };
        outputs.push(SettledOutput {
            target: settled_target,
            kind,
            roots: roots.into_boxed_slice(),
        });
    }

    let mut explicit_resources = FxHashSet::default();
    #[cfg(test)]
    for &node in component_roots {
        if !matches!(
            graph.nodes.get(node.index()).map(|spec| &spec.kind),
            Some(CircuitNodeKind::Electrical(NodeType::Comparator { .. }))
        ) {
            return Err(format!("component {node:?} is not a comparator"));
        }
        let roots = (0..15)
            .map(|threshold| equations.comparator_source(node, threshold))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("component {node:?} output: {error:?}"))?;
        outputs.push(SettledOutput {
            target: SettledTarget::ComponentOutput(node),
            kind: SettledValueKind::Strength,
            roots: roots.into_boxed_slice(),
        });
    }
    #[cfg(not(test))]
    let _ = component_roots;
    let mut explicit_contacts = FxHashSet::default();
    for &(resource, source) in resource_roots {
        check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
        if !explicit_resources.insert(resource) {
            return Err(format!("duplicate exposed resource {resource:?}"));
        }
        if owners.is_some_and(|owners| owners.resources.get(resource.index()).copied().flatten().is_none()) {
            return Err(format!("exposed resource {resource:?} is outside the candidate"));
        }
        #[cfg(test)]
        if live_states {
            let spec = graph.resources.get(resource.index()).ok_or("missing live candidate resource")?;
            if !spec.contacts.contains(&source) {
                return Err(format!("resource {resource:?} source is not a contact"));
            }
            for &contact in spec.contacts.iter() {
                let root = equations
                    .leaf(LayerInputSource::State(StateTest::ResourceAt { resource, contact }))
                    .map_err(|error| format!("live resource {resource:?} state: {error:?}"))?;
                explicit_contacts.insert((resource, contact));
                outputs.push(SettledOutput {
                    target: SettledTarget::ResourceState { resource, source: contact },
                    kind: SettledValueKind::Bool,
                    roots: Box::new([root]),
                });
            }
            continue;
        }
        let (canonical, actors) = equations
            .feedback_resource_contract(resource)
            .ok_or_else(|| format!("exposed resource {resource:?} lacks a complete source lane"))?;
        if canonical != source {
            return Err(format!("exposed resource {resource:?} has a mismatched source contact"));
        }
        #[cfg(test)]
        if equations.candidate_contact_states {
            for &contact in graph.resources[resource.index()].contacts.iter() {
                let root = equations.candidate_contact_presence(resource, contact)?;
                explicit_contacts.insert((resource, contact));
                outputs.push(SettledOutput {
                    target: SettledTarget::ResourceState { resource, source: contact },
                    kind: SettledValueKind::Bool,
                    roots: Box::new([root]),
                });
            }
            continue;
        }
        if actors.len() != 1 || graph.resources[resource.index()].contacts.len() != 2 {
            return Err(format!("exposed resource {resource:?} has ambiguous destination ownership"));
        }
        let root = equations
            .shared_far_presence(&actors)
            .map_err(|error| format!("exposed resource {resource:?} state: {error:?}"))?;
        explicit_contacts.insert((resource, source));
        outputs.push(SettledOutput {
            target: SettledTarget::ResourceState { resource, source },
            kind: SettledValueKind::Bool,
            roots: Box::new([root]),
        });
    }

    // Only feedback inputs reachable from requested outputs, or from an update
    // that is itself reachable, need a synthesized next-state output. Equation
    // construction may discover leaves in branches that later reduce away.
    let requested_roots = outputs.iter().flat_map(|output| output.roots.iter().copied()).collect::<Vec<_>>();
    let feedback_updates = reachable_feedback_updates(&mut equations, &requested_roots, monitor, |equations, resource, source| {
        #[cfg(test)]
        if equations.candidate_contact_states {
            return equations.candidate_contact_presence(resource, source);
        }
        let (canonical, owners) = equations
            .feedback_resource_contract(resource)
            .ok_or_else(|| format!("feedback resource {resource:?} lacks a bounded shared-Far state contract"))?;
        if canonical != source {
            return Err(format!("feedback resource {resource:?} state source is not its canonical Far contact"));
        }
        // shortcut: the candidate model retains only common-Far presence. It
        // treats any pulling owner as removing that shared source, regardless
        // of which Near contact wins the physical destination selection.
        equations
            .shared_far_presence(&owners)
            .map_err(|error| format!("feedback resource {resource:?} update: {error:?}"))
    })?;
    for (resource, source, next_state) in feedback_updates {
        if explicit_contacts.contains(&(resource, source)) {
            continue;
        }
        outputs.push(SettledOutput {
            target: SettledTarget::ResourceState { resource, source },
            kind: SettledValueKind::Bool,
            roots: Box::new([next_state]),
        });
    }

    let roots = outputs
        .iter()
        .flat_map(|output| output.roots.iter().copied())
        .enumerate()
        .map(|(index, root)| (LayerRoot::Output(index), root))
        .collect();
    let output_roots = outputs.iter().flat_map(|output| output.roots.iter().copied()).collect::<Vec<_>>();
    let live_nonconstant = live_nonconstant_expressions(&equations.reduced.expressions, &output_roots, monitor)?;
    let mut equation_support = equations.expanded_components.clone();
    equation_support.extend(
        equations
            .signal_memo
            .live_expanded_wire_nodes(graph, &equations.expanded, &live_nonconstant),
    );
    for memo in &equations.reset_excluded_signal_memos {
        equation_support.extend(memo.live_expanded_wire_nodes(graph, &equations.expanded, &live_nonconstant));
    }
    let program = reduction::reduce_roots(&equations.reduced.inputs, &equations.reduced.expressions, roots, monitor)?;
    let mut mapped = program.roots.iter();
    for output in &mut outputs {
        for root in &mut output.roots {
            *root = mapped.next().ok_or("missing logical computation root")?.1;
        }
    }
    let circuit = SettledCircuit {
        program,
        outputs: outputs.into_boxed_slice(),
        retained_cells: Box::new([]),
        retained_effects: Box::new([]),
        declines: Box::new([]),
        timing: None,
    };
    circuit.validate()?;
    Ok((circuit, equation_support))
}

pub(super) fn reachable_feedback_updates(
    equations: &mut Equations<'_>,
    requested_roots: &[LayerExprId],
    monitor: &TaskMonitor,
    mut lower_update: impl FnMut(&mut Equations<'_>, ResourceId, ContactId) -> Result<LayerExprId, String>,
) -> Result<Vec<(ResourceId, ContactId, LayerExprId)>, String> {
    let mut liveness = FeedbackStateLiveness::default();
    liveness.add_roots(equations, requested_roots, monitor)?;
    let mut pending = liveness.take_live_feedback_sources(equations);
    let mut updates = Vec::new();
    let mut index = 0;
    while index < pending.len() {
        check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
        let (resource, source) = pending[index];
        index += 1;
        let next_state = lower_update(equations, resource, source)?;
        updates.push((resource, source, next_state));
        liveness.add_roots(equations, &[next_state], monitor)?;
        pending.extend(liveness.take_live_feedback_sources(equations));
    }
    Ok(updates)
}

#[derive(Default)]
struct FeedbackStateLiveness {
    visited: FxHashSet<LayerExprId>,
    live_resource_inputs: FxHashSet<(ResourceId, ContactId)>,
    emitted_sources: FxHashSet<(ResourceId, ContactId)>,
}

impl FeedbackStateLiveness {
    fn add_roots(&mut self, equations: &Equations<'_>, roots: &[LayerExprId], monitor: &TaskMonitor) -> Result<(), String> {
        check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
        let mut pending = roots.to_vec();
        let mut work = 0usize;
        while let Some(expression) = pending.pop() {
            work = work.checked_add(1).ok_or("feedback liveness work counter overflow")?;
            if work & 0x3fff == 0 {
                check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
            }
            if !self.visited.insert(expression) {
                continue;
            }
            let expression_index = expression.0 as usize;
            let current = equations
                .reduced
                .expressions
                .get(expression_index)
                .ok_or("feedback liveness root references a missing expression")?;
            match current {
                LayerExpr::Constant(_) => {}
                LayerExpr::Input(input) => {
                    let binding = equations
                        .reduced
                        .inputs
                        .get(input.0 as usize)
                        .ok_or("feedback liveness expression references a missing input")?;
                    if let LayerInputSource::State(StateTest::ResourceAt { resource, contact }) = binding.source {
                        self.live_resource_inputs.insert((resource, contact));
                    }
                }
                LayerExpr::Not(child) => {
                    if child.0 as usize >= expression_index {
                        return Err("feedback liveness expression has a missing or forward operand".into());
                    }
                    pending.push(*child);
                }
                LayerExpr::All(children) | LayerExpr::Any(children) => {
                    if children.iter().any(|child| child.0 as usize >= expression_index) {
                        return Err("feedback liveness expression has a missing or forward operand".into());
                    }
                    pending.extend(children.iter().copied());
                }
            }
        }
        Ok(())
    }

    fn take_live_feedback_sources(&mut self, equations: &Equations<'_>) -> Vec<(ResourceId, ContactId)> {
        equations
            .feedback_state_sources
            .iter()
            .copied()
            .filter(|source| self.live_resource_inputs.contains(source) && self.emitted_sources.insert(*source))
            .collect()
    }
}

fn live_nonconstant_expressions(expressions: &[LayerExpr], roots: &[LayerExprId], monitor: &TaskMonitor) -> Result<FxHashSet<LayerExprId>, String> {
    let mut live = vec![false; expressions.len()];
    for &root in roots {
        *live.get_mut(root.0 as usize).ok_or("observable logical equation root is missing")? = true;
    }
    let mut nonconstant = FxHashSet::default();
    for index in (0..expressions.len()).rev() {
        if index & 0x3fff == 0 && monitor.cancelled() {
            return Err("cancelled while deriving live logical equation support".into());
        }
        if !live[index] {
            continue;
        }
        let id = LayerExprId(index as u32);
        let operands: &[LayerExprId] = match &expressions[index] {
            LayerExpr::Constant(_) => &[],
            LayerExpr::Input(_) => {
                nonconstant.insert(id);
                &[]
            }
            LayerExpr::Not(value) => {
                nonconstant.insert(id);
                std::slice::from_ref(value)
            }
            LayerExpr::All(values) | LayerExpr::Any(values) => {
                nonconstant.insert(id);
                values
            }
        };
        for &operand in operands {
            if operand.0 as usize >= index {
                return Err("logical equation has a missing or forward operand".into());
            }
            live[operand.0 as usize] = true;
        }
    }
    Ok(nonconstant)
}

pub(super) fn assume_computation_frame_observers_off(equations: &mut Equations<'_>) {
    // shortcut: assumes settled observer pulses; lower their events before admitting timed observer roots.
    for (index, node) in equations.graph.nodes.iter().enumerate() {
        if matches!(
            node.kind,
            CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. } | CircuitNodeKind::Electrical(NodeType::Observer { .. })
        ) {
            equations.resets.insert(NodeId::new(index as u32));
        }
    }
}

/// Candidate-only lane inference for equation research. The caller must not use this
/// as a production cell/reset certificate.
pub(super) fn infer_raw_resource_lane(contacts: &[ContactId], pistons: &[PistonSpec]) -> Result<Option<(ContactId, Vec<PistonId>)>, String> {
    if contacts.len() < 2 || contacts.iter().copied().collect::<FxHashSet<_>>().len() != contacts.len() {
        return Ok(None);
    }
    let mut owners = Vec::new();
    let mut canonical = None;
    let mut covered = FxHashSet::default();
    for (index, piston) in pistons.iter().enumerate() {
        let PistonLane::SingleResource { far } = piston.lane else {
            continue;
        };
        if !contacts.contains(&far) || !piston.sticky {
            continue;
        }
        if !contacts.contains(&piston.near) || piston.near == far {
            return Ok(None);
        }
        if canonical.is_some_and(|previous| previous != far) {
            return Err("resource lane owners disagree on the canonical Far contact".into());
        }
        canonical = Some(far);
        owners.push(PistonId::new(index as u32));
        covered.insert(piston.near);
        covered.insert(far);
    }
    if !(1..=5).contains(&owners.len()) || contacts.iter().any(|contact| !covered.contains(contact)) {
        return Ok(None);
    }
    Ok(canonical.map(|canonical| (canonical, owners)))
}

/// `shortcut:` one Boolean records canonical Far occupancy; retain owner arbitration and hand off to Graph if that model cannot be bounded.
pub(super) fn install_candidate_resource_lane(equations: &mut Equations<'_>, resource: ResourceId, canonical: ContactId, owners: Vec<PistonId>) {
    let contacts = &equations.graph.resources[resource.index()].contacts;
    if (1..=5).contains(&owners.len()) && contacts.contains(&canonical) {
        equations.unique_cell_resources.insert(resource);
    }
    equations.cells.insert(resource, (canonical, owners.clone()));
    for actor in owners {
        equations.actors.insert(actor);
        let piston = equations.graph.pistons[actor.index()];
        equations.disabled_destinations.insert((resource, piston.near));
        for (index, node) in equations.graph.nodes.iter().enumerate() {
            let watched = match node.kind {
                CircuitNodeKind::Observer { watched, .. } | CircuitNodeKind::BuiltinReset { watched, .. } => watched,
                _ => continue,
            };
            if matches!(equations.graph.sites.get(watched.index()).map(|site| site.source), Some(SiteSource::Contact(contact)) if contact == piston.base)
            {
                equations.resets.insert(NodeId::new(index as u32));
            }
        }
    }
}

#[cfg(test)]
pub(crate) struct ComputationDiagnostic {
    pub(crate) failures: Vec<String>,
    pub(crate) unresolved_resources: Vec<ResourceId>,
    pub(crate) raw_expressions: usize,
    pub(crate) raw_inputs: usize,
    pub(crate) raw_work: usize,
    pub(crate) raw_bytes: usize,
    pub(crate) reduced_expressions: usize,
    pub(crate) reduced_inputs: usize,
    pub(crate) reduced_work: usize,
    pub(crate) reduced_bytes: usize,
}

#[cfg(test)]
pub(crate) fn diagnostic_computation_failures(
    graph: &CompileGraph,
    power_targets: &[PowerTarget],
    monitor: &TaskMonitor,
) -> Result<ComputationDiagnostic, String> {
    let mut equations = Equations::new(graph, monitor).map_err(|error| format!("{error:?}"))?;
    equations.retain_classic = true;
    equations.inline_repeaters = true;
    equations.resource_semantics = ResourceSemantics::Feedback {
        reject_unrepresented_destinations: false,
    };
    equations.stable_wire_modes = true;
    equations.diagnostic_unknown_resources = true;
    let mut failures = Vec::new();
    for &target in power_targets {
        check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
        if let Err(error) = equations.query(target, 0) {
            failures.push(format!("output {target:?}: {error:?}"));
        }
    }
    let mut state_index = 0;
    while state_index < equations.feedback_state_sources.len() {
        check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
        let (resource, source) = equations.feedback_state_sources[state_index];
        state_index += 1;
        match equations.feedback_resource_contract(resource) {
            Some((canonical, owners)) if canonical == source => {
                if let Err(error) = equations.shared_far_presence(&owners) {
                    failures.push(format!("resource state {resource:?} at {source:?}: {error:?}"));
                }
            }
            _ => failures.push(format!("resource state {resource:?} at {source:?}: no bounded shared-Far cell contract")),
        }
    }
    let unresolved_resources = equations
        .reduced
        .inputs
        .iter()
        .filter_map(|input| match input.source {
            LayerInputSource::State(StateTest::ResourceAt { resource, .. } | StateTest::ResourceEnabled(resource))
                if !equations.cells.contains_key(&resource) && !equations.registers.contains_key(&resource) =>
            {
                Some(resource)
            }
            _ => None,
        })
        .collect::<FxHashSet<_>>()
        .into_iter()
        .collect();
    Ok(ComputationDiagnostic {
        failures,
        unresolved_resources,
        raw_expressions: equations.raw.expressions.len(),
        raw_inputs: equations.raw.inputs.len(),
        raw_work: equations.raw.budget.work,
        raw_bytes: equations.raw.budget.bytes,
        reduced_expressions: equations.reduced.expressions.len(),
        reduced_inputs: equations.reduced.inputs.len(),
        reduced_work: equations.reduced.budget.work,
        reduced_bytes: equations.reduced.budget.bytes,
    })
}

fn power_value_kind(graph: &CompileGraph, target: PowerTarget) -> SettledValueKind {
    match target {
        PowerTarget::Input { node, .. }
            if matches!(
                graph.nodes[node.index()].kind,
                CircuitNodeKind::Piston(_)
                    | CircuitNodeKind::Electrical(
                        NodeType::Repeater { .. }
                            | NodeType::Torch
                            | NodeType::Lamp
                            | NodeType::CopperBulb
                            | NodeType::Hopper
                            | NodeType::Trapdoor
                            | NodeType::CommandBlock { .. }
                            | NodeType::NoteBlock { .. }
                    )
            ) =>
        {
            SettledValueKind::Bool
        }
        _ => SettledValueKind::Strength,
    }
}

/// Compile one ordinary power query with the same equation policy as settled outputs.
/// This is test-only instrumentation for checking whether retained controller queries
/// depend on logical register inputs; production output ownership is unchanged.
#[cfg(test)]
pub(crate) fn diagnostic_query(
    graph: &CompileGraph,
    target: PowerTarget,
    monitor: &TaskMonitor,
) -> Result<(ReducedLayerProgram, LayerExprId), String> {
    check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
    let mut equations = Equations::new(graph, monitor).map_err(|error| format!("{error:?}"))?;
    equations.retain_classic = true;
    equations.inline_repeaters = true;
    let root = equations.query(target, 0).map_err(|error| format!("{error:?}"))?;
    let roots = [(LayerRoot::Output(0), root)].into_iter().collect();
    let program = reduction::reduce_roots(&equations.reduced.inputs, &equations.reduced.expressions, roots, monitor)?;
    let root = program.roots.first().map(|(_, root)| *root).ok_or("diagnostic query lost its root")?;
    Ok((program, root))
}

/// Trace raw electrical support without the recognized transfer-frame disabled-Near fact.
/// Includes resource guards that are true during reset and transfer phases.
#[cfg(test)]
pub(crate) fn diagnostic_all_phase_raw_query(
    graph: &CompileGraph,
    target: PowerTarget,
    monitor: &TaskMonitor,
) -> Result<(ReducedLayerProgram, LayerExprId), String> {
    check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
    let mut equations = Equations::new(graph, monitor).map_err(|error| format!("{error:?}"))?;
    equations.retain_classic = true;
    equations.inline_repeaters = true;
    let state = FxHashMap::default();
    // Deliberately do not apply Equations::disabled_destinations here: this is the
    // support cone across every transfer phase, not just its stable published frame.
    let mut tracer = super::signals::SignalTracer::new(graph, &equations.power, &equations.wires, &state, LayerInputRole::RequestSample);
    let root = tracer.query(target, 0, &mut equations.raw).map_err(|error| format!("{error:?}"))?;
    let roots = [(LayerRoot::Output(0), root)].into_iter().collect();
    let program = reduction::reduce_roots(&equations.raw.inputs, &equations.raw.expressions, roots, monitor)?;
    let root = program.roots.first().map(|(_, root)| *root).ok_or("all-phase raw query lost its root")?;
    Ok((program, root))
}

fn externally_used_guards(graph: &CompileGraph, owned: &FxHashSet<NodeId>, monitor: &TaskMonitor) -> Result<FxHashSet<GuardId>, String> {
    let outside_site = |site: SiteId| match graph.sites[site.index()].source {
        SiteSource::Component(node) => !owned.contains(&node),
        SiteSource::Contact(contact) => !owned.contains(&graph.contacts[contact.index()].node),
        SiteSource::Constant(_) => false,
    };
    let outside_receiver = |receiver: Option<CallbackReceiver>, target: SiteId| match receiver {
        Some(CallbackReceiver::Component(node)) => !owned.contains(&node),
        Some(CallbackReceiver::PistonBase(piston) | CallbackReceiver::PistonHead(piston)) => !owned.contains(&graph.pistons[piston.index()].node),
        Some(CallbackReceiver::Contact(contact)) => !owned.contains(&graph.contacts[contact.index()].node),
        None => outside_site(target),
    };

    let mut roots = Vec::new();
    for link in &graph.power_links {
        check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
        let outside = match link.to {
            PowerTarget::Input { node, .. } => !owned.contains(&node),
            PowerTarget::Conductor { site, .. } => outside_site(site),
        };
        if outside {
            roots.push(link.guard);
        }
    }
    for route in &graph.routes {
        for delivery in route.deliveries.iter() {
            check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
            if outside_receiver(delivery.receiver, delivery.target) {
                roots.push(delivery.guard);
            }
        }
    }
    for relation in &graph.wire_relations {
        check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
        if !owned.contains(&relation.from) {
            roots.push(relation.guard);
        }
    }
    for contact in &graph.contacts {
        check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
        if !owned.contains(&contact.node) {
            roots.extend(contact.variants.iter().map(|variant| variant.when));
        }
    }
    for comparator in &graph.comparator_inputs {
        check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
        if !owned.contains(&comparator.node) {
            roots.extend(comparator.far_override.map(|(_, _, guard)| guard));
        }
    }

    let mut used = FxHashSet::default();
    let mut pending = roots;
    while let Some(guard) = pending.pop() {
        check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
        if !used.insert(guard) {
            continue;
        }
        match graph.guards.get(guard.index()).ok_or("external boundary references a missing guard")? {
            Guard::Always | Guard::State(_) | Guard::Site { .. } => {}
            Guard::Not(child) => pending.push(*child),
            Guard::All(children) | Guard::Any(children) => pending.extend(children.iter().copied()),
        }
    }
    Ok(used)
}

pub(super) fn externally_read_contacts(
    graph: &CompileGraph,
    catalog: &BoundaryCatalog,
    owned: &FxHashSet<NodeId>,
    external_guards: &FxHashSet<GuardId>,
) -> FxHashSet<ContactId> {
    let mut contacts = FxHashSet::default();
    for port in &catalog.ports {
        if port.direction != BoundaryDirection::Output {
            continue;
        }
        let observed_site = match port.kind {
            BoundaryPortKind::CapturedWire { wire, neighbor, .. } if !owned.contains(&wire) => Some(neighbor),
            BoundaryPortKind::WireNeighborhood { wire, neighbor } if !owned.contains(&wire) => Some(neighbor),
            BoundaryPortKind::GuardSite { guard, site, .. } if external_guards.contains(&guard) => Some(site),
            BoundaryPortKind::ObserverWatch { observer, site } if !owned.contains(&observer) => Some(site),
            BoundaryPortKind::FixedRead { node, site, .. } if !owned.contains(&node) => Some(site),
            _ => None,
        };
        if let Some(site) = observed_site {
            if let SiteSource::Contact(contact) = graph.sites[site.index()].source {
                contacts.insert(contact);
            }
        }
    }
    contacts
}

pub(super) fn externally_observed(
    graph: &CompileGraph,
    kind: BoundaryPortKind,
    owned: &FxHashSet<NodeId>,
    external_guards: &FxHashSet<GuardId>,
    external_contact_reads: &FxHashSet<ContactId>,
) -> bool {
    let outside_site = |site: SiteId| match graph.sites[site.index()].source {
        SiteSource::Component(node) => !owned.contains(&node),
        // A contact's occupancy/profile can be read by outside redstone or memory.
        SiteSource::Contact(contact) => !owned.contains(&graph.contacts[contact.index()].node),
        SiteSource::Constant(_) => false,
    };
    match kind {
        BoundaryPortKind::Power { link } => match graph.power_links[link as usize].to {
            PowerTarget::Input { node, .. } => !owned.contains(&node),
            PowerTarget::Conductor { site, .. } => outside_site(site),
        },
        BoundaryPortKind::Callback { occurrence } => {
            let delivery = &graph.routes[occurrence.route.index()].deliveries[occurrence.ordinal as usize];
            match delivery.receiver {
                Some(CallbackReceiver::Component(node)) => !owned.contains(&node),
                Some(CallbackReceiver::PistonBase(piston) | CallbackReceiver::PistonHead(piston)) => {
                    !owned.contains(&graph.pistons[piston.index()].node)
                }
                Some(CallbackReceiver::Contact(contact)) => !owned.contains(&graph.contacts[contact.index()].node),
                None => outside_site(delivery.target),
            }
        }
        BoundaryPortKind::ObserverWatch { observer, .. } => !owned.contains(&observer),
        BoundaryPortKind::WireRelation { relation } => outside_site(graph.wire_relations[relation as usize].to),
        BoundaryPortKind::CapturedWire { wire, .. } | BoundaryPortKind::WireNeighborhood { wire, .. } => !owned.contains(&wire),
        BoundaryPortKind::GuardSite { guard, .. } | BoundaryPortKind::GuardState { guard, .. } => external_guards.contains(&guard),
        BoundaryPortKind::ContactAlternative { contact, .. } => {
            !owned.contains(&graph.contacts[contact.index()].node) || external_contact_reads.contains(&contact)
        }
        // These non-value observations remain explicit until the adapter has a
        // contract for their geometry, comparator override, or side effect.
        _ => true,
    }
}

#[cfg(test)]
mod frame_power_target_tests {
    use super::*;

    #[test]
    fn terminal_repeater_owns_self_updates_but_not_foreign_piston_callbacks() {
        let repeater = NodeId::new(7);
        let foreign = NodeId::new(8);

        assert!(terminal_repeater_receiver_is_safe(CallbackReceiver::Component(repeater), repeater, false));
        assert!(terminal_repeater_receiver_is_safe(CallbackReceiver::Component(foreign), repeater, true));
        assert!(!terminal_repeater_receiver_is_safe(CallbackReceiver::Component(foreign), repeater, false));
        assert!(!terminal_repeater_receiver_is_safe(
            CallbackReceiver::PistonHead(PistonId::new(3)),
            repeater,
            false
        ));
    }
}

#[cfg(test)]
mod scoped_sink_tests {
    use super::*;

    #[test]
    fn scoped_frame_requires_explicit_support_for_semantic_sinks() {
        assert!(scoped_frame_sink_kind_supported(&CircuitNodeKind::Wire(0), false, false, false));
        assert!(scoped_frame_sink_kind_supported(
            &CircuitNodeKind::Electrical(NodeType::Repeater {
                delay: 1,
                facing_diode: false
            }),
            true,
            false,
            false,
        ));
        assert!(!scoped_frame_sink_kind_supported(
            &CircuitNodeKind::Electrical(NodeType::Repeater {
                delay: 1,
                facing_diode: false
            }),
            false,
            false,
            false,
        ));
        assert!(scoped_frame_sink_kind_supported(
            &CircuitNodeKind::Electrical(NodeType::Repeater {
                delay: 1,
                facing_diode: false
            }),
            false,
            true,
            false,
        ));
        assert!(!scoped_frame_sink_kind_supported(
            &CircuitNodeKind::Electrical(NodeType::Lamp),
            false,
            false,
            false,
        ));
        assert!(!scoped_frame_sink_kind_supported(
            &CircuitNodeKind::Electrical(NodeType::Lamp),
            true,
            false,
            false,
        ));
        for kind in [NodeType::Lamp, NodeType::CopperBulb, NodeType::Hopper, NodeType::Trapdoor] {
            assert!(scoped_frame_has_unmodeled_native_effect(&CircuitNodeKind::Electrical(kind)));
        }
        assert!(scoped_frame_has_unmodeled_native_effect(&CircuitNodeKind::Electrical(
            NodeType::NoteBlock {
                instrument: mchprs_blocks::blocks::Instrument::Bass,
                note: 0,
            }
        )));
        assert!(!scoped_frame_has_unmodeled_native_effect(&CircuitNodeKind::Electrical(NodeType::Lever)));
        assert!(!scoped_frame_sink_kind_supported(
            &CircuitNodeKind::Electrical(NodeType::Comparator {
                mode: mchprs_blocks::blocks::ComparatorMode::Compare,
                far_input: None,
                facing_diode: false,
            }),
            true,
            false,
            false,
        ));
        assert!(scoped_frame_sink_kind_supported(
            &CircuitNodeKind::Electrical(NodeType::Comparator {
                mode: mchprs_blocks::blocks::ComparatorMode::Compare,
                far_input: None,
                facing_diode: false,
            }),
            false,
            false,
            true,
        ));
        assert!(scoped_frame_sink_kind_supported(
            &CircuitNodeKind::Electrical(NodeType::Comparator {
                mode: mchprs_blocks::blocks::ComparatorMode::Compare,
                far_input: None,
                facing_diode: false,
            }),
            false,
            true,
            false,
        ));
        assert!(!scoped_frame_sink_kind_supported(
            &CircuitNodeKind::Electrical(NodeType::NoteBlock {
                instrument: mchprs_blocks::blocks::Instrument::Bass,
                note: 0,
            }),
            true,
            false,
            false,
        ));
        assert!(scoped_frame_sink_kind_supported(
            &CircuitNodeKind::Piston(PistonId::new(1)),
            false,
            true,
            false
        ));
        assert!(!scoped_frame_sink_kind_supported(
            &CircuitNodeKind::Piston(PistonId::new(1)),
            false,
            false,
            false,
        ));
    }

    #[test]
    fn comparator_side_sink_requires_all_strengths_to_preserve_current_state() {
        use mchprs_blocks::blocks::ComparatorMode;

        let literal = |strength| Some(ComparatorOverride::Literal(strength));
        assert!(comparator_side_state_is_invariant(
            ComparatorMode::Compare,
            literal(15),
            false,
            15,
            true,
            false
        ));
        assert!(comparator_side_state_is_invariant(
            ComparatorMode::Compare,
            literal(0),
            false,
            0,
            false,
            false
        ));

        assert!(!comparator_side_state_is_invariant(
            ComparatorMode::Compare,
            literal(15),
            false,
            14,
            true,
            false
        ));
        assert!(!comparator_side_state_is_invariant(
            ComparatorMode::Compare,
            literal(15),
            false,
            15,
            true,
            true
        ));
        assert!(!comparator_side_state_is_invariant(
            ComparatorMode::Compare,
            literal(15),
            true,
            15,
            true,
            false
        ));
        assert!(!comparator_side_state_is_invariant(
            ComparatorMode::Compare,
            literal(7),
            false,
            7,
            true,
            false
        ));
        assert!(!comparator_side_state_is_invariant(
            ComparatorMode::Subtract,
            literal(15),
            false,
            15,
            true,
            false
        ));
        assert!(!comparator_side_state_is_invariant(ComparatorMode::Compare, None, false, 15, true, false));
    }
}

#[cfg(test)]
mod positive_scope_ownership_tests {
    use super::*;

    #[test]
    fn diagnostic_positions_link_each_known_block() {
        let source = diagnostic_position(Some(mchprs_blocks::BlockPos::new(-1, 2, 3)));
        let target = diagnostic_position(Some(mchprs_blocks::BlockPos::new(4, 5, 6)));
        assert_eq!(diagnostic_position(None), "position unavailable");
        let component = crate::player::diagnostic_component(&format!("{source} to {target}"));
        let parts = component["extra"].as_array().unwrap();
        assert_eq!(parts[1]["click_event"]["command"], "/hl -1 2 3");
        assert_eq!(parts[3]["click_event"]["command"], "/hl 4 5 6");
    }

    #[test]
    fn whole_plot_budget_failure_identifies_the_output() {
        assert_eq!(
            computation_output_error(Some(mchprs_blocks::BlockPos::new(-1, 2, 3)), ConstructionError::BudgetExceeded),
            "equation output at (-1, 2, 3): equation construction exceeded its budget"
        );
    }

    #[test]
    fn computation_scope_does_not_claim_unrelated_island_members() {
        let island_nodes = [NodeId::new(1), NodeId::new(2), NodeId::new(3), NodeId::new(4)];
        let equation_support = [NodeId::new(1)].into_iter().collect();
        let output_nodes = [NodeId::new(2)].into_iter().collect();
        let retained_nodes = [NodeId::new(2)].into_iter().collect();
        let native_nodes = [NodeId::new(3)].into_iter().collect();

        let owned = positive_computation_nodes(&island_nodes, &equation_support, &output_nodes, &retained_nodes, &native_nodes).unwrap();

        assert_eq!(owned, [NodeId::new(1)]);
        assert!(!owned.contains(&NodeId::new(3)));
        assert!(!owned.contains(&NodeId::new(4)));
    }

    #[test]
    fn computation_scope_rejects_equation_support_that_is_also_native() {
        let island_nodes = [NodeId::new(1), NodeId::new(2)];
        let equation_support = [NodeId::new(1)].into_iter().collect();
        let output_nodes = FxHashSet::default();
        let retained_nodes = FxHashSet::default();
        let native_nodes = [NodeId::new(1)].into_iter().collect();

        assert!(positive_computation_nodes(&island_nodes, &equation_support, &output_nodes, &retained_nodes, &native_nodes).is_none());
        assert_eq!(
            first_native_overlap(&island_nodes, &equation_support, &output_nodes, &retained_nodes, &native_nodes),
            Some(NodeId::new(1))
        );
    }

    #[test]
    fn overlap_witness_is_deterministic_and_excludes_retained_nodes() {
        let island = [NodeId::new(1), NodeId::new(2), NodeId::new(3)];
        let support = [NodeId::new(3), NodeId::new(1)].into_iter().collect();
        let outputs = [NodeId::new(2)].into_iter().collect();
        let native = island.into_iter().collect();
        let retained = [NodeId::new(1)].into_iter().collect();
        assert_eq!(
            first_native_overlap(&island, &support, &outputs, &retained, &native),
            Some(NodeId::new(2))
        );
    }

    #[test]
    fn unclaimed_side_branch_cannot_callback_into_a_computed_node() {
        let owned = [NodeId::new(1)].into_iter().collect();
        let native = [NodeId::new(2)].into_iter().collect();
        let target = NodeId::new(1);

        assert!(!incoming_callback_source_has_boundary_proof(NodeId::new(3), target, &owned, &native));
        assert!(incoming_callback_source_has_boundary_proof(NodeId::new(2), target, &owned, &native));
        assert!(incoming_callback_source_has_boundary_proof(NodeId::new(1), target, &owned, &native));
    }
}
