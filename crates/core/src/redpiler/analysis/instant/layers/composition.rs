//! Symbolic FIFO composition; callback-time requests and live acceptance are separate cuts.
//! Callers must supply source-proven callback order. Settled corridor equations alone
//! cannot authorize a sample. Base callbacks run before the conditional Far transfer.
use super::*;
use mchprs_blocks::blocks::Block;
use rustc_hash::FxHashSet;
use std::collections::VecDeque;

mod connected;
pub(super) mod corridors;
pub(super) use connected::compile_connected;

struct Request {
    /// This catalog cell owns the requested computation.
    cell: u32,
    /// This actor is the request's deduplication and single-use identity.
    actor: PistonId,
    /// This predicate was captured at delivery rather than recomputed at acceptance.
    requested: LayerExprId,
    /// These source occurrences justify the qualifying callback.
    causes: Box<[RouteOccurrence]>,
    /// These earlier original callbacks are the cuts that can create this request.
    enqueue_triggers: Box<[RouteOccurrence]>,
    /// These actors are consumed whenever this particular event predicate is true.
    ancestors: FxHashSet<PistonId>,
}

pub(super) struct Acceptance {
    /// This actor owns the open base-change phase.
    actor: PistonId,
    /// This action passed live revalidation at its FIFO prefix.
    pub(super) accepted: LayerExprId,
    /// This conditional transfer snapshots source presence before base callbacks.
    transfer: Option<(CellResourceTransfer, LayerExprId)>,
    /// This catalog cell owns the token write.
    cell: u32,
    /// These accepted predecessors justify pruning feedback without Boolean path expansion.
    pub(super) ancestors: FxHashSet<PistonId>,
}

pub(super) struct Composition<'a> {
    /// This immutable circuit supplies directional power and contact geometry.
    graph: &'a CompileGraph,
    /// These contracts supply the existing request, acceptance and ownership equations.
    catalog: &'a BoundaryCatalog,
    /// This index is shared across all conditional acceptance prefixes.
    power: &'a PowerIndex,
    /// These wires must be substituted, never read as independent boundary inputs.
    wires: FxHashSet<NodeId>,
    /// These predicates represent state after preceding conditional writes.
    state: FxHashMap<StateTest, LayerExprId>,
    /// These flags preserve per-actor request deduplication.
    pending: FxHashMap<PistonId, LayerExprId>,
    /// These flags permit at most one accepted computation per actor per wave.
    consumed: FxHashMap<PistonId, LayerExprId>,
    /// Experimental whole-wave mode consumes a movable resource once per cycle.
    #[cfg(test)]
    resource_consumed: Option<FxHashMap<ResourceId, LayerExprId>>,
    /// These potentially queued requests keep their source FIFO order.
    requests: VecDeque<Request>,
    /// This prevents accepting a nested action before the open action's Far/Near cut.
    open: Option<PistonId>,
    /// These shared expressions contain no physical wire snapshot state.
    pub(super) builder: LayerBuilder<'a>,
    /// These ordered accepted-use candidates are cold construction witnesses.
    pub(super) layers: Vec<CausalLayer>,
    /// These conditional writes preserve shared-resource selection and destinations.
    pub(super) resources: Vec<LayerResourceOutput>,
}

impl<'a> Composition<'a> {
    pub(super) fn new(
        graph: &'a CompileGraph,
        catalog: &'a BoundaryCatalog,
        power: &'a PowerIndex,
        component: &RegionComponent,
        monitor: &'a TaskMonitor,
    ) -> Result<Self, ConstructionError> {
        let wires = component
            .corridors
            .iter()
            .flat_map(|&id| catalog.corridors[id as usize].wires.iter().copied())
            .collect();
        Ok(Self {
            graph,
            catalog,
            power,
            wires,
            state: FxHashMap::default(),
            pending: FxHashMap::default(),
            consumed: FxHashMap::default(),
            #[cfg(test)]
            resource_consumed: None,
            requests: VecDeque::new(),
            open: None,
            builder: LayerBuilder::new(monitor)?,
            layers: Vec::new(),
            resources: Vec::new(),
        })
    }

