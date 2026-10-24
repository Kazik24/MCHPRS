//! Test-only checks of source-derived one-shot eligibility at native graph cuts.
use super::*;

const MAX_RECORDS: usize = 64;

#[derive(Debug, Clone, Copy)]
enum Cut {
    Sample,
    Accepted,
    ResetPublication,
}

#[derive(Debug, Clone)]
pub(crate) struct Record {
    pub(crate) actor: PistonId,
    pub(crate) tick: u64,
    pub(crate) phase: &'static str,
    pub(crate) cut: &'static str,
    pub(crate) fired: bool,
    pub(crate) candidate_main: bool,
    pub(crate) candidate_quasi: bool,
    pub(crate) actual_main: u8,
    pub(crate) actual_quasi: u8,
    /// Bounded source evidence captured only for a candidate/native mismatch.
    pub(crate) mismatch_evidence: String,
}

#[derive(Debug, Clone)]
pub(crate) struct Report {
    pub(crate) actor: PistonId,
    pub(crate) queried_actors: usize,
    pub(crate) total_accepted: u64,
    pub(crate) samples: u64,
    pub(crate) selected_accepts: u64,
    pub(crate) reset_publications: u64,
    pub(crate) main_matches: u64,
    pub(crate) quasi_matches: u64,
    pub(crate) main_mismatches: u64,
    pub(crate) quasi_mismatches: u64,
    pub(crate) combined_matches: u64,
    pub(crate) combined_mismatches: u64,
    pub(crate) evaluation_errors: u64,
    pub(crate) last_error: Option<String>,
    pub(crate) records: Vec<Record>,
    pub(crate) initial_state: String,
    /// Unique live leaves used by queried power roots vs. fire-eligibility roots.
    pub(crate) source_summary: String,
}

pub(crate) struct Probe {
    queries: Box<[PowerQuery]>,
    query_by_actor: Box<[Option<usize>]>,
    circuit: SettledCircuit,
    evaluation: crate::redpiler::program_graph::LogicalFiringEvaluation,
    use_by_actor: Box<[Option<usize>]>,
    fired: Box<[bool]>,
    reset_pending: Box<[bool]>,
    reset_uses_by_actor: Box<[Box<[usize]>]>,
    changes: Vec<(LayerInputId, bool)>,
    report: Report,
}

#[derive(Debug, Clone)]
struct PowerQuery {
    actor: PistonId,
    node: NodeId,
    main_root: usize,
    quasi_root: usize,
    input_slots: Box<[LayerInputId]>,
}

