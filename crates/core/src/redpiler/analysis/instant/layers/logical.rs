//! Settled logical equations, independent of physical callback and reset execution.
use super::*;
use crate::redpiler::components::NodeType;
use rustc_hash::FxHashSet;
#[cfg(test)]
mod controller;
#[cfg(test)]
mod counter;
#[cfg(test)]
pub(crate) use controller::controller_support;
#[cfg(test)]
#[path = "../../../../../benches/support/cpus.rs"]
#[allow(dead_code)]
mod cpus;
#[cfg(test)]
mod divider;
mod equations;
#[cfg(test)]
mod expansion_tests;
#[cfg(test)]
mod fpu;
#[cfg(test)]
mod fpu_computation_tests;
#[cfg(test)]
mod fpu_kernel_bench;
mod islands;
mod native_inputs;
mod settled;
pub(crate) mod timing;
pub(crate) use islands::LogicalIsland;
#[cfg(test)]
pub(crate) use settled::compile_computation;
#[cfg(test)]
pub(crate) use settled::compile_event_boundary;
#[cfg(test)]
pub(crate) use settled::compile_event_boundary_cut_feedback;
#[cfg(test)]
pub(crate) use settled::compile_event_boundary_with_retained_resources;
#[cfg(test)]
pub(crate) use settled::compile_one_shot_boundary;
#[cfg(test)]
pub(crate) use settled::compile_public_boundary;
#[cfg(test)]
pub(crate) use settled::diagnostic_all_phase_raw_query;
#[cfg(test)]
pub(crate) use settled::diagnostic_computation_failures;
#[cfg(test)]
pub(crate) use settled::diagnostic_query;
#[cfg(test)]
pub(crate) use settled::diagnostic_source_cycle_edges;
pub(crate) use settled::{compile_device, compile_devices, compile_scoped_devices, compile_settled};
#[cfg(test)]
mod adders11;
#[cfg(test)]
mod settled_tests;
#[cfg(test)]
mod test_samples;

struct Equations<'a> {
    graph: &'a CompileGraph,
    power: PowerIndex,
    wires: FxHashSet<NodeId>,
    resets: FxHashSet<NodeId>,
    cells: FxHashMap<ResourceId, (ContactId, Vec<PistonId>)>,
    /// Only these payloads have a recognized bounded owner set and canonical source.
    unique_cell_resources: FxHashSet<ResourceId>,
    /// Near contacts held disabled by an admitted delayed token-transfer restoration.
    disabled_destinations: FxHashSet<(ResourceId, ContactId)>,
    actors: FxHashSet<PistonId>,
    registers: FxHashMap<ResourceId, BudMemoryRead>,
    /// Value producers whose source semantics were expanded into this equation set.
    expanded_components: FxHashSet<NodeId>,
    inline_repeaters: bool,
    stateful_repeaters: FxHashSet<NodeId>,
    raw: LayerBuilder<'a>,
    signal_memo: signals::SignalMemo,
    reduced: LayerBuilder<'a>,
    expanded: FxHashMap<LayerExprId, LayerExprId>,
    sources: FxHashMap<LayerInputSource, LayerExprId>,
    pending: FxHashSet<LayerInputSource>,
    pending_repeaters: FxHashSet<NodeId>,
    retain_classic: bool,
    resource_semantics: ResourceSemantics,
    /// Fired-state inputs allocated for actors that own a requested feedback resource.
    fired_inputs: FxHashMap<PistonId, LayerInputId>,
    /// Shared source resources whose feedback was represented by owner fired inputs.
    firing_resources: FxHashMap<ResourceId, (ContactId, Vec<PistonId>)>,
    /// Prevalidated common-Far contracts built from one sticky-owner index.
    one_shot_contracts: FxHashMap<ResourceId, (ContactId, Vec<PistonId>)>,
    /// Diagnostic-only mode: retain otherwise unknown payload locations as leaves so one
    /// traversal can report every blocker. Executable compilation never enables this.
    diagnostic_unknown_resources: bool,
    /// Typed first resource-location cycle seen by an event-boundary diagnostic build.
    #[cfg(test)]
    resource_cycle_witness: Option<(ResourceId, ContactId)>,
    /// Full typed cycle found by the production post-propagation equation analysis.
    #[cfg(test)]
    source_cycle_witness: Vec<LayerInputSource>,
    /// All post-propagation internal edges in the SCC containing that witness.
    #[cfg(test)]
    source_scc_edges: Vec<(LayerInputSource, LayerInputSource)>,
    /// Defer sources to the iterative equation worklist, recording dependency edges.
    defer_sources: bool,
    /// Allow dynamic sampling of native dot-to-cross wire mode in settled equations.
    stable_wire_modes: bool,
    deferred_parent: Option<LayerInputSource>,
    deferred_roots: FxHashSet<LayerInputSource>,
    deferred_children: FxHashMap<LayerInputSource, FxHashSet<LayerInputSource>>,
    feedback_state_sources: Vec<(ResourceId, ContactId)>,
}

#[derive(Clone, Copy, Default)]
enum ResourceSemantics {
    /// Model resource locations at the settled cut without opening feedback state.
    #[default]
    Settled,
    /// Turn recognized source-location cycles into state inputs; optionally reject unknown destinations.
    Feedback { reject_unrepresented_destinations: bool },
    /// Replace canonical source occupancy with the actors' one-use fired bits.
    OneShot,
}