    #[cfg(test)]
    fn new_resource_wave(
        graph: &'a CompileGraph,
        catalog: &'a BoundaryCatalog,
        power: &'a PowerIndex,
        component: &RegionComponent,
        monitor: &'a TaskMonitor,
    ) -> Result<Self, ConstructionError> {
        let mut actors = FxHashMap::default();
        for &cell in &component.cells {
            let cell = catalog.cells.get(cell as usize).ok_or("resource wave references a missing cell")?;
            for use_ in &cell.uses {
                let transfer = use_
                    .retraction
                    .transfer
                    .ok_or("resource wave requires a modeled transfer for every actor")?;
                let mut candidates = cell.resources.iter().filter(|resource| resource.owners.contains(&use_.actor));
                let candidate = candidates.next().ok_or("resource wave actor has no source resource contract")?;
                if candidates.next().is_some()
                    || candidate.resource != transfer.resource
                    || candidate.source != transfer.source
                    || candidate.present != transfer.when_at_source
                {
                    return Err("resource wave actor has ambiguous source resources".into());
                }
                let resource = graph
                    .resources
                    .get(transfer.resource.index())
                    .ok_or("resource wave references a missing resource")?;
                if transfer.source == transfer.destination
                    || !resource.contacts.contains(&transfer.source)
                    || !resource.contacts.contains(&transfer.destination)
                    || transfer.when_at_source.index() >= cell.pins.len()
                {
                    return Err("resource wave transfer is outside its modeled resource".into());
                }
                if actors.insert(use_.actor, transfer.resource).is_some() {
                    return Err("resource wave actor has multiple uses".into());
                }
            }
        }
        let mut composition = Self::new(graph, catalog, power, component, monitor)?;
        composition.resource_consumed = Some(FxHashMap::default());
        Ok(composition)
    }

    fn use_(&self, cell: u32, actor: PistonId) -> Result<&CellUse, ConstructionError> {
        self.catalog
            .cells
            .get(cell as usize)
            .and_then(|cell| cell.uses.iter().find(|use_| use_.actor == actor))
            .ok_or_else(|| "scheduled actor is not owned by its cell".into())
    }

    pub(super) fn state(&mut self, test: StateTest) -> Result<LayerExprId, ConstructionError> {
        if let Some(&value) = self.state.get(&test) {
            return Ok(value);
        }
        let input = self.builder.input(LayerInputSource::State(test), LayerInputRole::RequestSample)?;
        let value = self.builder.input_expr(input)?;
        self.state.insert(test, value);
        Ok(value)
    }

    /// Build a callback guard at the current composed FIFO prefix, including
    /// captured site variants. Wire-arrival callbacks use this rather than the
    /// ordinary electrical tracer, which correctly has no captured-site view.
    pub(super) fn captured_guard(&mut self, id: GuardId) -> Result<Option<LayerExprId>, ConstructionError> {
        self.captured_guard_inner(id, 0)
    }

    fn captured_guard_inner(&mut self, id: GuardId, depth: usize) -> Result<Option<LayerExprId>, ConstructionError> {
        self.builder.visit()?;
        if depth >= 128 {
            return Err(ConstructionError::BudgetExceeded);
        }
        match self.graph.guards[id.index()].clone() {
            Guard::Always => self.builder.constant(true).map(Some),
            Guard::State(test) => self.state(test).map(Some),
            Guard::Site { view: OperandView::Live, .. } => {
                let mut tracer = signals::SignalTracer::new(self.graph, self.power, &self.wires, &self.state, LayerInputRole::RequestSample);
                match tracer.guard(id, &mut self.builder) {
                    Ok(value) => Ok(Some(value)),
                    Err(ConstructionError::Unsupported(_)) => Ok(None),
                    Err(error) => Err(error),
                }
            }
            Guard::Site {
                site,
                view: OperandView::Captured,
                test,
            } => self.captured_site_test(site, test, depth + 1),
            Guard::Not(child) => {
                let Some(value) = self.captured_guard_inner(child, depth + 1)? else {
                    return Ok(None);
                };
                self.builder.not(value).map(Some)
            }
            Guard::All(children) | Guard::Any(children) => {
                let is_all = matches!(self.graph.guards[id.index()], Guard::All(_));
                let mut values = Vec::with_capacity(children.len());
                for child in children.iter().copied() {
                    let Some(value) = self.captured_guard_inner(child, depth + 1)? else {
                        return Ok(None);
                    };
                    values.push(value);
                }
                if is_all {
                    self.builder.all(values).map(Some)
                } else {
                    self.builder.any(values).map(Some)
                }
            }
        }
    }