impl Probe {
    pub(crate) fn new(actor: PistonId, backend: &GraphBackend, circuit: SettledCircuit) -> Result<Self, String> {
        circuit.validate()?;
        if actor.index() >= backend.program.pistons().len() {
            return Err(format!("logical firing probe actor {actor:?} is out of range"));
        }
        let mut roots_by_node = vec![[None, None]; backend.program.nodes().len()];
        for output in circuit.outputs.iter() {
            let SettledTarget::Power(PowerTarget::Input { node, port }) = output.target else {
                continue;
            };
            if output.kind != SettledValueKind::Bool || output.roots.len() != 1 {
                continue;
            }
            let Some(roots) = roots_by_node.get_mut(node.index()) else { continue };
            roots[match port {
                InputPort::Main => 0,
                InputPort::Quasi => 1,
                InputPort::Side => continue,
            }] = Some(output.roots[0].0 as usize);
        }
        let mut queries = Vec::new();
        let mut query_by_actor = vec![None; backend.program.pistons().len()];
        for (index, spec) in backend.program.pistons().iter().enumerate() {
            let Some([Some(main_root), Some(quasi_root)]) = roots_by_node.get(spec.node.index()).copied() else {
                continue;
            };
            let actor = PistonId::new(index as u32);
            query_by_actor[index] = Some(queries.len());
            queries.push(PowerQuery {
                actor,
                node: spec.node,
                main_root,
                quasi_root,
                input_slots: input_dependencies(&circuit.program, &[main_root, quasi_root]).into_boxed_slice(),
            });
        }
        if query_by_actor.get(actor.index()).copied().flatten().is_none() {
            return Err("firing probe circuit is missing the selected actor Main/Quasi roots".to_owned());
        }
        validate_reset_entry(backend, &circuit)?;
        let mut initial_inputs = Vec::with_capacity(circuit.program.inputs.len());
        for input in circuit.program.inputs.iter() {
            initial_inputs.push(match input.source {
                LayerInputSource::InstantFired { .. } => false,
                source => backend.sample_settled_input(source).map_err(|error| error.to_owned())?,
            });
        }
        let evaluation = crate::redpiler::program_graph::LogicalFiringEvaluation::new(&circuit, backend.program.pistons().len(), &initial_inputs)?;
        let uses = circuit.firing.as_ref().map_or(&[][..], |plan| plan.uses.as_ref());
        let actor_count = backend.program.pistons().len();
        let mut use_by_actor = vec![None; actor_count];
        for (use_index, use_) in uses.iter().enumerate() {
            use_by_actor[use_.actor.index()] = Some(use_index);
        }
        let use_count = uses.len();
        let source_summary = summarize_sources(&circuit, &queries);
        let mut restore_routes = vec![Vec::<(PistonId, ContactId, bool)>::new(); backend.program.routes().len()];
        for (index, spec) in backend.program.pistons().iter().enumerate() {
            let actor = PistonId::new(index as u32);
            if let Some(route) = spec.routes.resource_restored_near {
                restore_routes[route.index()].push((actor, spec.near, false));
            }
            if let Some(route) = spec.routes.resource_restored_far {
                restore_routes[route.index()].push((actor, spec.far, true));
            }
        }
        let mut reset_uses_by_actor = vec![Vec::new(); actor_count];
        for (use_index, use_) in uses.iter().enumerate() {
            match &use_.reset {
                CellReset::Observer { wake, .. } => reset_uses_by_actor[wake.watched_actor.index()].push(use_index),
                CellReset::Payload { notification, .. } => {
                    for &(source, _, extending) in &restore_routes[notification.route.index()] {
                        if !extending {
                            reset_uses_by_actor[source.index()].push(use_index);
                        }
                    }
                }
                CellReset::InputRestoration { contact, notification, .. } => {
                    for &(source, source_contact, extending) in &restore_routes[notification.route.index()] {
                        if source_contact == *contact && extending == (source_contact == backend.program.pistons()[source.index()].far) {
                            reset_uses_by_actor[source.index()].push(use_index);
                        }
                    }
                }
            }
        }
        let initial_state = format_initial_state(backend, &circuit);
        let query_count = queries.len();
        Ok(Self {
            queries: queries.into_boxed_slice(),
            query_by_actor: query_by_actor.into_boxed_slice(),
            changes: Vec::with_capacity(circuit.program.inputs.len()),
            circuit,
            evaluation,
            fired: vec![false; use_by_actor.len()].into_boxed_slice(),
            reset_pending: vec![false; use_count].into_boxed_slice(),
            reset_uses_by_actor: reset_uses_by_actor
                .into_iter()
                .map(Vec::into_boxed_slice)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            use_by_actor: use_by_actor.into_boxed_slice(),
            report: Report {
                actor,
                queried_actors: query_count,
                total_accepted: 0,
                samples: 0,
                selected_accepts: 0,
                reset_publications: 0,
                main_matches: 0,
                quasi_matches: 0,
                main_mismatches: 0,
                quasi_mismatches: 0,
                combined_matches: 0,
                combined_mismatches: 0,
                evaluation_errors: 0,
                last_error: None,
                records: Vec::new(),
                initial_state,
                source_summary,
            },
        })
    }