impl ResourceSemantics {
    fn uses_feedback(self) -> bool {
        matches!(self, Self::Feedback { .. })
    }

    fn rejects_unrepresented_destinations(self) -> bool {
        matches!(
            self,
            Self::Feedback {
                reject_unrepresented_destinations: true
            }
        )
    }

    fn uses_one_shot_firing(self) -> bool {
        matches!(self, Self::OneShot)
    }
}

/// One cursor per active operator; earlier-child IDs bound its depth by the raw arena size.
struct ExpandFrame {
    /// The raw operator being completed.
    id: LayerExprId,
    /// The next operand to visit, kept across source-triggered arena growth.
    next_child: usize,
}

impl<'a> Equations<'a> {
    fn new(graph: &'a CompileGraph, monitor: &'a TaskMonitor) -> Result<Self, ConstructionError> {
        let catalog = graph.plans.instant.boundaries.as_ref().ok_or("missing cell boundaries")?;
        let disabled_destinations = disabled_destinations(graph, catalog);
        let mut resource_cell_counts = FxHashMap::<ResourceId, usize>::default();
        for resource in catalog.cells.iter().flat_map(|cell| cell.resources.iter()) {
            *resource_cell_counts.entry(resource.resource).or_default() += 1;
        }
        let cells: FxHashMap<_, _> = catalog
            .cells
            .iter()
            .flat_map(|cell| {
                cell.resources
                    .iter()
                    .map(|resource| (resource.resource, (resource.source, resource.owners.to_vec())))
            })
            .collect();
        let unique_cell_resources: FxHashSet<_> = resource_cell_counts
            .into_iter()
            .filter_map(|(resource, count)| (count == 1).then_some(resource))
            .collect();
        let one_shot_contracts = common_far_contracts(graph, &cells, &unique_cell_resources);
        Ok(Self {
            graph,
            power: index_power(graph),
            wires: graph
                .nodes
                .iter()
                .enumerate()
                .filter_map(|(id, node)| matches!(node.kind, CircuitNodeKind::Wire(_)).then_some(NodeId::new(id as u32)))
                .collect(),
            resets: catalog
                .cells
                .iter()
                .flat_map(|cell| cell.uses.iter().filter_map(|use_| use_.reset.observer()))
                .collect(),
            cells,
            unique_cell_resources,
            disabled_destinations,
            actors: catalog.cells.iter().flat_map(|cell| cell.uses.iter().map(|use_| use_.actor)).collect(),
            registers: graph.plans.bud_memory_reads.iter().map(|read| (read.resource, *read)).collect(),
            expanded_components: FxHashSet::default(),
            inline_repeaters: false,
            stateful_repeaters: graph
                .power_links
                .iter()
                .filter_map(|link| match link.to {
                    PowerTarget::Input { node, port: InputPort::Side }
                        if matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Electrical(NodeType::Repeater { .. })) =>
                    {
                        Some(node)
                    }
                    _ => None,
                })
                .collect(),
            raw: LayerBuilder::new(monitor)?,
            signal_memo: signals::SignalMemo::default(),
            reduced: LayerBuilder::new(monitor)?,
            expanded: FxHashMap::default(),
            sources: FxHashMap::default(),
            pending: FxHashSet::default(),
            pending_repeaters: FxHashSet::default(),
            retain_classic: false,
            resource_semantics: ResourceSemantics::default(),
            fired_inputs: FxHashMap::default(),
            firing_resources: FxHashMap::default(),
            one_shot_contracts,
            diagnostic_unknown_resources: false,
            #[cfg(test)]
            resource_cycle_witness: None,
            #[cfg(test)]
            source_cycle_witness: Vec::new(),
            #[cfg(test)]
            source_scc_edges: Vec::new(),
            defer_sources: false,
            stable_wire_modes: false,
            deferred_parent: None,
            deferred_roots: FxHashSet::default(),
            deferred_children: FxHashMap::default(),
            feedback_state_sources: Vec::new(),
        })
    }

    fn input_power(&mut self, node: NodeId, port: InputPort) -> Result<LayerExprId, ConstructionError> {
        self.query(PowerTarget::Input { node, port }, 0)
    }

    fn query(&mut self, target: PowerTarget, threshold: u8) -> Result<LayerExprId, ConstructionError> {
        let state = FxHashMap::default();
        let mut tracer = signals::SignalTracer::new(self.graph, &self.power, &self.wires, &state, LayerInputRole::RequestSample)
            .with_disabled_destinations(&self.disabled_destinations)
            .with_memo(std::mem::take(&mut self.signal_memo));
        if self.stable_wire_modes {
            tracer = tracer.with_stable_wire_modes();
        }
        let raw = tracer.query(target, threshold, &mut self.raw);
        self.signal_memo = tracer.into_memo();
        self.expand(raw?)
    }

    fn powered(&mut self, actor: PistonId) -> Result<LayerExprId, ConstructionError> {
        let node = self.graph.pistons[actor.index()].node;
        let main = self.input_power(node, InputPort::Main)?;
        let quasi = self.input_power(node, InputPort::Quasi)?;
        self.reduced.any([main, quasi])
    }

    fn expand(&mut self, id: LayerExprId) -> Result<LayerExprId, ConstructionError> {
        let mut pending = Some(id);
        let mut stack = Vec::new();
        loop {
            if let Some(current) = pending.take() {
                self.reduced.visit()?;
                if self.expanded.contains_key(&current) {
                    continue;
                }
                match self.raw.expression(current)? {
                    LayerExpr::Constant(value) => {
                        let value = self.reduced.constant(*value)?;
                        self.expanded.insert(current, value);
                    }
                    LayerExpr::Input(input) => {
                        let source = self
                            .raw
                            .inputs
                            .get(input.0 as usize)
                            .ok_or("logical expression references a missing input")?
                            .source;
                        let value = self.source(source)?;
                        self.expanded.insert(current, value);
                    }
                    LayerExpr::Not(_) | LayerExpr::All(_) | LayerExpr::Any(_) => stack.push(ExpandFrame { id: current, next_child: 0 }),
                }
                continue;
            }

            let Some(frame) = stack.last_mut() else {
                return self
                    .expanded
                    .get(&id)
                    .copied()
                    .ok_or_else(|| "logical expression was not expanded".into());
            };
            let child = self.expand_child(frame)?;
            if let Some(child) = child {
                frame.next_child += 1;
                pending = Some(child);
                continue;
            }

            let frame = stack.pop().expect("the current expansion frame exists");
            let value = self.finish_expand(frame.id)?;
            self.expanded.insert(frame.id, value);
        }
    }

    fn expand_child(&self, frame: &ExpandFrame) -> Result<Option<LayerExprId>, ConstructionError> {
        let child = match self.raw.expression(frame.id)? {
            LayerExpr::Not(child) if frame.next_child == 0 => Some(*child),
            LayerExpr::Not(_) => None,
            LayerExpr::All(children) | LayerExpr::Any(children) => children.get(frame.next_child).copied(),
            _ => return Err("logical expression is not an operator".into()),
        };
        if child.is_some_and(|child| child.0 >= frame.id.0) {
            return Err("logical expression references a non-prior root".into());
        }
        Ok(child)
    }

    fn finish_expand(&mut self, id: LayerExprId) -> Result<LayerExprId, ConstructionError> {
        match self.raw.expression(id)? {
            LayerExpr::Not(child) => {
                let child = self.expanded.get(child).copied().ok_or("logical child was not expanded")?;
                self.reduced.not(child)
            }
            LayerExpr::All(_) => self.finish_combine(id, true),
            LayerExpr::Any(_) => self.finish_combine(id, false),
            _ => return Err("logical expression is not an operator".into()),
        }
    }

    fn finish_combine(&mut self, id: LayerExprId, all: bool) -> Result<LayerExprId, ConstructionError> {
        let children = match self.raw.expression(id)? {
            LayerExpr::All(children) | LayerExpr::Any(children) => children,
            _ => return Err("logical expression is not an operator".into()),
        };
        let values = children
            .iter()
            .map(|child| self.expanded.get(child).copied().ok_or_else(|| "logical child was not expanded".into()))
            .collect::<Result<Vec<_>, ConstructionError>>()?;
        self.reduced.combine(values, all)
    }

    fn leaf(&mut self, source: LayerInputSource) -> Result<LayerExprId, ConstructionError> {
        let input = self.reduced.input(source, LayerInputRole::RequestSample)?;
        self.reduced.input_expr(input)
    }

    fn fired_leaf(&mut self, actor: PistonId) -> Result<LayerExprId, ConstructionError> {
        let source = LayerInputSource::InstantFired { actor };
        let input = self.reduced.input(source, LayerInputRole::RequestSample)?;
        self.fired_inputs.insert(actor, input);
        self.reduced.input_expr(input)
    }

    fn one_shot_source_presence(&mut self, resource: ResourceId) -> Result<Option<LayerExprId>, ConstructionError> {
        if !self.resource_semantics.uses_one_shot_firing() {
            return Ok(None);
        }
        let Some((source, owners)) = self.one_shot_contracts.get(&resource).cloned() else {
            return Ok(None);
        };
        let fired = owners.iter().map(|&actor| self.fired_leaf(actor)).collect::<Result<Vec<_>, _>>()?;
        let any_fired = self.reduced.any(fired)?;
        let present = self.reduced.not(any_fired)?;
        self.firing_resources.insert(resource, (source, owners));
        Ok(Some(present))
    }

    fn one_shot_live_destination(&self, resource: ResourceId, contact: ContactId) -> bool {
        self.resource_semantics.uses_one_shot_firing()
            && self.one_shot_contracts.contains_key(&resource)
            && self.graph.resources[resource.index()].contacts.contains(&contact)
    }

    fn source(&mut self, source: LayerInputSource) -> Result<LayerExprId, ConstructionError> {
        if self.defer_sources {
            if let Some(node) = self.inline_repeaters.then(|| self.repeater_source_node(source)).flatten() {
                if self.stateful_repeaters.contains(&node) {
                    return self.binary_source_leaf(source);
                }
                if self.pending_repeaters.contains(&node) {
                    self.stateful_repeaters.extend(self.pending_repeaters.iter().copied());
                    self.stateful_repeaters.insert(node);
                    return self.binary_source_leaf(source);
                }
                // Repeater equations are currently expanded as a special case while
                // discovering source templates. Keep this recursion explicitly
                // bounded; ordinary source dependencies are still lowered by the
                // iterative worklist, and an unusually deep repeater chain declines
                // to the exact graph path instead of risking stack exhaustion.
                const MAX_INLINE_REPEATER_DEPTH: usize = 128;
                if self.pending_repeaters.len() >= MAX_INLINE_REPEATER_DEPTH {
                    return Err(ConstructionError::Unsupported(
                        "logical repeater equation exceeds its inline depth bound".into(),
                    ));
                }
                self.pending_repeaters.insert(node);
                let result = self.input_power(node, InputPort::Main);
                self.pending_repeaters.remove(&node);
                let value = result?;
                return if self.stateful_repeaters.contains(&node) {
                    self.binary_source_leaf(source)
                } else {
                    Ok(value)
                };
            }
            if let Some(parent) = self.deferred_parent {
                self.deferred_children.entry(parent).or_default().insert(source);
            } else {
                self.deferred_roots.insert(source);
            }
            return self.leaf(source);
        }
        if let Some(&value) = self.sources.get(&source) {
            return Ok(value);
        }
        let repeater = self.inline_repeaters.then(|| self.repeater_source_node(source)).flatten();
        if self.pending.contains(&source) || repeater.is_some_and(|node| self.pending_repeaters.contains(&node)) {
            #[cfg(test)]
            if let LayerInputSource::State(StateTest::ResourceAt { resource, contact }) = source {
                self.resource_cycle_witness.get_or_insert((resource, contact));
            }
            if self.resource_semantics.uses_feedback() {
                if let Some(value) = self.feedback_resource_leaf(source)? {
                    return Ok(value);
                }
            }
            if self.retain_classic
                && matches!(source, LayerInputSource::State(StateTest::ResourceAt { resource, .. }) if !self.cells.contains_key(&resource))
            {
                return self.leaf(source);
            }
            if self.inline_repeaters {
                self.stateful_repeaters.extend(self.pending_repeaters.iter().copied());
                self.stateful_repeaters.extend(repeater);
            }
            return Err(ConstructionError::FeedbackRequiresRegister(source));
        }
        if self.pending.len() >= 256 {
            return Err("logical equation exceeds its source nesting bound".into());
        }
        self.pending.insert(source);
        if let Some(node) = repeater {
            self.pending_repeaters.insert(node);
        }
        let result = self.source_value(source);
        self.pending.remove(&source);
        if let Some(node) = repeater {
            self.pending_repeaters.remove(&node);
        }
        let value = match result {
            Ok(value) => value,
            Err(ConstructionError::FeedbackRequiresRegister(_))
                if self.inline_repeaters
                    && self
                        .repeater_source_node(source)
                        .is_some_and(|node| self.stateful_repeaters.contains(&node)) =>
            {
                self.binary_source_leaf(source)?
            }
            Err(error) => return Err(error),
        };
        let value = if self.inline_repeaters
            && self
                .repeater_source_node(source)
                .is_some_and(|node| self.stateful_repeaters.contains(&node))
        {
            self.binary_source_leaf(source)?
        } else {
            value
        };
        self.sources.insert(source, value);
        Ok(value)
    }

    /// Retain the catalog-cell closure touching selected resources or actors in Graph.
    /// Their original callbacks remain the only authority that advances those resources.
    fn retain_cells_touching(
        &mut self,
        selected_resources: &[ResourceId],
        selected_actors: &[PistonId],
    ) -> Result<(Vec<ResourceId>, Vec<PistonId>), ConstructionError> {
        let catalog = self.graph.plans.instant.boundaries.as_ref().ok_or("missing cell boundaries")?;
        let mut resources: FxHashSet<_> = selected_resources.iter().copied().collect();
        let mut actors: FxHashSet<_> = selected_actors.iter().copied().collect();
        let mut retained_cells = FxHashSet::default();

        loop {
            let mut changed = false;
            for (index, cell) in catalog.cells.iter().enumerate() {
                if retained_cells.contains(&index)
                    || (!cell.resources.iter().any(|resource| resources.contains(&resource.resource))
                        && !cell.uses.iter().any(|use_| actors.contains(&use_.actor)))
                {
                    continue;
                }
                changed |= retained_cells.insert(index);
                actors.extend(cell.uses.iter().map(|use_| use_.actor));
                resources.extend(cell.resources.iter().map(|resource| resource.resource));
            }
            if !changed {
                break;
            }
        }

        let mut retained_resets = FxHashSet::default();
        for index in retained_cells {
            let cell = &catalog.cells[index];
            for use_ in &cell.uses {
                if let Some(reset) = use_.reset.observer() {
                    retained_resets.insert(reset);
                }
            }
        }
        for resource in &resources {
            self.cells.remove(resource);
            self.unique_cell_resources.remove(resource);
        }
        for actor in &actors {
            self.actors.remove(actor);
        }
        for reset in retained_resets {
            self.resets.remove(&reset);
        }
        self.disabled_destinations.retain(|(resource, _)| !resources.contains(resource));

        let mut resources: Vec<_> = resources.into_iter().collect();
        resources.sort_unstable();
        let mut actors: Vec<_> = actors.into_iter().collect();
        actors.sort_unstable();
        Ok((resources, actors))
    }

    fn feedback_resource_leaf(&mut self, source: LayerInputSource) -> Result<Option<LayerExprId>, ConstructionError> {
        let LayerInputSource::State(StateTest::ResourceAt { resource, contact }) = source else {
            return Ok(None);
        };
        let Some((canonical, _owners)) = self.feedback_resource_contract(resource) else {
            return Ok(None);
        };
        let spec = &self.graph.resources[resource.index()];
        if !spec.contacts.contains(&contact) {
            return Ok(None);
        }
        if contact != canonical {
            // The candidate tracks only whether the payload remains at common Far.
            // It deliberately does not invent which Near receives a pulled token.
            return Ok(Some(self.reduced.constant(false)?));
        }
        if !self.feedback_state_sources.contains(&(resource, canonical)) {
            self.feedback_state_sources.push((resource, canonical));
        }
        let state = self.leaf(LayerInputSource::State(StateTest::ResourceAt {
            resource,
            contact: canonical,
        }))?;
        Ok(Some(state))
    }

    pub(super) fn feedback_resource_contract(&self, resource: ResourceId) -> Option<(ContactId, Vec<PistonId>)> {
        if !self.unique_cell_resources.contains(&resource) {
            return None;
        }
        let (canonical, owners) = self.cells.get(&resource)?;
        if !(1..=5).contains(&owners.len()) {
            return None;
        }
        let spec = self.graph.resources.get(resource.index())?;
        if !spec.contacts.contains(canonical) {
            return None;
        }
        let owner_set: FxHashSet<_> = owners.iter().copied().collect();
        if owner_set.len() != owners.len()
            || self.graph.pistons.iter().enumerate().any(|(index, piston)| {
                matches!(piston.lane, PistonLane::SingleResource { far } if far == *canonical)
                    && piston.sticky
                    && !owner_set.contains(&PistonId::new(index as u32))
            })
        {
            return None;
        }
        let mut covered = FxHashSet::default();
        covered.insert(*canonical);
        for &owner in owners {
            let piston = self.graph.pistons.get(owner.index())?;
            if !piston.sticky || piston.lane != (PistonLane::SingleResource { far: *canonical }) || !spec.contacts.contains(&piston.near) {
                return None;
            }
            covered.insert(piston.near);
        }
        spec.contacts
            .iter()
            .all(|contact| covered.contains(contact))
            .then(|| (*canonical, owners.clone()))
    }

    pub(super) fn shared_far_presence(&mut self, owners: &[PistonId]) -> Result<LayerExprId, ConstructionError> {
        let powered = owners.iter().map(|&owner| self.powered(owner)).collect::<Result<Vec<_>, _>>()?;
        self.shared_far_presence_from(powered)
    }

    fn shared_far_presence_from(&mut self, powered: Vec<LayerExprId>) -> Result<LayerExprId, ConstructionError> {
        self.reduced.all(powered)
    }

    fn repeater_source_node(&self, source: LayerInputSource) -> Option<NodeId> {
        let LayerInputSource::SourceAbove { site, .. } = source else {
            return None;
        };
        let SiteSource::Component(node) = self.graph.sites[site.index()].source else {
            return None;
        };
        matches!(
            self.graph.nodes[node.index()].kind,
            CircuitNodeKind::Electrical(NodeType::Repeater { .. })
        )
        .then_some(node)
    }

    fn binary_source_leaf(&mut self, source: LayerInputSource) -> Result<LayerExprId, ConstructionError> {
        let LayerInputSource::SourceAbove { site, .. } = source else {
            return Err("logical repeater state is not a source input".into());
        };
        self.leaf(LayerInputSource::SourceAbove {
            site,
            port: PowerPort::Output,
            threshold: 0,
        })
    }

    fn guard_equation(&mut self, guard: GuardId) -> Result<LayerExprId, ConstructionError> {
        let state = FxHashMap::default();
        let mut tracer = signals::SignalTracer::new(self.graph, &self.power, &self.wires, &state, LayerInputRole::RequestSample)
            .with_disabled_destinations(&self.disabled_destinations)
            .with_memo(std::mem::take(&mut self.signal_memo));
        if self.stable_wire_modes {
            tracer = tracer.with_stable_wire_modes();
        }
        let raw = tracer.guard(guard, &mut self.raw);
        self.signal_memo = tracer.into_memo();
        let raw = raw?;
        self.expand(raw)
    }

    fn input_above(&mut self, node: NodeId, port: InputPort, threshold: u8) -> Result<LayerExprId, ConstructionError> {
        if threshold >= 15 {
            self.reduced.constant(false)
        } else {
            self.query(PowerTarget::Input { node, port }, threshold)
        }
    }

    fn comparator_output_above(
        &mut self,
        node: NodeId,
        mode: mchprs_blocks::blocks::ComparatorMode,
        output_threshold: u8,
        literal_input: Option<u8>,
    ) -> Result<LayerExprId, ConstructionError> {
        if output_threshold >= 15 {
            return self.reduced.constant(false);
        }
        let main_above = if let Some(strength) = literal_input {
            self.reduced.constant(strength > output_threshold)?
        } else {
            self.input_above(node, InputPort::Main, output_threshold)?
        };
        let mut clauses = vec![main_above];
        for side_level in 1..=15u8 {
            let side_at_least = self.input_above(node, InputPort::Side, side_level - 1)?;
            let main_sufficient = match mode {
                mchprs_blocks::blocks::ComparatorMode::Compare => {
                    if let Some(strength) = literal_input {
                        self.reduced.constant(strength >= side_level)?
                    } else {
                        self.input_above(node, InputPort::Main, side_level - 1)?
                    }
                }
                mchprs_blocks::blocks::ComparatorMode::Subtract => {
                    let required = u16::from(output_threshold) + u16::from(side_level);
                    if required >= 15 {
                        self.reduced.constant(false)?
                    } else if let Some(strength) = literal_input {
                        self.reduced.constant(u16::from(strength) > required)?
                    } else {
                        self.input_above(node, InputPort::Main, required as u8)?
                    }
                }
            };
            let side_too_high = self.reduced.not(side_at_least)?;
            clauses.push(self.reduced.any([side_too_high, main_sufficient])?);
        }
        self.reduced.all(clauses)
    }

    fn comparator_source(&mut self, node: NodeId, threshold: u8) -> Result<LayerExprId, ConstructionError> {
        let mode = match self.graph.nodes[node.index()].kind {
            CircuitNodeKind::Electrical(NodeType::Comparator { mode, .. }) => mode,
            _ => return Err("logical comparator equation requested for a non-comparator".into()),
        };
        let plan = self
            .graph
            .comparator_inputs
            .iter()
            .find(|plan| plan.node == node)
            .copied()
            .ok_or_else(|| format!("comparator {node:?} has no input plan"))?;

        if let Some(source) = plan.direct_override {
            return match source {
                ComparatorOverride::Literal(strength) => self.comparator_output_above(node, mode, threshold, Some(strength)),
                ComparatorOverride::LiveComponentOutput(_) => Err(ConstructionError::Unsupported(
                    "logical comparator has an unsupported live direct override".into(),
                )),
            };
        }
        let rear_output = self.comparator_output_above(node, mode, threshold, None)?;
        let Some((_, override_source, guard)) = plan.far_override else {
            return Ok(rear_output);
        };
        let override_strength = match override_source {
            ComparatorOverride::Literal(strength) => strength,
            ComparatorOverride::LiveComponentOutput(_) => {
                return Err(ConstructionError::Unsupported(
                    "logical comparator has an unsupported live far override".into(),
                ));
            }
        };
        let rear_meets_override_threshold = if plan.far_threshold == 0 {
            self.reduced.constant(true)?
        } else {
            self.input_above(node, InputPort::Main, plan.far_threshold - 1)?
        };
        let guard = self.guard_equation(guard)?;
        let below_override_threshold = self.reduced.not(rear_meets_override_threshold)?;
        let override_active = self.reduced.all([below_override_threshold, guard])?;
        let rear_active = self.reduced.not(override_active)?;
        let override_output = self.comparator_output_above(node, mode, threshold, Some(override_strength))?;
        let rear_value = self.reduced.all([rear_active, rear_output])?;
        let override_value = self.reduced.all([override_active, override_output])?;
        self.reduced.any([rear_value, override_value])
    }

    fn source_value(&mut self, source: LayerInputSource) -> Result<LayerExprId, ConstructionError> {
        let value = match source {
            LayerInputSource::WireDot { node } => {
                if !matches!(self.graph.nodes.get(node.index()).map(|node| &node.kind), Some(CircuitNodeKind::Wire(_))) {
                    return Err(format!("wire-mode input refers to a non-wire component {node:?}").into());
                }
                self.leaf(source)?
            }
            LayerInputSource::State(StateTest::PistonAvailability { piston, is }) => {
                if self.actors.contains(&piston) || self.registers.values().any(|read| read.storage == piston) {
                    self.reduced.constant(is == PistonAvailability::Ready)?
                } else {
                    self.leaf(source)?
                }
            }
            LayerInputSource::State(StateTest::ResourceEnabled(resource)) => {
                if self.resource_semantics.uses_one_shot_firing() {
                    // Only canonical far-side ResourceAt values are represented by
                    // owner fired bits. Keep enablement live as an exact Graph phase
                    // input, including for private destinations and register lanes.
                    return self.leaf(source);
                }
                if !self.cells.contains_key(&resource) && !self.registers.contains_key(&resource) {
                    return if self.retain_classic || self.diagnostic_unknown_resources {
                        self.leaf(source)
                    } else {
                        Err(format!("unclassified logical resource {resource:?}").into())
                    };
                }
                self.reduced.constant(true)?
            }
            LayerInputSource::State(StateTest::ResourceAt { resource, contact }) => {
                if let Some(read) = self.registers.get(&resource).copied() {
                    let value = self.leaf(LayerInputSource::State(StateTest::ResourceAt {
                        resource,
                        contact: read.source,
                    }))?;
                    let spec = self.graph.pistons[read.storage.index()];
                    let PistonLane::SingleResource { far } = spec.lane else {
                        return Err("logical register has unsupported storage geometry".into());
                    };
                    if contact == read.source {
                        value
                    } else if contact == spec.near || contact == far {
                        self.reduced.not(value)?
                    } else {
                        return Err("logical register read is outside its storage lane".into());
                    }
                } else if let Some((origin, actors)) = self.cells.get(&resource).cloned() {
                    if origin == contact {
                        if let Some(present) = self.one_shot_source_presence(resource)? {
                            return Ok(present);
                        }
                        let values = actors.into_iter().map(|actor| self.powered(actor)).collect::<Result<Vec<_>, _>>()?;
                        self.reduced.all(values)?
                    } else if self.one_shot_live_destination(resource, contact) {
                        // The logical source occupancy is the fired-bit contract;
                        // preserve any moved destination as an exact live Graph state
                        // input rather than inventing a destination transfer.
                        self.leaf(source)?
                    } else if self.resource_semantics.uses_feedback()
                        && self.feedback_resource_contract(resource).is_some()
                        && self.graph.resources[resource.index()].contacts.contains(&contact)
                    {
                        if self.resource_semantics.rejects_unrepresented_destinations() {
                            return Err(ConstructionError::Invalid(format!(
                                "logical computation cannot expose an unresolved transferred resource contact: {resource:?} at {contact:?}"
                            )));
                        }
                        self.reduced.constant(false)?
                    } else {
                        if self.retain_classic {
                            return Err(ConstructionError::Unsupported(format!(
                                "logical cell destination requires an explicit transfer equation: {resource:?} at {contact:?}"
                            )));
                        }
                        self.reduced.constant(false)?
                    }
                } else {
                    return if (self.retain_classic && !self.resource_semantics.uses_feedback()) || self.diagnostic_unknown_resources {
                        self.leaf(source)
                    } else {
                        Err(format!("unclassified logical resource {resource:?}").into())
                    };
                }
            }
            LayerInputSource::State(StateTest::PistonExtended(actor) | StateTest::PistonHeadPresent(actor)) => {
                if let Some(read) = self.registers.values().find(|read| read.storage == actor).copied() {
                    let value = self.leaf(LayerInputSource::State(StateTest::ResourceAt {
                        resource: read.resource,
                        contact: read.source,
                    }))?;
                    if matches!(self.graph.pistons[actor.index()].lane, PistonLane::SingleResource { far } if far == read.source) {
                        value
                    } else {
                        self.reduced.not(value)?
                    }
                } else if self.actors.contains(&actor) {
                    self.reduced.constant(true)?
                } else {
                    return if self.retain_classic {
                        self.leaf(source)
                    } else {
                        Err(format!("unclassified logical actor {actor:?}").into())
                    };
                }
            }
            LayerInputSource::SourceAbove { site, port: _, threshold } => {
                let SiteSource::Component(node) = self.graph.sites[site.index()].source else {
                    return Err("logical input is not a component".into());
                };
                let kind = &self.graph.nodes[node.index()].kind;
                let binary_source = matches!(
                    kind,
                    CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::PressurePlate)
                        | CircuitNodeKind::Electrical(NodeType::Repeater { .. } | NodeType::Torch | NodeType::Observer { .. })
                        | CircuitNodeKind::Observer { .. }
                        | CircuitNodeKind::BuiltinReset { .. }
                );
                let can_sample_binary_source = binary_source
                    && (self.retain_classic
                        || matches!(
                            kind,
                            CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::PressurePlate)
                        ));
                let retained_state_source = matches!(
                    kind,
                    CircuitNodeKind::Observer { .. }
                        | CircuitNodeKind::BuiltinReset { .. }
                        | CircuitNodeKind::Electrical(
                            NodeType::Repeater { .. } | NodeType::Torch | NodeType::Comparator { .. } | NodeType::Observer { .. }
                        )
                );
                // These equations describe the settled computation, after private
                // reset pulses. Observable reset effects remain in retained_effects.
                if threshold >= 15 || self.resets.contains(&node) {
                    self.reduced.constant(false)?
                } else if self.inline_repeaters && matches!(kind, CircuitNodeKind::Electrical(NodeType::Repeater { .. })) {
                    if self.stateful_repeaters.contains(&node) {
                        self.binary_source_leaf(source)?
                    } else {
                        let value = self.input_power(node, InputPort::Main)?;
                        self.expanded_components.insert(node);
                        value
                    }
                } else if self.resource_semantics.uses_feedback() && matches!(kind, CircuitNodeKind::Electrical(NodeType::Torch)) {
                    let main = self.input_power(node, InputPort::Main)?;
                    let value = self.reduced.not(main)?;
                    self.expanded_components.insert(node);
                    value
                } else if self.resource_semantics.uses_feedback() && matches!(kind, CircuitNodeKind::Electrical(NodeType::Comparator { .. })) {
                    let value = self.comparator_source(node, threshold)?;
                    self.expanded_components.insert(node);
                    value
                } else if can_sample_binary_source {
                    // Direction was checked by SignalTracer before it emitted this
                    // boundary. These source classes emit only 0 or 15, so all
                    // positive thresholds and directional views share one bit.
                    self.leaf(LayerInputSource::SourceAbove {
                        site,
                        port: PowerPort::Output,
                        threshold: 0,
                    })?
                } else if self.retain_classic && retained_state_source {
                    self.leaf(source)?
                } else {
                    match self.graph.nodes[node.index()].kind {
                        CircuitNodeKind::Electrical(NodeType::Constant) => {
                            let ComponentState::Electrical(ref state) = self.graph.nodes[node.index()].state else {
                                return Err("constant source has no electrical state".into());
                            };
                            self.reduced.constant(state.output_strength > threshold)?
                        }
                        CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::PressurePlate) => self.leaf(source)?,
                        CircuitNodeKind::Electrical(NodeType::Repeater { .. } | NodeType::Lamp) => self.input_power(node, InputPort::Main)?,
                        CircuitNodeKind::Electrical(NodeType::Torch) => {
                            let power = self.input_power(node, InputPort::Main)?;
                            let value = self.reduced.not(power)?;
                            self.expanded_components.insert(node);
                            value
                        }
                        ref kind => {
                            return Err(ConstructionError::Unsupported(format!("unsupported logical source {node:?}: {kind:?}")));
                        }
                    }
                }
            }
            other => return Err(format!("unsupported logical input {other:?}").into()),
        };
        Ok(value)
    }
}