    fn captured_site_test(&mut self, site: SiteId, test: SiteTest, depth: usize) -> Result<Option<LayerExprId>, ConstructionError> {
        match self.graph.sites[site.index()].source {
            SiteSource::Constant(profile) => self
                .builder
                .constant(signals::profile_matches(self.graph.profiles[profile.index()], Some(profile), test))
                .map(Some),
            SiteSource::Component(node) => {
                // GraphBackend exposes component profile identity as None even
                // when the component has a saved raw state.
                if matches!(test, SiteTest::Profile(_)) {
                    return self.builder.constant(false).map(Some);
                }
                let spec = &self.graph.nodes[node.index()];
                let block = match spec.state {
                    ComponentState::Wire(wire) => Some(Block::RedstoneWire { wire }),
                    _ => spec.raw_state.map(Block::from_id),
                };
                let Some(block) = block else {
                    return Ok(None);
                };
                let value = match test {
                    SiteTest::Solid => block.is_solid(),
                    SiteTest::Transparent => block.is_transparent(),
                    SiteTest::IsWire => matches!(block, Block::RedstoneWire { .. }),
                    // GraphBackend resolves component profile identity as None.
                    SiteTest::Profile(_) => false,
                };
                self.builder.constant(value).map(Some)
            }
            SiteSource::Contact(contact) => {
                let variants = self.graph.contacts[contact.index()].variants.clone();
                let mut terms = Vec::new();
                let mut unmatched = self.builder.constant(true)?;
                for variant in variants.iter() {
                    self.builder.visit()?;
                    let Some(condition) = self.captured_guard_inner(variant.when, depth + 1)? else {
                        return Ok(None);
                    };
                    let selected = self.builder.all([unmatched, condition])?;
                    if signals::profile_matches(self.graph.profiles[variant.profile.index()], Some(variant.profile), test) {
                        terms.push(selected);
                    }
                    let not_condition = self.builder.not(condition)?;
                    unmatched = self.builder.all([unmatched, not_condition])?;
                }
                self.builder.any(terms).map(Some)
            }
        }
    }

    fn pins(&mut self, cell: u32, actor: PistonId, event: LayerExprId) -> Result<Vec<LayerExprId>, ConstructionError> {
        for pin in &self.catalog.cells[cell as usize].pins {
            if let CellPin::Read(CellRead::State(test)) = pin {
                self.state(*test)?;
            }
        }
        let mut signals = signals::SignalTracer::new(self.graph, self.power, &self.wires, &self.state, LayerInputRole::RequestSample);
        let mut values = Vec::with_capacity(self.catalog.cells[cell as usize].pins.len());
        for index in 0..self.catalog.cells[cell as usize].pins.len() {
            self.builder.visit()?;
            let pin = &self.catalog.cells[cell as usize].pins[index];
            let value = match pin {
                CellPin::Read(CellRead::State(test)) => self.state[test],
                CellPin::Read(CellRead::Activation(target)) => {
                    if *target == actor {
                        event
                    } else {
                        self.builder.constant(false)?
                    }
                }
                CellPin::Read(CellRead::Contribution(link)) => signals.contribution(*link, 0, &mut self.builder)?,
                CellPin::Gate { kind, inputs } => {
                    let operands = inputs
                        .iter()
                        .map(|id| values.get(id.index()).copied().ok_or("cell pin has a missing or forward operand"))
                        .collect::<Result<Vec<_>, _>>()?;
                    self.builder.gate(*kind, operands)?
                }
            };
            values.push(value);
        }
        Ok(values)
    }

    /// The event is already qualified; its source order must be established by the caller.
    #[cfg(test)]
    pub(super) fn sample(&mut self, cell: u32, actor: PistonId, event: LayerExprId, causes: Box<[RouteOccurrence]>) -> Result<(), ConstructionError> {
        self.sample_after(cell, actor, event, causes, &FxHashSet::default())
    }

    pub(super) fn sample_after(
        &mut self,
        cell: u32,
        actor: PistonId,
        event: LayerExprId,
        causes: Box<[RouteOccurrence]>,
        ancestors: &FxHashSet<PistonId>,
    ) -> Result<(), ConstructionError> {
        self.sample_after_from(cell, actor, event, causes.clone(), causes, ancestors)
    }