    fn observe(&mut self, backend: &GraphBackend, actor: PistonId, cut: Cut) {
        let Some(query_index) = self.query_by_actor.get(actor.index()).copied().flatten() else {
            return;
        };
        let query = &self.queries[query_index];
        self.changes.clear();
        for &input_id in query.input_slots.iter() {
            let input = &self.circuit.program.inputs[input_id.0 as usize];
            if matches!(input.source, LayerInputSource::InstantFired { .. }) {
                continue;
            }
            match backend.sample_settled_input(input.source) {
                Ok(value) => self.changes.push((input_id, value)),
                Err(error) => {
                    self.fail(error);
                    return;
                }
            }
        }
        if let Err(error) = self.evaluation.update_inputs(&self.changes) {
            self.fail(&error);
            return;
        }
        let candidate_main = match self.evaluation.expression_value(query.main_root) {
            Ok(value) => value,
            Err(error) => {
                self.fail(&error);
                return;
            }
        };
        let candidate_quasi = match self.evaluation.expression_value(query.quasi_root) {
            Ok(value) => value,
            Err(error) => {
                self.fail(&error);
                return;
            }
        };
        let main = backend.input_strength(query.node, InputPort::Main);
        let quasi = backend.input_strength(query.node, InputPort::Quasi);
        self.report.main_matches += u64::from(candidate_main == (main > 0));
        self.report.main_mismatches += u64::from(candidate_main != (main > 0));
        self.report.quasi_matches += u64::from(candidate_quasi == (quasi > 0));
        self.report.quasi_mismatches += u64::from(candidate_quasi != (quasi > 0));
        let candidate_powered = candidate_main || candidate_quasi;
        let actual_powered = main > 0 || quasi > 0;
        self.report.combined_matches += u64::from(candidate_powered == actual_powered);
        self.report.combined_mismatches += u64::from(candidate_powered != actual_powered);
        match cut {
            Cut::Sample => self.report.samples += 1,
            Cut::Accepted => self.report.selected_accepts += 1,
            Cut::ResetPublication => {}
        }
        let mismatched = candidate_main != (main > 0) || candidate_quasi != (quasi > 0);
        let record = Record {
            tick: backend.future.clock.logical_tick,
            phase: phase_name(backend.future.clock.phase),
            cut: match cut {
                Cut::Sample => "sample",
                Cut::Accepted => "accepted",
                Cut::ResetPublication => "reset-publication",
            },
            actor: query.actor,
            fired: self.fired[query.actor.index()],
            candidate_main,
            candidate_quasi,
            actual_main: main,
            actual_quasi: quasi,
            mismatch_evidence: if mismatched {
                mismatch_source_evidence(backend, query, &self.circuit, &self.changes)
            } else {
                String::new()
            },
        };
        if mismatched && self.report.records.len() < MAX_RECORDS {
            self.report.records.push(record);
        }
    }

    fn accepted(&mut self, backend: &GraphBackend, request: PistonRequest, accepted: bool) {
        if !accepted {
            return;
        }
        if self.query_by_actor.get(request.piston.index()).copied().flatten().is_some() {
            self.observe(backend, request.piston, Cut::Accepted);
        }
        if !matches!(
            request.action,
            mchprs_world::PistonAction::Retract | mchprs_world::PistonAction::RetractWithoutPull
        ) {
            return;
        }
        if self.use_by_actor.get(request.piston.index()).copied().flatten().is_none() {
            return;
        }
        self.report.total_accepted += 1;
        if let Err(error) = self.evaluation.accept_notify(request.piston) {
            self.fail(&error);
        } else {
            self.fired[request.piston.index()] = true;
        }
    }

    fn sampled(&mut self, backend: &GraphBackend, actor: PistonId) {
        self.observe(backend, actor, Cut::Sample);
    }

