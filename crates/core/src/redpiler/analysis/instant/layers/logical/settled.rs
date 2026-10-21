//! Compose settled value equations; physical ordering belongs only to the boundary adapter.
use super::*;

pub(crate) fn compile_device(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<SettledCircuit, String> {
    let plan = compile_settled(graph, monitor)?;
    if graph.plans.bud_memory_reads.is_empty() {
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

pub(crate) fn compile_settled(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<SettledCircuit, String> {
    check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
    let mut equations = Equations::new(graph, monitor).map_err(|error| format!("{error:?}"))?;
    equations.retain_classic = true;
    equations.inline_repeaters = true;
    let catalog = graph.plans.instant.boundaries.as_ref().ok_or("missing settled circuit boundaries")?;
    let mut owned: FxHashSet<_> = catalog.corridors.iter().flat_map(|corridor| corridor.wires.iter().copied()).collect();
    owned.extend(equations.resets.iter().copied());
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
        retained_effects: retained_effects.into_boxed_slice(),
        declines: declines.into_boxed_slice(),
        timing: None,
    };
    circuit.validate()?;
    Ok(circuit)
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
    check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
    if power_targets.is_empty() && register_reads.is_empty() {
        return Err("public-boundary compilation requires at least one requested root".into());
    }
    let mut equations = Equations::new(graph, monitor).map_err(|error| format!("{error:?}"))?;
    equations.retain_classic = true;
    equations.inline_repeaters = true;

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
        retained_effects: Box::new([]),
        declines: Box::new([]),
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
    check_cancelled(monitor).map_err(|error| format!("{error:?}"))?;
    if power_targets.is_empty() {
        return Err("logical computation requires at least one public output".into());
    }
    let mut equations = Equations::new(graph, monitor).map_err(|error| format!("{error:?}"))?;
    equations.retain_classic = true;
    equations.inline_repeaters = true;
    equations.feedback_resources = true;
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
        // The first threshold is the public Boolean level; it preserves 0 vs
        // any powered output while avoiding fifteen redundant strength roots.
        let root = equations
            .query(target, 0)
            .map_err(|error| format!("logical computation output {target:?}: {error:?}"))?;
        outputs.push(SettledOutput {
            target: SettledTarget::Power(target),
            kind: SettledValueKind::Bool,
            roots: Box::new([root]),
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
        retained_effects: Box::new([]),
        declines: Box::new([]),
        timing: None,
    };
    circuit.validate()?;
    Ok(circuit)
}

#[cfg(test)]
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
#[cfg(test)]
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

/// Install a local lane only in the test candidate compiler. `shortcut:` this assumes
/// inferred payload lanes are resettable computation cells during a data frame; native
/// trajectory validation must confirm that frame separately from observable tick timing.
#[cfg(test)]
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
    equations.feedback_resources = true;
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