    /// Record the earlier parent callback that generated a sampled child callback.
    pub(super) fn sample_after_from(
        &mut self,
        cell: u32,
        actor: PistonId,
        event: LayerExprId,
        causes: Box<[RouteOccurrence]>,
        enqueue_triggers: Box<[RouteOccurrence]>,
        ancestors: &FxHashSet<PistonId>,
    ) -> Result<(), ConstructionError> {
        let use_ = self.use_(cell, actor)?;
        if causes.is_empty() || enqueue_triggers.is_empty() || causes.iter().any(|cause| !use_.activations.contains(cause)) {
            return Err("sample has no original actor callback witness".into());
        }
        // This event implies prior acceptance; a failed sample never adds an ancestor.
        if ancestors.contains(&actor) {
            return Ok(());
        }
        let requested_pin = use_.requested;
        let values = self.pins(cell, actor, event)?;
        self.push_request(cell, actor, values[requested_pin.index()], causes, enqueue_triggers, ancestors)
    }

    /// The original callback already sampled this request; acceptance still reads live state.
    pub(super) fn queued_sample(
        &mut self,
        cell: u32,
        actor: PistonId,
        requested: LayerExprId,
        cause: RouteOccurrence,
    ) -> Result<(), ConstructionError> {
        if !self.use_(cell, actor)?.activations.contains(&cause) {
            return Err("queued request has no original actor callback witness".into());
        }
        self.push_request(cell, actor, requested, vec![cause].into(), vec![cause].into(), &FxHashSet::default())
    }

    fn push_request(
        &mut self,
        cell: u32,
        actor: PistonId,
        sampled: LayerExprId,
        causes: Box<[RouteOccurrence]>,
        enqueue_triggers: Box<[RouteOccurrence]>,
        ancestors: &FxHashSet<PistonId>,
    ) -> Result<(), ConstructionError> {
        let pending = self.pending.get(&actor).copied().unwrap_or(self.builder.constant(false)?);
        let not_pending = self.builder.not(pending)?;
        let requested = self.builder.all([sampled, not_pending])?;
        // A statically false sample cannot create a native FIFO request or any
        // later acceptance/resource effects. Drop it before expanding its
        // callback paths; keep every nonconstant sample for live revalidation.
        if matches!(self.builder.expression(requested)?, LayerExpr::Constant(false)) {
            return Ok(());
        }
        let pending_after = self.builder.any([pending, requested])?;
        self.pending.insert(actor, pending_after);
        self.requests.push_back(Request {
            cell,
            actor,
            requested,
            causes,
            enqueue_triggers,
            ancestors: ancestors.clone(),
        });
        Ok(())
    }