    fn reset_publication(&mut self, backend: &GraphBackend, event: DeferredPistonEvent) {
        let source_actor_valid = backend
            .state
            .pistons
            .get(event.piston.index())
            .is_some_and(|state| state.revision == event.revision && state.availability == PistonAvailability::Ready);
        let resource_published =
            event.resource.is_some_and(|resource_event| {
                backend.state.resources.get(resource_event.resource.index()).is_some_and(|state| {
                    state.revision == resource_event.revision.wrapping_add(1) && state.at == resource_event.contact && state.enabled
                })
            });
        let mut reset_started = Vec::new();
        for &use_index in self.reset_uses_by_actor.get(event.piston.index()).into_iter().flatten() {
            let use_ = &self.circuit.firing.as_ref().expect("validated firing plan").uses[use_index];
            let matched = source_actor_valid
                && match &use_.reset {
                    CellReset::Observer { wake, .. } => !event.extending && event.piston == wake.watched_actor,
                    CellReset::Payload { resource, notification, .. } => {
                        let spec = backend.program.pistons()[event.piston.index()];
                        !event.extending
                            && resource_published
                            && event.resource.is_some_and(|r| r.resource == *resource && r.contact == spec.near)
                            && spec.routes.resource_restored_near == Some(notification.route)
                    }
                    CellReset::InputRestoration {
                        resource,
                        contact,
                        notification,
                        ..
                    } => {
                        let spec = backend.program.pistons()[event.piston.index()];
                        let source_route = if *contact == spec.far {
                            spec.routes.resource_restored_far
                        } else if *contact == spec.near {
                            spec.routes.resource_restored_near
                        } else {
                            None
                        };
                        resource_published
                            && event.resource.is_some_and(|r| r.resource == *resource && r.contact == *contact)
                            && source_route == Some(notification.route)
                    }
                };
            if matched {
                reset_started.push(use_index);
            }
        }
        for use_index in reset_started {
            self.reset_pending[use_index] = true;
        }

        if !event.extending || !deferred_actor_published(backend, event) {
            return;
        }
        if let Some(use_index) = self.use_by_actor.get(event.piston.index()).copied().flatten()
            && self.fired[event.piston.index()]
            && self.reset_pending[use_index]
        {
            let reset_complete = match &self.circuit.firing.as_ref().expect("validated firing plan").uses[use_index].reset {
                CellReset::Observer { node, .. } => self.observer_idle(backend, *node),
                CellReset::Payload { .. } | CellReset::InputRestoration { .. } => true,
            };
            if !reset_complete {
                return;
            }
            let actor = event.piston;
            if let Err(error) = self.evaluation.reset_notify(actor) {
                self.fail(&error);
            } else {
                self.fired[actor.index()] = false;
                self.reset_pending[use_index] = false;
                self.report.reset_publications += 1;
                self.observe(backend, actor, Cut::ResetPublication);
            }
        }
    }

    fn fail(&mut self, error: &str) {
        self.report.evaluation_errors += 1;
        self.report.last_error = Some(error.to_owned());
    }

    pub(crate) fn into_report(self) -> Report {
        self.report
    }
}

