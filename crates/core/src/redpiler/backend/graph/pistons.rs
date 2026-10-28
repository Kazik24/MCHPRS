#[cfg(test)]
use super::graph_test_trace;
use super::*;
use std::ops::Deref;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PistonRequest {
    pub(super) piston: PistonId,
    pub(super) action: mchprs_world::PistonAction,
    /// This identifies the actual callback route, when dispatch supplied one.
    pub(super) source: Option<RouteId>,
}

/// Native FIFO order and first-source retention for each `(piston, action)` pair.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct PistonRequestQueue {
    fifo: VecDeque<PistonRequest>,
}

impl PistonRequestQueue {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn push_back_if_absent(&mut self, request: PistonRequest) -> bool {
        if self
            .fifo
            .iter()
            .any(|queued| queued.piston == request.piston && queued.action == request.action)
        {
            return false;
        }
        self.fifo.push_back(request);
        true
    }

    pub(super) fn pop_front(&mut self) -> Option<PistonRequest> {
        self.fifo.pop_front()
    }
    pub(super) fn clear(&mut self) {
        self.fifo.clear();
    }

    #[cfg(test)]
    pub(super) fn push_front(&mut self, request: PistonRequest) -> bool {
        if self
            .fifo
            .iter()
            .any(|queued| queued.piston == request.piston && queued.action == request.action)
        {
            return false;
        }
        self.fifo.push_front(request);
        true
    }

    #[cfg(test)]
    pub(super) fn set_front_source(&mut self, source: Option<RouteId>) {
        self.fifo.front_mut().expect("request queue front exists").source = source;
    }
}

impl Deref for PistonRequestQueue {
    type Target = VecDeque<PistonRequest>;

    fn deref(&self) -> &Self::Target {
        &self.fifo
    }
}

impl<'a> IntoIterator for &'a PistonRequestQueue {
    type Item = &'a PistonRequest;
    type IntoIter = std::collections::vec_deque::Iter<'a, PistonRequest>;