    /// Expose the base-change cut before token transfer and downstream Far callbacks.
    pub(super) fn begin_next(&mut self) -> Result<Option<Acceptance>, ConstructionError> {
        if self.open.is_some() {
            return Err("nested acceptance before the current Far/Near cut".into());
        }
        let Some(request) = self.requests.pop_front() else {
            return Ok(None);
        };
        let use_ = self.use_(request.cell, request.actor)?.clone();
        let active = self.builder.constant(true)?;
        let values = self.pins(request.cell, request.actor, active)?;
        #[cfg(test)]
        let resource = use_.retraction.transfer.map(|transfer| transfer.resource);
        #[cfg(test)]
        let before = if let Some(consumed) = &self.resource_consumed {
            let resource = resource.ok_or("resource wave requires a modeled transfer")?;
            consumed.get(&resource).copied().unwrap_or(self.builder.constant(false)?)
        } else {
            self.consumed.get(&request.actor).copied().unwrap_or(self.builder.constant(false)?)
        };
        #[cfg(not(test))]
        let before = self.consumed.get(&request.actor).copied().unwrap_or(self.builder.constant(false)?);
        let not_consumed = self.builder.not(before)?;
        #[cfg(test)]
        let accepted = if self.resource_consumed.is_some() {
            let transfer = use_.retraction.transfer.ok_or("resource wave requires a modeled transfer")?;
            self.builder.all([
                request.requested,
                values[use_.valid.index()],
                values[transfer.when_at_source.index()],
                not_consumed,
            ])?
        } else {
            self.builder.all([request.requested, values[use_.valid.index()], not_consumed])?
        };
        #[cfg(not(test))]
        let accepted = self.builder.all([request.requested, values[use_.valid.index()], not_consumed])?;
        let after = self.builder.any([before, accepted])?;
        #[cfg(test)]
        if let Some(consumed) = &mut self.resource_consumed {
            consumed.insert(resource.ok_or("resource wave requires a modeled transfer")?, after);
        } else {
            self.consumed.insert(request.actor, after);
        }
        #[cfg(not(test))]
        self.consumed.insert(request.actor, after);
        let pending = self.pending.get(&request.actor).copied().unwrap_or(self.builder.constant(false)?);
        let not_requested = self.builder.not(request.requested)?;
        self.pending.insert(request.actor, self.builder.all([pending, not_requested])?);
        let transfer = if let Some(transfer) = use_.retraction.transfer {
            Some((transfer, self.builder.all([accepted, values[transfer.when_at_source.index()]])?))
        } else {
            None
        };
        self.set_availability(request.actor, PistonAvailability::BasePendingHeadPresent, accepted)?;
        self.layers.push(CausalLayer {
            order: LayerOrder::Fifo,
            uses: vec![LayerUse {
                use_id: LayerUseId(request.actor.index() as u32),
                actor: request.actor,
                requested: request.requested,
                accepted,
                consumed_before: before,
                consumed_after: after,
                cause: UseCause {
                    boundary_occurrences: request.causes,
                    enqueue_triggers: request.enqueue_triggers,
                },
            }]
            .into(),
        });
        self.open = Some(request.actor);
        let mut ancestors = request.ancestors;
        ancestors.insert(request.actor);
        Ok(Some(Acceptance {
            actor: request.actor,
            accepted,
            transfer,
            cell: request.cell,
            ancestors,
        }))
    }

    /// Move the token exactly once before the caller samples Far/Near callback consequences.
    pub(super) fn finish_base(&mut self, acceptance: Acceptance) -> Result<LayerExprId, ConstructionError> {
        if self.open != Some(acceptance.actor) {
            return Err("acceptance phase does not own the open actor".into());
        }
        self.set_availability(acceptance.actor, PistonAvailability::BasePending, acceptance.accepted)?;
        self.write(StateTest::PistonHeadPresent(acceptance.actor), false, acceptance.accepted)?;
        let transfer_when = if let Some((transfer, when)) = acceptance.transfer {
            self.write(StateTest::ResourceEnabled(transfer.resource), false, when)?;
            for &contact in &self.graph.resources[transfer.resource.index()].contacts {
                self.write(
                    StateTest::ResourceAt {
                        resource: transfer.resource,
                        contact,
                    },
                    contact == transfer.destination,
                    when,
                )?;
            }
            let enabled = self.state(StateTest::ResourceEnabled(transfer.resource))?;
            let at_source = self.state(StateTest::ResourceAt {
                resource: transfer.resource,
                contact: transfer.source,
            })?;
            let present = self.builder.all([enabled, at_source])?;
            let absent = self.builder.not(present)?;
            self.resources.push(LayerResourceOutput {
                cell: acceptance.cell,
                resource: transfer.resource,
                source: transfer.source,
                destination: transfer.destination,
                transfer_when: when,
                source_absent_after: absent,
            });
            when
        } else {
            self.builder.constant(false)?
        };
        self.open = None;
        Ok(transfer_when)
    }

    fn write(&mut self, test: StateTest, value: bool, when: LayerExprId) -> Result<(), ConstructionError> {
        let old = self.state(test)?;
        let next = if value {
            self.builder.any([old, when])?
        } else {
            let not_when = self.builder.not(when)?;
            self.builder.all([old, not_when])?
        };
        self.state.insert(test, next);
        Ok(())
    }

    fn set_availability(&mut self, actor: PistonId, value: PistonAvailability, when: LayerExprId) -> Result<(), ConstructionError> {
        for is in [
            PistonAvailability::Ready,
            PistonAvailability::HeadPending,
            PistonAvailability::BasePendingHeadPresent,
            PistonAvailability::BasePending,
            PistonAvailability::PublishingHeadRemoval,
        ] {
            self.write(StateTest::PistonAvailability { piston: actor, is }, is == value, when)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