fn mismatch_source_evidence(backend: &GraphBackend, query: &PowerQuery, circuit: &SettledCircuit, sampled_inputs: &[(LayerInputId, bool)]) -> String {
    const MAX_SOURCES: usize = 12;
    let actor = &backend.program.nodes()[query.node.index()];
    let mut evidence = format!("actor={:?}@{:?} kind={:?}", query.actor, actor.position, actor.kind);
    if let Some((net, attenuation, source_attenuation)) = backend.program.classic().input_by_node[query.node.index()] {
        let source = backend.program.classic().nets[net as usize].source;
        let source_state = match backend.state.components.get(source.index()) {
            Some(ComponentState::Electrical(state)) => format!("powered:{}, strength:{}", state.powered, state.output_strength),
            _ => "non-electrical".to_owned(),
        };
        evidence.push_str(&format!(
            " classic_main={{net:{net}, active:{}, signal:{}, attenuation:{attenuation}, source_attenuation:{source_attenuation}, source:{source:?}@{:?}/{:?}, source_state:{source_state}}}",
            backend.state.classic_active[net as usize],
            backend.state.classic_signals[net as usize],
            backend.program.nodes()[source.index()].position,
            backend.program.nodes()[source.index()].kind,
        ));
    }
    let mut sources = Vec::new();
    for &(input_id, sampled) in sampled_inputs {
        let LayerInputSource::SourceAbove { site, port, threshold } = circuit.program.inputs[input_id.0 as usize].source else {
            continue;
        };
        let site_spec = backend.program.sites()[site.index()];
        let raw_strength = backend.site_power(site, port);
        let source_state = match site_spec.source {
            SiteSource::Component(node) => {
                let kind = &backend.program.nodes()[node.index()].kind;
                let state = match backend.state.components.get(node.index()) {
                    Some(ComponentState::Electrical(state)) => format!("powered:{}, strength:{}", state.powered, state.output_strength),
                    _ => "non-electrical".to_owned(),
                };
                format!(
                    "component={node:?}@{:?}/{kind:?} state={state}",
                    backend.program.nodes()[node.index()].position
                )
            }
            source => format!("{source:?}"),
        };
        sources.push(format!(
            "{site:?}@{:?} source={source_state} port={port:?} threshold={threshold} raw_strength={raw_strength} sampled={sampled}",
            site_spec.position,
        ));
        if sources.len() == MAX_SOURCES {
            break;
        }
    }
    evidence.push_str(&format!(" SourceAbove=[{}]", sources.join("; ")));
    evidence
}

/// Report source categories only; do not dump the potentially very large input or graph tables.
fn summarize_sources(circuit: &SettledCircuit, queries: &[PowerQuery]) -> String {
    #[derive(Default)]
    struct Counts {
        total: usize,
        fired: usize,
        canonical_resource_at: usize,
        moved_resource_at: usize,
        resource_enabled: usize,
        other_state: usize,
        source_above: usize,
        boundary_event: usize,
        boundary_request: usize,
        power_contribution: usize,
        local_reset_output: usize,
        other: usize,
    }

    fn count_sources(circuit: &SettledCircuit, roots: &[usize]) -> Counts {
        let mut counts = Counts::default();
        let inputs = input_dependencies(&circuit.program, roots);
        let firing_resources: FxHashMap<_, _> = circuit
            .firing
            .as_ref()
            .into_iter()
            .flat_map(|plan| plan.resources.iter())
            .map(|resource| (resource.resource, resource.source))
            .collect();
        counts.total = inputs.len();
        for input in inputs {
            match circuit.program.inputs[input.0 as usize].source {
                LayerInputSource::InstantFired { .. } => counts.fired += 1,
                LayerInputSource::State(StateTest::ResourceAt { resource, contact }) => {
                    if let Some(&source) = firing_resources.get(&resource) {
                        if source == contact {
                            counts.canonical_resource_at += 1;
                        } else {
                            counts.moved_resource_at += 1;
                        }
                    } else {
                        counts.other_state += 1;
                    }
                }
                LayerInputSource::State(StateTest::ResourceEnabled(_)) => counts.resource_enabled += 1,
                LayerInputSource::State(_) | LayerInputSource::BoundaryState { .. } => counts.other_state += 1,
                LayerInputSource::SourceAbove { .. } => counts.source_above += 1,
                LayerInputSource::WireDot { .. } => counts.other += 1,
                LayerInputSource::BoundaryEvent { .. } => counts.boundary_event += 1,
                LayerInputSource::BoundaryRequest { .. } => counts.boundary_request += 1,
                LayerInputSource::PowerContribution { .. } => counts.power_contribution += 1,
                LayerInputSource::LocalResetOutput { .. } => counts.local_reset_output += 1,
            }
        }
        counts
    }

    fn format(label: &str, counts: Counts) -> String {
        format!(
            "{label}={{unique_inputs:{}, fired:{}, canonical_resource_at:{}, moved_resource_at:{}, resource_enabled:{}, other_state:{}, source_above:{}, boundary_event:{}, boundary_request:{}, power_contribution:{}, local_reset_output:{}, other:{}}}",
            counts.total,
            counts.fired,
            counts.canonical_resource_at,
            counts.moved_resource_at,
            counts.resource_enabled,
            counts.other_state,
            counts.source_above,
            counts.boundary_event,
            counts.boundary_request,
            counts.power_contribution,
            counts.local_reset_output,
            counts.other,
        )
    }

    let queried_roots: Vec<_> = queries.iter().flat_map(|query| [query.main_root, query.quasi_root]).collect();
    let eligibility_roots: Vec<_> = circuit
        .firing
        .as_ref()
        .into_iter()
        .flat_map(|plan| plan.uses.iter())
        .filter_map(|use_| circuit.outputs.get(use_.power_eligible_output as usize))
        .flat_map(|output| output.roots.iter().map(|root| root.0 as usize))
        .collect();
    let (mut observers, mut payloads, mut input_restorations) = (0, 0, 0);
    if let Some(plan) = &circuit.firing {
        for use_ in &plan.uses {
            match use_.reset {
                CellReset::Observer { .. } => observers += 1,
                CellReset::Payload { .. } => payloads += 1,
                CellReset::InputRestoration { .. } => input_restorations += 1,
            }
        }
    }
    format!(
        "{} {}; reset_witnesses={{observer:{observers}, payload:{payloads}, input_restoration:{input_restorations}}}",
        format("queried_main_quasi", count_sources(circuit, &queried_roots)),
        format("fire_eligibility", count_sources(circuit, &eligibility_roots)),
    )
}