fn common_far_contracts(
    graph: &CompileGraph,
    cells: &FxHashMap<ResourceId, (ContactId, Vec<PistonId>)>,
    unique_resources: &FxHashSet<ResourceId>,
) -> FxHashMap<ResourceId, (ContactId, Vec<PistonId>)> {
    let mut sticky_far_owners = FxHashMap::<ContactId, Vec<PistonId>>::default();
    for (index, piston) in graph.pistons.iter().enumerate() {
        if piston.sticky {
            if let PistonLane::SingleResource { far } = piston.lane {
                sticky_far_owners.entry(far).or_default().push(PistonId::new(index as u32));
            }
        }
    }

    let mut contracts = FxHashMap::default();
    for (&resource, &(canonical, ref owners)) in cells {
        if !unique_resources.contains(&resource) || !(1..=5).contains(&owners.len()) {
            continue;
        }
        let Some(spec) = graph.resources.get(resource.index()) else {
            continue;
        };
        let owner_set: FxHashSet<_> = owners.iter().copied().collect();
        if owner_set.len() != owners.len() || !spec.contacts.contains(&canonical) {
            continue;
        }
        let mut indexed = sticky_far_owners.get(&canonical).cloned().unwrap_or_default();
        indexed.sort_unstable();
        let mut declared = owner_set.iter().copied().collect::<Vec<_>>();
        declared.sort_unstable();
        if indexed != declared {
            continue;
        }
        let mut covered = FxHashSet::default();
        covered.insert(canonical);
        let mut valid = true;
        for &owner in owners {
            let Some(piston) = graph.pistons.get(owner.index()) else {
                valid = false;
                break;
            };
            if !piston.sticky || piston.lane != (PistonLane::SingleResource { far: canonical }) || !spec.contacts.contains(&piston.near) {
                valid = false;
                break;
            }
            covered.insert(piston.near);
        }
        if valid && spec.contacts.iter().all(|contact| covered.contains(contact)) {
            contracts.insert(resource, (canonical, owners.clone()));
        }
    }
    contracts
}