    fn into_iter(self) -> Self::IntoIter {
        self.fifo.iter()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DeferredPistonEvent {
    pub(super) piston: PistonId,
    pub(super) revision: u64,
    pub(super) resource: Option<DeferredResourceEvent>,
    pub(super) due_tick: u64,
    pub(super) extending: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DeferredResourceEvent {
    pub(super) resource: ResourceId,
    pub(super) revision: u64,
    pub(super) contact: ContactId,
}

impl GraphBackend {
    #[inline(always)]
    pub(super) fn sample_piston(&mut self, piston: PistonId) -> Result<(), GraphExecutionError> {
        self.sample_piston_inner(piston, None)
    }

    pub(super) fn sample_piston_from(&mut self, piston: PistonId, route: RouteId) -> Result<(), GraphExecutionError> {
        self.sample_piston_inner(piston, Some(route))
    }

    fn sample_piston_inner(&mut self, piston: PistonId, source: Option<RouteId>) -> Result<(), GraphExecutionError> {
        if self.logical_node_owned(self.program.pistons()[piston.index()].node) && !self.active_frame_retained_actors.contains(&piston) {
            let node = self.program.pistons()[piston.index()].node;
            return Err(self.unsupported(node, "logical frame received an unowned piston sample"));
        }
        #[cfg(test)]
        let spec = self.program.pistons()[piston.index()];
        let state = self.state.pistons[piston.index()];
        if state.availability.base_pending() {
            return Ok(());
        }
        let (action, powered) = {
            if self.logical_bud_owner_by_actor.get(piston.index()).is_some_and(Option::is_some) {
                self.counts.logical_bud_samples += 1;
            } else {
                self.counts.piston_samples += 1;
            }
            #[cfg(test)]
            if let Some(activity) = self.test_recorder.memory_cell_activity.get_mut(piston.index()) {
                activity[0] += 1;
            }
            let powered = self.piston_powered(piston);
            #[cfg(test)]
            #[cfg(test)]
            if self.test_recording {
                self.test_recorder.observed_operations.push(GraphObservedOperation::Sample {
                    actor: self.program.nodes()[spec.node.index()].position.unwrap(),
                    extended: state.extended,
                    powered,
                });
                self.test_recorder.native_piston_trace.push(crate::redstone::piston::trace::Entry {
                    tick: self.future.clock.logical_tick,
                    phase: native_trace_phase(self.future.clock.phase),
                    operation: crate::redstone::piston::trace::Operation::Sample {
                        pos: self.program.nodes()[spec.node.index()].position.unwrap(),
                        extended: state.extended,
                        powered,
                    },
                });
            }
            let action = match (powered, state.extended) {
                (true, false) if self.piston_lane_available(piston) => mchprs_world::PistonAction::Extend,
                (false, true) if self.early_resource_retraction(piston) => mchprs_world::PistonAction::RetractWithoutPull,
                (false, true) => mchprs_world::PistonAction::Retract,
                _ => {
                    #[cfg(test)]
                    self.test_recorder.piston_sample_census.sample(piston, powered, None);
                    return Ok(());
                }
            };
            (action, powered)
        };
        #[cfg(test)]
        self.test_recorder.piston_sample_census.sample(piston, powered, Some(action));
        #[cfg(not(test))]
        let _ = powered;
        self.enqueue_piston(PistonRequest { piston, action, source });
        Ok(())
    }

    pub(super) fn early_resource_retraction(&self, piston: PistonId) -> bool {
        let spec = self.program.pistons()[piston.index()];
        match spec.lane {
            PistonLane::SingleResource { far } => self
                .piston_resource_at(piston, far)
                .and_then(|resource| self.pending_resource_extension(resource, far, spec.facing))
                .is_some_and(|event| {
                    event.due_tick > self.future.clock.logical_tick
                        || matches!(self.future.clock.phase, GraphPhase::ScheduledTicks | GraphPhase::PistonActions)
                }),
            PistonLane::Empty => false,
        }
    }

    pub(super) fn enqueue_piston(&mut self, request: PistonRequest) {
        if self.future.piston_requests.push_back_if_absent(request) {
            #[cfg(test)]
            if self.test_recording {
                self.test_recorder.observed_operations.push(GraphObservedOperation::Request {
                    actor: self.program.nodes()[self.program.pistons()[request.piston.index()].node.index()]
                        .position
                        .unwrap(),
                    action: request.action,
                });
            }
            #[cfg(test)]
            graph_test_trace!(self, format!("piston request {:?} {:?}", request.piston, request.action));
        }
    }

    pub(super) fn drain_piston_actions(&mut self) -> Result<(), GraphExecutionError> {
        let result = (|| {
            while let Some(request) = self.future.piston_requests.pop_front() {
                #[cfg(test)]
                if self.test_recorder.test_recording {
                    self.test_recorder
                        .popped_piston_requests
                        .push((request.piston, request.action, request.source));
                }
                if self.logical_node_owned(self.program.pistons()[request.piston.index()].node)
                    && !self.active_frame_retained_actors.contains(&request.piston)
                {
                    let node = self.program.pistons()[request.piston.index()].node;
                    return Err(self.unsupported(node, "logical frame received an unowned piston action"));
                }
                if self.active_frame_retained_actors.contains(&request.piston) {
                    self.cancel_pull_wave_continuation();
                    self.accept_action(request)?;
                } else if !self.run_instant_kernel(request)? {
                    self.accept_action(request)?;
                }
            }
            Ok(())
        })();
        self.cancel_pull_wave_continuation();
        result
    }

    fn accept_action(&mut self, request: PistonRequest) -> Result<(), GraphExecutionError> {
        self.accept_action_using(request, None)
    }

    pub(super) fn accept_compiled_pull(&mut self, request: PistonRequest, accepted: bool) -> Result<(), GraphExecutionError> {
        #[cfg(test)]
        if self.test_recording {
            let node = self.program.pistons()[request.piston.index()].node;
            let powered = self.input_strength(node, InputPort::Main) > 0 || self.input_strength(node, InputPort::Quasi) > 0;
            assert_eq!(
                accepted,
                !powered && self.piston_retraction_available(request.piston, false),
                "compiled ready-pull acceptance"
            );
        }
        self.accept_action_using(request, Some(accepted))
    }

    pub(super) fn accept_action_using(&mut self, request: PistonRequest, compiled: Option<bool>) -> Result<(), GraphExecutionError> {
        #[cfg(test)]
        let spec = self.program.pistons()[request.piston.index()];
        let state = self.state.pistons[request.piston.index()];
        #[cfg(test)]
        if self.test_recording {
            self.test_recorder.observed_operations.push(GraphObservedOperation::AcceptanceAttempt {
                actor: self.program.nodes()[spec.node.index()].position.unwrap(),
                action: request.action,
            });
        }
        let accepted = compiled.unwrap_or_else(|| self.native_action_accepted(request));
        #[cfg(test)]
        self.record_accepted_boundary_probe(request, accepted);
        if accepted
            && matches!(
                request.action,
                mchprs_world::PistonAction::Retract | mchprs_world::PistonAction::RetractWithoutPull
            )
            && self.native_action_clock_owner.get(request.piston.index()).is_some_and(Option::is_some)
        {
            self.run_native_action_clock_step(request.piston, request.action)?;
        }
        #[cfg(test)]
        if accepted
            && matches!(
                request.action,
                mchprs_world::PistonAction::Retract | mchprs_world::PistonAction::RetractWithoutPull
            )
            && self.native_action_clock_owner.get(request.piston.index()).is_some_and(Option::is_some)
            && self.test_recording
        {
            self.test_recorder.observed_operations.push(GraphObservedOperation::RetractionDispatch {
                actor: self.program.nodes()[spec.node.index()].position.unwrap(),
                without_pull: request.action == mchprs_world::PistonAction::RetractWithoutPull,
            });
        }
        if self.logical_bud_owner_by_actor.get(request.piston.index()).is_some_and(Option::is_some) {
            if accepted {
                self.apply_logical_bud_action(request)?;
                self.counts.logical_bud_accepts += 1;
                self.publish_piston_action(request, state.extended);
                #[cfg(test)]
                if self.test_recording {
                    self.test_recorder.observed_operations.push(GraphObservedOperation::Accepted {
                        actor: self.program.nodes()[spec.node.index()].position.unwrap(),
                        action: request.action,
                    });
                }
            }
            return Ok(());
        }
        match request.action {
            mchprs_world::PistonAction::Extend if accepted => {
                #[cfg(test)]
                graph_test_trace!(self, format!("piston accept {:?} {:?}", request.piston, request.action));
                self.extend_connections(request.piston)?;
                self.accepted(request, state.extended);
                #[cfg(test)]
                if self.test_recording {
                    self.test_recorder.observed_operations.push(GraphObservedOperation::Accepted {
                        actor: self.program.nodes()[spec.node.index()].position.unwrap(),
                        action: request.action,
                    });
                }
            }
            mchprs_world::PistonAction::Retract | mchprs_world::PistonAction::RetractWithoutPull if accepted => {
                #[cfg(test)]
                graph_test_trace!(self, format!("piston accept {:?} {:?}", request.piston, request.action));
                self.retract_connections(request.piston, request.action == mchprs_world::PistonAction::RetractWithoutPull)?;
                self.accepted(request, state.extended);
                #[cfg(test)]
                if self.test_recording {
                    self.test_recorder.observed_operations.push(GraphObservedOperation::Accepted {
                        actor: self.program.nodes()[spec.node.index()].position.unwrap(),
                        action: request.action,
                    });
                }
            }
            _ => {
                #[cfg(test)]
                graph_test_trace!(self, format!("piston cancelled {:?} {:?}", request.piston, request.action));
            }
        }
        Ok(())
    }

    pub(super) fn native_action_accepted(&self, request: PistonRequest) -> bool {
        let state = self.state.pistons[request.piston.index()];
        let powered = self.piston_powered(request.piston);
        match request.action {
            mchprs_world::PistonAction::Extend => {
                powered && !state.extended && state.availability == PistonAvailability::Ready && self.piston_lane_available(request.piston)
            }
            mchprs_world::PistonAction::Retract | mchprs_world::PistonAction::RetractWithoutPull => {
                !powered
                    && state.extended
                    && !state.availability.base_pending()
                    && self.piston_retraction_available(request.piston, request.action == mchprs_world::PistonAction::RetractWithoutPull)
            }
        }
    }

    fn apply_logical_bud_action(&mut self, request: PistonRequest) -> Result<(), GraphExecutionError> {
        let actor = request.piston;
        let spec = self.program.pistons()[actor.index()];
        match request.action {
            mchprs_world::PistonAction::Extend => {
                let resource = self.piston_resource_at(actor, spec.near);
                let revision = {
                    let state = &mut self.state.pistons[actor.index()];
                    state.revision = state.revision.wrapping_add(1);
                    state.availability = PistonAvailability::HeadPending;
                    state.head_present = true;
                    state.revision
                };
                let deferred_resource = match (resource, spec.lane) {
                    (Some(resource), PistonLane::SingleResource { far }) => {
                        self.set_resource_contact(resource, far, false)
                            .map(|(resource, revision)| DeferredResourceEvent {
                                resource,
                                revision,
                                contact: far,
                            })
                    }
                    _ => None,
                };
                self.future.deferred_pistons.push_back(DeferredPistonEvent {
                    piston: actor,
                    revision,
                    resource: deferred_resource,
                    due_tick: self.future.clock.logical_tick + self.logical_bud_delay(actor),
                    extending: true,
                });
                let resource_moved = deferred_resource.is_some();
                self.deliver_logical_bud_cut(
                    actor,
                    LogicalBudEventCut::ExtendFarShape,
                    resource_moved,
                    false,
                    false,
                    false,
                    false,
                    false,
                )?;
                self.deliver_logical_bud_cut(
                    actor,
                    LogicalBudEventCut::ExtendNearAfterTransfer,
                    resource_moved,
                    false,
                    false,
                    false,
                    false,
                    false,
                )?;
                self.state.pistons[actor.index()].extended = true;
                self.deliver_logical_bud_cut(actor, LogicalBudEventCut::ExtendBase, resource_moved, false, false, false, false, false)?;
            }
            mchprs_world::PistonAction::Retract | mchprs_world::PistonAction::RetractWithoutPull => {
                let without_pull = request.action == mchprs_world::PistonAction::RetractWithoutPull;
                let resource_at_far = match spec.lane {
                    PistonLane::SingleResource { far } => self.piston_resource_at(actor, far),
                    PistonLane::Empty => None,
                };
                let matching_extension = match (resource_at_far, spec.lane) {
                    (Some(resource), PistonLane::SingleResource { far }) => self.pending_resource_extension(resource, far, spec.facing).is_some(),
                    _ => false,
                };
                let resource_enabled = resource_at_far.is_some_and(|resource| self.state.resources[resource.index()].enabled);
                let restore_far = matching_extension;
                let pull_resource = !without_pull && resource_enabled;
                let interrupted_head = self.state.pistons[actor.index()].availability == PistonAvailability::HeadPending;
                let revision = {
                    let state = &mut self.state.pistons[actor.index()];
                    state.revision = state.revision.wrapping_add(1);
                    state.availability = if interrupted_head {
                        PistonAvailability::PublishingHeadRemoval
                    } else {
                        PistonAvailability::BasePendingHeadPresent
                    };
                    state.revision
                };
                if interrupted_head {
                    self.deliver_logical_bud_cut(actor, LogicalBudEventCut::RetractNearBeforeBase, false, true, false, false, false, false)?;
                    self.state.pistons[actor.index()].head_present = false;
                    self.state.pistons[actor.index()].availability = PistonAvailability::BasePending;
                    self.deliver_logical_bud_cut(actor, LogicalBudEventCut::RetractBaseAfterNear, false, true, false, false, false, false)?;
                } else {
                    self.deliver_logical_bud_cut(
                        actor,
                        LogicalBudEventCut::RetractBaseBeforeTransfer,
                        false,
                        false,
                        false,
                        false,
                        false,
                        false,
                    )?;
                    self.state.pistons[actor.index()].availability = PistonAvailability::BasePending;
                    self.state.pistons[actor.index()].head_present = false;
                }
                let restored_far = if restore_far {
                    let resource = resource_at_far.expect("matching extension requires a far resource");
                    let PistonLane::SingleResource { far } = spec.lane else { unreachable!() };
                    self.set_resource_contact(resource, far, true)
                } else {
                    None
                };
                if restored_far.is_some() {
                    self.deliver_logical_bud_cut(
                        actor,
                        LogicalBudEventCut::RetractResourceRestoredFar,
                        false,
                        interrupted_head,
                        true,
                        false,
                        false,
                        false,
                    )?;
                }
                let resource = match (pull_resource, resource_at_far, spec.lane) {
                    (true, Some(resource), PistonLane::SingleResource { .. }) => self.set_resource_contact(resource, spec.near, false),
                    _ => None,
                };
                if !without_pull && resource.is_some() {
                    self.deliver_logical_bud_cut(
                        actor,
                        LogicalBudEventCut::RetractFarPulled,
                        true,
                        interrupted_head,
                        false,
                        true,
                        false,
                        false,
                    )?;
                }
                self.deliver_logical_bud_cut(
                    actor,
                    LogicalBudEventCut::RetractNearAfterTransfer,
                    false,
                    interrupted_head,
                    false,
                    resource.is_some(),
                    false,
                    false,
                )?;
                let deferred_resource = resource.map(|(resource, revision)| DeferredResourceEvent {
                    resource,
                    revision,
                    contact: spec.near,
                });
                self.future.deferred_pistons.push_back(DeferredPistonEvent {
                    piston: actor,
                    revision,
                    resource: deferred_resource,
                    due_tick: self.future.clock.logical_tick + self.logical_bud_delay(actor),
                    extending: false,
                });
            }
        }
        Ok(())
    }

    fn logical_bud_delay(&self, actor: PistonId) -> u64 {
        self.program
            .logical_bud_owner(actor)
            .map_or(2, |plan| u64::from(plan.transition.publication_delay_ticks))
    }

    pub(super) fn accepted(&mut self, request: PistonRequest, extended: bool) {
        self.counts.piston_actions += 1;
        #[cfg(test)]
        if let Some(activity) = self.test_recorder.memory_cell_activity.get_mut(request.piston.index()) {
            activity[1] += 1;
        }
        self.publish_piston_action(request, extended);
    }

    pub(super) fn publish_piston_action(&mut self, request: PistonRequest, extended: bool) {
        let spec = self.program.pistons()[request.piston.index()];
        #[cfg(test)]
        if self.test_recording {
            self.test_recorder.native_piston_trace.push(crate::redstone::piston::trace::Entry {
                tick: self.future.clock.logical_tick,
                phase: native_trace_phase(self.future.clock.phase),
                operation: crate::redstone::piston::trace::Operation::Applied(mchprs_world::PistonEvent {
                    pos: spec.position,
                    sticky: spec.sticky,
                    facing: spec.facing,
                    action: request.action,
                }),
            });
        }
        self.effects.push(PresentationEffect::BlockAction {
            position: self.position(spec.node),
            action: BlockAction::Piston {
                action: request.action,
                piston: RedstonePiston {
                    facing: spec.facing.into(),
                    sticky: spec.sticky,
                    extended,
                },
            },
        });
    }

    pub(super) fn set_resource_contact(&mut self, resource: ResourceId, contact: ContactId, enabled: bool) -> Option<(ResourceId, u64)> {
        let index = resource.index();
        debug_assert!(self.program.resources()[index].contacts.contains(&contact));
        debug_assert!(
            self.program
                .resources_at_contact(contact)
                .iter()
                .all(|other| { other.index() == index || self.state.resources[other.index()].at != contact })
        );
        let state = &mut self.state.resources[index];
        if state.at == contact && state.enabled == enabled {
            return None;
        }
        state.at = contact;
        state.enabled = enabled;
        state.revision = state.revision.wrapping_add(1);
        Some((ResourceId::new(index as u32), state.revision))
    }

    fn resource_for_lane(&self, piston: PistonId) -> Option<ResourceId> {
        self.program.resource_for_piston(piston)
    }

    pub(super) fn piston_resource_at(&self, piston: PistonId, contact: ContactId) -> Option<ResourceId> {
        let resource = self.resource_for_lane(piston)?;
        (self.state.resources[resource.index()].at == contact).then_some(resource)
    }

    pub(super) fn pending_resource_extension(&self, resource_id: ResourceId, contact: ContactId, facing: BlockFace) -> Option<&DeferredPistonEvent> {
        let state = self.state.resources[resource_id.index()];
        if state.enabled || state.at != contact {
            return None;
        }

        self.future.deferred_pistons.iter().find(|event| {
            event.extending
                && self.program.pistons()[event.piston.index()].facing == facing
                && event
                    .resource
                    .is_some_and(|resource| resource.resource == resource_id && resource.revision == state.revision && resource.contact == contact)
        })
    }

    pub(super) fn resource_contact_available(&self, resource: ResourceId, contact: ContactId, vacated_head: Option<PistonId>) -> bool {
        if let Some(actor) = self.program.private_resource_owner(resource) {
            let spec = self.program.pistons()[actor.index()];
            if !self.program.resources()[resource.index()].contacts.contains(&contact) {
                return false;
            }
            // Ownership closure proves that only this actor's head can obstruct either contact.
            let piston = self.state.pistons[actor.index()];
            if contact == spec.near
                && piston.head_present
                && matches!(
                    piston.availability,
                    PistonAvailability::Ready | PistonAvailability::HeadPending | PistonAvailability::BasePendingHeadPresent
                )
                && vacated_head != Some(actor)
            {
                return false;
            }
            return self.resource_contact_is_free(resource, contact, vacated_head);
        }
        self.general_resource_contact_available(resource, contact, vacated_head)
    }

    pub(super) fn general_resource_contact_available(&self, resource: ResourceId, contact: ContactId, vacated_head: Option<PistonId>) -> bool {
        let Some(spec) = self.program.resources().get(resource.index()) else {
            return false;
        };
        if !spec.contacts.contains(&contact)
            || self
                .program
                .resources_at_contact(contact)
                .iter()
                .any(|other| other.index() != resource.index() && self.state.resources[other.index()].at == contact)
        {
            return false;
        }
        for &id in self.program.base_pistons_at_contact(contact) {
            if !self.state.pistons[id.index()].availability.base_pending() {
                return false;
            }
        }
        for &id in self.program.head_pistons_at_contact(contact) {
            let state = self.state.pistons[id.index()];
            let head_is_present = state.head_present
                && matches!(
                    state.availability,
                    PistonAvailability::Ready | PistonAvailability::HeadPending | PistonAvailability::BasePendingHeadPresent
                );
            if head_is_present && vacated_head != Some(id) {
                return false;
            }
        }
        self.resource_contact_is_free(resource, contact, vacated_head)
    }

    fn resource_contact_is_free(&self, resource: ResourceId, contact: ContactId, vacated_head: Option<PistonId>) -> bool {
        let state = self.state.resources[resource.index()];
        if state.at == contact && state.enabled {
            return true;
        }
        if vacated_head.is_some_and(|id| self.program.pistons()[id.index()].near == contact) {
            return true;
        }
        self.contact_block(contact) == Block::Air
    }

    pub(super) fn piston_retraction_available(&self, piston: PistonId, without_pull: bool) -> bool {
        let spec = self.program.pistons()[piston.index()];
        let PistonLane::SingleResource { far } = spec.lane else {
            return true;
        };
        let Some(resource) = self.piston_resource_at(piston, far) else {
            return true;
        };
        let state = self.state.resources[resource.index()];
        let matching_extension = self.pending_resource_extension(resource, far, spec.facing).is_some();
        if !state.enabled {
            !matching_extension || self.resource_contact_available(resource, far, None)
        } else {
            without_pull || self.resource_contact_available(resource, spec.near, Some(piston))
        }
    }

    pub(super) fn contact_block(&self, contact: ContactId) -> Block {
        let variant = self.program.contacts()[contact.index()]
            .variants
            .iter()
            .find(|variant| self.guard_matches(variant.when, None))
            .expect("compiled contact has a selected variant");
        self.program
            .profile_block(variant.profile)
            .expect("compiled contact profile was validated")
    }

    pub(super) fn piston_lane_available(&self, piston: PistonId) -> bool {
        let spec = self.program.pistons()[piston.index()];
        let near = self.contact_block(spec.near);
        if ContainerType::from_block(near).is_some() || matches!(near, Block::MovingPiston { .. } | Block::PistonHead { .. }) {
            return false;
        }
        let Some(resource) = self.piston_resource_at(piston, spec.near) else {
            return near == Block::Air && self.resource_at_contact(spec.near).is_none();
        };
        let state = self.state.resources[resource.index()];
        let PistonLane::SingleResource { far } = spec.lane else {
            return false;
        };
        state.enabled
            && near == self.program.profile_block(self.program.resources()[resource.index()].material).unwrap()
            && self.resource_contact_available(resource, far, None)
    }

    fn resource_at_contact(&self, contact: ContactId) -> Option<ResourceId> {
        self.program
            .resources_at_contact(contact)
            .iter()
            .copied()
            .find(|resource| self.state.resources[resource.index()].at == contact)
    }

    fn extend_connections(&mut self, piston: PistonId) -> Result<(), GraphExecutionError> {
        let spec = self.program.pistons()[piston.index()];
        let resource_to_move = self.piston_resource_at(piston, spec.near);
        if let (Some(resource), PistonLane::SingleResource { far }) = (resource_to_move, spec.lane) {
            debug_assert!(self.resource_contact_available(resource, far, None));
        }
        let revision = {
            let state = &mut self.state.pistons[piston.index()];
            state.revision = state.revision.wrapping_add(1);
            state.availability = PistonAvailability::HeadPending;
            state.head_present = true;
            state.revision
        };
        let resource = match (resource_to_move, spec.lane) {
            (Some(resource), PistonLane::SingleResource { far }) => self.set_resource_contact(resource, far, false),
            _ => None,
        };
        let deferred_resource = resource.map(|(resource, revision)| DeferredResourceEvent {
            resource,
            revision,
            contact: match spec.lane {
                PistonLane::SingleResource { far } => far,
                PistonLane::Empty => unreachable!(),
            },
        });
        // Nested callbacks must see the transfer's direction and deadline.
        self.future.deferred_pistons.push_back(DeferredPistonEvent {
            piston,
            revision,
            resource: deferred_resource,
            due_tick: self.future.clock.logical_tick + 2,
            extending: true,
        });
        if resource.is_some()
            && let Some(route) = spec.routes.far_shape_changed
        {
            self.deliver_route(route, None)?;
        }
        self.deliver_route(spec.routes.near_changed, None)?;
        if resource.is_some() {
            self.deliver_route(spec.routes.near_changed, None)?;
        }
        self.state.pistons[piston.index()].extended = true;
        self.deliver_route(spec.routes.base_changed, None)?;
        #[cfg(test)]
        graph_test_trace!(self, format!("piston extend {:?} revision={revision}", piston));
        Ok(())
    }

    pub(super) fn retract_connections(&mut self, piston: PistonId, without_pull: bool) -> Result<(), GraphExecutionError> {
        let spec = self.program.pistons()[piston.index()];
        let resource_at_far = match spec.lane {
            PistonLane::SingleResource { far } => self.piston_resource_at(piston, far),
            PistonLane::Empty => None,
        };
        let matching_extension = match (resource_at_far, spec.lane) {
            (Some(resource), PistonLane::SingleResource { far }) => self.pending_resource_extension(resource, far, spec.facing).is_some(),
            _ => false,
        };
        let resource_enabled = resource_at_far.is_some_and(|resource| self.state.resources[resource.index()].enabled);
        let restore_far = matching_extension;
        let pull_resource = !without_pull && resource_enabled;
        if let (Some(resource), PistonLane::SingleResource { far }) = (resource_at_far, spec.lane) {
            debug_assert!(if restore_far {
                self.resource_contact_available(resource, far, None)
            } else if pull_resource {
                self.resource_contact_available(resource, spec.near, Some(piston))
            } else {
                true
            });
        }
        let interrupted_head = self.state.pistons[piston.index()].availability == PistonAvailability::HeadPending;
        let revision = {
            let state = &mut self.state.pistons[piston.index()];
            state.revision = state.revision.wrapping_add(1);
            state.availability = if interrupted_head {
                PistonAvailability::PublishingHeadRemoval
            } else {
                PistonAvailability::BasePendingHeadPresent
            };
            state.revision
        };
        if interrupted_head {
            self.deliver_route(spec.routes.near_changed, None)?;
            self.state.pistons[piston.index()].head_present = false;
            self.state.pistons[piston.index()].availability = PistonAvailability::BasePending;
            self.deliver_route(spec.routes.base_changed, None)?;
        } else {
            self.deliver_route(spec.routes.base_changed, None)?;
            self.state.pistons[piston.index()].availability = PistonAvailability::BasePending;
            self.state.pistons[piston.index()].head_present = false;
        }
        let restored_far = if restore_far {
            let resource = resource_at_far.expect("restore requires a selected resource");
            let PistonLane::SingleResource { far } = spec.lane else {
                unreachable!();
            };
            self.set_resource_contact(resource, far, true)
        } else {
            None
        };
        if restored_far.is_some()
            && let Some(route) = spec.routes.resource_restored_far
        {
            self.deliver_route(route, None)?;
        }
        let resource = match (pull_resource, resource_at_far, spec.lane) {
            (true, Some(resource), PistonLane::SingleResource { .. }) => self.set_resource_contact(resource, spec.near, false),
            _ => None,
        };
        if !without_pull && resource.is_some() {
            if let Some(route) = spec.routes.far_changed {
                self.deliver_route(route, None)?;
            }
        }
        self.deliver_route(spec.routes.near_changed, None)?;
        let deferred_resource = resource.map(|(resource, revision)| DeferredResourceEvent {
            resource,
            revision,
            contact: spec.near,
        });
        self.future.deferred_pistons.push_back(DeferredPistonEvent {
            piston,
            revision,
            resource: deferred_resource,
            due_tick: self.future.clock.logical_tick + 2,
            extending: false,
        });
        #[cfg(test)]
        graph_test_trace!(self, format!("piston retract {:?} revision={revision}", piston));
        Ok(())
    }

    pub(super) fn publish_deferred_pistons(&mut self) -> Result<(), GraphExecutionError> {
        let due = self
            .future
            .deferred_pistons
            .iter()
            .take_while(|event| event.due_tick <= self.future.clock.logical_tick)
            .count();
        for _ in 0..due {
            let event = self.future.deferred_pistons.pop_front().unwrap();
            self.apply_deferred_event(event)?;
        }
        Ok(())
    }

    pub(super) fn apply_deferred_event(&mut self, event: DeferredPistonEvent) -> Result<(), GraphExecutionError> {
        if self.logical_bud_owner_by_actor.get(event.piston.index()).is_some_and(Option::is_some) {
            self.apply_logical_bud_deferred_event(event)?;
            #[cfg(test)]
            return Ok(());
        }
        let state = self.state.pistons[event.piston.index()];
        let spec = self.program.pistons()[event.piston.index()];
        let actor_valid = state.revision == event.revision;
        self.counts.deferred_publications += 1;

        if event.extending {
            self.apply_deferred_resource(event, spec)?;
            if actor_valid {
                self.state.pistons[event.piston.index()].availability = PistonAvailability::Ready;
                self.state.pistons[event.piston.index()].head_present = true;
                self.deliver_route(spec.routes.head_restored, None)?;
            } else {
                #[cfg(test)]
                graph_test_trace!(self, format!("piston stale {:?} revision={}", event.piston, event.revision));
            }
        } else {
            if actor_valid {
                self.state.pistons[event.piston.index()].extended = false;
                self.state.pistons[event.piston.index()].head_present = false;
                self.state.pistons[event.piston.index()].availability = PistonAvailability::Ready;
                self.deliver_route(spec.routes.base_restored, None)?;
            } else {
                #[cfg(test)]
                graph_test_trace!(self, format!("piston stale {:?} revision={}", event.piston, event.revision));
            }
            self.apply_deferred_resource(event, spec)?;
        }
        #[cfg(test)]
        graph_test_trace!(self, format!("piston restored {:?} revision={}", event.piston, event.revision));
        Ok(())
    }

    fn apply_logical_bud_deferred_event(&mut self, event: DeferredPistonEvent) -> Result<(), GraphExecutionError> {
        let spec = self.program.pistons()[event.piston.index()];
        let actor_revision_current = self.state.pistons[event.piston.index()].revision == event.revision;
        self.counts.deferred_publications += 1;
        self.counts.logical_bud_publications += 1;
        if event.extending {
            let resource_revision_current = self.apply_logical_bud_resource(event, spec, LogicalBudEventCut::DeferredResourceFar)?;
            if actor_revision_current {
                let actor = &mut self.state.pistons[event.piston.index()];
                actor.availability = PistonAvailability::Ready;
                actor.head_present = true;
                self.deliver_logical_bud_cut(
                    event.piston,
                    LogicalBudEventCut::DeferredHead,
                    false,
                    false,
                    false,
                    false,
                    true,
                    resource_revision_current,
                )?;
            }
        } else {
            if actor_revision_current {
                let actor = &mut self.state.pistons[event.piston.index()];
                actor.extended = false;
                actor.head_present = false;
                actor.availability = PistonAvailability::Ready;
                self.deliver_logical_bud_cut(event.piston, LogicalBudEventCut::DeferredBase, false, false, false, false, true, false)?;
            }
            self.apply_logical_bud_resource(event, spec, LogicalBudEventCut::DeferredResourceNear)?;
        }
        Ok(())
    }

    fn apply_logical_bud_resource(
        &mut self,
        event: DeferredPistonEvent,
        _spec: PistonSpec,
        cut: LogicalBudEventCut,
    ) -> Result<bool, GraphExecutionError> {
        let Some(resource_event) = event.resource else { return Ok(false) };
        let current = self.state.resources[resource_event.resource.index()];
        let valid = current.revision == resource_event.revision
            && current.at == resource_event.contact
            && !current.enabled
            && self.resource_contact_available(resource_event.resource, resource_event.contact, None);
        if !valid {
            return Ok(false);
        }
        self.set_resource_contact(resource_event.resource, resource_event.contact, true)
            .expect("validated disabled logical resource must become enabled");
        self.deliver_logical_bud_cut(event.piston, cut, false, false, false, false, false, true)?;
        Ok(true)
    }

    fn apply_deferred_resource(&mut self, event: DeferredPistonEvent, spec: PistonSpec) -> Result<(), GraphExecutionError> {
        let Some(resource_event) = event.resource else {
            return Ok(());
        };
        let current = self.state.resources[resource_event.resource.index()];
        if current.revision != resource_event.revision
            || current.at != resource_event.contact
            || current.enabled
            || !self.resource_contact_available(resource_event.resource, resource_event.contact, None)
        {
            #[cfg(test)]
            graph_test_trace!(
                self,
                format!("resource stale {:?} revision={}", resource_event.resource, resource_event.revision)
            );
            return Ok(());
        }
        let (_, revision) = self
            .set_resource_contact(resource_event.resource, resource_event.contact, true)
            .expect("validated disabled resource must become enabled");
        debug_assert_eq!(revision, resource_event.revision.wrapping_add(1));
        let route = if event.extending {
            spec.routes.resource_restored_far
        } else {
            spec.routes.resource_restored_near
        };
        if let Some(route) = route {
            self.deliver_route(route, None)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod request_queue_tests {
    use super::*;

    fn request(actor: u32, action: mchprs_world::PistonAction, source: Option<u32>) -> PistonRequest {
        PistonRequest {
            piston: PistonId::new(actor),
            action,
            source: source.map(RouteId::new),
        }
    }

    #[test]
    fn request_queue_deduplicates_only_the_piston_action_pair_and_keeps_first_source() {
        use mchprs_world::PistonAction::{Extend, Retract, RetractWithoutPull};

        let mut queue = PistonRequestQueue::new();
        assert!(queue.push_back_if_absent(request(0, Retract, Some(7))));
        assert!(!queue.push_back_if_absent(request(0, Retract, Some(8))));
        assert!(queue.push_back_if_absent(request(0, Extend, Some(9))));
        assert!(queue.push_back_if_absent(request(0, RetractWithoutPull, Some(10))));
        assert!(queue.push_back_if_absent(request(1, Retract, Some(11))));

        assert_eq!(
            queue.iter().copied().collect::<Vec<_>>(),
            vec![
                request(0, Retract, Some(7)),
                request(0, Extend, Some(9)),
                request(0, RetractWithoutPull, Some(10)),
                request(1, Retract, Some(11)),
            ]
        );
    }

    #[test]
    fn request_queue_pop_clears_membership_before_callbacks_can_reenqueue() {
        use mchprs_world::PistonAction::Retract;

        let mut queue = PistonRequestQueue::new();
        let first = request(0, Retract, Some(3));
        assert!(queue.push_back_if_absent(first));
        assert_eq!(queue.pop_front(), Some(first));
        assert!(queue.push_back_if_absent(request(0, Retract, Some(4))));
        assert_eq!(queue.front().copied(), Some(request(0, Retract, Some(4))));
    }

    #[test]
    fn request_queue_test_mutations_and_clear_keep_membership_in_sync() {
        use mchprs_world::PistonAction::Retract;

        let mut queue = PistonRequestQueue::new();
        let first = request(0, Retract, Some(2));
        assert!(queue.push_back_if_absent(first));
        let mut cloned = queue.clone();
        queue.set_front_source(None);
        assert_eq!(queue.front().copied(), Some(request(0, Retract, None)));
        assert_eq!(cloned.front().copied(), Some(first));

        let removed = cloned.pop_front().unwrap();
        assert!(cloned.push_front(removed));
        assert!(!cloned.push_back_if_absent(request(0, Retract, Some(9))));
        assert!(cloned.push_back_if_absent(request(1, Retract, Some(10))));

        queue.clear();
        cloned.clear();
        assert!(queue.is_empty());
        assert!(cloned.is_empty());
        assert!(queue.push_back_if_absent(request(0, Retract, Some(12))));
        assert!(cloned.push_back_if_absent(request(0, Retract, Some(13))));
    }
}

#[cfg(test)]
fn native_trace_phase(phase: super::GraphPhase) -> mchprs_world::AdvancePhase {
    match phase {
        super::GraphPhase::BetweenTicks => mchprs_world::AdvancePhase::BetweenTicks,
        super::GraphPhase::ScheduledTicks => mchprs_world::AdvancePhase::ScheduledTicks,
        super::GraphPhase::PistonActions => mchprs_world::AdvancePhase::PistonEvents,
        super::GraphPhase::DeferredConnections => mchprs_world::AdvancePhase::MovingEntities,
    }
}
