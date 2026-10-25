//! Compose settled value equations; physical ordering belongs only to the boundary adapter.
use super::*;

pub(crate) fn compile_device(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<SettledCircuit, String> {
    let plan = compile_settled(graph, monitor)?;
    if graph.plans.bud_memory_reads.is_empty() {
        if let Some(computation) = try_computation_frame(graph, monitor) {
            return Ok(computation);
        }
        return compile_one_shot_device(graph, plan, monitor);
    }
    let Ok(timing) = super::timing::infer(graph, monitor) else {
        return compile_one_shot_device(graph, plan, monitor);
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
        return compile_one_shot_device(graph, plan, monitor);
    }
    let mut compact = compile_public_boundary(graph, &targets, &graph.plans.bud_memory_reads, monitor)?;
    compact.timing = Some(timing);
    Ok(compact)
}

fn try_computation_frame(graph: &CompileGraph, monitor: &TaskMonitor) -> Option<SettledCircuit> {
    let timing = super::timing::infer(graph, monitor).ok()?;
    let clock = timing.actor_specs.iter().find(|actor| actor.piston == timing.clock)?;
    if !clock.sticky || !matches!(clock.lane, PistonLane::SingleResource { .. }) {
        return None;
    }
    let Some((mut targets, retained_cells, retained_effects)) = frame_retained_output_cells(graph) else {
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
        return None;
    }
    let mut computation = match compile_computation(graph, &targets, monitor) {
        Ok(computation) => computation,
        Err(_) => return None,
    };
    let unsupported = unsupported_computation_frame_inputs(graph, &computation);
    if !computation.declines.is_empty() || !unsupported.is_empty() {
        return None;
    }
    if !computation
        .outputs
        .iter()
        .any(|output| matches!(output.target, SettledTarget::ResourceState { .. }))
    {
        return None;
    }
    for (index, node) in graph.nodes.iter().enumerate() {
        if !matches!(node.kind, CircuitNodeKind::Electrical(NodeType::CommandBlock { .. })) {
            continue;
        }
        let target = PowerTarget::Input {
            node: NodeId::new(index as u32),
            port: InputPort::Main,
        };
        let retained = retained_effects
            .iter()
            .any(|port| matches!(port.kind, BoundaryPortKind::Power { link } if graph.power_links[link as usize].to == target));
        let published = computation.outputs.iter().any(|output| output.target == SettledTarget::Power(target));
        if retained == published {
            return None;
        }
    }
    computation.retained_cells = retained_cells.into_boxed_slice();
    computation.retained_effects = retained_effects.into_boxed_slice();
    computation.timing = Some(timing);
    Some(computation)
}

/// Keep command outputs backed by a single certified payload cell in Graph's
/// exact event schedule, while the bulk FPU remains a settled computation.
fn frame_retained_output_cells(graph: &CompileGraph) -> Option<(Vec<PowerTarget>, Vec<CellUse>, Vec<BoundaryPort>)> {
    let Some(catalog) = graph.plans.instant.boundaries.as_ref() else {
        return None;
    };
    let mut targets = frame_power_targets(graph);
    let mut retained_cells = Vec::<CellUse>::new();
    let mut retained_effects = Vec::<BoundaryPort>::new();
    let command_nodes = graph
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            matches!(node.kind, CircuitNodeKind::Electrical(NodeType::CommandBlock { .. })).then_some(NodeId::new(index as u32))
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
    let state_sources: FxHashMap<_, _> = circuit
        .outputs
        .iter()
        .filter_map(|output| match output.target {
            SettledTarget::ResourceState { resource, source } => Some((resource, source)),
            _ => None,
        })
        .collect();
    circuit
        .program
        .inputs
        .iter()
        .filter_map(|input| {
            let supported = match input.source {
                LayerInputSource::State(StateTest::ResourceAt { resource, contact }) => state_sources.get(&resource) == Some(&contact),
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

/// Compile terminal wires and command-block power as public frame outputs.
pub(super) fn frame_power_targets(graph: &CompileGraph) -> Vec<PowerTarget> {
    let consumed: FxHashSet<_> = graph
        .power_links
        .iter()
        .filter_map(|link| match graph.sites.get(link.from.index())?.source {
            SiteSource::Component(node) if matches!(graph.nodes.get(node.index())?.kind, CircuitNodeKind::Wire(_)) => Some(node),
            _ => None,
        })
        .collect();
    let mut targets = graph
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            let id = NodeId::new(index as u32);
            let terminal_wire = matches!(node.kind, CircuitNodeKind::Wire(_)) && !consumed.contains(&id);
            let command_output = matches!(node.kind, CircuitNodeKind::Electrical(NodeType::CommandBlock { .. }));
            (terminal_wire || command_output).then_some(PowerTarget::Input {
                node: id,
                port: InputPort::Main,
            })
        })
        .collect::<Vec<_>>();
    targets.sort_unstable();
    targets
}

fn compile_one_shot_device(graph: &CompileGraph, fallback: SettledCircuit, monitor: &TaskMonitor) -> Result<SettledCircuit, String> {
    let Some(actor) = first_single_lamp_candidate(graph)? else {
        return Ok(fallback);
    };
    check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
    if let Some(piston) = graph.pistons.get(actor.index()) {
        let targets = [
            PowerTarget::Input {
                node: piston.node,
                port: InputPort::Main,
            },
            PowerTarget::Input {
                node: piston.node,
                port: InputPort::Quasi,
            },
        ];
        if let Ok(candidate) = compile_one_shot_boundary(graph, &targets, &[], monitor) {
            if supports_single_lamp_cell(graph, &candidate, actor) {
                return Ok(candidate);
            }
        } else {
            check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
        }
    }
    Ok(fallback)
}

pub(super) fn first_single_lamp_candidate(graph: &CompileGraph) -> Result<Option<PistonId>, String> {
    let Some(catalog) = graph.plans.instant.boundaries.as_ref() else {
        return Ok(None);
    };
    let mut ports_by_owner = FxHashMap::<BoundaryOwner, Vec<BoundaryPort>>::default();
    for port in catalog.ports.iter().copied() {
        ports_by_owner.entry(port.owner).or_default().push(port);
    }
    let mut candidate = None;
    for (cell_index, cell) in catalog.cells.iter().enumerate() {
        if cell.uses.len() != 1 {
            continue;
        }
        let use_ = &cell.uses[0];
        if use_.reset.observer().is_none() || use_.retraction.transfer.is_none() {
            continue;
        }
        let owner = BoundaryOwner::Cell(u32::try_from(cell_index).map_err(|_| "too many catalog cells")?);
        let owner_ports = ports_by_owner.get(&owner).map_or(&[][..], Vec::as_slice);
        let lamp_occurrences: FxHashSet<_> = owner_ports
            .iter()
            .filter_map(|port| match port.kind {
                BoundaryPortKind::Callback { occurrence }
                    if port.owner == owner && port.direction == BoundaryDirection::Output && callback_targets_lamp(graph, occurrence) =>
                {
                    Some(occurrence)
                }
                _ => None,
            })
            .collect();
        if lamp_occurrences.is_empty() {
            continue;
        }
        let has_owned_corridor = catalog.ports.iter().any(
            |port| matches!(port.kind, BoundaryPortKind::Callback { occurrence } if lamp_occurrences.contains(&occurrence) && port.owner != owner),
        );
        if !has_owned_corridor {
            candidate = Some(use_.actor);
            break;
        }
    }
    Ok(candidate)
}

pub(super) fn callback_targets_lamp(graph: &CompileGraph, occurrence: RouteOccurrence) -> bool {
    let Some(route) = graph.routes.get(occurrence.route.index()) else {
        return false;
    };
    let Some(delivery) = route.deliveries.get(occurrence.ordinal as usize) else {
        return false;
    };
    let Some(CallbackReceiver::Component(node)) = delivery.receiver else {
        return false;
    };
    graph
        .nodes
        .get(node.index())
        .is_some_and(|node| matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Lamp)))
}

fn supports_single_lamp_cell(graph: &CompileGraph, circuit: &SettledCircuit, actor: PistonId) -> bool {
    let Some(firing) = circuit.firing.as_ref() else {
        return false;
    };
    if firing.uses.len() != 1 || firing.uses[0].actor != actor || firing.uses[0].effect.transfer.is_none() {
        return false;
    }
    if firing.uses[0].reset.observer().is_none() {
        return false;
    }
    let Some(catalog) = graph.plans.instant.boundaries.as_ref() else {
        return false;
    };
    let Some((cell_index, _)) = catalog
        .cells
        .iter()
        .enumerate()
        .find(|(_, cell)| cell.uses.iter().any(|use_| use_.actor == actor))
    else {
        return false;
    };
    let owner = BoundaryOwner::Cell(cell_index as u32);
    let lamp_occurrences: FxHashSet<_> = circuit
        .retained_effects
        .iter()
        .filter_map(|port| match port.kind {
            BoundaryPortKind::Callback { occurrence }
                if port.owner == owner && port.direction == BoundaryDirection::Output && callback_targets_lamp(graph, occurrence) =>
            {
                Some(occurrence)
            }
            _ => None,
        })
        .collect();
    if lamp_occurrences.is_empty() {
        return false;
    }
    for port in circuit.retained_effects.iter() {
        if port.owner != owner || port.direction != BoundaryDirection::Output {
            return false;
        }
        match port.kind {
            BoundaryPortKind::Callback { occurrence } if lamp_occurrences.contains(&occurrence) => {}
            BoundaryPortKind::Power { link } => {
                let Some(link) = graph.power_links.get(link as usize) else {
                    return false;
                };
                let PowerTarget::Input { node, port: InputPort::Main } = link.to else {
                    return false;
                };
                if !matches!(
                    graph.nodes.get(node.index()).map(|node| &node.kind),
                    Some(CircuitNodeKind::Electrical(NodeType::Lamp))
                ) {
                    return false;
                }
            }
            BoundaryPortKind::GuardState { guard, .. } => {
                if !lamp_occurrences.iter().any(|occurrence| {
                    let Some(delivery) = graph
                        .routes
                        .get(occurrence.route.index())
                        .and_then(|route| route.deliveries.get(occurrence.ordinal as usize))
                    else {
                        return false;
                    };
                    guard_contains(graph, delivery.guard, guard)
                }) {
                    return false;
                }
            }
            BoundaryPortKind::ContactAlternative { contact, .. } => {
                if !lamp_occurrences.iter().any(|occurrence| {
                    graph
                        .routes
                        .get(occurrence.route.index())
                        .and_then(|route| route.deliveries.get(occurrence.ordinal as usize))
                        .and_then(|delivery| graph.sites.get(delivery.target.index()))
                        .is_some_and(|site| site.source == SiteSource::Contact(contact))
                }) {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

fn guard_contains(graph: &CompileGraph, root: GuardId, needle: GuardId) -> bool {
    let mut pending = vec![root];
    let mut seen = FxHashSet::default();
    while let Some(guard) = pending.pop() {
        if guard == needle {
            return true;
        }
        if !seen.insert(guard) {
            continue;
        }
        match graph.guards.get(guard.index()) {
            Some(Guard::Not(child)) => pending.push(*child),
            Some(Guard::All(children) | Guard::Any(children)) => pending.extend(children.iter().copied()),
            Some(Guard::Always | Guard::State(_) | Guard::Site { .. }) | None => {}
        }
    }
    false
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
        firing: None,
        retained_cells: Box::new([]),
        retained_effects: retained_effects.into_boxed_slice(),
        declines: declines.into_boxed_slice(),
        timing: None,
    };
    circuit.validate()?;
    Ok(circuit)
}

/// Select the complete, stably ordered power-query boundary for a settled
/// artifact. One-shot candidates use these same roots so they cannot silently
/// narrow the public/effect boundary compared with the legacy compiler.
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

/// Compile selected power/register roots with explicit per-actor fired-state cuts.
/// The artifact exposes power eligibility only; Graph remains authoritative for
/// callback delivery, FIFO acceptance, action execution, and reset completion.
pub(crate) fn compile_one_shot_boundary(
    graph: &CompileGraph,
    power_targets: &[PowerTarget],
    register_reads: &[BudMemoryRead],
    monitor: &TaskMonitor,
) -> Result<SettledCircuit, String> {
    compile_boundary(graph, power_targets, register_reads, BoundaryKind::OneShot, &[], monitor)
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
    OneShot,
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
    // Event and one-shot queries observe the repeater's current state; delayed
    // propagation remains owned by the ordinary graph event path. Settled
    // boundaries inline repeaters to model the sampled settled cut.
    equations.inline_repeaters = kind == BoundaryKind::Settled;
    equations.stable_wire_modes = kind == BoundaryKind::Settled;
    equations.defer_sources = true;
    {
        equations.resource_semantics = if kind == BoundaryKind::OneShot {
            ResourceSemantics::OneShot
        } else {
            ResourceSemantics::Settled
        };
        if equations.resource_semantics.uses_one_shot_firing() {
            // Reset pulses remain live boundary inputs until the event adapter proves
            // the exact completion edge that rearms each actor.
            equations.resets.clear();
        }
    }
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

    {
        if equations.resource_semantics.uses_one_shot_firing() {
            // Lower once to discover the feedback owners reachable from the requested
            // roots. Each eligibility batch can itself reveal another layer of owners,
            // so lower the complete root set again after every batch before deciding
            // that discovery has reached a fixed point.
            let mut emitted = FxHashSet::default();
            loop {
                lower_output_roots(equations, &mut outputs, monitor)?;
                let mut actors: Vec<_> = equations.fired_inputs.keys().copied().filter(|actor| !emitted.contains(actor)).collect();
                actors.sort_unstable();
                if actors.is_empty() {
                    break;
                }
                equations.defer_sources = true;
                equations.expanded.clear();
                equations.sources.clear();
                equations.pending.clear();
                equations.pending_repeaters.clear();
                for actor in actors {
                    check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
                    emitted.insert(actor);
                    let fired = equations
                        .fired_leaf(actor)
                        .map_err(|error| format!("one-shot fired input for {actor:?}: {error:?}"))?;
                    let node = graph.pistons[actor.index()].node;
                    let main = equations
                        .input_power(node, InputPort::Main)
                        .map_err(|error| format!("one-shot Main eligibility for {actor:?}: {error:?}"))?;
                    let quasi = equations
                        .input_power(node, InputPort::Quasi)
                        .map_err(|error| format!("one-shot Quasi eligibility for {actor:?}: {error:?}"))?;
                    let not_fired = equations
                        .reduced
                        .not(fired)
                        .map_err(|error| format!("one-shot eligibility for {actor:?}: {error:?}"))?;
                    let not_main = equations
                        .reduced
                        .not(main)
                        .map_err(|error| format!("one-shot eligibility for {actor:?}: {error:?}"))?;
                    let not_quasi = equations
                        .reduced
                        .not(quasi)
                        .map_err(|error| format!("one-shot eligibility for {actor:?}: {error:?}"))?;
                    let eligible = equations
                        .reduced
                        .all([not_fired, not_main, not_quasi])
                        .map_err(|error| format!("one-shot eligibility for {actor:?}: {error:?}"))?;
                    outputs.push(SettledOutput {
                        target: SettledTarget::InstantPowerEligible(actor),
                        kind: SettledValueKind::Bool,
                        roots: Box::new([eligible]),
                    });
                }
                // The lowered arena has to be queried afresh for the next batch: source
                // memo entries and expression IDs belong to the previous builder.
            }
        } else {
            lower_output_roots(equations, &mut outputs, monitor)?;
        }
    }
    let mut firing = if equations.resource_semantics.uses_one_shot_firing() && !equations.fired_inputs.is_empty() {
        let catalog = graph.plans.instant.boundaries.as_ref().ok_or("missing one-shot cell boundaries")?;
        let mut actors: Vec<_> = equations.fired_inputs.keys().copied().collect();
        actors.sort_unstable();
        let mut uses = Vec::with_capacity(actors.len());
        let mut use_indices = FxHashMap::default();
        for actor in actors {
            let mut matching = catalog.cells.iter().flat_map(|cell| cell.uses.iter()).filter(|use_| use_.actor == actor);
            let use_ = matching.next().ok_or_else(|| format!("fired actor {actor:?} has no catalog use"))?;
            if matching.next().is_some() {
                return Err(format!("fired actor {actor:?} has ambiguous catalog uses"));
            }
            let power_eligible_output = outputs
                .iter()
                .position(|output| output.target == SettledTarget::InstantPowerEligible(actor))
                .and_then(|index| u32::try_from(index).ok())
                .ok_or_else(|| format!("fired actor {actor:?} has no power eligibility root"))?;
            let use_index = u32::try_from(uses.len()).map_err(|_| "too many one-shot uses")?;
            use_indices.insert(actor, use_index);
            uses.push(LogicalFireUse {
                actor,
                fired_input: equations.fired_inputs[&actor],
                power_eligible_output,
                reset: use_.reset.clone(),
                activation_routes: use_.activations.clone(),
                effect: use_.retraction.clone(),
            });
        }
        let mut resources = equations
            .firing_resources
            .iter()
            .map(|(&resource, &(source, ref owners))| {
                let mut owner_indices = owners
                    .iter()
                    .map(|owner| {
                        use_indices
                            .get(owner)
                            .copied()
                            .ok_or_else(|| format!("firing resource {resource:?} has unbound owner {owner:?}"))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                owner_indices.sort_unstable();
                owner_indices.dedup();
                Ok(LogicalFireResource {
                    resource,
                    source,
                    owners: owner_indices.into_boxed_slice(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        resources.sort_unstable_by_key(|row| row.resource);
        Some(LogicalFiringPlan {
            uses: uses.into_boxed_slice(),
            resources: resources.into_boxed_slice(),
        })
    } else {
        None
    };
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
    if let Some(firing) = &mut firing {
        for use_ in &mut firing.uses {
            use_.fired_input = program
                .inputs
                .iter()
                .position(|input| input.source == LayerInputSource::InstantFired { actor: use_.actor })
                .and_then(|index| u32::try_from(index).ok().map(LayerInputId))
                .ok_or_else(|| format!("reduced program lost fired input for {:?}", use_.actor))?;
        }
    }
    let retained_effects = if equations.resource_semantics.uses_one_shot_firing() {
        firing
            .as_ref()
            .map(|plan| logical_action_retained_effects(graph, plan, monitor))
            .transpose()?
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let circuit = SettledCircuit {
        program,
        outputs: outputs.into_boxed_slice(),
        firing,
        retained_cells: Box::new([]),
        retained_effects: retained_effects.into_boxed_slice(),
        declines: Box::new([]),
        timing: None,
    };
    circuit.validate()?;
    Ok(circuit)
}

/// Keep only catalogued outputs that leave the one-shot cell/corridor closure.
/// Internal callback rows remain represented by their original source-derived
/// activation/retraction records and are omitted from the external effect list.
fn logical_action_retained_effects(graph: &CompileGraph, firing: &LogicalFiringPlan, monitor: &TaskMonitor) -> Result<Vec<BoundaryPort>, String> {
    let catalog = graph
        .plans
        .instant
        .boundaries
        .as_ref()
        .ok_or("one-shot actions require the source boundary catalog")?;
    let active_actors: FxHashSet<_> = firing.uses.iter().map(|use_| use_.actor).collect();
    let mut active_cells = FxHashSet::default();
    for (index, cell) in catalog.cells.iter().enumerate() {
        if cell.uses.iter().any(|use_| active_actors.contains(&use_.actor)) {
            active_cells.insert(BoundaryOwner::Cell(u32::try_from(index).map_err(|_| "too many catalog cells")?));
        }
    }

    // A shared occurrence is an exact source-derived connection between catalog
    // owners. Corridors may bridge several actor cells; unrelated cells stay outside.
    let mut occurrence_owners = FxHashMap::<RouteOccurrence, Vec<(BoundaryOwner, BoundaryDirection)>>::default();
    for port in catalog.ports.iter() {
        if let BoundaryPortKind::Callback { occurrence } = port.kind {
            occurrence_owners.entry(occurrence).or_default().push((port.owner, port.direction));
        }
    }
    let allowed = |owner: BoundaryOwner| match owner {
        BoundaryOwner::Cell(_) => active_cells.contains(&owner),
        BoundaryOwner::Corridor(_) => true,
    };
    let mut adjacency = FxHashMap::<BoundaryOwner, FxHashSet<BoundaryOwner>>::default();
    for owners in occurrence_owners.values_mut() {
        owners.sort_unstable_by_key(|(owner, direction)| {
            let owner = match owner {
                BoundaryOwner::Cell(index) => (0, *index),
                BoundaryOwner::Corridor(index) => (1, *index),
            };
            (owner, *direction as u8)
        });
        owners.dedup();
        let outputs: Vec<_> = owners
            .iter()
            .filter_map(|(owner, direction)| (*direction == BoundaryDirection::Output && allowed(*owner)).then_some(*owner))
            .collect();
        let inputs: Vec<_> = owners
            .iter()
            .filter_map(|(owner, direction)| (*direction == BoundaryDirection::Input && allowed(*owner)).then_some(*owner))
            .collect();
        for &output in &outputs {
            for &input in &inputs {
                if output != input {
                    adjacency.entry(output).or_default().insert(input);
                    adjacency.entry(input).or_default().insert(output);
                }
            }
        }
    }
    let mut owners = active_cells.clone();
    let mut pending: Vec<_> = active_cells.iter().copied().collect();
    while let Some(owner) = pending.pop() {
        check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
        for &next in adjacency.get(&owner).into_iter().flatten() {
            if owners.insert(next) {
                pending.push(next);
            }
        }
    }

    let mut owned_nodes = FxHashSet::default();
    for owner in &owners {
        match *owner {
            BoundaryOwner::Cell(index) => {
                let cell = catalog.cells.get(index as usize).ok_or("one-shot owner references a missing cell")?;
                for use_ in cell.uses.iter() {
                    let actor = graph.pistons.get(use_.actor.index()).ok_or("one-shot cell references a missing piston")?;
                    owned_nodes.insert(actor.node);
                    if let Some(reset) = use_.reset.observer() {
                        owned_nodes.insert(reset);
                    }
                    for contact in [actor.base, actor.near, actor.far] {
                        if let Some(contact) = graph.contacts.get(contact.index()) {
                            owned_nodes.insert(contact.node);
                        }
                    }
                }
                for pin in cell.resources.iter() {
                    if let Some(contact) = graph.contacts.get(pin.source.index()) {
                        owned_nodes.insert(contact.node);
                    }
                }
            }
            BoundaryOwner::Corridor(index) => {
                let corridor = catalog
                    .corridors
                    .get(index as usize)
                    .ok_or("one-shot owner references a missing corridor")?;
                owned_nodes.extend(corridor.wires.iter().copied());
                owned_nodes.extend(corridor.conductive_sites.iter().filter_map(|site| {
                    graph.sites.get(site.index()).and_then(|site| match site.source {
                        SiteSource::Component(node) => Some(node),
                        SiteSource::Contact(contact) => graph.contacts.get(contact.index()).map(|contact| contact.node),
                        SiteSource::Constant(_) => None,
                    })
                }));
            }
        }
    }
    let external_guards = externally_used_guards(graph, &owned_nodes, monitor)?;
    let external_contact_reads = externally_read_contacts(graph, catalog, &owned_nodes, &external_guards);
    Ok(catalog
        .ports
        .iter()
        .filter(|port| {
            port.direction == BoundaryDirection::Output
                && owners.contains(&port.owner)
                && externally_observed(graph, port.kind, &owned_nodes, &external_guards, &external_contact_reads)
        })
        .copied()
        .collect())
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
        firing: None,
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
pub(crate) fn compile_computation(graph: &CompileGraph, power_targets: &[PowerTarget], monitor: &TaskMonitor) -> Result<SettledCircuit, String> {
    check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
    if power_targets.is_empty() {
        return Err("logical computation requires at least one public output".into());
    }
    let mut equations = Equations::new(graph, monitor).map_err(|error| format!("{error:?}"))?;
    equations.retain_classic = true;
    equations.inline_repeaters = true;
    equations.resource_semantics = ResourceSemantics::Feedback {
        reject_unrepresented_destinations: true,
    };
    equations.stable_wire_modes = true;
    assume_computation_frame_observers_off(&mut equations);
    // This test-only extractor deliberately asks whether the local stationary lane
    // is enough to form a candidate state equation. It does not grant production
    // lifecycle/reset admission to the inferred owner.
    for (index, resource) in graph.resources.iter().enumerate() {
        let id = ResourceId::new(index as u32);
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
        let roots = (0..if kind == SettledValueKind::Strength { 15 } else { 1 })
            .map(|threshold| equations.query(target, threshold))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("logical computation output {target:?}: {error:?}"))?;
        outputs.push(SettledOutput {
            target: SettledTarget::Power(target),
            kind,
            roots: roots.into_boxed_slice(),
        });
    }

    // Next-state equations can expose additional feedback resources. Walk this
    // growing set until every discovered state has exactly one update root.
    let mut state_index = 0;
    while state_index < equations.feedback_state_sources.len() {
        check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
        let (resource, source) = equations.feedback_state_sources[state_index];
        state_index += 1;
        let (canonical, owners) = equations
            .feedback_resource_contract(resource)
            .ok_or_else(|| format!("feedback resource {resource:?} lacks a bounded shared-Far state contract"))?;
        if canonical != source {
            return Err(format!("feedback resource {resource:?} state source is not its canonical Far contact"));
        }
        // shortcut: the candidate model retains only common-Far presence. It
        // treats any pulling owner as removing that shared source, regardless
        // of which Near contact wins the physical destination selection.
        let next_state = equations
            .shared_far_presence(&owners)
            .map_err(|error| format!("feedback resource {resource:?} update: {error:?}"))?;
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
        firing: None,
        retained_cells: Box::new([]),
        retained_effects: Box::new([]),
        declines: Box::new([]),
        timing: None,
    };
    circuit.validate()?;
    Ok(circuit)
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