/// A contract proves that an accepted pull disables the payload at Near until
/// its matching delayed restoration. Only these exact positive state pairs
/// can be folded by the electrical guard tracer at a settled logical cut.
fn disabled_destinations(graph: &CompileGraph, catalog: &BoundaryCatalog) -> FxHashSet<(ResourceId, ContactId)> {
    let mut result = FxHashSet::default();
    for cell in &catalog.cells {
        for resource in &cell.resources {
            for &actor in &resource.owners {
                let Some(use_) = cell.uses.iter().find(|use_| use_.actor == actor) else {
                    continue;
                };
                let Some(transfer) = use_.retraction.transfer else {
                    continue;
                };
                let Some(wake) = use_.retraction.wake.resource else {
                    continue;
                };
                let reset_witness = use_
                    .reset
                    .observer_wake()
                    .is_none_or(|wake| !wake.on_base_changed.is_empty() && !wake.on_base_restored.is_empty());
                let actor_spec = graph.pistons[actor.index()];
                if reset_witness
                    && transfer.resource == resource.resource
                    && transfer.source == resource.source
                    && transfer.destination == actor_spec.near
                    && transfer.destination != transfer.source
                    && transfer.when_at_source == resource.present
                    && resource.owners.contains(&actor)
                    && wake.resource == transfer.resource
                    && wake.contact == transfer.destination
                    && use_.retraction.wake.ticks_after_acceptance > 0
                    && actor_spec.routes.resource_restored_near == Some(wake.route)
                {
                    result.insert((transfer.resource, transfer.destination));
                }
            }
        }
    }
    result
}

#[cfg(test)]
fn equation_inputs(builder: &LayerBuilder<'_>, root: LayerExprId) -> FxHashSet<LayerInputSource> {
    let mut inputs = FxHashSet::default();
    let mut visited = FxHashSet::default();
    let mut pending = vec![root];
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        match &builder.expressions[id.0 as usize] {
            LayerExpr::Input(input) => {
                inputs.insert(builder.inputs[input.0 as usize].source);
            }
            LayerExpr::Not(child) => pending.push(*child),
            LayerExpr::All(children) | LayerExpr::Any(children) => pending.extend(children.iter().copied()),
            LayerExpr::Constant(_) => {}
        }
    }
    inputs
}