fn validate_reset_entry(backend: &GraphBackend, circuit: &SettledCircuit) -> Result<(), String> {
    if !circuit.declines.is_empty() {
        return Err("cannot probe a partial firing circuit".into());
    }
    let Some(plan) = &circuit.firing else {
        return Ok(());
    };
    for use_ in &plan.uses {
        let state = backend
            .state
            .pistons
            .get(use_.actor.index())
            .ok_or_else(|| format!("firing actor {:?} is outside initial state", use_.actor))?;
        if state.availability != PistonAvailability::Ready {
            return Err(format!("firing actor {:?} is not at a reset-entry Ready boundary", use_.actor));
        }
        if backend.future.deferred_pistons.iter().any(|event| event.piston == use_.actor) {
            return Err(format!("firing actor {:?} has an in-flight deferred event at probe entry", use_.actor));
        }
        if let CellReset::Observer { node, .. } = &use_.reset {
            let Some(ComponentState::Electrical(observer)) = backend.state.components.get(node.index()) else {
                return Err(format!("firing actor {:?} reset observer is not electrical", use_.actor));
            };
            if observer.powered || observer.pending_tick {
                return Err(format!("firing actor {:?} reset observer is active at probe entry", use_.actor));
            }
        }
    }
    for resource in &plan.resources {
        let state = backend
            .state
            .resources
            .get(resource.resource.index())
            .ok_or_else(|| format!("firing source resource {:?} is outside initial state", resource.resource))?;
        if state.at != resource.source || !state.enabled {
            return Err(format!(
                "firing source {:?} is not initially present at canonical contact {:?} (at={:?}, enabled={})",
                resource.resource, resource.source, state.at, state.enabled
            ));
        }
    }
    Ok(())
}

