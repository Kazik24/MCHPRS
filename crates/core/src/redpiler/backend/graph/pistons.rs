#[cfg(test)]
use super::graph_test_trace;
use super::*;

#[derive(Debug, Clone, Copy)]
pub(super) struct PistonRequest {
    pub(super) piston: PistonId,
    pub(super) action: mchprs_world::PistonAction,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct DeferredPistonEvent {
    pub(super) piston: PistonId,
    pub(super) revision: u64,
    pub(super) resource: Option<DeferredResourceEvent>,
    pub(super) due_tick: u64,
    pub(super) extending: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct DeferredResourceEvent {
    pub(super) resource: ResourceId,
    pub(super) revision: u64,
    pub(super) contact: ContactId,
}

impl GraphBackend {
    #[inline(always)]
    pub(super) fn sample_piston(&mut self, piston: PistonId) -> Result<(), GraphExecutionError> {
        let result = self.sample_piston_inner(piston);
        result
    }

    fn sample_piston_inner(&mut self, piston: PistonId) -> Result<(), GraphExecutionError> {
        let spec = self.program.pistons()[piston.index()];
        let state = self.state.pistons[piston.index()];
        if state.availability.base_pending() {
            return Ok(());
        }
        let action = if let Some(plan) = self.program.symbolic_actor_routine(piston).cloned() {
            self.counts.piston_samples += 1;
            let roots = self.symbolic_actor_roots(&plan, crate::redpiler::program_graph::ActorEvent::Sample);
            #[cfg(test)]
            if self.test_recording {
                let powered = self.input_strength(spec.node, InputPort::Main) > 0 || self.input_strength(spec.node, InputPort::Quasi) > 0;
                self.test_recorder.observed_operations.push(GraphObservedOperation::Sample {
                    actor: self.program.nodes()[spec.node.index()].position.unwrap(),
                    extended: state.extended,
                    powered,
                });
            }
            let Some(action) = crate::redpiler::program_graph::SymbolicActorRoutine::requested(&roots) else {
                return Ok(());
            };
            action
        } else {
            self.counts.piston_samples += 1;
            let powered = self.input_strength(spec.node, InputPort::Main) > 0 || self.input_strength(spec.node, InputPort::Quasi) > 0;
            #[cfg(test)]
            if self.test_recording {
                self.test_recorder.observed_operations.push(GraphObservedOperation::Sample {
                    actor: self.program.nodes()[spec.node.index()].position.unwrap(),
                    extended: state.extended,
                    powered,
                });
            }
            match (powered, state.extended) {
                (true, false) if self.piston_lane_available(piston) => mchprs_world::PistonAction::Extend,
                (false, true) if self.early_resource_retraction(piston) => mchprs_world::PistonAction::RetractWithoutPull,
                (false, true) => mchprs_world::PistonAction::Retract,
                _ => return Ok(()),
            }
        };
        self.enqueue_piston(PistonRequest { piston, action });
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
        if !self
            .future
            .piston_requests
            .iter()
            .any(|queued| queued.piston == request.piston && queued.action == request.action)
        {
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
            self.future.piston_requests.push_back(request);
        }
    }

    pub(super) fn drain_piston_actions(&mut self) -> Result<(), GraphExecutionError> {
        while let Some(request) = self.future.piston_requests.pop_front() {
            self.accept_action(request)?;
        }
        Ok(())
    }

    fn accept_action(&mut self, request: PistonRequest) -> Result<(), GraphExecutionError> {
        let spec = self.program.pistons()[request.piston.index()];
        let state = self.state.pistons[request.piston.index()];
        #[cfg(test)]
        if self.test_recording {
            self.test_recorder.observed_operations.push(GraphObservedOperation::AcceptanceAttempt {
                actor: self.program.nodes()[spec.node.index()].position.unwrap(),
                action: request.action,
            });
        }
        let accepted = if let Some(plan) = self.program.symbolic_actor_routine(request.piston).cloned() {
            let roots = self.symbolic_actor_roots(&plan, crate::redpiler::program_graph::ActorEvent::Accept);
            crate::redpiler::program_graph::SymbolicActorRoutine::accepts(&roots, request.action)
        } else {
            let powered = self.input_strength(spec.node, InputPort::Main) > 0 || self.input_strength(spec.node, InputPort::Quasi) > 0;
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
        };
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

    pub(super) fn accepted(&mut self, request: PistonRequest, extended: bool) {
        self.counts.piston_actions += 1;
        let spec = self.program.pistons()[request.piston.index()];
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

    fn piston_resource_at(&self, piston: PistonId, contact: ContactId) -> Option<ResourceId> {
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
