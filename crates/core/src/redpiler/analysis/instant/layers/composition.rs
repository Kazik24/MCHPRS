//! Symbolic FIFO composition; callback-time requests and live acceptance are separate cuts.
//! Callers must supply source-proven callback order. Settled corridor equations alone
//! cannot authorize a sample. Base callbacks run before the conditional Far transfer.
use super::*;
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
            requests: VecDeque::new(),
            open: None,
            builder: LayerBuilder::new(monitor)?,
            layers: Vec::new(),
            resources: Vec::new(),
        })
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
        let use_ = self.use_(cell, actor)?;
        if causes.is_empty() || causes.iter().any(|cause| !use_.activations.contains(cause)) {
            return Err("sample has no original actor callback witness".into());
        }
        // This event implies prior acceptance; a failed sample never adds an ancestor.
        if ancestors.contains(&actor) {
            return Ok(());
        }
        let requested_pin = use_.requested;
        let values = self.pins(cell, actor, event)?;
        self.push_request(cell, actor, values[requested_pin.index()], causes, ancestors)
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
        self.push_request(cell, actor, requested, vec![cause].into(), &FxHashSet::default())
    }

    fn push_request(
        &mut self,
        cell: u32,
        actor: PistonId,
        sampled: LayerExprId,
        causes: Box<[RouteOccurrence]>,
        ancestors: &FxHashSet<PistonId>,
    ) -> Result<(), ConstructionError> {
        let pending = self.pending.get(&actor).copied().unwrap_or(self.builder.constant(false)?);
        let not_pending = self.builder.not(pending)?;
        let requested = self.builder.all([sampled, not_pending])?;
        let pending_after = self.builder.any([pending, requested])?;
        self.pending.insert(actor, pending_after);
        self.requests.push_back(Request {
            cell,
            actor,
            requested,
            causes,
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
        let before = self.consumed.get(&request.actor).copied().unwrap_or(self.builder.constant(false)?);
        let not_consumed = self.builder.not(before)?;
        let accepted = self.builder.all([request.requested, values[use_.valid.index()], not_consumed])?;
        let after = self.builder.any([before, accepted])?;
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