fn input_dependencies(program: &ReducedLayerProgram, roots: &[usize]) -> Vec<LayerInputId> {
    let mut seen_expr = vec![false; program.expressions.len()];
    let mut seen_input = vec![false; program.inputs.len()];
    let mut stack = roots.iter().copied().collect::<Vec<_>>();
    while let Some(index) = stack.pop() {
        if index >= program.expressions.len() || std::mem::replace(&mut seen_expr[index], true) {
            continue;
        }
        match &program.expressions[index] {
            LayerExpr::Constant(_) => {}
            LayerExpr::Input(input) => seen_input[input.0 as usize] = true,
            LayerExpr::Not(value) => stack.push(value.0 as usize),
            LayerExpr::All(values) | LayerExpr::Any(values) => stack.extend(values.iter().map(|value| value.0 as usize)),
        }
    }
    seen_input
        .into_iter()
        .enumerate()
        .filter_map(|(index, used)| used.then_some(LayerInputId(index as u32)))
        .collect()
}

fn format_initial_state(backend: &GraphBackend, circuit: &SettledCircuit) -> String {
    let Some(plan) = &circuit.firing else {
        return "firing_owners=0; no firing plan".to_owned();
    };
    let ready = plan
        .uses
        .iter()
        .filter(|use_| backend.state.pistons[use_.actor.index()].availability == PistonAvailability::Ready)
        .count();
    let extended = plan.uses.iter().filter(|use_| backend.state.pistons[use_.actor.index()].extended).count();
    let present = plan
        .uses
        .iter()
        .filter(|use_| backend.state.pistons[use_.actor.index()].head_present)
        .count();
    let canonical = plan
        .resources
        .iter()
        .filter(|resource| {
            let state = backend.state.resources[resource.resource.index()];
            state.at == resource.source && state.enabled
        })
        .count();
    format!(
        "owners={}/{} Ready, extended={}, heads={}, sources={}/{} canonical-enabled",
        ready,
        plan.uses.len(),
        extended,
        present,
        canonical,
        plan.resources.len()
    )
}

fn deferred_actor_published(backend: &GraphBackend, event: DeferredPistonEvent) -> bool {
    backend.state.pistons.get(event.piston.index()).is_some_and(|state| {
        state.revision == event.revision && state.availability == PistonAvailability::Ready && state.extended && state.head_present
    })
}

impl Probe {
    fn observer_idle(&self, backend: &GraphBackend, node: NodeId) -> bool {
        matches!(
            backend.state.components.get(node.index()),
            Some(ComponentState::Electrical(state)) if !state.powered && !state.pending_tick
        )
    }
}

fn phase_name(phase: GraphPhase) -> &'static str {
    match phase {
        GraphPhase::BetweenTicks => "between-ticks",
        GraphPhase::ScheduledTicks => "scheduled-ticks",
        GraphPhase::PistonActions => "piston-actions",
        GraphPhase::DeferredConnections => "deferred-connections",
    }
}

impl GraphBackend {
    pub(crate) fn install_logical_firing_probe(&mut self, actor: PistonId, circuit: SettledCircuit) -> Result<(), String> {
        self.test_recorder.logical_firing_probe = Some(Probe::new(actor, self, circuit)?);
        Ok(())
    }

    pub(crate) fn take_logical_firing_probe(&mut self) -> Option<Report> {
        self.test_recorder.logical_firing_probe.take().map(Probe::into_report)
    }

    pub(super) fn record_logical_firing_sample(&mut self, actor: PistonId) {
        let Some(mut probe) = self.test_recorder.logical_firing_probe.take() else {
            return;
        };
        probe.sampled(self, actor);
        self.test_recorder.logical_firing_probe = Some(probe);
    }

    pub(super) fn record_logical_firing_accept(&mut self, request: PistonRequest, accepted: bool) {
        let Some(mut probe) = self.test_recorder.logical_firing_probe.take() else {
            return;
        };
        probe.accepted(self, request, accepted);
        self.test_recorder.logical_firing_probe = Some(probe);
    }

    pub(super) fn record_logical_firing_reset(&mut self, event: DeferredPistonEvent) {
        let Some(mut probe) = self.test_recorder.logical_firing_probe.take() else {
            return;
        };
        probe.reset_publication(self, event);
        self.test_recorder.logical_firing_probe = Some(probe);
    }
}
